//! GPU texture allocation, upload, state, and readback resources shared by every graphics stage.
use crate::gpu::{Gpu, Want, padded_row, target};
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
/// One texture the engine owns, with the size it was made for.
pub(crate) struct Target {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) size: (u32, u32),
}

/// One frame on its way off the GPU: the texture the blit converts into and the buffer the copy
/// lands in.
pub(crate) struct Slot {
    pub(crate) texture: Target,
    pub(crate) buffer: wgpu::Buffer,
    /// The map callback's: set once the bytes are there.
    pub(crate) ready: Arc<AtomicBool>,
    /// Patch seconds at the tick that DREW this frame. A readback is taken one or two ticks later,
    /// so an instant read where it is TAKEN is a tick period late against every other engine.
    pub(crate) at: f64,
}

/// What a reader's frames cost to make, and the size the source was when they were made — the
/// frame says so itself, because a viewer that reduced it must still know where a texel came from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shrunk {
    pub(crate) from: (u32, u32),
    pub(crate) to: (u32, u32),
}

/// The frames one reader has in flight. A copy started in one tick is taken a tick or two later,
/// so the render thread never waits on the device; a slot's LIST is its whole state, so there is
/// no flag to keep in step.
pub(crate) struct Ring {
    /// Nothing of theirs is on the device, so these are what a new copy goes into.
    pub(crate) free: Vec<Slot>,
    /// A copy is on the device or mapped, oldest first — so frames leave in the order asked for.
    pub(crate) flight: VecDeque<Slot>,
    /// What the last frames were copied into, handed back by whoever finished with them.
    pub(crate) spare: Spare,
    /// The destination size the pass reads, on the device.
    pub(crate) out_size: wgpu::Buffer,
    /// The source and target this ring was built for; a move in either remakes it.
    pub(crate) shrunk: Shrunk,
}

/// One node's GPU state, kept across plans so a topology edit costs no allocation.
pub(crate) struct State {
    pub(crate) submitted: Option<u64>,
    pub(crate) out: Option<Target>,
    /// One per [`Want`], sized with `out`; only a stage that reader watches has one.
    pub(crate) reads: [Option<Ring>; 4],
    /// One declared state buffer each: the texture the last tick left, and the one this tick
    /// writes. They swap after every render, which is the whole of how a node holds state.
    pub(crate) buffers: Vec<[Target; 2]>,
    /// How many times this node has rendered since those buffers were made — zero is a fresh
    /// state, which is how a body knows to seed itself.
    pub(crate) count: u32,
    pub(crate) uploads: Vec<Option<Target>>,
    /// The range each upload spanned, in the uniform's own order.
    pub(crate) ranges: Vec<[f32; 2]>,
    pub(crate) time: wgpu::Buffer,
    pub(crate) frame: wgpu::Buffer,
    pub(crate) resolution: wgpu::Buffer,
    pub(crate) params: Option<wgpu::Buffer>,
}

impl State {
    pub(crate) fn new(gpu: &Gpu, params: usize) -> Self {
        let uniform = |label: &str, size: u64| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        State {
            submitted: None,
            out: None,
            reads: [None, None, None, None],
            buffers: Vec::new(),
            count: 0,
            uploads: Vec::new(),
            time: uniform("time", 4),
            frame: uniform("frame", 4),
            resolution: uniform("resolution", 8),
            ranges: Vec::new(),
            params: (params > 0).then(|| uniform("params", params as u64)),
        }
    }
}
/// Buffers a reader has finished with, kept for the next frame. A fresh 30 MB allocation costs
/// four times the copy into it, because the kernel must zero every page.
pub(crate) type Spare = Arc<Mutex<Vec<Vec<u8>>>>;

/// Two is every buffer this path can have in hand at once; a third would only be held.
pub(crate) fn give_back(spare: &Spare, buffer: Vec<u8>) {
    let mut held = spare.lock().expect("the spare");
    if held.len() < 2 {
        held.push(buffer);
    }
}

