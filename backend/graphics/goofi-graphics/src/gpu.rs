//! One device for the whole engine: the adapter that answered, the sampler and the bind group
//! layouts every pipeline shares, and the 1x1 transparent texture an unwired input samples.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Every texture in the engine.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// What a reader takes off a stage, and the format the GPU converts into for it — so no texel is
/// ever converted on the CPU. A screen takes 8-bit in the byte order it reads, the recorder the
/// 8-bit RGBA texels, and a tap whichever width its viewers DRAW: f32 where
/// anything reads the numbers, 8-bit where every one of them draws pixels. The two taps are
/// separate readers because they are separate formats, and at most one of them is ever wanted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Want {
    Screen,
    Tap,
    TapU8,
    Record,
}

impl Want {
    pub const ALL: [Want; 4] = [Want::Screen, Want::Tap, Want::TapU8, Want::Record];

    pub const fn format(self) -> wgpu::TextureFormat {
        match self {
            #[cfg(not(target_os = "macos"))]
            Want::Screen => wgpu::TextureFormat::Bgra8Unorm,
            #[cfg(target_os = "macos")]
            Want::Screen => wgpu::TextureFormat::Rgba8Unorm,
            Want::Tap => wgpu::TextureFormat::Rgba32Float,
            Want::TapU8 => wgpu::TextureFormat::Rgba8Unorm,
            Want::Record => wgpu::TextureFormat::Rgba8Unorm,
        }
    }

    /// How many frames this reader keeps in flight. A screen must never miss one, so a copy is
    /// started while the last is still on the device; a viewer is PULLED and can never use more
    /// than the one it asked for.
    pub const fn depth(self) -> usize {
        match self {
            Want::Screen => 2,
            Want::Tap | Want::TapU8 => 1,
            Want::Record => 2,
        }
    }

    /// What one texel takes in that format.
    pub const fn texel(self) -> u32 {
        match self {
            Want::Screen => 4,
            Want::Tap => 16,
            Want::TapU8 => 4,
            Want::Record => 4,
        }
    }
}

/// The pass that converts a stage's output into what one reader takes, and shrinks it to that
/// reader's size on the way. It LOADS rather than samples, so a 1:1 pass is exact and a
/// reduction is the BOX AVERAGE of the source texels under each output texel — the answer the
/// CPU area kernel it replaces would give. `TAP_MAX` bounds the taps per axis: past that the
/// samples spread evenly across the box instead of covering it.
const BLIT: &str = "\
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> out_size: vec2f;

fn boxed(at: vec2f) -> vec4f {
    let src_size = vec2f(textureDimensions(src));
    let scale = src_size / out_size;
    let lo = (at - vec2f(0.5)) * scale;
    let span = max(scale, vec2f(1.0));
    let taps = min(vec2u(ceil(span)), vec2u(TAP_MAX_LIT, TAP_MAX_LIT));
    let step = span / vec2f(taps);
    var sum = vec4f(0.0);
    let last = vec2i(src_size) - vec2i(1);
    for (var y = 0u; y < taps.y; y++) {
        for (var x = 0u; x < taps.x; x++) {
            let p = lo + (vec2f(f32(x), f32(y)) + vec2f(0.5)) * step;
            sum = sum + textureLoad(src, clamp(vec2i(p), vec2i(0), last), 0);
        }
    }
    return sum / f32(taps.x * taps.y);
}

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i & 2u) * 2 - 1);
    return vec4f(x, y, 0.0, 1.0);
}
@fragment fn tap(@builtin(position) at: vec4f) -> @location(0) vec4f {
    return boxed(at.xy);
}
// An 8-bit tap: the [0,1] window, which is the range a viewer clamps a colour to anyway, mapped
// by the target's own unorm conversion. `meta.reduced.depth` carries that window to the viewer.
@fragment fn tap8(@builtin(position) at: vec4f) -> @location(0) vec4f {
    return clamp(boxed(at.xy), vec4f(0.0), vec4f(1.0));
}
@fragment fn screen(@builtin(position) at: vec4f) -> @location(0) vec4f {
    // A screen is OPAQUE: two platforms of three drop the fourth byte and the third composites
    // it, so a shader's own alpha would show through on one of them.
    return vec4f(boxed(at.xy).rgb, 1.0);
}
";

/// The most taps one output texel averages per axis.
const TAP_MAX: u32 = 16;

pub struct Gpu {
    pub adapter: String,
    pub backend: String,
    pub sampler: wgpu::Sampler,
    /// What an unwired texture input reads: present, transparent, never an error.
    pub blank: wgpu::TextureView,
    /// One conversion pipeline per [`Want`], and the layout the source texture binds through.
    blits: [wgpu::RenderPipeline; 4],
    blit_group: wgpu::BindGroupLayout,
    group0: [wgpu::BindGroupLayout; 2],
    textures: Mutex<HashMap<usize, Arc<wgpu::BindGroupLayout>>>,
    layouts: Mutex<HashMap<(bool, usize, usize), Arc<wgpu::PipelineLayout>>>,
    pub queue: wgpu::Queue,
    /// LAST, here and in every struct that holds one: fields drop in declaration order, and a
    /// resource outliving its device is a driver crash rather than an error.
    pub device: wgpu::Device,
}

/// The ONE device this process renders on, opened at the first ask. Nothing destroys it.
pub fn shared() -> Result<Arc<Gpu>, String> {
    static ONE: OnceLock<Result<Arc<Gpu>, String>> = OnceLock::new();
    ONE.get_or_init(|| Gpu::open().map(Arc::new)).clone()
}

