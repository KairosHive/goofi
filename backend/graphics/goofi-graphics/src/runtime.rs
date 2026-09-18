//! What the render thread owns: the plan, one GPU state per live node, and the tick that draws
//! every demanded stage once and reads back the ones somebody is watching.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use goofi_core::{Data, Meta};
use goofi_record::{Kind, Recorder, StreamId, StreamMeta};
use goofi_node::Uid;

use crate::gpu::{padded_row, Gpu, Want};
use crate::half::Tapped;
use crate::resources::{State, Slot, Spare, give_back, rows_into};
use crate::plan::{Input, Plan, Pass};
use crate::Host;

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

pub struct Runtime {
    plan: Plan,
    states: HashMap<Uid, State>,
    host: Arc<dyn Host>,
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
        host: Arc<dyn Host>,
        stats: Arc<Stats>,
        troubles: Troubles,
        shared: Arc<goofi_control::Shared>,
    ) -> Runtime {
        Runtime {
            gpu,
            plan: Plan::default(),
            states: HashMap::new(),
            host,
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
        let state = State::new(&self.gpu, params);
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
        let t = self.host.now();
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
            let produced = match &stage.pass {
                Pass::Host { source, program } => source.lock().unwrap().clone().filter(|frame| {
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
                Some(built) => match built.get() { Some(Ok(p)) => Some(p), _ => continue },
                None => None,
            };
            let Some(state) = self.states.get_mut(&stage.uid) else { continue };
            if let Err(error) = state.ensure_out(&self.gpu, stage.size, stage.wants(recording), stage.state) {
                self.trouble(stage.uid, Some(error));
                continue;
            }
            let clear = self.troubles.lock().expect("the render troubles")
                .get(&stage.uid).is_some_and(|why| why.starts_with("readback:"));
            if clear { self.trouble(stage.uid, None); }
            let Some(state) = self.states.get_mut(&stage.uid) else { continue };
            let shrunk_from = stage.size;
            for (k, cell) in stage.uploads.iter().enumerate() {
                if let Some(up) = cell.lock().unwrap().take() {
                    state.upload(&self.gpu, k, &up);
                }
            }
            state.write_uniforms(&self.gpu, t, stage.size, stage.decls, &stage.params);
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
                self.states[&stage.uid].draw(&self.gpu, &mut encoder, pipeline, &views);
            }
            let out_view = self.states[&stage.uid].out.as_ref().expect("output allocated").view.clone();
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
        let mut want: HashMap<Uid, (StreamId, (u32, u32), goofi_core::record::VideoQuality)> = HashMap::new();
        if recording {
            for stage in &self.plan.stages {
                if let Some(r) = &stage.record {
                    let id = StreamId {
                        uid: stage.uid,
                        node: r.node.clone(),
                        slot: r.slot.clone(),
                        engine: "graphics",
                    };
                    want.insert(stage.uid, (id, stage.size, r.quality));
                }
            }
        }
        for uid in self.taping.keys().copied().collect::<Vec<_>>() {
            let held = &self.taping[&uid];
            // The RECORDER owns whether a stream is open: a `record disarm` closes one behind the
            // render thread's back, and a tape kept over that would never open the next file.
            let kept = want.get(&uid).is_some_and(|(_, size, quality)| *size == held.size && *quality == held.quality)
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
                Some((_, size, _)) if *size != held.size => "resized",
                Some((_, _, quality)) if *quality != held.quality => "quality changed",
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
        for (uid, (id, size, quality)) in want {
            if self.taping.contains_key(&uid) || rec.is_open(&id) {
                continue;
            }
            let kind = Kind::Video { size, fps: f64::from(crate::FPS), quality };
            let live = match rec.open(&id, kind, t, StreamMeta::measured(Some(f64::from(crate::FPS)))) {
                Ok(()) => true,
                // A stream that will not open is this NODE's failure and nobody else's: the
                // recording keeps every other stream, and the entry the recorder filed says why.
                Err(why) => {
                    self.trouble(uid, Some(format!("not recorded: {why}")));
                    false
                }
            };
            self.taping.insert(uid, Tape { id, size, quality, started: t, missed: 0, said: Instant::now(), live });
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
                        // Readbacks from before a resize or quality change belong to the closed file.
                        let held = self.taping.get(&uid).filter(|tape| tape.live && tape.size == size && at >= tape.started);
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
    quality: goofi_core::record::VideoQuality,
    started: f64,
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
