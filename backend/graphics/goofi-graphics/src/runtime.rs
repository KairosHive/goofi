//! What the render thread owns: the plan, one GPU state per live node, and the tick that draws
//! every demanded stage once and reads back the ones somebody is watching.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use goofi_core::{Data, Meta};
use goofi_record::{Kind, Recorder, StreamId, StreamMeta};
use goofi_node::Uid;

use crate::gpu::{padded_row, target, Gpu, Want};
use crate::half::{Tapped, Upload};
use crate::plan::{Input, Plan};
use crate::shader;

/// What the graph asks of the render thread. Applied at the top of a tick, so no op waits on one.
pub enum Cmd {
    Insert(Uid, usize),
    Remove(Uid),
    Plan(Plan),
    Ui(Option<goofi_window::Ui>),
    Recorder(Arc<Recorder>),
}

#[derive(Default)]
pub struct Stats {
    pub frames: AtomicU64,
    pub stages: AtomicU64,
    pub tick_max_us: AtomicU64,
}

/// One texture the engine owns, with the size it was made for.
struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
}

/// One frame on its way off the GPU: the texture the blit converts into and the buffer the copy
/// lands in.
struct Slot {
    texture: Target,
    buffer: wgpu::Buffer,
    /// The map callback's: set once the bytes are there.
    ready: Arc<AtomicBool>,
    /// Patch seconds at the tick that DREW this frame. A readback is taken one or two ticks later,
    /// so an instant read where it is TAKEN is a tick period late against every other engine.
    at: f64,
}

/// What a reader's frames cost to make, and the size the source was when they were made — the
/// frame says so itself, because a viewer that reduced it must still know where a texel came from.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Shrunk {
    from: (u32, u32),
    to: (u32, u32),
}

/// The frames one reader has in flight. A copy started in one tick is taken a tick or two later,
/// so the render thread never waits on the device; a slot's LIST is its whole state, so there is
/// no flag to keep in step.
struct Ring {
    /// Nothing of theirs is on the device, so these are what a new copy goes into.
    free: Vec<Slot>,
    /// A copy is on the device or mapped, oldest first — so frames leave in the order asked for.
    flight: VecDeque<Slot>,
    /// What the last frames were copied into, handed back by whoever finished with them.
    spare: Spare,
    /// The destination size the pass reads, on the device.
    out_size: wgpu::Buffer,
    /// The source and target this ring was built for; a move in either remakes it.
    shrunk: Shrunk,
}

/// One node's GPU state, kept across plans so a topology edit costs no allocation.
struct State {
    out: Option<Target>,
    /// One per [`Want`], sized with `out`; only a stage that reader watches has one.
    reads: [Option<Ring>; 4],
    /// One declared state buffer each: the texture the last tick left, and the one this tick
    /// writes. They swap after every render, which is the whole of how a node holds state.
    buffers: Vec<[Target; 2]>,
    /// How many times this node has rendered since those buffers were made — zero is a fresh
    /// state, which is how a body knows to seed itself.
    count: u32,
    uploads: Vec<Option<Target>>,
    /// The range each upload spanned, in the uniform's own order.
    ranges: Vec<[f32; 2]>,
    time: wgpu::Buffer,
    frame: wgpu::Buffer,
    resolution: wgpu::Buffer,
    params: Option<wgpu::Buffer>,
}

pub struct Runtime {
    plan: Plan,
    states: HashMap<Uid, State>,
    time: Arc<goofi_core::time::Time>,
    stats: Arc<Stats>,
    /// The window thread, where a stage with a window on the machine's screen sends its frame.
    pub ui: Option<goofi_window::Ui>,
    presenting: HashMap<goofi_window::Id, Arc<Present>>,
    /// The one recorder, and the video stream each armed stage has open on it.
    recorder: Option<Arc<Recorder>>,
    taping: HashMap<Uid, Tape>,
    /// What an armed stage the recorder could not open a stream for wears, until it is disarmed
    /// or the recording ends. The engine folds it into the faults it settles.
    troubles: Troubles,
    shared: Arc<goofi_control::Shared>,
    /// What the graph asked for since the last tick. An op appends here and never waits on a
    /// render: a tick is long, and a lock a render holds is a lock an op cannot have.
    pub inbox: Arc<Mutex<Vec<Cmd>>>,
    /// Last, for the reason [`Gpu`] states.
    gpu: Arc<Gpu>,
}