/// The mapped rows into `bytes` as one tight frame, the 256-byte padding each row carries
/// dropped, and whether it holds one.
pub(crate) fn rows_into(bytes: &mut Vec<u8>, buffer: &wgpu::Buffer, (w, h): (u32, u32), want: Want) -> bool {
    let Ok(mapped) = buffer.slice(..).get_mapped_range() else { return false };
    let row = (w * want.texel()) as usize;
    let pitch = padded_row(w, want.texel()) as usize;
    bytes.resize(row * h as usize, 0);
    for y in 0..h as usize {
        match mapped.get(y * pitch..y * pitch + row) {
            Some(line) => bytes[y * row..(y + 1) * row].copy_from_slice(line),
            None => return false,
        }
    }
    true
}

impl Ring {
    fn new(gpu: &Gpu, want: Want, shrunk: Shrunk) -> Ring {
        let size = shrunk.to;
        let slots = (0..want.depth())
            .map(|_| {
                let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
                let texture = target(gpu, "readback", size, want.format(), usage);
                let view = texture.create_view(&Default::default());
                let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("readback"),
                    size: u64::from(padded_row(size.0, want.texel())) * u64::from(size.1),
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                Slot {
                    texture: Target { texture, view, size },
                    buffer,
                    ready: Arc::new(AtomicBool::new(false)),
                    at: 0.0,
                }
            })
            .collect();
        let out_size = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("out_size"),
            size: 8,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bytes = [(size.0 as f32).to_le_bytes(), (size.1 as f32).to_le_bytes()].concat();
        gpu.queue.write_buffer(&out_size, 0, &bytes);
        Ring { free: slots, flight: VecDeque::new(), spare: Spare::default(), out_size, shrunk }
    }
}

impl State {
    /// One tick done: what this tick wrote is what the next one reads.
    pub(crate) fn advance(&mut self) {
        for buffer in &mut self.buffers {
            buffer.swap(0, 1);
        }
        self.count = self.count.wrapping_add(1);
    }

    /// The output texture at `size`, the state buffers beside it, and one readback per reader
    /// that is there. All are remade when the size moves, which is what loses a feedback chain
    /// and a stateful node their history.
    pub(crate) fn ensure_out(
        &mut self,
        gpu: &Gpu,
        size: (u32, u32),
        wants: [Option<(u32, u32)>; 4],
        buffers: usize,
    ) -> Result<(), String> {
        // Validate every readback before allocating textures or changing the current rings.
        for w in Want::ALL {
            if let Some((width, height)) = wants[w as usize] {
                let bytes = u64::from(padded_row(width, w.texel())) * u64::from(height);
                if bytes > gpu.device.limits().max_buffer_size {
                    return Err(format!("readback: {width} by {height} exceeds the GPU buffer limit"));
                }
            }
        }
        if self.out.as_ref().is_none_or(|t| t.size != size) || self.buffers.len() != buffers {
            let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST;
            let texture = target(gpu, "out", size, crate::gpu::FORMAT, usage);
            let view = texture.create_view(&Default::default());
            self.out = Some(Target { texture, view, size });
            let fresh = || {
                let texture = target(gpu, "state", size, crate::gpu::FORMAT, usage);
                let view = texture.create_view(&Default::default());
                Target { texture, view, size }
            };
            self.buffers = (0..buffers).map(|_| [fresh(), fresh()]).collect();
            self.count = 0;
            self.submitted = None;
            self.reads = [None, None, None, None];
        }
        for w in Want::ALL {
            let held = &mut self.reads[w as usize];
            let want = wants[w as usize].map(|to| Shrunk { from: size, to });
            match (want, held.as_ref().map(|r| r.shrunk)) {
                (Some(a), Some(b)) if a == b => {}
                (Some(a), _) => *held = Some(Ring::new(gpu, w, a)),
                (None, _) => *held = None,
            }
        }
        Ok(())
    }

    pub(crate) fn submit_upload(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, up: &Upload) {
        let out = self.out.as_ref().expect("output allocated");
        if out.size == (up.width, up.height) {
            write_upload(gpu, &out.texture, up);
        } else {
            self.upload(gpu, 0, up);
            gpu.resize(
                encoder,
                &self.uploads[0].as_ref().unwrap().view,
                &self.out.as_ref().unwrap().view,
                &self.resolution,
            );
        }
    }

