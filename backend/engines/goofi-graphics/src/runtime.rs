//! What the render thread owns: the plan, one GPU state per live node, and the tick that draws
//! every demanded stage once and reads back the ones somebody is watching.
//!
//! Nothing the tick touches is held by another thread. What the graph asks arrives on a channel;
//! what a node's half uploads, and what the tick reads back for it, cross in cells with no lock on
//! either side; a readback travels to its reader as the mapped buffer it is, and the copy, the
//! encode and the free are that reader's. A tick is a tick's own work and no more, however many
//! viewers watch.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use goofi_supervisor::sync::{Mailbox, Mutex};
use goofi_supervisor::timing::Pace;
use std::time::{Duration, Instant};

use goofi_core::node::Uid;

use crate::gpu::{padded_row, Gpu, Want};
use crate::half::Cells;
use crate::resources::{Readback, Slot, State, FAILED, PENDING, READY};
use crate::plan::{Input, Pass, Plan};
use crate::resources::Made;
use goofi_core::texture::Texture;
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
    /// Whether the clock waits at its door, with nothing to draw.
    pub parked: AtomicBool,
}

/// What each stage the render thread refused wears, for the engine that settles it: the WHOLE
/// current set, put whenever it moves, so a settle reads one snapshot and the tick holds no lock.
pub type Troubles = Arc<Mailbox<HashMap<Uid, String>>>;

/// What the render thread and the engine read of each other: counters, flags and one cell each
/// way, nothing either waits on.
pub struct Links {
    pub stats: Arc<Stats>,
    pub troubles: Troubles,
    pub shared: Arc<goofi_runtime::Shared>,
    /// Whether a recording runs, as the recorder says: what puts an armed stage in demand.
    pub recording: Arc<AtomicBool>,
    /// The rate the clock paces, as `graphics.fps` last settled: what a recording is stamped with.
    pub fps: Arc<AtomicU32>,
    /// The fastest a viewer is served, as `system.viewer_fps` last settled: the pace a tap is
    /// read back at. A frame downloaded between two serves is one the reducer drops.
    pub viewer_fps: Arc<AtomicU32>,
}

/// How many stages one tick times: a begin and an end query each, of the most a set holds.
const TIMED_MAX: usize = (wgpu::QUERY_SET_MAX_QUERIES / 2) as usize;

/// How far one timed pass moves a stage's share: the readout is a mean, not one tick's pass.
const SHARE_ALPHA: f64 = 0.1;

/// The pass timer: a begin and an end timestamp per drawn stage, resolved in the tick's encoder
/// and read a tick or two later. One readout is in flight; a tick that finds it busy times nothing.
struct Timer {
    set: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    read: wgpu::Buffer,
    /// The stages the readout in flight timed, in query order; empty while the timer is free.
    flight: Vec<Arc<Cells>>,
    ready: Arc<AtomicU8>,
}

impl Timer {
    /// None on an adapter that keeps no timestamps: nothing is timed and no share is shown.
    fn new(gpu: &Gpu) -> Option<Timer> {
        if !gpu.device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let size = u64::from(wgpu::QUERY_SET_MAX_QUERIES) * 8;
        let buffer = |label, usage| gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false });
        Some(Timer {
            set: gpu.device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("pass times"), ty: wgpu::QueryType::Timestamp, count: wgpu::QUERY_SET_MAX_QUERIES }),
            resolved: buffer("pass times resolved", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            read: buffer("pass times read", wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ),
            flight: Vec::new(),
            ready: Arc::new(AtomicU8::new(PENDING)),
        })
    }

    /// The bytes `stages` timed passes resolve to.
    fn len(stages: usize) -> u64 {
        stages as u64 * 16
    }

    /// Resolve the passes this tick timed, into the buffer [`Self::send`] maps after the submit.
    fn resolve(&self, encoder: &mut wgpu::CommandEncoder, stages: usize) {
        encoder.resolve_query_set(&self.set, 0..2 * stages as u32, &self.resolved, 0);
        encoder.copy_buffer_to_buffer(&self.resolved, 0, &self.read, 0, Timer::len(stages));
    }

    fn send(&mut self, timed: Vec<Arc<Cells>>) {
        self.ready.store(PENDING, Ordering::Relaxed);
        let ready = self.ready.clone();
        self.read.slice(..Timer::len(timed.len())).map_async(wgpu::MapMode::Read, move |r| {
            ready.store(if r.is_ok() { READY } else { FAILED }, Ordering::Release);
        });
        self.flight = timed;
    }

    /// The readout the device finished, written to each stage as its share of a frame at `fps`.
    /// False when the device refused the map: the timer is of no more use.
    fn collect(&mut self, nanos_per_tick: f64, fps: f64) -> bool {
        if self.flight.is_empty() {
            return true;
        }
        match self.ready.load(Ordering::Acquire) {
            PENDING => return true,
            READY => {}
            _ => return false,
        }
        let Ok(times) = self.read.slice(..Timer::len(self.flight.len())).get_mapped_range() else { return false };
        for (cells, pair) in self.flight.iter().zip(times.chunks_exact(16)) {
            let stamp = |at: usize| u64::from_le_bytes(pair[at..at + 8].try_into().expect("eight bytes"));
            // A pair the device left unordered measured nothing.
            let Some(ticks) = stamp(8).checked_sub(stamp(0)) else { continue };
            note_share(cells, ticks as f64 * nanos_per_tick * 1e-9 * fps);
        }
        drop(times);
        self.read.unmap();
        self.flight.clear();
        true
    }
}