impl Runtime {
    pub fn new(
        gpu: Arc<Gpu>,
        time: Arc<goofi_core::time::Time>,
        stats: Arc<Stats>,
        troubles: Troubles,
        shared: Arc<goofi_control::Shared>,
    ) -> Runtime {
        Runtime {
            gpu,
            plan: Plan::default(),
            states: HashMap::new(),
            time,
            stats,
            ui: None,
            presenting: HashMap::new(),
            recorder: None,
            taping: HashMap::new(),
            troubles,
            shared,
            inbox: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// A birth's GPU state. `params` is the byte length of its uniform block, zero for a node
    /// that declares none.
    fn insert(&mut self, uid: Uid, params: usize) {
        let _gate = crate::gpu::gate();
        let uniform = |label: &str, size: u64| {
            self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let state = State {
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
        };
        self.states.insert(uid, state);
    }

    /// A node leaves, and the WHOLE plan goes with it: `Input::Stage` is an index into it. The
    /// settle that ends the batch builds the next one, and until it does this engine draws nothing.
    fn remove(&mut self, uid: Uid) {
        let _gate = crate::gpu::gate();
        self.states.remove(&uid);
        self.plan = Plan::default();
    }

    /// Every GPU object this engine holds, given back at once.
    pub fn clear(&mut self) {
        let _gate = crate::gpu::gate();
        self.plan = Plan::default();
        self.states.clear();
    }

    fn set_plan(&mut self, plan: Plan) {
        let _gate = crate::gpu::gate();
        self.plan = plan;
    }

    /// Everything the graph asked for since the last tick, applied on this thread — every GPU
    /// object this engine owns is made and unmade here.
    fn drain_inbox(&mut self) {
        let asked = std::mem::take(&mut *self.inbox.lock().expect("the inbox"));
        for cmd in asked {
            match cmd {
                Cmd::Insert(uid, params) => self.insert(uid, params),
                Cmd::Remove(uid) => self.remove(uid),
                Cmd::Plan(plan) => self.set_plan(plan),
                Cmd::Ui(ui) => self.ui = ui,
                Cmd::Recorder(r) => self.recorder = Some(r),
            }
        }
    }

    /// One tick: upload what arrived, write the uniforms, draw every demanded stage, and read
    /// back the ones with a reader.
    pub fn tick(&mut self) {
        self.drain_inbox();
        let began = Instant::now();
        let t = self.time.now();
        let recording = self.recorder.as_ref().is_some_and(|r| r.running());
        self.follow_record(recording, t);
        let want = self.plan.demanded(recording);
        if !want.contains(&true) {
            self.stats.frames.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let _gate = crate::gpu::gate();
        // BEFORE the take, because a map callback runs on a poll and nowhere else: polling only
        // after the submit below left every copy one whole tick older than it had to be.
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
        // What an earlier tick put on the device and the device has finished since.
        self.take();
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let mut started: Vec<(Uid, Want, Slot, (u32, u32))> = Vec::new();
        for (i, drawn) in want.iter().enumerate() {
            if !drawn {
                continue;
            }
            let stage = &self.plan.stages[i];
            let Some(Ok(pipeline)) = stage.pipeline.get() else { continue };
            let Some(state) = self.states.get_mut(&stage.uid) else { continue };
            state.ensure_out(&self.gpu, stage.size, stage.wants(recording), stage.state);
            let shrunk_from = stage.size;
            for (k, cell) in stage.uploads.iter().enumerate() {
                if let Some(up) = cell.lock().unwrap().take() {
                    state.upload(&self.gpu, k, &up);
                }
            }
            self.gpu.queue.write_buffer(&state.time, 0, &(t as f32).to_le_bytes());
            self.gpu.queue.write_buffer(&state.frame, 0, &state.count.to_le_bytes());
            let res = [(stage.size.0 as f32).to_le_bytes(), (stage.size.1 as f32).to_le_bytes()].concat();
            self.gpu.queue.write_buffer(&state.resolution, 0, &res);
            if let Some(buf) = &state.params {
                let bytes = shader::uniform_bytes(stage.decls, &stage.params, &state.ranges);
                self.gpu.queue.write_buffer(buf, 0, &bytes);
            }
            // Cloned handles, so reading another stage's output ends the borrow of `states`.
            let views: Vec<wgpu::TextureView> = stage
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
            let group0 = state.group0(&self.gpu);
            let group1 = self.gpu.texture_group(&views);
            let held: Vec<wgpu::TextureView> = state.buffers.iter().map(|b| b[0].view.clone()).collect();
            let group2 = self.gpu.texture_group(&held);
            // Cloned, so the readback below may take `states` mutably.
            let out_view = state.out.as_ref().expect("ensure_out made it").view.clone();
            // The output, then one target per state buffer — the order the prelude writes them in.
            let targets: Vec<wgpu::TextureView> =
                std::iter::once(out_view.clone()).chain(state.buffers.iter().map(|b| b[1].view.clone())).collect();
            {
                let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = targets
                    .iter()
                    .map(|view| {
                        Some(wgpu::RenderPassColorAttachment {
                            view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })
                    })
                    .collect();
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &group0, &[]);
                pass.set_bind_group(1, &group1, &[]);
                pass.set_bind_group(2, &group2, &[]);
                pass.draw(0..3, 0..1);
            }
            self.stats.stages.fetch_add(1, Ordering::Relaxed);
            self.states.get_mut(&stage.uid).expect("just borrowed").advance();
            for w in Want::ALL {
                // A stage the recorder could not open a stream for is read back for nobody.
                if w == Want::Record && !self.taping.get(&stage.uid).is_some_and(|t| t.live) {
                    continue;
                }
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
                started.push((stage.uid, w, slot, shrunk_from));
            }
        }
        self.gpu.queue.submit([encoder.finish()]);
        for (uid, w, mut slot, _from) in started {
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

    /// The video stream each armed stage should have open, opened, closed and counted to match
    /// settled state and the recorder's own. A stage whose size moved is a NEW file: a container
    /// holds one size, and a seam the file system shows beats one hidden inside a video.
    fn follow_record(&mut self, recording: bool, t: f64) {
        let Some(rec) = self.recorder.clone() else { return };
        let mut want: HashMap<Uid, (StreamId, (u32, u32))> = HashMap::new();
        if recording {
            for stage in &self.plan.stages {
                if let Some(r) = &stage.record {
                    let id = StreamId {
                        uid: stage.uid,
                        node: r.node.clone(),
                        slot: r.slot.clone(),
                        engine: "graphics",
                    };
                    want.insert(stage.uid, (id, stage.size));
                }
            }
        }
        for uid in self.taping.keys().copied().collect::<Vec<_>>() {
            let held = &self.taping[&uid];
            // The RECORDER owns whether a stream is open: a `record disarm` closes one behind the
            // render thread's back, and a tape kept over that would never open the next file.
            let kept = want.get(&uid).is_some_and(|(_, size)| *size == held.size)
                && (!held.live || rec.is_open(&held.id));
            if kept {
                if held.missed > 0 && held.said.elapsed() >= SAY_EVERY {
                    rec.dropped(&held.id, held.missed, t);
                    let held = self.taping.get_mut(&uid).expect("just read");
                    held.missed = 0;
                    held.said = Instant::now();
                }
                continue;
            }
            let why = match want.get(&uid) {
                Some((_, size)) if *size != held.size => "resized",
                Some(_) => "reopened",
                None if recording => "disarmed",
                None => "stopped",
            };
            let held = self.taping.remove(&uid).expect("just read");
            self.trouble(uid, None);
            if held.live {
                rec.close_later(&held.id, why, held.missed, t);
            }
        }
        for (uid, (id, size)) in want {
            // A stream the reaper has not finished closing is opened on a LATER tick: opening over
            // it would finalize the encoder on this thread, which is what `close_later` prevents.
            if self.taping.contains_key(&uid) || rec.is_open(&id) {
                continue;
            }
            let kind = Kind::Video { size, fps: f64::from(crate::FPS) };
            let live = match rec.open(&id, kind, t, StreamMeta::measured(Some(f64::from(crate::FPS)))) {
                Ok(()) => true,
                // A stream that will not open is this NODE's failure and nobody else's: the
                // recording keeps every other stream, and the entry the recorder filed says why.
                Err(why) => {
                    self.trouble(uid, Some(format!("not recorded: {why}")));
                    false
                }
            };
            self.taping.insert(uid, Tape { id, size, missed: 0, said: Instant::now(), live });
        }
    }

    /// Raise or clear what an armed stage wears, and ask for the settle that publishes it.
    fn trouble(&self, uid: Uid, why: Option<String>) {
        let mut held = self.troubles.lock().expect("the record troubles");
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
                let mut rows = ring.spare.lock().expect("the spare").pop().unwrap_or_default();
                let filled = rows_into(&mut rows, &slot.buffer, size, w);
                slot.buffer.unmap();
                let spare = ring.spare.clone();
                ring.free.push(slot);
                if !filled {
                    continue;
                }
                // Read off the stage and the borrow ended, so the recorder's own counters below
                // may take `self` mutably.
                let (window, tap_cell) = {
                    let stage = &self.plan.stages[i];
                    (stage.window, stage.tap.clone())
                };
                match w {
                    Want::Screen => {
                        if let (Some(id), Some(ui)) = (window, &self.ui) {
                            present(self.presenting.entry(id).or_default(), ui, id, size, rows, spare);
                        }
                    }
                    Want::Record => {
                        // A readback still in flight from the last size belongs to the file that
                        // size opened, which is closed — it is nobody's drop and nobody's frame.
                        let held = self.taping.get(&uid).filter(|tape| tape.live && tape.size == size);
                        if let Some((tape, rec)) = held.zip(self.recorder.as_ref()) {
                            let taken = rec.write_video(&tape.id, &rows, at);
                            self.taping.get_mut(&uid).expect("just read").missed += u64::from(!taken);
                        }
                        give_back(&spare, rows);
                    }
                    Want::Tap | Want::TapU8 => {
                        let shape = vec![size.1 as usize, size.0 as usize, 4];
                        // A frame that was shrunk on the way out says so, in the words
                        // `reduce_for_view` uses — otherwise it would understate its own origin
                        // and a viewer could not map a texel back to the pixel it came from.
                        let mut meta = Meta::new();
                        if shrunk.from != shrunk.to {
                            let area = goofi_view::ReduceMethod::Area;
                            let axes = [(0, shrunk.from.1 as usize, area), (1, shrunk.from.0 as usize, area)];
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
                            tap_cell.lock().expect("the tap").frame = Some(held);
                        }
                    }
                }
            }
        }
    }
}

/// One armed stage's open video stream: what the recorder files it under, the size it was opened
/// at, and the frames the encoder could not keep up with since the last time it was told.
struct Tape {
    id: StreamId,
    size: (u32, u32),
    missed: u64,
    said: Instant,
    /// Whether the recorder actually opened it. A stage that could not be encoded keeps its tape
    /// so nothing tries again every tick, and writes nothing.
    live: bool,
}

/// What each armed stage the recorder refused wears, shared with the engine that settles it.
pub type Troubles = Arc<Mutex<HashMap<Uid, String>>>;

/// How often a run of dropped frames reaches the manifest. Every drop is counted; a manifest
/// rewritten per frame would be the recording's own cost.
const SAY_EVERY: Duration = Duration::from_secs(1);

/// A frame as the screen takes it: its size, and its texels in the screen's own byte order.
type Frame = ((u32, u32), Vec<u8>);

/// One window's frame in flight.
#[derive(Default)]
struct Present {
    pending: Mutex<Option<Frame>>,
    posted: AtomicBool,
}

/// Hand the screen the newest frame, latest-wins, with at most ONE job outstanding. A job per
/// frame on an unbounded queue starved every other job the window thread had — an op among them,
/// because the clock always posted the next frame before the screen had finished the last. The
/// drawn buffer goes back to `spare` rather than being freed.
fn present(
    cell: &Arc<Present>,
    ui: &goofi_window::Ui,
    id: goofi_window::Id,
    size: (u32, u32),
    texels: Vec<u8>,
    spare: Spare,
) {
    if let Some((_, dropped)) = cell.pending.lock().expect("the pending frame").replace((size, texels)) {
        give_back(&spare, dropped);
    }
    if cell.posted.swap(true, Ordering::AcqRel) {
        return;
    }
    let cell = cell.clone();
    ui.post(move |host| {
        if let Some((size, texels)) = cell.pending.lock().expect("the pending frame").take() {
            host.present(id, size, &texels);
            give_back(&spare, texels);
        }
        // Cleared LAST, so the queue empties between two frames and the loop reaches its
        // other jobs. What arrived while this drew is picked up by the next tick's post.
        cell.posted.store(false, Ordering::Release);
    });
}

/// Buffers a reader has finished with, kept for the next frame. A fresh 30 MB allocation costs
/// four times the copy into it, because the kernel must zero every page.
type Spare = Arc<Mutex<Vec<Vec<u8>>>>;

/// Two is every buffer this path can have in hand at once; a third would only be held.
fn give_back(spare: &Spare, buffer: Vec<u8>) {
    let mut held = spare.lock().expect("the spare");
    if held.len() < 2 {
        held.push(buffer);
    }
}

/// The mapped rows into `bytes` as one tight frame, the 256-byte padding each row carries
/// dropped, and whether it holds one.
fn rows_into(bytes: &mut Vec<u8>, buffer: &wgpu::Buffer, (w, h): (u32, u32), want: Want) -> bool {
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
    fn advance(&mut self) {
        for buffer in &mut self.buffers {
            buffer.swap(0, 1);
        }
        self.count = self.count.wrapping_add(1);
    }

    /// The output texture at `size`, the state buffers beside it, and one readback per reader
    /// that is there. All are remade when the size moves, which is what loses a feedback chain
    /// and a stateful node their history.
    fn ensure_out(&mut self, gpu: &Gpu, size: (u32, u32), wants: [Option<(u32, u32)>; 4], buffers: usize) {
        if self.out.as_ref().is_none_or(|t| t.size != size) || self.buffers.len() != buffers {
            let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC;
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
    }

    /// One arrival into the texture its input samples.
    fn upload(&mut self, gpu: &Gpu, k: usize, up: &Upload) {
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
        let bytes: Vec<u8> = up.texels.iter().flat_map(|t| t.to_le_bytes()).collect();
        gpu.queue.write_texture(
            held.texture.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(up.width * 8), rows_per_image: None },
            wgpu::Extent3d { width: up.width, height: up.height, depth_or_array_layers: 1 },
        );
    }

    fn group0(&self, gpu: &Gpu) -> wgpu::BindGroup {
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

/// A stage's uniform block length, measured by the writer so there is one layout.
pub fn params_len(manifest: &goofi_node::NodeManifest) -> usize {
    let zeros: Vec<AtomicU64> = manifest.params.iter().map(|_| AtomicU64::new(0)).collect();
    let ranges = vec![[0.0, 1.0]; shader::array_inputs(manifest).count()];
    shader::uniform_bytes(manifest.params, &zeros, &ranges).len()
}