    /// One arrival into the texture its input samples.
    pub(crate) fn upload(&mut self, gpu: &Gpu, k: usize, up: &Upload) {
        if self.uploads.len() <= k {
            self.uploads.resize_with(k + 1, || None);
        }
        if self.ranges.len() <= k {
            self.ranges.resize(k + 1, [0.0, 1.0]);
        }
        self.ranges[k] = [up.lo, up.hi];
        let size = (up.width, up.height);
        if self.uploads[k].as_ref().is_none_or(|t| t.size != size) {
            let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
            let texture = target(gpu, "upload", size, crate::gpu::FORMAT, usage);
            let view = texture.create_view(&Default::default());
            self.uploads[k] = Some(Target { texture, view, size });
        }
        let held = self.uploads[k].as_ref().expect("just made");
        write_upload(gpu, &held.texture, up);
    }

    pub(crate) fn group0(&self, gpu: &Gpu) -> wgpu::BindGroup {
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: self.time.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: self.resolution.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&gpu.sampler) },
            wgpu::BindGroupEntry { binding: 4, resource: self.frame.as_entire_binding() },
        ];
        if let Some(p) = &self.params {
            entries.push(wgpu::BindGroupEntry { binding: 3, resource: p.as_entire_binding() });
        }
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: gpu.group0(self.params.is_some()),
            entries: &entries,
        })
    }
}

fn write_upload(gpu: &Gpu, texture: &wgpu::Texture, up: &Upload) {
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &up.texels,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(up.width * 8), rows_per_image: None },
        wgpu::Extent3d { width: up.width, height: up.height, depth_or_array_layers: 1 },
    );
}

use goofi_core::{Data, Value};
/// One arrival as the render thread takes it: `width * height * 4` f16 texels, row 0 the top,
/// and the range the frame's own values spanned — what a body needs to draw the frame to scale,
/// and the one thing a shader cannot work out for itself without reading every texel at every
/// texel.
pub struct Upload {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<u8>,
    pub lo: f32,
    pub hi: f32,
}

impl Upload {
    pub fn pixels(p: &goofi_core::texture::Pixels) -> Self {
        let channels = p.format.channels();
        let unit = p.format.bytes_per_channel();
        let mut texels = Vec::with_capacity(p.width as usize * p.height as usize * 8);
        for row in p.bytes.chunks_exact(p.stride) {
            for pixel in row[..p.width as usize * channels * unit].chunks_exact(channels * unit) {
                for channel in 0..4 {
                    let value = if channel >= channels {
                        1.0
                    } else if unit == 1 {
                        pixel[channel] as f32 / 255.0
                    } else {
                        f32::from_le_bytes(pixel[channel * 4..channel * 4 + 4].try_into().unwrap())
                    };
                    texels.extend(
                        half::f16::from_f32(if value.is_finite() { value } else { 0.0 }).to_bits().to_le_bytes(),
                    );
                }
            }
        }
        Self { width: p.width, height: p.height, texels, lo: 0.0, hi: 1.0 }
    }

    /// A frame as RGBA texels, unclamped. `[N]` is one row; `[H, W]` is gray; `[H, W, C]` fills
    /// the channels it has, with alpha 1 where it has none.
    pub fn of(frame: &Data) -> Option<Upload> {
        let Value::Array(a) = frame.value() else { return None };
        let (h, w, c) = match *a.shape() {
            [n] => (1, n, 1),
            [h, w] => (h, w, 1),
            [h, w, c] if (1..=4).contains(&c) => (h, w, c),
            _ => return None,
        };
        // A texture the device cannot make invalidates the whole frame's command buffer, so a
        // frame past the limit is no upload at all.
        if h == 0 || w == 0 || h > crate::plan::MAX_SIZE as usize || w > crate::plan::MAX_SIZE as usize {
            return None;
        }
        let x: Vec<f32> =
            a.as_bytes().chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four bytes"))).collect();
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for v in x.iter().filter(|v| v.is_finite()) {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
        let mut texels = Vec::with_capacity(h * w * 8);
        for i in 0..h * w {
            let s = &x[i * c..(i + 1) * c];
            let rgba = match c {
                1 => [s[0], s[0], s[0], 1.0],
                2 => [s[0], s[0], s[0], s[1]],
                3 => [s[0], s[1], s[2], 1.0],
                _ => [s[0], s[1], s[2], s[3]],
            };
            texels.extend(
                rgba.iter()
                    .flat_map(|v| half::f16::from_f32(if v.is_finite() { *v } else { 0.0 }).to_bits().to_le_bytes()),
            );
        }
        let (lo, hi) = if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) };
        Some(Upload { width: w as u32, height: h as u32, texels, lo, hi })
    }
}
