//! What the render thread owns: the plan, one GPU state per live node, and the tick that draws
//! every demanded stage once and reads back the ones somebody is watching.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use goofi_supervisor::sync::Mutex;
use std::time::Instant;

use goofi_core::{Data, Meta};
use goofi_node::Uid;

use crate::gpu::{padded_row, Gpu, Want};
use crate::half::Tapped;
use crate::resources::{State, Slot, Spare, give_back, rows_into};
use crate::plan::{Input, Plan, Pass};
use crate::shader;

/// What the graph asks of the render thread. Applied at the top of a tick, so no op waits on one.
pub enum Cmd {
    Insert(Uid, usize),
    Remove(Uid),
    Plan(Plan),
    Ui(Option<goofi_window::Ui>),
}

#[derive(Default)]
pub struct Stats {
    pub frames: AtomicU64,
    pub stages: AtomicU64,
    pub tick_max_us: AtomicU64,
}

pub struct Runtime {
    plan: Plan,
    states: HashMap<Uid, State>,
    time: Arc<goofi_supervisor::timing::Time>,
    stats: Arc<Stats>,
    /// The window thread, where a stage with a window on the machine's screen sends its frame.
    pub ui: Option<goofi_window::Ui>,
    presenting: HashMap<goofi_window::Id, Arc<Present>>,
    /// The swap chain configured on each presented window's surface, at the size it was last
    /// drawn at.
    chains: HashMap<goofi_window::Id, Chain>,
    /// The rate the clock paces, as `graphics.fps` last settled: what a recording is stamped with.
    pub fps: Arc<AtomicU32>,
    /// Whether a recording runs, as the recorder says: what puts an armed stage in demand.
    recording: Arc<AtomicBool>,
    /// What a stage could not do, worn until it can. The engine folds it into the faults it settles.
    troubles: Troubles,
    shared: Arc<goofi_runtime::Shared>,
    /// What the graph asked for since the last tick. An op appends here and never waits on a
    /// render: a tick is long, and a lock a render holds is a lock an op cannot have.
    pub inbox: Arc<Mutex<Vec<Cmd>>>,
    /// Last, for the reason [`Gpu`] states.
    gpu: Arc<Gpu>,
}