/// One more measured turn of a stage, as a share of the frame period, into its smoothed readout.
fn note_share(cells: &Cells, share: f64) {
    let held = f64::from_bits(cells.frame_share.load(Ordering::Relaxed));
    let next = if held > 0.0 { held + (share - held) * SHARE_ALPHA } else { share };
    cells.frame_share.store(next.to_bits(), Ordering::Relaxed);
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
    viewer_fps: Arc<AtomicU32>,
    /// The pace every tap is read back at, the viewers' rate: one pace, so every watched stage's
    /// frame lands on the same tick and a page paints them once.
    taps: Pace,
    /// Whether a recording runs, as the recorder says: what puts an armed stage in demand.
    recording: Arc<AtomicBool>,
    /// What a stage could not do, worn until it can: this thread's own copy, and the cell the
    /// engine takes a snapshot of to fold into the faults it settles.
    troubles: HashMap<Uid, String>,
    troubles_out: Troubles,
    shared: Arc<goofi_runtime::Shared>,
    /// What the graph asked for since the last tick. An op sends here and never waits on a
    /// render: a tick is long, and a lock a render holds is a lock an op cannot have.
    inbox: Receiver<Cmd>,
    timer: Option<Timer>,
    /// Last, for the reason [`Gpu`] states.
    gpu: Arc<Gpu>,
}

