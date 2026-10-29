//! The audio engine behind the `Engine` seam: this file owns the library, the plan and the clock,
//! the audio half (`runtime`) the instances and the arena, and a node's `control` half its thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use goofi_supervisor::sync::Mutex;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use goofi_audio_sdk::{AudioNode, BLOCK, MAX_PORTS};
use goofi_core::{Param, SlotType};
use goofi_node::{
    DrainWaker, Edit, EditorAction, Engine, GraphView, LibraryEntry, NodeManifest, NodeView,
    ParamGroups, ParamKey, Status, Uid, NATIVE,
};

mod chanmap;
mod control;
mod host;
pub use host::{hosts, NO_ASIO_NOTE};
pub use rt::performance_cores;
pub(crate) use rt::Realtime;

/// Open a stream through `$stream` in the sample format the DEVICE speaks, for input and output
/// alike. DSD has no PCM conversion, and `$refused` words that.
macro_rules! by_format {
    ($format:expr, $stream:ident, $refused:expr $(, $arg:expr)* $(,)?) => {
        match $format {
            cpal::SampleFormat::I8 => $stream::<i8>($($arg),*),
            cpal::SampleFormat::I16 => $stream::<i16>($($arg),*),
            cpal::SampleFormat::I24 => $stream::<cpal::I24>($($arg),*),
            cpal::SampleFormat::I32 => $stream::<i32>($($arg),*),
            cpal::SampleFormat::I64 => $stream::<i64>($($arg),*),
            cpal::SampleFormat::U8 => $stream::<u8>($($arg),*),
            cpal::SampleFormat::U16 => $stream::<u16>($($arg),*),
            cpal::SampleFormat::U24 => $stream::<cpal::U24>($($arg),*),
            cpal::SampleFormat::U32 => $stream::<u32>($($arg),*),
            cpal::SampleFormat::U64 => $stream::<u64>($($arg),*),
            cpal::SampleFormat::F32 => $stream::<f32>($($arg),*),
            cpal::SampleFormat::F64 => $stream::<f64>($($arg),*),
            other => Err($refused(other)),
        }
    };
}
pub(crate) use by_format;
pub(crate) mod nodes;
mod plan;
mod pool;
mod rt;
mod runtime;
mod scan;
mod wav;
pub mod vst3;

use control::{AudioHalf, AudioShared};
use goofi_runtime::{Desired, Handle, Plane, Shared};
use nodes::{audio_in, audio_out, Class};
use plan::Plan;
use runtime::{Fault, Frames, Msg, Playback, Retired, Runtime, Slot, OVERRUNS};

/// The rate until a device names one.
pub(crate) const RATE: f64 = 48_000.0;

/// A ceiling on a device open, which runs under the graph lock: a sound server that does not
/// answer must not wedge every op.
const OPEN_WAIT: Duration = Duration::from_secs(2);

/// The same ceiling for an ASIO driver, which loads a vendor runtime before it answers.
const ASIO_OPEN_WAIT: Duration = Duration::from_secs(10);

pub use goofi_supervisor::timing::Clock;

/// Hardware belongs to the internal clock alone: `drive(frames)` renders at the caller's speed,
/// and no live stream has a timeline that can meet it.
fn owns_devices(clock: Clock) -> bool {
    clock == Clock::Internal
}

/// The timing door: what the clock is doing, for `session status`.
#[derive(serde::Serialize)]
pub struct AudioStatus {
    pub clock: &'static str,
    pub device: Option<String>,
    pub rate: f64,
    pub channels: u16,
    pub callbacks: u64,
    pub xruns: u64,
    /// Callbacks that found the runtime held and played silence: the lock was taken under the
    /// graph lock, which is the thing to read, not an underrun.
    pub contended: u64,
    pub render_max_us: u64,
    /// Renders that took longer than their own block of frames: the host's underruns, which
    /// the sound-server backends do not report.
    pub late: u64,
    /// What the clock thread has waited for its render helpers, in all and at worst.
    pub pool_wait_us: u64,
    pub pool_wait_max_us: u64,
    /// The frames the host hands each callback, as it granted them; absent without a device.
    pub period: Option<u32>,
    /// The CPUs the render threads are seated on, on a machine with a slower kind of core too.
    pub cores: Option<String>,
    /// Per live node: what its `process` cost, mean and worst, over every block so far.
    pub nodes: Vec<NodeCost>,
}

#[derive(serde::Serialize)]
pub struct NodeCost {
    pub uid: String,
    pub type_name: &'static str,
    pub blocks: u64,
    pub mean_us: f64,
    pub max_us: f64,
}

#[derive(Default)]
pub(crate) struct Stats {
    callbacks: AtomicU64,
    xruns: AtomicU64,
    contended: AtomicU64,
    render_max_us: AtomicU64,
    late: AtomicU64,
    pool_wait_ns: AtomicU64,
    pool_wait_max_ns: AtomicU64,
    /// Raised by the stream's error callback; the drain closes the clock and tries the name once more.
    dead: AtomicBool,
}

/// The running output stream, owned by a thread of its own: `cpal::Stream` is not `Send` on
/// every host. It plays once the runtime is cut to it; dropping this stops it and waits for that.
struct DeviceClock {
    name: String,
    channels: u16,
    /// What `audio.buffer_size` asked for when this opened, and the period the host granted.
    buffer: u32,
    period: Option<u32>,
    go: Option<mpsc::Sender<()>>,
    done: mpsc::Receiver<()>,
    /// The output stream's entry in the resource index, for as long as the clock holds it.
    _lease: goofi_supervisor::scope::Lease,
}

impl DeviceClock {
    fn open(name: &str, buffer: u32, runtime: Arc<Mutex<Runtime>>, stats: Arc<Stats>, waker: Arc<DrainWaker>) -> Result<(DeviceClock, f64), String> {
        let (opened, on_open) = mpsc::channel::<Result<(f64, u16, Option<u32>), String>>();
        let (go, on_go) = mpsc::channel::<()>();
        let (done, on_done) = mpsc::channel::<()>();
        let device = name.to_string();
        goofi_supervisor::worker::thread("goofi-audio-clock")
            .spawn(move || {
                let stream = match open_output(&device, buffer, runtime, stats.clone(), waker.clone()) {
                    Ok((stream, rate, channels)) => {
                        let _ = opened.send(Ok((rate, channels, stream.buffer_size().ok())));
                        stream
                    }
                    Err(e) => {
                        let _ = opened.send(Err(e));
                        return;
                    }
                };
                if on_go.recv().is_ok() {
                    if let Err(e) = stream.play() {
                        goofi_supervisor::log::record(goofi_supervisor::log::Source::component("audio"), goofi_supervisor::log::Level::Error, None, format!("audio: {e}"));
                        stats.dead.store(true, Ordering::Release);
                        waker.notify();
                    }
                    let _ = on_go.recv();
                }
                drop(stream);
                let _ = done.send(());
            })
            .map_err(|e| format!("could not start the clock thread: {e}"))?;
        let wait = if crate::host::asio_driver(name).is_some() { ASIO_OPEN_WAIT } else { OPEN_WAIT };
        let (rate, channels, period) = on_open
            .recv_timeout(wait)
            .map_err(|_| format!("`{name}` did not open within {} s", wait.as_secs()))??;
        let lease = goofi_supervisor::scope::lease(goofi_supervisor::scope::Kind::Device, format!("audio out {name}"));
        Ok((DeviceClock { name: name.to_string(), channels, buffer, period, go: Some(go), done: on_done, _lease: lease }, rate))
    }

