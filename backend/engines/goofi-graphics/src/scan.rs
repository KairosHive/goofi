//! What the graphics engine registers from a node file, and the pipeline built for a shader.
//! A driver takes its time over a compile, so no op and no tick waits on one: the class holds a
//! cell a thread of its own fills, and the plan picks it up at the tick after.
//! A Python or a Rust file is a node the tick calls: its class holds what builds one.

use std::sync::Arc;

use goofi_node::{Body, NodeFile, NodeManifest, Read, Scanned};

use crate::gpu::Gpu;
use crate::{shader, GraphicsEngine};

/// A pipeline once its compile answers, or why the device refused it.
pub type Built = goofi_supervisor::ready::Ready<Arc<wgpu::RenderPipeline>>;

/// What a class draws its stage with.
pub enum Kind {
    Shader(Shader),
    /// A node the tick calls.
    Node(Runs),
}

/// What runs a node the tick calls.
pub enum Runs {
    /// A Python file's source, and the interpreter of the child that runs it.
    Python { interpreter: String, source: String },
    /// A Rust library, in this process or in a child.
    Rust(Arc<goofi_host_sdk::NodeFactory>),
}

/// A shader naga passed: its pipeline is asked of the device at the first node of the type.
pub struct Shader {
    job: Job,
    built: std::sync::OnceLock<Built>,
}

impl Shader {
    /// The pipeline's cell, once a node asked for it.
    pub fn built(&self) -> Option<&Built> {
        self.built.get()
    }

    /// Ask the device for the pipeline, once: a thread of its own compiles, and the settle it
    /// asks for is for a refusal, which only a plan can turn into the node's error.
    pub(crate) fn start(&self, gpu: &Arc<Gpu>, shared: &Arc<goofi_runtime::Shared>) -> Option<Built> {
        if self.built.get().is_some() {
            return None;
        }
        let (gpu, job, shared) = (gpu.clone(), self.job.clone(), shared.clone());
        let built = goofi_supervisor::ready::prepare("goofi-graphics-compile", move || compile(&gpu, &job));
        built.on_ready(move || shared.ask_settle());
        self.built.set(built.clone()).ok().map(|()| built)
    }
}

/// One graphics source as the engine holds it.
pub struct Class {
    pub manifest: &'static NodeManifest,
    pub feedback: bool,
    pub window: bool,
    /// The buffers this type carries between two ticks, in the order the prelude binds them.
    pub state: Vec<String>,
    pub kind: Kind,
    pub isolation: &'static goofi_node::IsolationCell,
}

impl Class {
    /// Whether the tick calls this class's node, where a shader's stage is a pass.
    pub fn node(&self) -> bool {
        matches!(self.kind, Kind::Node(_))
    }

    /// Why a stage of this class cannot draw: a shader the device refused.
    pub fn refusal(&self) -> Option<String> {
        match &self.kind {
            Kind::Shader(shader) => shader.built()?.error().map(|why| format!("shader: {why}")),
            Kind::Node(_) => None,
        }
    }
}

impl GraphicsEngine {
    /// A file that did not change keeps its class, and the pipeline built for it. One that no
    /// longer loads displaces its registration, so the palette greys the type.
    pub(crate) fn register_file(&mut self, file: &NodeFile, read: &Read) -> Scanned {
        let name = &file.type_name;
        // A file with no stamp to compare is read again: unreadable metadata proves nothing.
        let seen = file.stamp.map(|s| (file.path.clone(), s));
        let unchanged = seen.is_some() && self.stamps.get(name) == seen.as_ref() && self.classes.contains_key(name);
        let registered = match unchanged {
            true => Ok(false),
            false => self.class(name, read).map(|class| self.classes.insert(name.clone(), class).is_some()),
        };
        match registered {
            Ok(replaced) => {
                match seen {
                    Some(s) => self.stamps.insert(name.clone(), s),
                    None => self.stamps.remove(name),
                };
                Scanned::Registered { isolation: self.classes[name].isolation.get(), replaced }
            }
            Err(reason) => {
                self.classes.remove(name.as_str());
                self.stamps.remove(name);
                Scanned::Unavailable(reason)
            }
        }
    }

    /// A shader's header completed is its manifest, and its text plus the prelude is what naga
    /// judges. A Python or a Rust node makes one picture, and each of its inputs takes one wire.
    fn class(&self, type_name: &str, read: &Read) -> Result<Arc<Class>, String> {
        if let Body::Shader { source } = &read.body {
            let intro = shader::complete(&read.intro)?;
            let manifest = goofi_node::manifest_of(type_name, &intro)?;
            let shader = shader(manifest, source, &intro.state)?;
            return Ok(Arc::new(Class { manifest, feedback: intro.feedback, window: intro.window, state: intro.state, kind: Kind::Shader(shader), isolation: &goofi_node::SHADER }));
        }
        let manifest = goofi_node::manifest_of(type_name, &read.intro)?;
        if manifest.outputs.len() != 1 || manifest.outputs[0].kind != goofi_core::SlotType::Texture {
            return Err("a graphics node has one output, a TEXTURE".into());
        }
        if manifest.inputs.iter().any(|slot| slot.multi) {
            return Err("an input of a graphics node takes one wire".into());
        }
        let (runs, isolation) = match &read.body {
            Body::Rust { artifact, host } => {
                let (factory, tier) = goofi_runtime::hosted::factory(self.hub.clone(), artifact.clone(), host.clone(), manifest)?;
                (Runs::Rust(Arc::new(factory)), tier)
            }
            Body::Python { source, interpreter } => {
                let runs = Runs::Python { interpreter: interpreter.clone(), source: source.clone() };
                (runs, goofi_node::IsolationCell::leak(goofi_node::Isolation::Subprocess))
            }
            Body::Shader { .. } => unreachable!("a shader returned above"),
        };
        Ok(Arc::new(Class { manifest, feedback: false, window: false, state: Vec::new(), kind: Kind::Node(runs), isolation }))
    }
}

/// What one pipeline is built from: the whole text naga passed, and the shape of its layout.
#[derive(Clone)]
struct Job {
    source: String,
    params: bool,
    inputs: usize,
    state: usize,
}

/// `source` plus the prelude, once naga passes it: what a pipeline is built from when a node asks.
pub(crate) fn shader(manifest: &'static NodeManifest, source: &str, state: &[String]) -> Result<Shader, String> {
    let full = format!("{source}{}", shader::prelude(manifest, state));
    shader::validate(&full)?;
    let params = !manifest.params.is_empty() || shader::array_inputs(manifest).next().is_some();
    let inputs = manifest.inputs.len() + shader::array_inputs(manifest).count();
    Ok(Shader { job: Job { source: full, params, inputs, state: state.len() }, built: std::sync::OnceLock::new() })
}

/// The device's compile, on the preparing thread. wgpu keeps an error scope per thread, so the
/// refusal this catches is this pipeline's and a tick drawing meanwhile is untouched.
fn compile(gpu: &Gpu, job: &Job) -> Result<Arc<wgpu::RenderPipeline>, String> {
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(job.source.as_str().into()),
    });
    let layout = gpu.layout(job.params, job.inputs, job.state);
    // The output, then one target per state buffer: the order [`shader::prelude`] writes them in.
    let pipeline = crate::gpu::pipeline(&gpu.device, None, &module, &layout, "fs", &vec![crate::gpu::FORMAT; 1 + job.state]);
    match pollster::block_on(scope.pop()) {
        Some(e) => Err(format!("the device refused the pipeline: {e}")),
        None => Ok(Arc::new(pipeline)),
    }
}