impl Runtime {
    pub fn new(
        gpu: Arc<Gpu>,
        time: Arc<goofi_supervisor::timing::Time>,
        stats: Arc<Stats>,
        troubles: Troubles,
        shared: Arc<goofi_runtime::Shared>,
        recording: Arc<AtomicBool>,
        fps: Arc<AtomicU32>,
    ) -> Runtime {
        Runtime {
            gpu,
            plan: Plan::default(),
            states: HashMap::new(),
            time,
            stats,
            ui: None,
            presenting: HashMap::new(),
            chains: HashMap::new(),
            fps,
            recording,
            troubles,
            shared,
            inbox: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// A birth's GPU state. `params` is the byte length of its uniform block, zero for a node
    /// that declares none.
    fn insert(&mut self, uid: Uid, params: usize) {
        let state = State::new(&self.gpu, params);
        self.states.insert(uid, state);
    }

    /// A node leaves, and the WHOLE plan goes with it: `Input::Stage` is an index into it. The
    /// settle that ends the batch builds the next one, and until it does this engine draws nothing.
    fn remove(&mut self, uid: Uid) {
        self.states.remove(&uid);
        self.plan = Plan::default();
    }

    /// Every GPU object this engine holds, given back at once.
    pub fn clear(&mut self) {
        self.plan = Plan::default();
        self.states.clear();
        self.presenting.clear();
        self.chains.clear();
    }

    fn set_plan(&mut self, plan: Plan) {
        self.plan = plan;
        // A closed window's entry would hold its waiting frame for the engine's life.
        let live: HashSet<_> = self.plan.stages.iter().filter_map(|s| s.window).collect();
        self.presenting.retain(|id, _| live.contains(id));
        self.chains.retain(|id, _| live.contains(id));
    }

    /// The window's next frame to draw into, with its chain configured for `size`; none where the
    /// surface has nothing to give this tick. A chain outgrown, lost or stuck is made again next tick.
    fn acquire(&mut self, id: goofi_window::Id, surface: &Arc<wgpu::Surface<'static>>, size: (u32, u32)) -> Option<wgpu::SurfaceTexture> {
        if !self.chains.get(&id).is_some_and(|c| c.size == size && !c.stale) {
            match Chain::configure(&self.gpu, surface.clone(), size) {
                Ok(chain) => {
                    self.chains.insert(id, chain);
                }
                Err(why) => {
                    self.chains.remove(&id);
                    goofi_supervisor::log::record(goofi_supervisor::log::Source::component("graphics"), goofi_supervisor::log::Level::Error, None, format!("graphics: the window's surface refused {}x{}: {why}", size.0, size.1));
                    return None;
                }
            }
        }
        use wgpu::CurrentSurfaceTexture as At;
        match surface.get_current_texture() {
            At::Success(frame) => Some(frame),
            At::Suboptimal(frame) => {
                self.chains.get_mut(&id).expect("just made").stale = true;
                Some(frame)
            }
            At::Occluded => None,
            At::Timeout | At::Outdated | At::Lost | At::Validation => {
                self.chains.remove(&id);
                None
            }
        }
    }

    /// Everything the graph asked for since the last tick, applied on this thread — every GPU
    /// object this engine owns is made and unmade here.
    fn drain_inbox(&mut self) {
        let asked = std::mem::take(&mut *self.inbox.lock());
        for cmd in asked {
            match cmd {
                Cmd::Insert(uid, params) => self.insert(uid, params),
                Cmd::Remove(uid) => self.remove(uid),
                Cmd::Plan(plan) => self.set_plan(plan),
                Cmd::Ui(ui) => self.ui = ui,
            }
        }
    }

    /// Wait for the device to draw what the ticks so far submitted: what makes a tick under the
    /// external clock mean one drawn frame, where the timer clock lets the device run behind.
    pub fn finish(&mut self) {
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// One tick: upload what arrived, write the uniforms, draw every demanded stage, and read
    /// back the ones with a reader.
    pub fn tick(&mut self) {
        self.drain_inbox();
        let began = Instant::now();
        let t = self.time.now();
        let recording = self.recording.load(Ordering::Relaxed);
        let want = self.plan.demanded(recording);
        if !want.contains(&true) {
            self.stats.frames.fetch_add(1, Ordering::Relaxed);
            return;
        }
        // BEFORE the take, because a map callback runs on a poll and nowhere else: polling only
        // after the submit below left every copy one whole tick older than it had to be.
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
        // What an earlier tick put on the device and the device has finished since.
        self.take();
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let mut started: Vec<(Uid, Want, Slot)> = Vec::new();
        let mut shown: Vec<(goofi_window::Id, wgpu::SurfaceTexture)> = Vec::new();
        for (i, drawn) in want.iter().enumerate() {
            if !drawn {
                continue;
            }
            let stage = &self.plan.stages[i];
            let produced = match &stage.pass {
                Pass::Host { source, program } => source.lock().clone().filter(|frame| {
                    let matches = match (&frame.content, program) {
                        (crate::producer::Content::Pixels(_), None) => true,
                        (crate::producer::Content::Render(text), Some((compiled, _))) => text == compiled,
                        _ => false,
                    };
                    matches && (!stage.natural.0 || frame.size.0 == stage.size.0)
                        && (!stage.natural.1 || frame.size.1 == stage.size.1)
                }),
                Pass::Shader(_) => None,
            };
            let built = match (&stage.pass, &produced) {
                (Pass::Shader(built), _) => Some(built),
                (Pass::Host { program: Some((_, built)), .. }, Some(crate::producer::Produced { content: crate::producer::Content::Render(_), .. })) => Some(built),
                _ => None,
            };
            let pipeline = match built {
                Some(built) => match built.peek() { Some(Ok(p)) => Some(p), _ => continue },
                None => None,
            };
            let Some(state) = self.states.get_mut(&stage.uid) else { continue };
            if let Err(error) = state.ensure_out(&self.gpu, stage.size, stage.wants(recording), stage.state) {
                self.trouble(stage.uid, Some(error));
                continue;
            }
            let clear = self.troubles.lock()
                .get(&stage.uid).is_some_and(|why| why.starts_with("readback:"));
            if clear { self.trouble(stage.uid, None); }
            let Some(state) = self.states.get_mut(&stage.uid) else { continue };
            for (k, cell) in stage.cells.uploads.iter().enumerate() {
                if let Some(up) = cell.lock().take() {
                    state.upload(&self.gpu, k, &up);
                }
            }
            self.gpu.queue.write_buffer(&state.time, 0, &(t as f32).to_le_bytes());
            self.gpu.queue.write_buffer(&state.frame, 0, &state.count.to_le_bytes());
            let res = [(stage.size.0 as f32).to_le_bytes(), (stage.size.1 as f32).to_le_bytes()].concat();
            self.gpu.queue.write_buffer(&state.resolution, 0, &res);
            if let Some(buf) = &state.params {
                let bytes = shader::uniform_bytes(stage.decls, &stage.cells.params, &state.ranges);
                self.gpu.queue.write_buffer(buf, 0, &bytes);
            }
            if let Some(produced) = &produced {
                if state.submitted != Some(produced.index) {
                    if let crate::producer::Content::Pixels(pixels) = &produced.content {
                        state.submit_upload(&self.gpu, &mut encoder, pixels);
                        state.submitted = Some(produced.index);
                    }
                }
            }
            if let Some(pipeline) = pipeline {
                // Cloned handles, so reading another stage's output ends the borrow of `states`.
                let mut views: Vec<wgpu::TextureView> = stage
                    .inputs
                    .iter()
                    .map(|input| match input {
                        Input::Stage(j) => self
                            .states
                            .get(&self.plan.stages[*j].uid)
                            .and_then(|s| s.out.as_ref())
                            .map_or(&self.gpu.blank, |t| &t.view),
                        Input::Upload(k) => self.states[&stage.uid]
                            .uploads
                            .get(*k)
                            .and_then(|u| u.as_ref())
                            .map_or(&self.gpu.blank, |t| &t.view),
                        Input::None => &self.gpu.blank,
                    })
                    .cloned()
                    .collect();
                let state = &self.states[&stage.uid];
                // After the inputs, one head layout per ARRAY input, in the prelude's order.
                views.extend((0..stage.cells.uploads.len()).map(|k| {
                    state.layouts.get(k).and_then(|t| t.as_ref()).map_or(&self.gpu.blank, |t| &t.view).clone()
                }));
                let group0 = state.group0(&self.gpu);
                let group1 = self.gpu.texture_group(&views);
                let held: Vec<wgpu::TextureView> = state.buffers.iter().map(|b| b[0].view.clone()).collect();
                let group2 = self.gpu.texture_group(&held);
                // Cloned, so the readback below may take `states` mutably.
                let out_view = state.out.as_ref().expect("ensure_out made it").view.clone();
                // The output, then one target per state buffer — the order the prelude writes them in.
                let targets: Vec<wgpu::TextureView> =
                    std::iter::once(out_view.clone()).chain(state.buffers.iter().map(|b| b[1].view.clone())).collect();
                let mut pass = crate::gpu::clear_pass(&mut encoder, &targets);
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group0, &[]);
                pass.set_bind_group(1, &group1, &[]);
                pass.set_bind_group(2, &group2, &[]);
                pass.draw(0..3, 0..1);
            }
            let out_view = self.states[&stage.uid].out.as_ref().expect("output allocated").view.clone();
            self.stats.stages.fetch_add(1, Ordering::Relaxed);
            self.states.get_mut(&stage.uid).expect("just borrowed").advance();
            // Straight into the window's own swap chain: no readback, no copy, no X server.
            if let (Some(id), Some(surface)) = (stage.window, stage.surface.clone()) {
                let size = stage.size;
                if let Some(frame) = self.acquire(id, &surface, size) {
                    let chain = &self.chains[&id];
                    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(chain.format), ..Default::default() });
                    self.gpu.blit_with(&mut encoder, &chain.pipeline, &out_view, &view, &chain.out_size);
                    shown.push((id, frame));
                }
            }
            let stage = &self.plan.stages[i];
            for w in Want::ALL {
                let state = self.states.get_mut(&stage.uid).expect("just borrowed");
                let Some(ring) = state.reads[w as usize].as_mut() else { continue };
                let out_size = ring.out_size.clone();
                let Some(slot) = ring.free.pop() else { continue };
                let (width, height) = slot.texture.size;
                self.gpu.blit(&mut encoder, w, &out_view, &slot.texture.view, &out_size);
                encoder.copy_texture_to_buffer(
                    slot.texture.texture.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &slot.buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(padded_row(width, w.texel())),
                            rows_per_image: None,
                        },
                    },
                    wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                );
                started.push((stage.uid, w, slot));
            }
        }
        self.gpu.queue.submit([encoder.finish()]);
        for (id, frame) in shown {
            self.gpu.queue.present(frame);
            if let Some(ui) = &self.ui {
                let cell = self.presenting.entry(id).or_default();
                cell.drawn.fetch_add(1, Ordering::Relaxed);
                post(cell, ui, id);
            }
        }
        for (uid, w, mut slot) in started {
            slot.at = t;
            let ready = slot.ready.clone();
            slot.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                ready.store(r.is_ok(), Ordering::Release);
            });
            if let Some(ring) = self.states.get_mut(&uid).and_then(|s| s.reads[w as usize].as_mut()) {
                ring.flight.push_back(slot);
            }
        }
        self.stats.frames.fetch_add(1, Ordering::Relaxed);
        self.stats.tick_max_us.fetch_max(began.elapsed().as_micros() as u64, Ordering::Relaxed);
    }

    /// Raise or clear what an armed stage wears, and ask for the settle that publishes it.
    fn trouble(&self, uid: Uid, why: Option<String>) {
        let mut held = self.troubles.lock();
        let was = match why {
            Some(why) => held.insert(uid, why),
            None => held.remove(&uid),
        };
        if was != held.get(&uid).cloned() {
            self.shared.replan.store(true, Ordering::Release);
            self.shared.waker.notify();
        }
    }

    /// Every frame the device has finished, given to the reader that asked for it. The GPU wrote
    /// that reader's own format, so this is a copy of rows and never a conversion.
    fn take(&mut self) {
        for i in 0..self.plan.stages.len() {
            for w in Want::ALL {
                let uid = self.plan.stages[i].uid;
                let Some(ring) = self.states.get_mut(&uid).and_then(|s| s.reads[w as usize].as_mut()) else {
                    continue;
                };
                if !ring.flight.front().is_some_and(|slot| slot.ready.load(Ordering::Acquire)) {
                    continue;
                }
                let slot = ring.flight.pop_front().expect("the front was ready");
                slot.ready.store(false, Ordering::Relaxed);
                let (size, at) = (slot.texture.size, slot.at);
                let shrunk = ring.shrunk;
                let mut rows = ring.spare.lock().pop().unwrap_or_default();
                let filled = rows_into(&mut rows, &slot.buffer, size, w);
                slot.buffer.unmap();
                let spare = ring.spare.clone();
                ring.free.push(slot);
                if !filled {
                    continue;
                }
                // Read off the stage and the borrow ended, so the recorder's own counters below
                // may take `self` mutably.
                let (window, cells) = {
                    let stage = &self.plan.stages[i];
                    (stage.window, stage.cells.clone())
                };
                match w {
                    Want::Screen => {
                        if let (Some(id), Some(ui)) = (window, &self.ui) {
                            present(self.presenting.entry(id).or_default(), ui, id, size, rows, spare);
                        }
                    }
                    Want::Record => {
                        // Numbered here, so a readback the half or the queue let go leaves a gap
                        // the recorder counts; dated by the tick that drew it.
                        let state = self.states.get_mut(&uid).expect("just borrowed");
                        state.recorded += 1;
                        let mut meta = Meta::new().with_sfreq(Some(f64::from(self.fps.load(Ordering::Relaxed)))).with_index(Some(state.recorded));
                        meta.set_time(Some(at));
                        let texels = Arc::new(goofi_core::record::Texels { width: size.0, height: size.1, rgba: rows, meta });
                        let mut held = cells.record.lock();
                        if held.len() >= crate::half::RECORD_HELD {
                            held.pop_front();
                        }
                        held.push_back(texels);
                    }
                    Want::Tap | Want::TapU8 => {
                        let shape = vec![size.1 as usize, size.0 as usize, 4];
                        // A frame that was shrunk on the way out says so, in the words
                        // `reduce_for_view` uses — otherwise it would understate its own origin
                        // and a viewer could not map a texel back to the pixel it came from.
                        let mut meta = Meta::new();
                        if shrunk.from != shrunk.to {
                            let axes = [(0, shrunk.from.1 as usize), (1, shrunk.from.0 as usize)];
                            goofi_core::reduce::note_reduced(&mut meta, &axes);
                        }
                        let held = if w == Want::TapU8 {
                            // The window the GPU clamped to, said the way `quantize_u8` says it,
                            // so a viewer maps a texel back through the one rule.
                            goofi_core::reduce::note_depth(&mut meta, 0.0, 1.0);
                            Some(Tapped::Texels { shape, bytes: rows, meta })
                        } else {
                            Data::array_f32(shape, rows, meta).ok().map(Tapped::Full)
                        };
                        if let Some(held) = held {
                            *cells.tap.lock() = Some(held);
                        }
                    }
                }
            }
        }
    }
}