impl Runtime {
    pub fn new(gpu: Arc<Gpu>, time: Arc<goofi_supervisor::timing::Time>, links: Links, inbox: Receiver<Cmd>) -> Runtime {
        let Links { stats, troubles, shared, recording, fps, viewer_fps } = links;
        Runtime {
            timer: Timer::new(&gpu),
            gpu,
            plan: Plan::default(),
            states: HashMap::new(),
            time,
            stats,
            ui: None,
            presenting: HashMap::new(),
            chains: HashMap::new(),
            fps,
            viewer_fps,
            taps: Pace::new(Duration::ZERO),
            recording,
            troubles: HashMap::new(),
            troubles_out: troubles,
            shared,
            inbox,
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
        self.trouble(uid, None);
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
        while let Ok(cmd) = self.inbox.try_recv() {
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

    /// The pass's pipeline: the one the stage keeps, else the one its build answered, kept from
    /// here on. `None` while the build runs or once it refused — a stage that is not drawn.
    fn pipeline(&mut self, i: usize) -> Option<Arc<wgpu::RenderPipeline>> {
        let stage = &mut self.plan.stages[i];
        if let Some(held) = &stage.pipeline {
            return Some(held.clone());
        }
        let Pass::Shader(built) = &stage.pass else { return None };
        let Some(Ok(pipeline)) = built.peek() else { return None };
        stage.pipeline = Some(pipeline.clone());
        Some(pipeline)
    }

    /// A node's turn in the tick. What the stages before it drew goes to the device and is read
    /// back; the node's half runs it while this thread waits; the picture it made is uploaded as
    /// the stage's output. False while the stage has no output to show.
    fn call(&mut self, i: usize, encoder: &mut wgpu::CommandEncoder, wants: [Option<(u32, u32)>; 4], t: f64) -> bool {
        let began = Instant::now();
        let (uid, cells) = (self.plan.stages[i].uid, self.plan.stages[i].cells.clone());
        let shows = |states: &HashMap<Uid, State>| states.get(&uid).is_some_and(|state| state.out.is_some());
        let Some(meeting) = cells.node.as_ref().filter(|meeting| meeting.ready.load(Ordering::Acquire)) else {
            return shows(&self.states);
        };
        // The child joins where its CUDA device is this device: its pictures then stay there. A
        // child that started again joins again, and the buffers are made for it anew.
        if let (Some(door), Some(state)) = (&meeting.door, self.states.get_mut(&uid)) {
            if let Some(joined) = crate::share::Link::accept(door) {
                state.link = Some(joined);
                meeting.linked.store(true, Ordering::Relaxed);
            }
        }
        let stamped = |texture: Texture| {
            let mut meta = goofi_core::Meta::new();
            meta.set_procstart(Some(t));
            meta.set_procend(Some(t));
            goofi_core::Data::texture(texture, meta)
        };
        let mut inputs: crate::half::Asked = Vec::new();
        let mut copies: Vec<(wgpu::Buffer, (u32, u32))> = Vec::new();
        let mut copied: Vec<usize> = Vec::new();
        // What went wrong in this turn, and whether it was the link that failed.
        let (mut wrong, mut broken): (Option<String>, bool) = (None, false);
        for (slot, input) in self.plan.stages[i].inputs.iter().enumerate() {
            let behind = match input {
                Input::Stage(j) => self.states.get(&self.plan.stages[*j].uid).and_then(|state| state.out.as_ref()).map(|out| (out.texture.clone(), out.size)),
                Input::None => None,
                // A frame off another engine's wire is the half's already.
                Input::Upload(_) => continue,
            };
            let (Some((texture, size)), Some(state)) = (behind, self.states.get_mut(&uid)) else {
                inputs.push((slot, None));
                continue;
            };
            // Into memory the child maps, where the two share; else into a buffer this thread maps.
            match state.link.as_mut().map(|link| link.input(&self.gpu, slot, size)) {
                Some(Ok((buffer, shared))) => {
                    crate::resources::copy_out(encoder, &texture, &buffer, size);
                    inputs.push((slot, stamped(Texture::Shared(shared)).ok()));
                }
                Some(Err(why)) => {
                    (wrong, broken) = (Some(why), true);
                    break;
                }
                None => {
                    let buffer = state.download(&self.gpu, slot, size);
                    crate::resources::copy_out(encoder, &texture, &buffer, size);
                    copies.push((buffer, size));
                    copied.push(slot);
                }
            }
        }
        let linked = self.states.get(&uid).is_some_and(|state| state.link.is_some());
        if wrong.is_none() && (linked || !copies.is_empty()) {
            // The node reads what is drawn, so everything encoded so far goes to the device now.
            // A child that shares writes where the last tick's copy read, so that is done first.
            let drawn = std::mem::replace(encoder, self.gpu.device.create_command_encoder(&Default::default()));
            let index = self.gpu.queue.submit([drawn.finish()]);
            if linked {
                let _ = self.gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: None });
            }
            for (slot, picture) in copied.into_iter().zip(crate::resources::pictures(&self.gpu, &copies)) {
                match picture.and_then(|pixels| stamped(Texture::Pixels(pixels))) {
                    Ok(frame) => inputs.push((slot, Some(frame))),
                    Err(why) => wrong = Some(format!("readback: {why}")),
                }
            }
        }
        let made = if wrong.is_none() { meeting.call(&cells.bell, inputs) } else { None };
        let texture = made.as_ref().and_then(|frame| frame.as_texture().ok());
        let link = self.states.get_mut(&uid).and_then(|state| state.link.as_mut());
        let picture = match (texture, link) {
            (Some(Texture::Pixels(pixels)), _) => Some(Made::of(pixels)),
            (Some(Texture::Shared(named)), link) => match link.and_then(|link| link.buffer(named.id)) {
                Some((buffer, shared)) => Some(Made::Shared(buffer, shared)),
                None => {
                    wrong = Some("the node returned a texture in memory this engine does not share with it".into());
                    None
                }
            },
            (None, _) => None,
        };
        // What the child asked for in this run: a target of the size of the pictures it makes.
        if let Some(Err(why)) = self.states.get_mut(&uid).and_then(|state| state.link.as_mut()).map(|link| link.asked(&self.gpu)) {
            (wrong, broken) = (Some(why), true);
        }
        if let Some(picture) = &picture {
            cells.produced.store(crate::half::pack(picture.size()), Ordering::Relaxed);
        }
        if let Some(state) = self.states.get_mut(&uid) {
            if let Err(why) = state.show(&self.gpu, encoder, picture.as_ref(), wants) {
                wrong = Some(why);
            }
            // A link that failed is gone: the node's pictures cross as pixels from the next tick.
            if broken {
                state.link = None;
                meeting.linked.store(false, Ordering::Relaxed);
            }
        }
        if wrong.is_some() || self.troubles.contains_key(&uid) {
            self.trouble(uid, wrong);
        }
        note_share(&cells, began.elapsed().as_secs_f64() * f64::from(self.fps.load(Ordering::Relaxed)));
        shows(&self.states)
    }

    /// One tick: upload what arrived, write the uniforms, draw every demanded stage, and read
    /// back the ones with a reader. Answers whether a stage was demanded.
    pub fn tick(&mut self) -> bool {
        self.drain_inbox();
        let began = Instant::now();
        let t = self.time.now();
        let recording = self.recording.load(Ordering::Relaxed);
        let want = self.plan.demanded(recording);
        // A stage no one asks for is not drawn: it takes none of the frame, and says so where
        // its turn is timed (a pass by the device's timestamps, a node's call by this clock).
        for (stage, _) in self.plan.stages.iter().zip(&want).filter(|(_, drawn)| !**drawn) {
            if self.timer.is_some() || matches!(stage.pass, Pass::Node) {
                stage.cells.frame_share.store(0f64.to_bits(), Ordering::Relaxed);
            }
        }
        if !want.contains(&true) {
            self.stats.frames.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        // BEFORE the take, because a map callback runs on a poll and nowhere else: polling only
        // after the submit below left every copy one whole tick older than it had to be.
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
        // What an earlier tick put on the device and the device has finished since.
        self.take();
        let (nanos, fps) = (f64::from(self.gpu.queue.get_timestamp_period()), f64::from(self.fps.load(Ordering::Relaxed)));
        if self.timer.as_mut().is_some_and(|timer| !timer.collect(nanos, fps)) {
            self.timer = None;
            goofi_supervisor::log::record(goofi_supervisor::log::Source::component("graphics"), goofi_supervisor::log::Level::Error, None, "graphics: the device refused to map the pass times; no node shows its share of the frame".to_string());
        }
        // The set this tick's passes write their times to, while no readout is in flight.
        let timing = self.timer.as_ref().filter(|timer| timer.flight.is_empty()).map(|timer| timer.set.clone());
        let mut timed: Vec<Arc<Cells>> = Vec::new();
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let mut started: Vec<(Uid, Want, Box<Slot>)> = Vec::new();
        let mut shown: Vec<(goofi_window::Id, wgpu::SurfaceTexture)> = Vec::new();
        // A tap is read back on the viewers' grid, not the clock's: a window and a recording take
        // every frame, a viewer only as many as it is served.
        self.taps.set_period(Duration::from_secs_f64(1.0 / f64::from(self.viewer_fps.load(Ordering::Relaxed).max(1))));
        let taps_due = self.taps.due();
        let mut tapped = false;
        for (i, drawn) in want.iter().enumerate() {
            if !drawn {
                continue;
            }
            let (uid, size, wants, buffers) = {
                let stage = &self.plan.stages[i];
                (stage.uid, stage.size, stage.wants(recording), stage.state)
            };
            if !self.states.contains_key(&uid) {
                continue;
            }
            if matches!(self.plan.stages[i].pass, Pass::Node) {
                if !self.call(i, &mut encoder, wants, t) {
                    continue;
                }
            } else {
                let Some(pipeline) = self.pipeline(i) else { continue };
                let readied = match self.states.get_mut(&uid) {
                    Some(state) => state.ensure_out(&self.gpu, size, wants, buffers),
                    None => continue,
                };
                if let Err(error) = readied {
                    self.trouble(uid, Some(error));
                    continue;
                }
                if self.troubles.get(&uid).is_some_and(|why| why.starts_with("readback:")) {
                    self.trouble(uid, None);
                }
                let stage = &self.plan.stages[i];
                let Some(state) = self.states.get_mut(&stage.uid) else { continue };
                for (k, cell) in stage.cells.uploads.iter().enumerate() {
                    if let Some(up) = cell.take() {
                        state.upload(&self.gpu, k, &up);
                        // Back to the half, which frees it or fills it again: not this thread's.
                        stage.cells.spent.push(up);
                    }
                }
                self.gpu.queue.write_buffer(&state.time, 0, &(self.time.position_at(t) as f32).to_le_bytes());
                self.gpu.queue.write_buffer(&state.frame, 0, &state.count.to_le_bytes());
                let res = [(stage.size.0 as f32).to_le_bytes(), (stage.size.1 as f32).to_le_bytes()].concat();
                self.gpu.queue.write_buffer(&state.resolution, 0, &res);
                if let Some(buf) = &state.params {
                    let ranges = &state.ranges[..state.ranges.len().min(stage.arrays)];
                    let bytes = shader::uniform_bytes(stage.decls, &stage.cells.params, ranges);
                    self.gpu.queue.write_buffer(buf, 0, &bytes);
                }
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
                views.extend((0..stage.arrays).map(|k| {
                    state.layouts.get(k).and_then(|t| t.as_ref()).map_or(&self.gpu.blank, |t| &t.view).clone()
                }));
                let group0 = state.group0(&self.gpu);
                let group1 = self.gpu.bind_textures(&stage.layouts.0, &views);
                let held: Vec<wgpu::TextureView> = state.buffers.iter().map(|b| b[0].view.clone()).collect();
                let group2 = self.gpu.bind_textures(&stage.layouts.1, &held);
                // Cloned, so the readback below may take `states` mutably.
                let out_view = state.out.as_ref().expect("ensure_out made it").view.clone();
                // The output, then one target per state buffer — the order the prelude writes them in.
                let targets: Vec<wgpu::TextureView> =
                    std::iter::once(out_view.clone()).chain(state.buffers.iter().map(|b| b[1].view.clone())).collect();
                let query = timing.as_ref().filter(|_| timed.len() < TIMED_MAX).map(|set| (set, 2 * timed.len() as u32));
                if query.is_some() {
                    timed.push(stage.cells.clone());
                }
                let mut pass = crate::gpu::clear_pass(&mut encoder, &targets, query);
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group0, &[]);
                pass.set_bind_group(1, &group1, &[]);
                pass.set_bind_group(2, &group2, &[]);
                pass.draw(0..3, 0..1);
                drop(pass);
            }
            let stage = &self.plan.stages[i];
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
                let tap = matches!(w, Want::Tap | Want::TapU8);
                if tap && !taps_due {
                    continue;
                }
                let state = self.states.get_mut(&stage.uid).expect("just borrowed");
                let Some(ring) = state.reads[w as usize].as_mut() else { continue };
                ring.reclaim();
                let Some(slot) = ring.free.pop_back() else { continue };
                tapped |= tap;
                let (width, height) = slot.texture.size;
                self.gpu.blit(&mut encoder, w, &out_view, &slot.texture.view, &ring.out_size);
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
        if let (Some(timer), false) = (&self.timer, timed.is_empty()) {
            timer.resolve(&mut encoder, timed.len());
        }
        self.gpu.queue.submit([encoder.finish()]);
        if let (Some(timer), false) = (self.timer.as_mut(), timed.is_empty()) {
            timer.send(timed);
        }
        if tapped {
            // The next readback is a period on; a clock that fell behind starts the pace again.
            self.taps.advance();
        }
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
                ready.store(if r.is_ok() { READY } else { FAILED }, Ordering::Release);
            });
            if let Some(ring) = self.states.get_mut(&uid).and_then(|s| s.reads[w as usize].as_mut()) {
                ring.flight.push_back(slot);
            }
        }
        self.stats.frames.fetch_add(1, Ordering::Relaxed);
        self.stats.tick_max_us.fetch_max(began.elapsed().as_micros() as u64, Ordering::Relaxed);
        true
    }

    /// Raise or clear what an armed stage wears, and ask for the settle that publishes it.
    fn trouble(&mut self, uid: Uid, why: Option<String>) {
        let was = match why {
            Some(why) => self.troubles.insert(uid, why),
            None => self.troubles.remove(&uid),
        };
        if was != self.troubles.get(&uid).cloned() {
            self.troubles_out.put(Box::new(self.troubles.clone()));
            self.shared.replan.store(true, Ordering::Release);
            self.shared.waker.notify();
        }
    }

    /// Every frame the device has finished, handed to the reader that asked for it, mapped: the
    /// reader copies it out on its own thread and gives the slot back. Nothing here reads a texel.
    fn take(&mut self) {
        let sfreq = f64::from(self.fps.load(Ordering::Relaxed));
        for i in 0..self.plan.stages.len() {
            let (uid, window) = {
                let stage = &self.plan.stages[i];
                (stage.uid, stage.window)
            };
            for w in Want::ALL {
                let Some(state) = self.states.get_mut(&uid) else { break };
                let mut finished: Vec<Readback> = Vec::new();
                let mut refused = false;
                {
                    let Some(ring) = state.reads[w as usize].as_mut() else { continue };
                    ring.reclaim();
                    while ring.flight.front().is_some_and(|slot| slot.ready.load(Ordering::Acquire) != PENDING) {
                        let slot = ring.flight.pop_front().expect("the front was answered");
                        match slot.ready.load(Ordering::Acquire) {
                            READY => finished.push(ring.hand(slot, w, 0, sfreq)),
                            // A map the device refused was never mapped: the slot is free again,
                            // and the stage says so rather than holding its ring for ever.
                            _ => {
                                slot.ready.store(PENDING, Ordering::Relaxed);
                                ring.free.push_back(slot);
                                refused = true;
                            }
                        }
                    }
                }
                if refused {
                    self.trouble(uid, Some("readback: the device refused to map the frame".to_string()));
                    continue;
                }
                let cells = &self.plan.stages[i].cells;
                for mut readback in finished {
                    match w {
                        Want::Screen => {
                            if let (Some(id), Some(ui)) = (window, &self.ui) {
                                present(self.presenting.entry(id).or_default(), ui, id, readback);
                            }
                        }
                        Want::Record => {
                            // Numbered here, so a readback the half or the queue let go leaves a
                            // gap the recorder counts; dated by the tick that drew it.
                            state.recorded += 1;
                            readback.index = state.recorded;
                            cells.record.push(readback);
                            cells.bell.ring(0);
                        }
                        Want::Tap | Want::TapU8 => {
                            // Latest wins; what this displaces goes back to the ring on the way out.
                            drop(cells.tap.put(Box::new(readback)));
                            cells.bell.ring(0);
                        }
                    }
                }
            }
        }
    }
}

