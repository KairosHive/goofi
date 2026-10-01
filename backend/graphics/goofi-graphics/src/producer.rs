//! Host producers run on the shared host executor, inside the node's own runtime thread, and
//! submit what they render into the graphics resource owner.
use crate::scan::{Class, Kind};
use goofi_core::texture::Texture;
use goofi_core::{Data, SlotType, Value};
use goofi_host_sdk::Node;
use goofi_node::{NodeManifest, ParamGroups};
use std::path::Path;
use std::sync::Arc;
use goofi_supervisor::sync::Mutex;

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
pub type Source = Arc<Mutex<Option<Produced>>>;

impl crate::GraphicsEngine {
    pub fn set_python(&mut self, python: goofi_python::catalog::Python) {
        self.python = Some(python);
    }

    pub(crate) fn register_host(&mut self, path: &Path, name: &str) -> Result<bool, String> {
        let (manifest, factory, isolation): (_, Factory, _) = if path.extension().is_some_and(|e| e == "rs") {
            let base = goofi_build::base_dir(&goofi_supervisor::home::dir());
            let artifact = goofi_build::built(&goofi_build::GRAPHICS, path, &base)?;
            let opened = goofi_build::open(&artifact)?;
            let intro = goofi_node::parse_introspection(&opened.describe)?;
            if let Some(why) =
                goofi_node::illegal_slot(&intro).or_else(|| goofi_node::foreign_output(&intro, Some(SlotType::Texture)))
            {
                return Err(why);
            }
            let manifest = goofi_node::leak_manifest(name.into(), &intro)?;
            let loaded = Arc::new(unsafe { goofi_host_sdk::host::Loaded::open(opened.library, manifest) }?);
            (manifest, Arc::new(move |_| loaded.instantiate()), &goofi_node::NATIVE)
        } else {
            use goofi_python::catalog::{Probed, probe, routed};
            match probe(path, self.python.as_ref()) {
                Probed::InProcess(d) | Probed::Subprocess(d) => {
                    let subproc = self.python.as_ref().map(|p| p.subproc.as_str()).unwrap_or_default();
                    let (manifest, factory, tier) = routed(self.iox.clone(), d, subproc);
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
        let class = Arc::new(Class {
            manifest,
            feedback: false,
            window: false,
            state: Vec::new(),
            kind: Kind::Host(factory),
            isolation,
        });
        let displaced = self.classes.insert(name.into(), class);
        let replaced = displaced.is_some();
        crate::gpu::give_back(displaced);
        Ok(replaced)
    }
}


/// What a producer emitted, into the cell the engine compiles and uploads from. Answers whether
/// the frame asks for a settle: a new size, or a render source that moved.
pub fn produced(source: &Source, frame: &Data) -> bool {
    let Value::Texture(texture) = frame.value() else { return false };
    let content = match &**texture {
        Texture::Pixels(pixels) => Content::Pixels(Arc::new(crate::resources::Upload::pixels(pixels))),
        Texture::Render { source, .. } => Content::Render(Arc::from(source.as_str())),
    };
    let mut latest = source.lock();
    let size = texture.size();
    let changed = latest.as_ref().is_none_or(|old| old.size != size)
        || match (&content, latest.as_ref().map(|old| &old.content)) {
            (Content::Render(new), Some(Content::Render(old))) => new != old,
            (Content::Render(_), _) | (Content::Pixels(_), Some(Content::Render(_))) => true,
            _ => false,
        };
    *latest = Some(Produced { size, index: frame.meta().index().unwrap_or(0), content });
    changed
}