/// What each stage the render thread refused wears, shared with the engine that settles it.
pub type Troubles = Arc<Mutex<HashMap<Uid, String>>>;

/// A frame as the screen takes it: its size, its texels in the screen's own byte order, and
/// where the buffer goes back to once drawn.
type Frame = ((u32, u32), Vec<u8>, Spare);

/// One window's frame in flight, or the count of frames its surface showed since the window
/// thread last heard.
#[derive(Default)]
struct Present {
    pending: Mutex<Option<Frame>>,
    drawn: AtomicU64,
    posted: AtomicBool,
}

/// One swap chain: a window's surface configured at one size, and the pass into its format.
struct Chain {
    _surface: Arc<wgpu::Surface<'static>>,
    size: (u32, u32),
    /// The format the blit writes: the surface's own, less any sRGB suffix, so a linear value
    /// lands as the readback path lands it.
    format: wgpu::TextureFormat,
    pipeline: Arc<wgpu::RenderPipeline>,
    out_size: wgpu::Buffer,
    /// The surface said its last frame was suboptimal: configured again before the next.
    stale: bool,
}

impl Chain {
    /// Configure `surface` for `size`: a non-sRGB format where one is offered, and no
    /// transparency. A configuration the device refuses is this window's failure, said once.
    fn configure(gpu: &Gpu, surface: Arc<wgpu::Surface<'static>>, size: (u32, u32)) -> Result<Chain, String> {
        let caps = surface.get_capabilities(&gpu.adapter);
        let native = caps.formats.iter().copied().find(|f| !f.is_srgb()).or(caps.formats.first().copied()).unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        let format = native.remove_srgb_suffix();
        // The clock never waits on a vblank: a mode that queues or replaces, never one that
        // blocks the acquire until the display takes the last frame.
        let present_mode = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate, wgpu::PresentMode::Fifo]
            .into_iter()
            .find(|m| caps.present_modes.contains(m))
            .unwrap_or_default();
        let alpha_mode = [wgpu::CompositeAlphaMode::Opaque, wgpu::CompositeAlphaMode::Auto].into_iter().find(|m| caps.alpha_modes.contains(m)).unwrap_or_default();
        let refused = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(&gpu.device, &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: native,
            color_space: Default::default(),
            width: size.0.max(1),
            height: size.1.max(1),
            present_mode,
            desired_maximum_frame_latency: 1,
            alpha_mode,
            view_formats: if format == native { vec![] } else { vec![format] },
        });
        if let Some(refused) = pollster::block_on(refused.pop()) {
            return Err(refused.to_string());
        }
        let out_size = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen size"),
            size: 8,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&out_size, 0, &[(size.0 as f32).to_le_bytes(), (size.1 as f32).to_le_bytes()].concat());
        Ok(Chain { pipeline: gpu.screen_blit(format), _surface: surface, size, format, out_size, stale: false })
    }
}