    /// Start the callbacks — only once the runtime is cut to this stream's rate and width.
    fn play(&self) {
        if let Some(go) = &self.go {
            let _ = go.send(());
        }
    }
}

/// Lowers the flag when the clock thread ends, however it ends.
impl Drop for DeviceClock {
    fn drop(&mut self) {
        self.go = None;
        let _ = self.done.recv_timeout(goofi_transport::SHUTDOWN_WAIT);
    }
}

/// The host default is what a `default` name means.
pub(crate) const DEFAULT_DEVICE: &str = "default";

/// What an input names its device to say the name resolved and nothing was opened.
pub(crate) const NO_DEVICE: &str = "the external clock owns no device";

/// Open `name` with `buffer` frames per callback, or the host's own buffer for 0. A host left to
/// itself may hold seconds (pipewire-pulse holds 2 s); `render_into` is size-agnostic.
fn open_output(name: &str, buffer: u32, runtime: Arc<Mutex<Runtime>>, stats: Arc<Stats>, waker: Arc<DrainWaker>) -> Result<(cpal::Stream, f64, u16), String> {
    let device = host::device(host::Kind::Output, name)?;
    let supported = device.default_output_config().map_err(|e| format!("`{name}`: {e}"))?;
    let format = supported.sample_format();
    let mut config = supported.config();
    if buffer > 0 {
        config.buffer_size = cpal::BufferSize::Fixed(buffer);
    }
    let rate = f64::from(config.sample_rate);
    let channels = config.channels;
    // The sample format the DEVICE speaks, as on the input side; the runtime renders `f32`.
    let refused = |f| format!("the device's sample format {f} is one goofi does not write");
    let open = |f| {
        crate::by_format!(f, output_stream, refused, &device, config, runtime.clone(), stats.clone(), waker.clone())
    };
    let stream = open(format).map_err(|e| format!("`{name}`: {e}"))?;
    Ok((stream, rate, channels))
}

