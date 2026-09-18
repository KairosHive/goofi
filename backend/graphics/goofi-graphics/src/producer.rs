//! Host producers use the shared lifecycle and submit into the graphics resource owner.
use crate::scan::{Class, Kind};
use goofi_core::texture::Texture;
use goofi_core::{Data, SlotType, Value};
use goofi_host_sdk::Node;
use goofi_node::{NodeManifest, ParamGroups, ParamKey, Status, Uid};
use std::path::Path;
use std::sync::{Arc, Mutex};

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
            let base = goofi_build::base_dir(&goofi_core::home::dir());
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
                    let (manifest, factory, tier) = routed(d, subproc);
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
                goofi_host::common_decls(manifest)
                    .filter(|d| !manifest.params.iter().any(|p| p.group == d.group && p.name == d.name)),
            )
            .collect();
        let manifest = Box::leak(Box::new(NodeManifest {
            params: Box::leak(params.into_boxed_slice()),
            type_name: manifest.type_name,
            tags: manifest.tags,
            doc: manifest.doc,
            inputs: manifest.inputs,
            outputs: manifest.outputs,
            producer: manifest.producer,
        }));
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

#[derive(Clone)]
pub struct Lifetime {
    pub halt: Arc<goofi_transport::Halt>,
    local: Arc<goofi_host::local::Local>,
}
impl Lifetime {
    pub fn stop(&self) {
        self.halt.stop();
        self.local.wake();
    }
}

pub struct Worker {
    local: Arc<goofi_host::local::Local>,
    halt: Arc<goofi_transport::Halt>,
    manifest: &'static NodeManifest,
    params: ParamGroups,
}

impl Worker {
    pub fn start(
        uid: Uid,
        manifest: &'static NodeManifest,
        factory: Factory,
        params: ParamGroups,
        engine: &crate::GraphicsEngine,
        source: Source,
    ) -> Self {
        let time = engine.time.clone();
        let shared = engine.shared.clone();
        let report = shared.clone();
        let halt = Arc::new(goofi_transport::Halt::default());
        let publish_halt = halt.clone();
        let report_halt = halt.clone();
        let local = goofi_host::local::Local::new(
            move |_, frame| {
                if publish_halt.stopped() {
                    return;
                }
                let Value::Texture(texture) = frame.value() else { return };
                let content = match &**texture {
                    Texture::Pixels(pixels) => Content::Pixels(Arc::new(crate::resources::Upload::pixels(pixels))),
                    Texture::Render { source, .. } => Content::Render(Arc::from(source.as_str())),
                };
                let mut latest = source.lock().unwrap();
                let size = texture.size();
                let changed = latest.as_ref().is_none_or(|old| old.size != size)
                    || match (&content, latest.as_ref().map(|old| &old.content)) {
                        (Content::Render(new), Some(Content::Render(old))) => new != old,
                        (Content::Render(_), _) | (Content::Pixels(_), Some(Content::Render(_))) => true,
                        _ => false,
                    };
                *latest = Some(Produced { size, index: frame.meta().index().unwrap_or(0), content });
                if changed {
                    shared.ask_settle();
                }
            },
            move |status| {
                if let goofi_host::runtime::WireStatus::Health(status) = status {
                    // Evaluated params belong to the control half; this worker receives only values.
                    if !matches!(status, Status::ParamValues { .. } | Status::BindingErrors { .. }) {
                        let mut reports = report.reports.lock().unwrap();
                        if report_halt.stopped() {
                            return;
                        }
                        reports.push((uid, status));
                        report.waker.notify();
                    }
                }
            },
        );
        let f = factory;
        let build: goofi_host::runtime::NodeBuild = Box::new(move |p| f(p));
        let env =
            goofi_host::runtime::NodeEnv { engine: "graphics", node: Some(format!("{uid}")), evaluator: None, time };
        if let Err(error) =
            goofi_host::runtime::spawn(manifest, build, params.clone(), local.clone(), env, halt.clone())
        {
            use goofi_host::runtime::Transport;
            local.report(goofi_host::runtime::WireStatus::Health(Status::Fault {
                fault: Some(goofi_node::NodeFault::Process {
                    msg: format!("could not start graphics producer: {error}"),
                    since: 0.0,
                }),
            }));
            halt.release();
        }
        Self { local, halt, manifest, params }
    }
    pub fn lifetime(&self) -> Lifetime {
        Lifetime { halt: self.halt.clone(), local: self.local.clone() }
    }
    pub fn sync(&mut self, cx: &goofi_control::Cx<'_>) {
        let decls = crate::engine::decls_of(self.manifest);
        let mut next = ParamGroups::new();
        for (d, value) in decls.iter().zip(&cx.values) {
            next.entry(d.group.into()).or_default().insert(d.name.into(), value.clone());
        }
        let pulses = cx
            .pulses
            .iter()
            .filter_map(|i| decls.get(*i))
            .map(|d| goofi_host::runtime::Control::PulseParam { key: ParamKey::new(d.group, d.name) })
            .collect();
        self.local.params(&self.params, &next, pulses);
        self.params = next;
    }

    pub fn input(&self, inbox: usize, frame: Data) {
        if let Some(slot) = self.manifest.inputs.get(inbox) {
            self.local.input(slot.name, frame);
        }
    }
    pub fn unwired(&self, inbox: usize) {
        if let Some(slot) = self.manifest.inputs.get(inbox) {
            self.local.control(goofi_host::runtime::Control::InSlot { slot: slot.name.into(), wires: Vec::new() });
        }
    }
    pub fn refresh(&self) {
        for d in
            self.manifest.params.iter().filter(|d| matches!(d.spec, goofi_node::ParamSpec::Str { refresh: true, .. }))
        {
            self.local.control(goofi_host::runtime::Control::RefreshParam { key: ParamKey::new(d.group, d.name) });
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.halt.stop();
        self.local.wake();
    }
}