/// Hand the screen the newest frame, latest-wins. The drawn buffer goes back to `spare` rather
/// than being freed.
fn present(
    cell: &Arc<Present>,
    ui: &goofi_window::Ui,
    id: goofi_window::Id,
    size: (u32, u32),
    texels: Vec<u8>,
    spare: Spare,
) {
    if let Some((_, dropped, spare)) = cell.pending.lock().replace((size, texels, spare)) {
        give_back(&spare, dropped);
    }
    post(cell, ui, id);
}

/// Tell the window thread what the cell holds, with at most ONE job outstanding: a job per frame
/// on an unbounded queue starved every other job the window thread had, an op among them.
fn post(cell: &Arc<Present>, ui: &goofi_window::Ui, id: goofi_window::Id) {
    if cell.posted.swap(true, Ordering::AcqRel) {
        return;
    }
    let cell = cell.clone();
    ui.post(move |host| {
        if let Some((size, texels, spare)) = cell.pending.lock().take() {
            host.present(id, size, &texels);
            give_back(&spare, texels);
        }
        let drawn = cell.drawn.swap(0, Ordering::Relaxed);
        if drawn > 0 {
            host.presented(id, drawn);
        }
        // Cleared LAST, so the queue empties between two frames and the loop reaches its
        // other jobs. What arrived while this ran is picked up by the next tick's post.
        cell.posted.store(false, Ordering::Release);
    });
}

/// A stage's uniform block length, measured by the writer so there is one layout.
pub fn params_len(manifest: &goofi_node::NodeManifest) -> usize {
    let zeros = goofi_runtime::cells_of(&goofi_node::ParamGroups::new(), manifest.params);
    let ranges = vec![[0.0, 1.0, 1.0]; shader::array_inputs(manifest).count()];
    shader::uniform_bytes(manifest.params, &zeros, &ranges).len()
}
