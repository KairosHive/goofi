//! The browser host: one shader node drawn into a canvas, paced by the page's animation frames.
//! The glue crate owns the page; this is the engine's side of it, and holds no engine logic.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use goofi_node::NodeManifest;
use wgpu::web_sys::HtmlCanvasElement;

use crate::gpu::{self, Gpu, Want};
use crate::pipeline::{compile, Job};
use crate::resources::State;
use crate::Host;

/// The page's clock: the seconds the last animation frame carried.
#[derive(Default)]
struct Frames(AtomicU64);

impl Host for Frames {
    fn now(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// One generator on one canvas.
pub struct Browser {
    clock: Frames,
    canvas: HtmlCanvasElement,
    manifest: &'static NodeManifest,
    params: Vec<AtomicU64>,
    state: State,
    surface: wgpu::Surface<'static>,
    size: (u32, u32),
    pipeline: Arc<wgpu::RenderPipeline>,
    /// Last: fields drop in declaration order, and everything above is the device's.
    gpu: Gpu,
}

impl Browser {
    /// Judge and build `source` as the node `type_name`, on a device of the page's own.
    pub async fn open(canvas: HtmlCanvasElement, type_name: &str, source: &str) -> Result<Browser, String> {
        let (intro, manifest, job) = Job::shader(type_name, source)?;
        if !manifest.inputs.is_empty() || !intro.state.is_empty() {
            return Err(format!("`{type_name}` takes inputs or carries state; the page draws a generator alone for now"));
        }
        let instance = gpu::instance();
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|e| format!("no surface on the canvas: {e}"))?;
        let gpu = Gpu::open(instance).await?;
        let pipeline = compile(&gpu, &job).await?;
        let params = manifest.params.iter().map(|d| AtomicU64::new(d.spec.to_param().scalar().to_bits())).collect();
        let state = State::new(&gpu, crate::shader::params_len(manifest));
        let mut browser = Browser { clock: Frames::default(), canvas, manifest, params, state, surface, size: (0, 0), pipeline, gpu };
        browser.follow_canvas();
        Ok(browser)
    }

    /// The surface at the canvas's own size, reconfigured when the page resizes it.
    fn follow_canvas(&mut self) {
        let size = (self.canvas.width().max(1), self.canvas.height().max(1));
        if size == self.size {
            return;
        }
        self.size = size;
        self.surface.configure(&self.gpu.device, &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: Want::Screen.format(),
            color_space: wgpu::SurfaceColorSpace::default(),
            width: size.0,
            height: size.1,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
        });
    }

    /// One frame at `now` seconds: the stage drawn at the canvas size, then blitted onto it.
    pub fn tick(&mut self, now: f64) -> Result<(), String> {
        self.clock.0.store(now.to_bits(), Ordering::Relaxed);
        let t = self.clock.now();
        self.follow_canvas();
        self.state.ensure_out(&self.gpu, self.size, [None; 4], 0)?;
        self.state.write_uniforms(&self.gpu, t, self.size, self.manifest.params, &self.params);
        // A frame the canvas withholds this time — occluded, outdated, lost — is skipped, not an error.
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => return Ok(()),
        };
        let screen = frame.texture.create_view(&Default::default());
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.state.draw(&self.gpu, &mut encoder, &self.pipeline, &[]);
        let out = &self.state.out.as_ref().expect("ensure_out made it").view;
        self.gpu.blit(&mut encoder, Want::Screen, out, &screen, &self.state.resolution);
        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(frame);
        self.state.advance();
        Ok(())
    }
}
