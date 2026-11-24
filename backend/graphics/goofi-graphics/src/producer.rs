//! Host producers run on the shared host executor, inside the node's own runtime thread, and
//! submit what they render into the graphics resource owner.
use crate::scan::{Class, Kind};
use goofi_core::texture::Texture;
use goofi_core::{Data, SlotType, Value};
use goofi_host_sdk::Node;
use goofi_node::{NodeManifest, ParamGroups};
use goofi_supervisor::sync::Mailbox;
use std::path::Path;
use std::sync::Arc;

pub(crate) type Factory = Arc<dyn Fn(&ParamGroups) -> Box<dyn Node> + Send + Sync>;
#[derive(Clone)]
pub enum Content {
    Pixels(Arc<crate::resources::Upload>),
    Render(Arc<str>),
}
#[derive(Clone)]
pub struct Produced {
    pub size: (u32, u32),
    pub index: u64,
    pub content: Content,
}

/// What a host producer made, on its way to its two readers: the render thread, which uploads
/// or draws it, and the engine, which sizes the stage and compiles a render program off it. One
/// cell each, latest wins, so neither reader waits on the producer and the producer waits on no
/// reader; each keeps what it last took.
#[derive(Default)]
pub struct Cells {
    pub(crate) to_render: Mailbox<Produced>,
    pub(crate) to_engine: Mailbox<Produced>,
}

pub type Source = Arc<Cells>;

impl crate::GraphicsEngine {

    pub fn set_python(&mut self, python: goofi_python::catalog::Python) {
        self.python = Some(python);
    }

    /// Name the executable whose `host` mode runs a node built after boot.
    pub fn set_host(&mut self, exe: std::path::PathBuf) {
        self.host = Some(exe);
    }

    pub(crate) fn register_host(&mut self, path: &Path, name: &str) -> Result<Arc<Class>, String> {
        let (manifest, factory, isolation): (_, Factory, _) = if path.extension().is_some_and(|e| e == "rs") {
            let base = goofi_supervisor::layout::runtime().build();
            let artifact = goofi_build::built(&goofi_build::GRAPHICS, path, &base)?;
            let (manifest, factory, tier) = goofi_runtime::hosted::load(
                self.hub.clone(), artifact, name, Some(SlotType::Texture), self.host.as_deref(), self.booted,
            )?;
            (manifest, Arc::from(factory), tier)
        } else {
            use goofi_python::catalog::{Probed, probed, routed};
            let probed = probed(path, name, self.python.as_ref())
                .ok_or("no Python interpreter provisioned — run `cargo run -p goofi-init`")?;
            match probed {
                Probed::InProcess(d) | Probed::Subprocess(d) => {
                    let subproc = self.python.as_ref().map(|p| p.subproc.as_str()).unwrap_or_default();
                    let (manifest, factory, tier) = routed(self.hub.clone(), d, subproc);
                    (manifest, Arc::from(factory), tier)
                }
                Probed::Unavailable(why) => return Err(why),
            }
        };
        if manifest.outputs.len() != 1
            || manifest.outputs[0].name != "out"
            || manifest.outputs[0].kind != SlotType::Texture
        {
            return Err("a graphics host node must declare one TEXTURE output named `out`".into());
        }
        if manifest.inputs.iter().any(|s| s.kind == SlotType::Texture || s.multi) {
            return Err(
                "graphics host inputs must be single CPU frames; use shader nodes for texture processing".into()
            );
        }
        let params: Vec<_> = manifest
            .params
            .iter()
            .copied()
            .chain(
                goofi_runtime::common_decls(manifest)
                    .into_iter()
                    .filter(|d| !manifest.params.iter().any(|p| p.group == d.group && p.name == d.name)),
            )
            .collect();
        // Keyed by the base manifest's address: it is interned by content, so the address is one.
        let base = manifest as *const NodeManifest as u64;
        let manifest = goofi_node::interned(manifest.type_name, base, || NodeManifest {
            params: Box::leak(params.into_boxed_slice()),
            type_name: manifest.type_name,
            tags: manifest.tags,
            doc: manifest.doc,
            inputs: manifest.inputs,
            outputs: manifest.outputs,
            producer: manifest.producer,
        });
        Ok(Arc::new(Class { manifest, feedback: false, window: false, state: Vec::new(), kind: Kind::Host(factory), isolation }))
    }
}


/// What a producer emitted, into the cells the engine compiles and the render thread uploads
/// from. Answers whether the frame asks for a settle: a new size, or a render source that moved,
/// judged against `last`, the producer's own memory of what it put out before.
pub fn produced(source: &Source, last: &mut Option<Produced>, frame: &Data) -> bool {
    let Value::Texture(texture) = frame.value() else { return false };
    let content = match &**texture {
        Texture::Pixels(pixels) => Content::Pixels(Arc::new(crate::resources::Upload::pixels(pixels))),
        Texture::Render { source, .. } => Content::Render(Arc::from(source.as_str())),
    };
    let size = texture.size();
    let changed = last.as_ref().is_none_or(|old| old.size != size)
        || match (&content, last.as_ref().map(|old| &old.content)) {
            (Content::Render(new), Some(Content::Render(old))) => new != old,
            (Content::Render(_), _) | (Content::Pixels(_), Some(Content::Render(_))) => true,
            _ => false,
        };
    let made = Produced { size, index: frame.meta().index().unwrap_or(0), content };
    source.to_render.put(Box::new(made.clone()));
    source.to_engine.put(Box::new(made.clone()));
    *last = Some(made);
    changed
}
