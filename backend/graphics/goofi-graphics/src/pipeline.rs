//! One `.wgsl` file judged and built: its header read as the manifest, naga's verdict on the
//! text plus the prelude, and the render pipeline the device makes of it.

use std::sync::{Arc, OnceLock};

use goofi_core::probe::Introspection;
use goofi_node::NodeManifest;

use crate::gpu::Gpu;
use crate::shader;

/// A pipeline once the compile answers, or why the device refused it.
pub type Built = Arc<OnceLock<Result<Arc<wgpu::RenderPipeline>, String>>>;

/// What one pipeline is built from: the whole text naga passed, and the shape of its layout.
pub struct Job {
    source: String,
    params: bool,
    inputs: usize,
    state: usize,
}

impl Job {
    /// One shader file: its header is the manifest, its text plus the prelude is what naga
    /// judges, and only then is a pipeline asked for.
    pub fn shader(type_name: &str, source: &str) -> Result<(Introspection, &'static NodeManifest, Job), String> {
        let intro = shader::header(source)?;
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
        Ok((intro, manifest, job))
    }

    /// A host node's texture program: a body with no header, against the node's own manifest.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn program(manifest: &'static NodeManifest, source: &str) -> Result<Job, String> {
        let full = format!("{source}{}", shader::prelude(manifest, &[]));
        shader::validate(&full)?;
        Ok(Job { source: full, params: !manifest.params.is_empty(), inputs: manifest.inputs.len(), state: 0 })
    }
}

/// The pipeline, or the validation error the device raised while making it.
#[cfg(not(target_arch = "wasm32"))]
pub async fn compile(gpu: &Gpu, job: &Job) -> Result<Arc<wgpu::RenderPipeline>, String> {
    // The gate covers the device calls; the scope's answer is awaited without it.
    let (pipeline, verdict) = {
        let _gate = crate::gpu::gate();
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = build(gpu, job, &module(gpu, job));
        (pipeline, scope.pop())
    };
    match verdict.await {
        Some(e) => Err(format!("the device refused the pipeline: {e}")),
        None => Ok(Arc::new(pipeline)),
    }
}

/// The same verdict from the module's own compilation report: wgpu 30 on the web panics on an
/// error scope's empty answer (the `null` it reads as a value), so a page never pops one.
#[cfg(target_arch = "wasm32")]
pub async fn compile(gpu: &Gpu, job: &Job) -> Result<Arc<wgpu::RenderPipeline>, String> {
    let (module, report) = {
        let _gate = crate::gpu::gate();
        let module = module(gpu, job);
        let report = module.get_compilation_info();
        (module, report)
    };
    let errors: Vec<String> = report
        .await
        .messages
        .iter()
        .filter(|m| m.message_type == wgpu::CompilationMessageType::Error)
        .map(|m| match &m.location {
            Some(at) => format!(":{}:{} {}", at.line_number, at.line_position, m.message),
            None => m.message.clone(),
        })
        .collect();
    if !errors.is_empty() {
        return Err(format!("the device refused the pipeline: {}", errors.join("\n")));
    }
    let _gate = crate::gpu::gate();
    Ok(Arc::new(build(gpu, job, &module)))
}

fn module(gpu: &Gpu, job: &Job) -> wgpu::ShaderModule {
    gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(job.source.as_str().into()),
    })
}

fn build(gpu: &Gpu, job: &Job, module: &wgpu::ShaderModule) -> wgpu::RenderPipeline {
    let layout = gpu.layout(job.params, job.inputs, job.state);
    // The output, then one target per state buffer — the order [`shader::prelude`] writes them in.
    let targets: Vec<Option<wgpu::ColorTargetState>> = (0..1 + job.state)
        .map(|_| {
            Some(wgpu::ColorTargetState { format: crate::gpu::FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })
        })
        .collect();
    gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs"),
            targets: &targets,
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}