/// Held for EVERY operation on that device: a compile, a tick, a birth, a teardown. Lock order is
/// the runtime mutex, then this.
pub fn gate() -> std::sync::MutexGuard<'static, ()> {
    static GATE: Mutex<()> = Mutex::new(());
    GATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Give a value that owns GPU objects back, behind the gate.
pub fn give_back<T>(x: T) {
    let _gate = gate();
    drop(x);
}

impl Gpu {
    fn open() -> Result<Gpu, String> {
        // No display handle: this engine never opens a window, and asking for one would refuse
        // the device on a headless machine — a server, a CI runner — that has a GPU regardless.
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::PRIMARY;
        // `with_env` LAST, so `WGPU_BACKEND` overrides rather than is overridden — Windows is the
        // platform with two backends, and a broken ICD on one is a driver away from either.
        let instance = wgpu::Instance::new(desc.with_env());
        let ask = |fallback| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: fallback,
                compatible_surface: None,
                ..Default::default()
            }))
        };
        // The software adapter is a slow answer, never no answer — and on Windows it is WARP.
        let adapter = ask(false)
            .or_else(|refused| ask(true).map_err(|_| refused))
            .map_err(|e| format!("no GPU adapter answered: {e}"))?;
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("goofi-graphics"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("`{}` refused a device: {e}", info.name))?;
        device.on_uncaptured_error(Arc::new(|e| eprintln!("graphics: {e}")));

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("goofi-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blank = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("blank"),
            size: one_texel(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            blank.as_image_copy(),
            &[0u8; 8],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8), rows_per_image: None },
            one_texel(),
        );
        let blank = blank.create_view(&Default::default());

        let uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let sampler_entry = wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = |entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries })
        };
        let group0 = [
            layout(&[uniform(0), uniform(1), sampler_entry, uniform(4)]),
            layout(&[uniform(0), uniform(1), sampler_entry, uniform(3), uniform(4)]),
        ];
        let blit_group = layout(&[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    // `textureLoad` never filters, so this accepts every float format the engine has.
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            uniform(1),
        ]);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT.replace("TAP_MAX_LIT", &format!("{TAP_MAX}u")).into()),
        });
        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit"),
            bind_group_layouts: &[Some(&blit_group)],
            immediate_size: 0,
        });
        let blit = |entry: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("blit"),
                layout: Some(&blit_layout),
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
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let blits = [
            blit("screen", Want::Screen.format()),
            blit("tap", Want::Tap.format()),
            blit("tap8", Want::TapU8.format()),
            blit("tap8", Want::Record.format()),
        ];
        Ok(Gpu {
            device,
            queue,
            adapter: info.name.clone(),
            backend: info.backend.to_string(),
            sampler,
            blank,
            blits,
            blit_group,
            group0,
            textures: Mutex::new(HashMap::new()),
            layouts: Mutex::new(HashMap::new()),
        })
    }

    /// One conversion pass, encoded: `src` into `into`, in the format `want` reads and at the
    /// size `out_size` holds — which the caller owns, because the pass reads it on the device.
    pub fn blit(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        want: Want,
        src: &wgpu::TextureView,
        into: &wgpu::TextureView,
        out_size: &wgpu::Buffer,
    ) {
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.blit_group,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                wgpu::BindGroupEntry { binding: 1, resource: out_size.as_entire_binding() },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: into,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.blits[want as usize]);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Group 0 is the frame's own: time, the frame count, resolution, the sampler, and the params
    /// when there are any.
    pub fn group0(&self, params: bool) -> &wgpu::BindGroupLayout {
        &self.group0[usize::from(params)]
    }

    /// A group of sampled textures, one layout per count: group 1 is a stage's inputs and group 2
    /// its state buffers. Zero is an empty group, so every stage binds all three.
    pub fn textures(&self, count: usize) -> Arc<wgpu::BindGroupLayout> {
        self.textures
            .lock()
            .unwrap()
            .entry(count)
            .or_insert_with(|| {
                let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..count as u32)
                    .map(|binding| wgpu::BindGroupLayoutEntry {
                        binding,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    })
                    .collect();
                Arc::new(self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &entries,
                }))
            })
            .clone()
    }

    /// One group of sampled textures, bound: a stage's inputs, or the state its last tick left.
    pub fn texture_group(&self, views: &[wgpu::TextureView]) -> wgpu::BindGroup {
        let entries: Vec<wgpu::BindGroupEntry> = views
            .iter()
            .enumerate()
            .map(|(i, v)| wgpu::BindGroupEntry { binding: i as u32, resource: wgpu::BindingResource::TextureView(v) })
            .collect();
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.textures(views.len()),
            entries: &entries,
        })
    }

    pub fn layout(&self, params: bool, inputs: usize, state: usize) -> Arc<wgpu::PipelineLayout> {
        if let Some(held) = self.layouts.lock().unwrap().get(&(params, inputs, state)) {
            return held.clone();
        }
        let (group1, group2) = (self.textures(inputs), self.textures(state));
        let made = Arc::new(self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(self.group0(params)), Some(&group1), Some(&group2)],
            immediate_size: 0,
        }));
        self.layouts.lock().unwrap().insert((params, inputs, state), made.clone());
        made
    }
}

fn one_texel() -> wgpu::Extent3d {
    wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }
}

/// A texture the engine renders into and reads back from.
pub fn target(
    gpu: &Gpu,
    label: &str,
    (w, h): (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// A `copy_texture_to_buffer` row pitch: the GPU pads every row to 256 bytes.
pub fn padded_row(width: u32, texel: u32) -> u32 {
    (width * texel).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}
