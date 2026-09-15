//! The engine's scan of one `nodes_graphics/` folder, and the thread that builds a pipeline for
//! what it registered. A driver takes its time over a compile, so no op waits on one: the class
//! holds a cell, and the plan picks it up at the tick after it is filled.

use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};

use goofi_node::{NodeManifest, Scanned, ScannedType};

use crate::gpu::Gpu;
use crate::{shader, GraphicsEngine};

/// A pipeline once the compile thread answers, or why the device refused it.
pub type Built = Arc<OnceLock<Result<Arc<wgpu::RenderPipeline>, String>>>;

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
        let outcome = match engine.register(&path, &type_name) {
            Ok(replaced) => Scanned::Registered { isolation: engine.classes[&type_name].isolation.get(), replaced },
            Err(reason) => {
                // A file that no longer loads displaces its registration, so the palette greys the
                // type rather than offering one nothing can build.
                crate::gpu::give_back(engine.classes.remove(type_name.as_str()));
                Scanned::Unavailable(reason)
            }
        };
        out.push(ScannedType { type_name, stamp, outcome });
    }
    out
}

impl GraphicsEngine {
    /// One file: its header is the manifest, its text plus the prelude is what naga judges, and
    /// only then does a pipeline get asked for.
    pub(crate) fn register(&mut self, path: &Path, type_name: &str) -> Result<bool, String> {
        if path.extension().is_some_and(|ext| ext != "wgsl") {
            return self.register_host(path, type_name);
        }
        let source = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let intro = shader::header(&source)?;
        if let Some(reason) = goofi_node::illegal_slot(&intro) {
            return Err(reason);
        }
        let manifest = goofi_node::leak_manifest(type_name.to_string(), &intro)?;
        let full = format!("{source}{}", shader::prelude(manifest, &intro.state));
        shader::validate(&full)?;
        let job = Job {
            source: full,
            params: !manifest.params.is_empty() || shader::array_inputs(manifest).next().is_some(),
            inputs: manifest.inputs.len(),
            state: intro.state.len(),
        };
        let pipeline = self.compiler.build(job);
        let class =
            Arc::new(Class { manifest, feedback: intro.feedback, window: intro.window, state: intro.state, kind: Kind::Shader(pipeline), isolation: &goofi_node::SHADER });
        let displaced = self.classes.insert(type_name.to_string(), class);
        let replaced = displaced.is_some();
        crate::gpu::give_back(displaced);
        Ok(replaced)
    }
}

/// What one pipeline is built from: the whole text naga passed, and the shape of its layout.
pub struct Job {
    source: String,
    params: bool,
    inputs: usize,
    state: usize,
}

impl Compiler {
    pub fn program(&self, manifest: &'static NodeManifest, source: &str) -> Result<Built, String> {
        let full = format!("{source}{}", shader::prelude(manifest, &[]));
        shader::validate(&full)?;
        Ok(self.build(Job { source: full, params: !manifest.params.is_empty(), inputs: manifest.inputs.len(), state: 0 }))
    }
}

/// One job on the compile thread's queue: what to build, and where to leave it.
struct Order {
    job: Job,
    cell: Built,
    shared: Arc<goofi_control::Shared>,
}

/// Where every pipeline in the process is built: ONE thread, beside the one device, so no op
/// waits on a compile.
fn compiler() -> Option<&'static mpsc::Sender<Order>> {
    static ONE: OnceLock<Option<mpsc::Sender<Order>>> = OnceLock::new();
    ONE.get_or_init(|| {
        let gpu = crate::gpu::shared().ok()?;
        let (jobs, take) = mpsc::channel::<Order>();
        goofi_core::worker::thread("goofi-graphics-compile")
            .spawn(move || {
                while let Ok(order) = take.recv() {
                    let _ = order.cell.set(compile(&gpu, &order.job));
                    // The tick picks the cell up by itself; the settle is for a refusal, which
                    // only a plan can turn into the node's standing error.
                    order.shared.ask_settle();
                    // The order may hold the last handle on the pipeline it just built.
                    crate::gpu::give_back(order);
                }
            })
            .ok()?;
        Some(jobs)
    })
    .as_ref()
}

/// One engine's end of that queue: the shared state a finished compile must wake.
#[derive(Clone)]
pub struct Compiler(pub Arc<goofi_control::Shared>);

impl Compiler {
    pub fn build(&self, job: Job) -> Built {
        let cell: Built = Arc::new(OnceLock::new());
        let order = Order { job, cell: cell.clone(), shared: self.0.clone() };
        match compiler() {
            Some(jobs) => {
                let _ = jobs.send(order);
            }
            None => {
                let _ = cell.set(Err("no compile thread".into()));
            }
        }
        cell
    }
}

fn compile(gpu: &Gpu, job: &Job) -> Result<Arc<wgpu::RenderPipeline>, String> {
    let _gate = crate::gpu::gate();
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(job.source.as_str().into()),
    });
    let layout = gpu.layout(job.params, job.inputs, job.state);
    // The output, then one target per state buffer — the order [`shader::prelude`] writes them in.
    let targets: Vec<Option<wgpu::ColorTargetState>> = (0..1 + job.state)
        .map(|_| {
            Some(wgpu::ColorTargetState { format: crate::gpu::FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })
        })
        .collect();
    let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            targets: &targets,
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    });
    match pollster::block_on(scope.pop()) {
        Some(e) => Err(format!("the device refused the pipeline: {e}")),
        None => Ok(Arc::new(pipeline)),
    }
}
