//! The engine's scan of one `nodes_graphics/` folder, and the pipeline built for what it
//! registered. A driver takes its time over a compile, so no op and no tick waits on one: the
//! class holds a cell a thread of its own fills, and the plan picks it up at the tick after.

use std::path::Path;
use std::sync::Arc;

use goofi_node::{NodeManifest, Scanned, ScannedType};

use crate::gpu::Gpu;
use crate::{shader, GraphicsEngine};

/// A pipeline once its compile answers, or why the device refused it.
pub type Built = goofi_supervisor::ready::Ready<Arc<wgpu::RenderPipeline>>;

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

pub enum Kind {
    Shader(Built),
    Host(crate::producer::Factory),
}

pub(crate) fn scan(engine: &mut GraphicsEngine, dir: &Path) -> Vec<ScannedType> {
    let mut out = Vec::new();
    for (path, type_name, stamp) in goofi_node::node_files(dir, "graphics") {
        // A file with no stamp to compare is read again: unreadable metadata proves nothing.
        let seen = stamp.map(|s| (path.clone(), s));
        let unchanged = seen.is_some() && engine.stamps.get(&type_name) == seen.as_ref() && engine.classes.contains_key(&type_name);
        let registered = match unchanged {
            true => Ok(false),
            false => engine.register(&path, &type_name).map(|class| engine.classes.insert(type_name.clone(), class).is_some()),
        };
        let outcome = match registered {
            Ok(replaced) => {
                match seen {
                    Some(s) => engine.stamps.insert(type_name.clone(), s),
                    None => engine.stamps.remove(&type_name),
                };
                Scanned::Registered { isolation: engine.classes[&type_name].isolation.get(), replaced }
            }
            Err(reason) => {
                // A file that no longer loads displaces its registration, so the palette greys the
                // type rather than offering one nothing can build.
                engine.classes.remove(type_name.as_str());
                engine.stamps.remove(&type_name);
                Scanned::Unavailable(reason)
            }
        };
        goofi_supervisor::progress::scanned(dir, &path);
        out.push(ScannedType { type_name, stamp, outcome });
    }
    out
}

impl GraphicsEngine {
    /// One file: its header is the manifest, its text plus the prelude is what naga judges, and
    /// only then does a pipeline get asked for.
    pub(crate) fn register(&mut self, path: &Path, type_name: &str) -> Result<Arc<Class>, String> {
        if path.extension().is_some_and(|ext| ext != "wgsl") {
            return self.register_host(path, type_name);
        }
        let source = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let intro = shader::header(&source)?;
        let manifest = goofi_node::manifest_of(type_name, &intro, Some(goofi_core::SlotType::Texture))?;
        let pipeline = shader(&self.gpu, &self.shared, manifest, &source, &intro.state)?;
        Ok(Arc::new(Class { manifest, feedback: intro.feedback, window: intro.window, state: intro.state, kind: Kind::Shader(pipeline), isolation: &goofi_node::SHADER }))
    }
}

/// What one pipeline is built from: the whole text naga passed, and the shape of its layout.
struct Job {
    source: String,
    params: bool,
    inputs: usize,
    state: usize,
}

/// `source` plus the prelude, once naga passes it, built into a pipeline on a thread of its own,
/// as every compile is: the tick picks the cell up by itself once it is filled, and the settle it
/// asks for is for a refusal, which only a plan can turn into the node's error.
pub(crate) fn shader(gpu: &Arc<Gpu>, shared: &Arc<goofi_runtime::Shared>, manifest: &'static NodeManifest, source: &str, state: &[String]) -> Result<Built, String> {
    let full = format!("{source}{}", shader::prelude(manifest, state));
    shader::validate(&full)?;
    let params = !manifest.params.is_empty() || shader::array_inputs(manifest).next().is_some();
    let job = Job { source: full, params, inputs: manifest.inputs.len(), state: state.len() };
    let gpu = gpu.clone();
    let built = goofi_supervisor::ready::prepare("goofi-graphics-compile", move || compile(&gpu, &job));
    let shared = shared.clone();
    built.on_ready(move || shared.ask_settle());
    Ok(built)
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