/// One window's frame in flight, or the count of frames its surface showed since the window
/// thread last heard.
#[derive(Default)]
struct Present {
    /// The newest frame read back for the window, mapped, until the window thread takes it.
    frame: Mailbox<Readback>,
    drawn: AtomicU64,
    posted: AtomicBool,
    /// The window thread's own rows, kept between two frames; nothing else takes this.
    rows: Mutex<Vec<u8>>,
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

/// Hand the screen the newest frame, latest-wins: what this displaces goes back to its ring.
fn present(cell: &Arc<Present>, ui: &goofi_window::Ui, id: goofi_window::Id, frame: Readback) {
    drop(cell.frame.put(Box::new(frame)));
    post(cell, ui, id);
}

/// Tell the window thread what the cell holds, with at most ONE job outstanding: a job per frame
/// on an unbounded queue starved every other job the window thread had, an op among them. The
/// rows are copied off the mapped buffer THERE, on the window's own thread.
fn post(cell: &Arc<Present>, ui: &goofi_window::Ui, id: goofi_window::Id) {
    if cell.posted.swap(true, Ordering::AcqRel) {
        return;
    }
    let cell = cell.clone();
    ui.post(move |host| {
        if let Some(frame) = cell.frame.take() {
            let mut rows = cell.rows.lock();
            rows.clear();
            if frame.rows_into(&mut rows) {
                host.present(id, frame.size(), &rows);
            }
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
pub fn params_len(manifest: &goofi_core::node::NodeManifest) -> usize {
    let zeros = goofi_runtime::cells_of(&goofi_core::node::ParamGroups::new(), manifest.params);
    let ranges = vec![[0.0, 1.0, 1.0, 1.0]; shader::array_inputs(manifest).count()];
    shader::uniform_bytes(manifest.params, &zeros, &ranges).len()
}