/// The clock's callback, rendering `f32` and handing the device whatever word it speaks, through
/// a scratch buffer that grows once to the host's period and is then reused.
fn output_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    runtime: Arc<Mutex<Runtime>>,
    stats: Arc<Stats>,
    waker: Arc<DrainWaker>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let died = stats.clone();
    let mut scratch: Vec<f32> = Vec::new();
    let (rate, channels) = (config.sample_rate, config.channels as usize);
    let mut realtime = Realtime::Unasked;
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                realtime.claim(data.len() / channels, rate);
                stats.callbacks.fetch_add(1, Ordering::Relaxed);
                let started = Instant::now();
                scratch.resize(data.len(), 0.0);
                match runtime.try_lock() {
                    // A panic must not unwind through cpal's callback: the block is silence and
                    // the cause is logged; the runtime's lock takes the poison over.
                    Some(mut rt) => {
                        // The render helpers keep this thread's deadline, so they take its grant.
                        rt.pool.realtime((data.len() / channels) as u32, rate);
                        if let Err(p) = catch_unwind(AssertUnwindSafe(|| rt.render_into(&mut scratch))) {
                            scratch.fill(0.0);
                            goofi_supervisor::log::record(goofi_supervisor::log::Source::component("audio"), goofi_supervisor::log::Level::Error, None, format!("the render panicked: {}", goofi_node::panic_message(p)));
                        }
                        let (waited, worst) = rt.pool.waited();
                        stats.pool_wait_ns.store(waited.as_nanos() as u64, Ordering::Relaxed);
                        stats.pool_wait_max_ns.store(worst.as_nanos() as u64, Ordering::Relaxed);
                    }
                    None => {
                        scratch.fill(0.0);
                        stats.contended.fetch_add(1, Ordering::Relaxed);
                    }
                }
                for (out, v) in data.iter_mut().zip(scratch.iter()) {
                    *out = T::from_sample_(*v);
                }
                let took = started.elapsed();
                stats.render_max_us.fetch_max(took.as_micros() as u64, Ordering::Relaxed);
                if took.as_secs_f64() * f64::from(rate) > (data.len() / channels) as f64 {
                    stats.late.fetch_add(1, Ordering::Relaxed);
                }
            },
            // An underrun the backend reports recovers on its own and is an xrun; a device that
            // is gone, or a sound server that dropped the stream, is death.
            move |e| match e.kind() {
                cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::StreamInvalidated => {
                    died.dead.store(true, Ordering::Release);
                    waker.notify();
                }
                // The stream plays on at ordinary priority, so this is the deadline lost rather
                // than a period missed: counting it as an xrun would hide the very thing to read.
                cpal::ErrorKind::RealtimeDenied => goofi_supervisor::log::record(goofi_supervisor::log::Source::component("audio"), goofi_supervisor::log::Level::Error, None, format!("audio: {e}")),
                _ => {
                    died.xruns.fetch_add(1, Ordering::Relaxed);
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}

/// The rings a device or a port fills, minted per instance: the DSP half's ends in the birth,
/// the control half's in the ports. A node that owns no OS handle gets neither.
fn rings_for(type_name: &str, chans: Arc<AtomicU16>, uid: Uid, ui: Option<goofi_window::Ui>, shared: Arc<AudioShared>) -> (nodes::Birth, control::Ports) {
    let mut birth = nodes::Birth { chans: chans.clone(), ui, uid: Some(uid), shared: Some(shared), ..Default::default() };
    let mut ports = control::Ports::default();
    match type_name {
        nodes::audio_in::TYPE => {
            let (producer, consumer) = rtrb::RingBuffer::new(control::DEVICE_RING);
            birth.inbox = Some(consumer);
            ports.audio_in = Some((Arc::new(Mutex::new(producer)), chans));
        }
        nodes::audio_playback::TYPE => {
            let (producer, consumer) = rtrb::RingBuffer::new(control::DEVICE_RING);
            birth.inbox = Some(consumer);
            ports.play = Some((producer, chans));
        }
        nodes::midi_in::TYPE => {
            let (producer, consumer) = rtrb::RingBuffer::new(control::NOTE_RING);
            birth.notes = Some(consumer);
            ports.midi_in = Some(Arc::new(Mutex::new(producer)));
        }
        _ => {}
    }
    (birth, ports)
}

pub(crate) struct Instance {
    pub(crate) idx: usize,
    /// Which occupant of `idx` this is: a plan compiled for an earlier one must not drive it.
    pub(crate) serial: u64,
    pub(crate) manifest: &'static NodeManifest,
    /// Answers `channels` on the control thread; the box that processes never leaves the audio
    /// thread.
    pub(crate) twin: Box<dyn AudioNode>,
    pub(crate) control: Handle,
    /// The channel count each Array input last saw — what the plan sizes its inbox by.
    pub(crate) chans: Vec<Arc<AtomicU16>>,
    /// Per param, the channel count its last computed result had — what sizes its block.
    pub(crate) param_chans: Vec<Arc<AtomicU16>>,
    /// The ring size each Array input asks for, in floats, sized by the frames it takes.
    wanted: Vec<Arc<AtomicUsize>>,
    /// What each inbox ring and each output's rings were minted for; a plan or a frame past it
    /// re-mints them.
    minted: Minted,
    /// A plugin's params as its window was last told them, from the declared defaults on.
    shown: Vec<f64>,
    /// The audio thread's tally of this occupant's `process` cost.
    spent: Arc<runtime::Spent>,
    /// The node still being made, on a thread of its own; `None` once it is on the audio thread.
    making: Option<Making>,
}

/// A node being made off this thread — a plugin's instantiation, its `prepare`, its saved state —
/// and everything else its slot needs, held until the node lands and the slot goes whole.
struct Making {
    node: goofi_supervisor::ready::Ready<Box<dyn AudioNode>>,
    /// The rate it is prepared at; one that moved by the landing is prepared again.
    rate: f64,
    unborn: Unborn,
    born: Arc<AtomicBool>,
}

/// A slot but its node.
struct Unborn {
    idx: usize,
    serial: u64,
    type_name: &'static str,
    params: Arc<[AtomicU64]>,
    ramps: Arc<[runtime::Ramps]>,
    inboxes: Vec<Frames>,
    blocks: rtrb::Consumer<f32>,
    taps: Vec<rtrb::Producer<f32>>,
    recs: Vec<rtrb::Producer<f32>>,
    spent: Arc<runtime::Spent>,
}

impl Unborn {
    fn slot(self, uid: Uid, node: Box<dyn AudioNode>) -> Slot {
        Slot {
            uid,
            type_name: self.type_name,
            serial: self.serial,
            node,
            params: self.params,
            ramps: self.ramps,
            inboxes: self.inboxes,
            blocks: self.blocks,
            taps: self.taps,
            recs: self.recs,
            dead: false,
            overruns: 0,
            cost: None,
            spent: self.spent,
            fault: None,
        }
    }
}

/// A device being opened on a thread of its own, and the one to go back to if it refuses.
struct Opening {
    name: String,
    buffer: u32,
    clock: goofi_supervisor::ready::Ready<(DeviceClock, f64)>,
    previous: Option<String>,
}

#[derive(Default)]
struct Minted {
    /// Floats per inbox ring.
    inboxes: Vec<usize>,
    /// Channels per output's tap and recording rings.
    outs: Vec<u16>,
}

pub struct AudioEngine {
    hub: Arc<goofi_transport::Hub>,
    instance: String,
    time: Arc<goofi_supervisor::timing::Time>,
    clock: Clock,
    device: Option<DeviceClock>,
    /// The open under way, on a thread of its own; the settle that finds it answered takes it.
    opening: Option<Opening>,
    /// The name and buffer size last tried and what they answered: a pair that failed is not tried
    /// again until one moves.
    tried: Option<(String, u32, Option<String>)>,
    stats: Arc<Stats>,
    shared: Arc<Shared>,
    /// What only an audio control half needs: the clock's rate, what drives it, and the edits a
    /// plugin's own window made.
    audio: Arc<AudioShared>,
    classes: HashMap<&'static str, Class>,
    /// The child a bundle is scanned in, and the platform's plugin folders: the composition root's.
    vst3: Option<(PathBuf, Vec<PathBuf>)>,
    /// The window thread: where a plugin is loaded and its editor lives. None without a display.
    ui: Option<goofi_window::Ui>,
    runtime: Arc<Mutex<Runtime>>,
    inbox: rtrb::Producer<Msg>,
    outbox: rtrb::Consumer<Retired>,
    free: Vec<usize>,
    slab_len: usize,
    next_serial: u64,
    live: HashMap<Uid, Instance>,
    workspace: Option<PathBuf>,
    /// A workspace that turned over, swept of dead blobs at the first settled state after it.
    sweep: bool,
    /// Nodes the audio thread put out of the plan — a panic, or the watchdog — until a restart.
    disabled: HashMap<Uid, String>,
    faults: goofi_runtime::Faults,
    pending: Vec<(Uid, Status)>,
    dirty: bool,
    last: Plan,
}

const SLAB: usize = 64;
const QUEUE: usize = 4096;

impl AudioEngine {
    pub fn new(hub: Arc<goofi_transport::Hub>, instance: String, time: Arc<goofi_supervisor::timing::Time>, waker: Arc<DrainWaker>, clock: Clock) -> Result<AudioEngine, String> {
        host::warm();
        let classes: HashMap<&'static str, Class> = nodes::BUILT_IN
            .iter()
            .map(|(type_name, m, make)| {
                // One per process, whatever the engine count: keyed by the static descriptor.
                let manifest = goofi_node::interned(type_name, *m as *const _ as u64, || NodeManifest {
                    type_name,
                    tags: m.tags,
                    doc: m.doc,
                    inputs: m.inputs,
                    outputs: m.outputs,
                    params: m.params,
                    producer: false,
                });
                (*type_name, Class { manifest, make: Arc::new(*make), plugin: None })
            })
            .collect();
        let (inbox, to_audio) = rtrb::RingBuffer::new(QUEUE);
        let (from_audio, outbox) = rtrb::RingBuffer::new(QUEUE);
        let anchor = Arc::new(runtime::Anchor::new(time.clone()));
        Ok(AudioEngine {
            instance,
            time,
            clock,
            device: None,
            tried: None,
            stats: Arc::new(Stats::default()),
            shared: Arc::new(Shared::new(waker.clone())),
            audio: Arc::new(AudioShared {
                rate: AtomicU64::new(RATE.to_bits()),
                buffer: AtomicU32::new(goofi_core::variables::DEFAULT_BUFFER),
                clock,
                edits: Mutex::new(Vec::new()),
                waker,
                anchor: anchor.clone(),
                swaps: Mutex::new(HashMap::new()),
                savers: Mutex::new(HashMap::new()),
            }),
            classes,
            vst3: None,
            ui: None,
            runtime: Arc::new(Mutex::new(Runtime::new(SLAB, to_audio, from_audio, anchor))),
            inbox,
            outbox,
            free: (0..SLAB).rev().collect(),
            slab_len: SLAB,
            next_serial: 0,
            live: HashMap::new(),
            workspace: None,
            sweep: false,
            disabled: HashMap::new(),
            opening: None,
            faults: goofi_runtime::Faults::default(),
            pending: Vec::new(),
            dirty: false,
            last: Plan::default(),
            hub,
        })
    }

    /// Where a `.vst3` bundle is scanned — a `goofi` answering `vst3-scan` — and which folders
    /// are scanned on the engine's own account, after every root.
    pub fn set_vst3(&mut self, scanner: PathBuf, dirs: Vec<PathBuf>) {
        self.vst3 = Some((scanner, dirs));
    }

    /// The window thread every plugin from here on is made on — before anything scans.
    pub fn set_ui(&mut self, ui: Option<goofi_window::Ui>) {
        self.ui = ui;
    }

    /// The external clock: render whole blocks until `frames` are ready, and hand them over
    /// interleaved — exactly what a device callback would receive.
    pub fn drive(&mut self, frames: usize) -> (Vec<f32>, u16) {
        let mut rt = self.runtime.lock();
        let channels = rt.channels();
        let mut out = vec![0.0; frames * channels as usize];
        rt.render_into(&mut out);
        (out, channels)
    }

    /// Say that the block the external clock renders next is NOW in patch time: the harness's
    /// hand on a clock a device would keep in step by itself.
    pub fn align(&mut self) {
        let rt = self.runtime.lock();
        let rate = self.audio.rate();
        self.audio.anchor.tie(self.time.now(), rate);
        drop(rt);
    }

    /// Take each `process` of `uid`'s current occupant to cost `cost`, not the wall time it took, so
    /// a test judges the watchdog without racing the scheduler; `None` measures again.
    pub fn state_cost(&mut self, uid: Uid, cost: Option<Duration>) {
        let Some(&Instance { idx, serial, .. }) = self.live.get(&uid) else { return };
        let mut rt = self.runtime.lock();
        rt.apply_pending();
        if let Some(slot) = rt.slab[idx].as_mut().filter(|s| s.serial == serial) {
            slot.cost = cost;
        }
    }

    pub fn flush_recording(&self) -> Vec<mpsc::Receiver<Result<(), String>>> {
        self.live.values().map(|node| node.control.flush()).collect()
    }

    /// Blocks the clock has rendered so far: the recorder's boundary on the derived timeline,
    /// since every block carries its number and no block is rendered between two reads of this.
    pub fn blocks(&self) -> u64 {
        self.audio.anchor.blocks.load(Ordering::Relaxed)
    }

    /// Read without the runtime lock: taking it under the graph lock would cost a callback its block.
    pub fn status(&self) -> AudioStatus {
        AudioStatus {
            clock: self.clock.name(),
            device: self.device.as_ref().map(|d| d.name.clone()),
            rate: self.audio.rate(),
            channels: self.device.as_ref().map_or(self.last.output.1, |d| d.channels),
            callbacks: self.stats.callbacks.load(Ordering::Relaxed),
            xruns: self.stats.xruns.load(Ordering::Relaxed),
            contended: self.stats.contended.load(Ordering::Relaxed),
            render_max_us: self.stats.render_max_us.load(Ordering::Relaxed),
            late: self.stats.late.load(Ordering::Relaxed),
            pool_wait_us: self.stats.pool_wait_ns.load(Ordering::Relaxed) / 1000,
            pool_wait_max_us: self.stats.pool_wait_max_ns.load(Ordering::Relaxed) / 1000,
            period: self.device.as_ref().and_then(|d| d.period),
            cores: rt::seat_name(),
            nodes: self
                .live
                .iter()
                .map(|(uid, inst)| {
                    let blocks = inst.spent.blocks.load(Ordering::Relaxed);
                    let nanos = inst.spent.nanos.load(Ordering::Relaxed);
                    NodeCost {
                        uid: uid.to_hex(),
                        type_name: inst.manifest.type_name,
                        blocks,
                        mean_us: if blocks > 0 { nanos as f64 / blocks as f64 / 1000.0 } else { 0.0 },
                        max_us: inst.spent.max_nanos.load(Ordering::Relaxed) as f64 / 1000.0,
                    }
                })
                .collect(),
        }
    }

    /// Where `uid`'s opaque state is kept between two births; named by type, so a chosen uid
    /// never hands one type the bytes another left.
    fn state_path(&self, uid: Uid, type_name: &str) -> Option<PathBuf> {
        Some(self.state_dir()?.join(uid.to_hex()).join(type_name))
    }

    fn state_dir(&self) -> Option<PathBuf> {
        Some(self.workspace.as_ref()?.join(".goofi").join("state"))
    }

    /// Every blob of a uid the patch does not hold — a delete whose undo a load ended — so a
    /// number minted again is never born on a dead node's bytes.
    fn sweep_state(&self) {
        let Some(entries) = self.state_dir().and_then(|d| std::fs::read_dir(d).ok()) else { return };
        for entry in entries.flatten() {
            let held = entry.file_name().to_str().and_then(Uid::from_hex).is_some_and(|u| self.live.contains_key(&u));
            if !held {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    /// Keep what `save` answered, or nothing: a node with no state leaves no file behind.
    fn write_state(&self, uid: Uid, type_name: &str, bytes: Vec<u8>) {
        let Some(path) = self.state_path(uid, type_name) else { return };
        let written = if bytes.is_empty() {
            std::fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
        } else {
            std::fs::create_dir_all(path.parent().expect("a state file has a directory")).and_then(|()| std::fs::write(&path, bytes))
        };
        if let Err(e) = written {
            goofi_supervisor::log::record(goofi_supervisor::log::Source::component("audio"), goofi_supervisor::log::Level::Error, None, format!("audio: could not keep {}: {e}", path.display()));
        }
    }

    /// What the audio thread handed back is dropped here, where dropping may take time — and a
    /// node it put out of the plan is recorded, for the settle this asks for to name and re-plan.
    fn discard_retired(&mut self) {
        while let Ok(retired) = self.outbox.pop() {
            match retired {
                Retired::Slot(slot) => self.write_state(slot.uid, slot.type_name, slot.node.save()),
                // The drop is the POINT: these come back so the audio thread never frees them.
                Retired::Plan(plan, arena) => drop((plan, arena)),
                Retired::Rings(rings) => drop(rings),
                Retired::Slab(slab) => drop(slab),
                Retired::Faulted { uid, serial, fault } => {
                    if self.live.get(&uid).is_some_and(|i| i.serial == serial) {
                        let msg = match fault {
                            Fault::Panic(msg) => msg,
                            Fault::Overrun => format!("process overran its budget {OVERRUNS} blocks in a row"),
                            Fault::NotANumber => "process put out a value that is not a number".to_string(),
                        };
                        self.disabled.insert(uid, msg);
                        self.dirty = true;
                    }
                }
            }
        }
    }

    /// A message always lands. A full ring means the audio thread has not run for a long time, so
    /// taking its lock to apply the backlog costs no block.
    fn send(&mut self, mut msg: Msg) {
        while let Err(rtrb::PushError::Full(back)) = self.inbox.push(msg) {
            self.runtime.lock().apply_pending();
            msg = back;
        }
    }

    fn slot_index(&mut self) -> usize {
        if let Some(idx) = self.free.pop() {
            return idx;
        }
        let bigger = self.slab_len * 2;
        self.send(Msg::Grow((0..bigger).map(|_| None).collect()));
        self.free.extend((self.slab_len + 1..bigger).rev());
        let idx = self.slab_len;
        self.slab_len = bigger;
        idx
    }

    /// Everything one node's runtime holds, read off the settled view: an audio-typed wire and a
    /// bare audio reference are plan edges, carried by the block rather than subscribed.
    fn desired_of(&self, view: &GraphView<'_>, uid: Uid, nv: &NodeView<'_>) -> Desired {
        let plane = Plane { kind: Some(SlotType::Audio), planned: &|b| plan::is_edge(b, &self.live), records: true };
        goofi_runtime::desired_of(view, uid, nv, nv.manifest.params, &plane)
    }

    /// A plugin's params as its controller counts them — normalized, in the plugin's own order —
    /// read off the record; `None` for a node that is no plugin, and where no window can show them.
    fn plugin_values(&self, uid: Uid, consts: &[Param]) -> Option<Vec<(u32, f64)>> {
        self.ui.as_ref()?;
        let inst = self.live.get(&uid)?;
        let derived = self.classes.get(inst.manifest.type_name)?.plugin.as_ref()?;
        let voice = consts.len() - derived.params.len();
        let values = derived
            .params
            .iter()
            .enumerate()
            .map(|(i, k)| (k.id, k.normalized(goofi_runtime::scalar(&consts[voice + i]) as f32)))
            .collect();
        Some(values)
    }

    /// Every enabled `type_name` node with the device its `param` names, by uid. For the outs the
    /// first is the clock's; the ins do not have to agree, and serve the ASIO driver rule alone.
    fn devices(&self, view: &GraphView<'_>, type_name: &str, param: usize) -> Vec<(Uid, String)> {
        let mut named: Vec<(Uid, String)> = self
            .live
            .iter()
            .filter(|(uid, inst)| inst.manifest.type_name == type_name && !self.disabled.contains_key(uid))
            .filter_map(|(uid, inst)| Some((*uid, plan::str_of(view.nodes.get(uid)?.params, &inst.manifest.params[param]))))
            .collect();
        named.sort_by_key(|(uid, _)| uid.0);
        named
    }

    /// Switch the output stream to `wanted`, the old one stopped first. A name that will not open
    /// is tried once: the previous clock reopens, and the error stands until the name moves.
    fn follow(&mut self, wanted: Option<&str>) -> Option<String> {
        if self.device.as_ref().map(|d| d.name.as_str()) == wanted {
            return None;
        }
        let Some(name) = wanted else {
            self.close();
            self.tried = None;
            self.opening = None;
            return None;
        };
        if let Some((tried, buffer, error)) = &self.tried {
            if tried == name && *buffer == self.audio.buffer() {
                return error.clone();
            }
        }
        if self.opening.as_ref().is_some_and(|o| o.name == name && o.buffer == self.audio.buffer()) {
            return None;
        }
        // The open runs on a thread of its own and the settle goes on: a device takes seconds to
        // answer, and the graph is not held for them. The clock lands at the settle after.
        let previous = self.close();
        let (runtime, stats, waker, buffer) = (self.runtime.clone(), self.stats.clone(), self.shared.waker.clone(), self.audio.buffer());
        let device = name.to_string();
        let clock = goofi_supervisor::ready::prepare(format!("goofi-audio-open {name}"), move || DeviceClock::open(&device, buffer, runtime, stats, waker));
        let drain = self.shared.waker.clone();
        clock.on_ready(move || drain.notify());
        self.opening = Some(Opening { name: name.to_string(), buffer, clock, previous });
        None
    }

    /// Every node whose making answered: its slot to the audio thread, whole, prepared again
    /// where the rate moved meanwhile; one that refused is out of the plan with its reason.
    fn land_births(&mut self) {
        let rate = self.audio.rate();
        let mut landed: Vec<(usize, Slot)> = Vec::new();
        let mut refused: Vec<(Uid, String)> = Vec::new();
        for (uid, inst) in self.live.iter_mut() {
            let Some(outcome) = inst.making.as_ref().and_then(|m| m.node.take()) else { continue };
            let Making { rate: made_at, mut unborn, born, .. } = inst.making.take().expect("read above");
            match outcome {
                Ok(mut node) => {
                    if made_at != rate {
                        node.prepare(rate);
                        unborn.inboxes.iter_mut().for_each(|inbox| inbox.rate = rate);
                    }
                    landed.push((unborn.idx, unborn.slot(*uid, node)));
                    born.store(true, Ordering::Release);
                }
                Err(why) => refused.push((*uid, why)),
            }
        }
        for (idx, slot) in landed {
            self.send(Msg::Insert { idx, slot });
        }
        for (uid, why) in refused {
            self.disabled.insert(uid, why);
        }
    }

    /// The device that answered its open, cut in and playing; one that refused is remembered as
    /// tried, and the device before it is asked back.
    fn land_device(&mut self) {
        let Some(outcome) = self.opening.as_ref().and_then(|o| o.clock.take()) else { return };
        let Opening { name, buffer, previous, .. } = self.opening.take().expect("read above");
        match outcome {
            Ok((clock, rate)) => {
                self.retune(rate, clock.channels);
                clock.play();
                self.device = Some(clock);
                self.tried = Some((name, buffer, None));
            }
            Err(error) => {
                self.tried = Some((name, buffer, Some(error)));
                if let Some(previous) = previous {
                    self.follow(Some(&previous));
                }
            }
        }
    }

    /// Stop the clock; the name it had. Its thread is waited for off this one.
    fn close(&mut self) -> Option<String> {
        let clock = self.device.take()?;
        self.runtime.lock().set_device(None);
        let name = clock.name.clone();
        let _ = goofi_supervisor::worker::spawn("goofi-audio-close", move || drop(clock));
        Some(name)
    }

    /// The device's rate and width, under the runtime lock: every instance — the ones still on
    /// the ring included — is re-prepared when the rate moved, and the FIFO is re-cut to the width.
    fn retune(&mut self, rate: f64, channels: u16) {
        let mut rt = self.runtime.lock();
        rt.apply_pending();
        if rate != self.audio.rate() {
            for slot in rt.slab.iter_mut().flatten() {
                slot.node.prepare(rate);
                slot.inboxes.iter_mut().for_each(|inbox| inbox.rate = rate);
            }
            self.audio.rate.store(rate.to_bits(), Ordering::Relaxed);
            rt.budget = Duration::from_secs_f64(BLOCK as f64 / rate) * runtime::BUDGET;
        }
        // Under the runtime lock, so the count and the clock name the same instant: what a block is
        // worth moved, and every block after this one is read against this tie.
        self.audio.anchor.tie(self.time.now(), rate);
        rt.set_device(Some(channels));
    }
}

impl AudioEngine {
    /// Rings sized to what each stage of `plan` carries, swapped in at both ends: the audio
    /// thread's by message ahead of the plan, the control half's at its next tick.
    fn fit_rings(&mut self, plan: &Plan) {
        let audio = self.audio.clone();
        for stage in &plan.stages {
            let Some((uid, inst)) = self.live.iter_mut().find(|(_, i)| i.idx == stage.idx && i.serial == stage.serial) else { continue };
            let mut rings = runtime::Rings::default();
            let mut swaps = audio.swaps.lock();
            for (i, minted) in inst.minted.inboxes.iter_mut().enumerate() {
                let floats = inst.wanted[i].load(Ordering::Relaxed);
                if floats == *minted {
                    continue;
                }
                *minted = floats;
                let (producer, consumer) = rtrb::RingBuffer::new(floats);
                rings.inboxes.push((i, Frames::new(consumer, Playback::of(inst.manifest), self.audio.rate())));
                swaps.entry(*uid).or_default().inboxes.push((i, producer));
            }
            for (o, minted) in inst.minted.outs.iter_mut().enumerate() {
                let width = stage.outs.get(o).map_or(1, |out| out.1);
                if width == *minted {
                    continue;
                }
                *minted = width;
                let (tap_in, tap_out) = rtrb::RingBuffer::new(control::tap_ring(width));
                let (rec_in, rec_out) = rtrb::RingBuffer::new(control::rec_ring(width));
                rings.taps.push((o, tap_in));
                rings.recs.push((o, rec_in));
                let held = swaps.entry(*uid).or_default();
                held.taps.push((o, tap_out));
                held.recs.push((o, rec_out));
            }
            drop(swaps);
            if rings.inboxes.is_empty() && rings.taps.is_empty() {
                continue;
            }
            let (idx, serial) = (inst.idx, inst.serial);
            self.send(Msg::Rings { idx, serial, rings });
        }
    }
}

impl Engine for AudioEngine {
    fn id(&self) -> &'static str {
        "audio"
    }

    fn dirty(&self) -> bool {
        self.dirty || self.shared.replan.load(Ordering::Acquire)
    }

    fn library(&self) -> Vec<LibraryEntry> {
        self.classes.values().map(|c| LibraryEntry { manifest: c.manifest, isolation: &NATIVE }).collect()
    }

    fn scan(&mut self, dir: &Path) -> Vec<goofi_node::ScannedType> {
        scan::scan(self, dir)
    }

    fn scan_own(&mut self) -> Vec<goofi_node::ScannedType> {
        let dirs = self.vst3.as_ref().map(|(_, dirs)| dirs.clone()).unwrap_or_default();
        dirs.iter().filter(|d| d.is_dir()).flat_map(|d| vst3::scan_dir(self, d)).collect()
    }

    fn remove_type(&mut self, type_name: &str) -> bool {
        !nodes::built_in(type_name) && self.classes.remove(type_name).is_some()
    }

    fn rust_sdk(&self) -> Option<&'static str> {
        Some(goofi_build::AUDIO.name)
    }

    fn set_workspace(&mut self, dir: &Path) {
        self.workspace = Some(dir.to_path_buf());
        self.sweep = true;
    }

    /// Every live node's state: an SDK node's under the runtime lock, where it is a copy, and a
    /// plugin's through its saver after — measured at 4 to 6 contended callbacks per save before.
    fn persist(&mut self) {
        let savers: HashMap<Uid, control::Saver> = self.audio.savers.lock().clone();
        let states: Vec<(Uid, &'static str, Option<Vec<u8>>)> = {
            let mut rt = self.runtime.lock();
            rt.apply_pending();
            rt.slab.iter().flatten().map(|s| (s.uid, s.type_name, (!savers.contains_key(&s.uid)).then(|| s.node.save()))).collect()
        };
        for (uid, type_name, bytes) in states {
            let bytes = bytes.unwrap_or_else(|| savers[&uid]());
            self.write_state(uid, type_name, bytes);
        }
    }

    /// A birth: the rings, the cells and the control half now, and the node itself — a plugin's
    /// instantiation, its `prepare`, its saved state — on a thread of its own. The slot goes to
    /// the audio thread whole once the node is there, and until then the plan leaves it out.
    fn insert(&mut self, uid: Uid, type_name: &str, generation: u64, params: &ParamGroups) -> Option<String> {
        let Some(Class { manifest, make, plugin }) = self.classes.get(type_name).cloned() else {
            return Some(format!("no audio node type `{type_name}`"));
        };
        let chans = Arc::new(AtomicU16::new(1));
        let (birth, ports) = rings_for(type_name, chans.clone(), uid, self.ui.clone(), self.audio.clone());
        let atomics: Arc<[AtomicU64]> = goofi_runtime::cells_of(params, manifest.params).into();
        let (inbox_in, inbox_out): (Vec<_>, Vec<_>) = manifest
            .inputs
            .iter()
            .filter(|s| s.kind != SlotType::Audio)
            .map(|_| rtrb::RingBuffer::<f32>::new(control::INBOX_SEED))
            .unzip();
        let (tap_in, tap_out): (Vec<_>, Vec<_>) =
            manifest.outputs.iter().map(|_| rtrb::RingBuffer::<f32>::new(control::tap_ring(1))).unzip();
        // Beside the tap and never in `rings_for`: a recording ring belongs to an OUTPUT, and the
        // rings there belong to a TYPE's OS handle.
        let (rec_in, rec_out): (Vec<_>, Vec<_>) = manifest
            .outputs
            .iter()
            .map(|_| rtrb::RingBuffer::<f32>::new(control::rec_ring(1)))
            .unzip();
        // The inboxes are built here so the plan can read their channel cells; the half itself is
        // made on its own thread, where an OS handle it opens never has to cross one.
        let inboxes: Vec<control::Inbox> = inbox_in.into_iter().map(control::Inbox::new).collect();
        let (inbox_chans, wanted) = AudioHalf::cells(&inboxes);
        let (blocks_in, blocks_out) = rtrb::RingBuffer::<f32>::new(control::BLOCKS_RING);
        let param_chans: Vec<Arc<AtomicU16>> = manifest.params.iter().map(|_| Arc::new(AtomicU16::new(1))).collect();
        let ramps: Arc<[runtime::Ramps]> = manifest.params.iter().map(|_| runtime::Ramps::default()).collect();
        let minted = Minted { inboxes: vec![control::INBOX_SEED; inbox_chans.len()], outs: vec![1; manifest.outputs.len()] };
        let spent = Arc::new(runtime::Spent::default());
        let born = Arc::new(AtomicBool::new(false));
        let control_birth = control::Birth {
            uid,
            manifest,
            params: atomics.clone(),
            ramps: ramps.clone(),
            evaluator: self.shared.evaluator.clone(),
            inboxes,
            blocks: blocks_in,
            param_chans: param_chans.clone(),
            taps: tap_out,
            recs: rec_out,
            ports,
            audio: self.audio.clone(),
            born: born.clone(),
        };
        let spawn = goofi_runtime::Spawn {
            engine: "audio",
            uid,
            instance: self.instance.clone(),
            base: goofi_transport::service_base(&self.instance, uid, generation),
            manifest,
            decls: manifest.params.to_vec(),
            params: atomics.clone(),
            time: self.time.clone(),
        };
        let control = match goofi_runtime::spawn(&self.hub, spawn, self.shared.clone(), move || AudioHalf::new(control_birth)) {
            Ok(handle) => handle,
            Err(e) => return Some(e),
        };
        let idx = self.slot_index();
        let serial = self.next_serial;
        self.next_serial += 1;
        let rate = self.audio.rate();
        // Read here, at the settle that births it: a restart's state is what its last box left.
        let state = self.state_path(uid, type_name).and_then(|p| std::fs::read(p).ok());
        let (ty, widest_in, widest_out, nparams) = (manifest.type_name, manifest.inputs.len(), manifest.outputs.len(), manifest.params.len());
        let making = make.clone();
        let node = goofi_supervisor::ready::prepare(format!("goofi-audio-birth {ty}"), move || {
            let mut node = making(birth);
            // Only the audio-rate params are ports; a control-rate one is a float in the scalar strip.
            let widest = node.audio_params(nparams).max(widest_in).max(widest_out);
            if widest > MAX_PORTS {
                return Err(format!("`{ty}` declares more than {MAX_PORTS} ports"));
            }
            node.prepare(rate);
            if let Some(bytes) = state {
                node.load(&bytes);
            }
            Ok(node)
        });
        let drain = self.shared.waker.clone();
        node.on_ready(move || drain.notify());
        let unborn = Unborn {
            idx,
            serial,
            type_name: manifest.type_name,
            params: atomics,
            ramps,
            inboxes: inbox_out.into_iter().map(|ring| Frames::new(ring, Playback::of(manifest), rate)).collect(),
            blocks: blocks_out,
            taps: tap_in,
            recs: rec_in,
            spent: spent.clone(),
        };
        let twin = make(nodes::Birth { chans, ..Default::default() });
        let shown = plugin.map(|d| d.params.iter().map(|k| k.default).collect()).unwrap_or_default();
        let making = Some(Making { node, rate, unborn, born });
        self.live.insert(uid, Instance { idx, serial, manifest, twin, control, chans: inbox_chans, param_chans, wanted, minted, shown, spent, making });
        self.dirty = true;
        self.shared.waker.notify();
        None
    }

    fn remove(&mut self, uid: Uid) {
        if let Some(inst) = self.live.remove(&uid) {
            inst.control.stop();
            // A swap the half never took would be adopted by the uid's next half, over fresh rings.
            self.audio.swaps.lock().remove(&uid);
            self.send(Msg::Remove(inst.idx));
            // The box comes back NOW, its state kept, so a restart's birth finds it: one callback
            // may find the runtime taken — a click at an authoring event, accepted.
            self.runtime.lock().apply_pending();
            self.discard_retired();
            self.free.push(inst.idx);
            self.disabled.remove(&uid);
            self.faults.forget(uid);
            self.pending.retain(|(u, _)| *u != uid);
            self.dirty = true;
        }
    }

    fn settle(&mut self, view: &GraphView<'_>, changed: &[Uid]) {
        self.land_births();
        self.land_device();
        self.dirty = false;
        if std::mem::take(&mut self.sweep) {
            self.sweep_state();
        }
        self.shared.replan.store(false, Ordering::Release);
        for uid in changed.iter().copied().filter(|u| self.live.contains_key(u)).collect::<Vec<_>>() {
            let Some(nv) = view.nodes.get(&uid) else { continue };
            let desired = self.desired_of(view, uid, nv);
            let values = self.plugin_values(uid, &desired.consts);
            if !self.live[&uid].control.send_if_changed(desired) {
                continue;
            }
            // Only what goofi moved is told to a plugin's window: the rest is the plugin's own.
            let (Some(ui), Some(values)) = (&self.ui, values) else { continue };
            let inst = self.live.get_mut(&uid).expect("looked up above");
            let moved: Vec<(u32, f64)> =
                values.iter().zip(&inst.shown).filter(|((_, v), was)| v.to_bits() != was.to_bits()).map(|(p, _)| *p).collect();
            inst.shown = values.iter().map(|(_, v)| *v).collect();
            if !moved.is_empty() {
                ui.post(move |_| vst3::editor::sync(uid, moved));
            }
        }
        let outs = self.devices(view, audio_out::TYPE, audio_out::P::DEVICE);
        let clock = outs.first().map(|(_, device)| device.clone());
        let agrees = |device: &String| Some(device) == clock.as_ref();
        let mut faults: Vec<(Uid, String)> = outs
            .iter()
            .filter(|(_, device)| !agrees(device))
            .map(|(uid, _)| (*uid, format!("the clock is on `{}`", clock.as_deref().unwrap_or_default())))
            .collect();
        // One ASIO driver per process ([`host::asio_driver`]): the clock's wins, and a node naming
        // another is refused here.
        let ins = self.devices(view, audio_in::TYPE, audio_in::P::DEVICE);
        let held = outs.iter().chain(ins.iter()).find_map(|(_, d)| host::asio_driver(d));
        if let Some(held) = held {
            let strays = outs.iter().chain(ins.iter()).filter(|(_, d)| host::asio_driver(d).is_some_and(|k| k != held));
            faults.extend(strays.map(|(uid, _)| (*uid, format!("the ASIO driver is `{held}`, and only one loads at a time"))));
        }
        if owns_devices(self.clock) {
            // A moved buffer size is a new open of the same name: the clock closes so `follow`
            // reopens it.
            let buffer = view.variables.get("audio.buffer_size").map_or(0.0, goofi_core::control::number_of);
            let buffer = if buffer.is_finite() && buffer >= 0.0 { buffer.round() as u32 } else { 0 };
            self.audio.buffer.store(buffer, Ordering::Relaxed);
            if self.device.as_ref().is_some_and(|d| d.buffer != buffer) {
                self.close();
            }
            if let Some(why) = self.follow(clock.as_deref()) {
                faults.extend(outs.iter().filter(|(_, device)| agrees(device)).map(|(uid, _)| (*uid, why.clone())));
            }
        }
        let silent: Vec<Uid> = faults.iter().map(|(u, _)| *u).collect();
        faults.extend(self.disabled.iter().map(|(u, m)| (*u, m.clone())));
        // A node still being made is out of the plan, with no fault: it is in setup.
        let mut out = self.disabled.clone();
        out.extend(self.live.iter().filter(|(_, i)| i.making.is_some()).map(|(u, _)| (*u, String::new())));
        let (plan, looped) = plan::compile(view, &self.live, &silent, &out);
        faults.extend(looped);
        let since = self.time.now();
        self.pending.extend(self.faults.settle(faults, since));
        self.fit_rings(&plan);
        if plan != self.last {
            let arena = vec![0.0; plan.arena_len];
            self.send(Msg::Plan { plan: plan.clone(), arena });
            self.last = plan;
        }
        if !self.pending.is_empty() {
            self.shared.waker.notify();
        }
    }

    /// What the audio plane alone decides, off the same `status()` the report reads, so the
    /// variables and `session status` cannot drift.
    fn published(&self) -> Vec<(&'static str, goofi_core::Data)> {
        use goofi_core::Data;
        let s = self.status();
        let device = s.device.unwrap_or_default();
        let driver = host::asio_driver(&device).unwrap_or_default().to_string();
        vec![
            ("audio.rate", Data::number(s.rate)),
            ("audio.channels", Data::number(f64::from(s.channels))),
            ("audio.driver", Data::text(driver)),
            ("audio.device", Data::text(device)),
            ("audio.hosts", Data::text(host::hosts())),
        ]
    }

    fn drain(&mut self, apply: &mut dyn FnMut(Uid, Status)) -> usize {
        // A node or a device whose making answered lands at the settle this asks for, never here:
        // the slot and the plan that uses it go to the audio thread in one step.
        if self.live.values().any(|i| i.making.as_ref().is_some_and(|m| m.node.is_ready()))
            || self.opening.as_ref().is_some_and(|o| o.clock.is_ready())
        {
            self.dirty = true;
        }
        self.discard_retired();
        // A stream that died is closed here, and its name tried once more at the settle this asks for.
        if self.stats.dead.swap(false, Ordering::Acquire) {
            self.close();
            self.tried = None;
            self.dirty = true;
        }
        self.shared.drain(&mut self.pending, &|u, born| self.live.get(&u).is_some_and(|i| i.control.born() == born), apply)
    }

    /// A refresh runs on the node's own thread, never under the graph lock.
    /// Both reach the control half; a pulse raises the param for one control tick.
    fn request(&mut self, uid: Uid, request: goofi_node::Request) {
        if let Some(inst) = self.live.get(&uid) {
            inst.control.request(request);
        }
    }

    /// A plugin has one, where a window can be opened at all.
    fn has_editor(&self, type_name: &str) -> bool {
        self.ui.is_some() && self.classes.get(type_name).is_some_and(|c| c.plugin.is_some())
    }

    fn editor(&mut self, uid: Uid, show: bool) -> Result<EditorAction, String> {
        let ui = self.ui.clone().ok_or("no window: goofi has no display here")?;
        let inst = self.live.get(&uid).ok_or_else(|| format!("no such node {uid}"))?;
        let title = format!("{} — goofi", inst.manifest.type_name);
        Ok(Box::new(move || ui.run(|host| vst3::editor::show(host, uid, &title, show))))
    }

    /// What every editor wrote since the last call, as the record spells it — latest wins per
    /// param, because a drag is many edits and the document wants the last.
    fn take_edits(&mut self) -> Vec<Edit> {
        let raw = std::mem::take(&mut *self.audio.edits.lock());
        let mut edits: Vec<Edit> = Vec::new();
        for (uid, id, v, origin) in raw {
            let Some(inst) = self.live.get(&uid) else { continue };
            let Some(derived) = self.classes.get(inst.manifest.type_name).and_then(|c| c.plugin.as_ref()) else { continue };
            let Some(i) = derived.params.iter().position(|k| k.id == id) else { continue };
            let decl = &inst.manifest.params[inst.manifest.params.len() - derived.params.len() + i];
            let steps = derived.params[i].steps;
            let mut value = decl.spec.to_param();
            match &mut value {
                Param::Num { value, int, .. } => value[0] = if *int { (v.clamp(0.0, 1.0) * steps).round() } else { v.clamp(0.0, 1.0) },
                Param::Str { value, options: Some(options), .. } => {
                    *value = options[((v.clamp(0.0, 1.0) * steps).round() as usize).min(options.len() - 1)].clone()
                }
                _ => continue,
            }
            let key = ParamKey::new(decl.group, decl.name);
            edits.retain(|e| !(e.uid == uid && e.key == key));
            edits.push(Edit { uid, key, value, origin });
        }
        edits
    }

    fn set_evaluator(&mut self, evaluator: Arc<dyn goofi_node::ExprEvaluator>) {
        *self.shared.evaluator.lock() = Some(evaluator);
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// Stop the clock and every control half, and WAIT for each to release its shared memory — a
    /// ceiling, because only a process about to EXIT has no "a moment later".
    fn shutdown(&mut self) {
        // Waited for here, not on a thread: nothing of the engine may outlive this.
        self.opening = None;
        if let Some(clock) = self.device.take() {
            self.runtime.lock().set_device(None);
            drop(clock);
        }
        goofi_runtime::stop_all(self.live.values().map(|i| &i.control));
        for uid in self.live.keys().copied().collect::<Vec<_>>() {
            self.remove(uid);
        }
        self.runtime.lock().render_block();
        self.discard_retired();
    }
}
