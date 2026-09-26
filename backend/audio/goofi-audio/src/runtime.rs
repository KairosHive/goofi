//! What the audio thread owns, and one block of it. Nothing here allocates, locks or blocks:
//! every message is a pointer move, every port is a view into the arena the plan laid out.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use goofi_audio_sdk::{cross, AudioNode, Block, Port, PortMut, BLOCK, MAX_CHANNELS, MAX_PORTS};
use goofi_node::{NodeManifest, ParamSpec, Uid};

use crate::plan::{Plan, Source, SILENCE};

/// Blocks in a row a node may exceed its budget before it leaves the plan.
pub const OVERRUNS: u8 = 8;

/// Blocks of wall time one block's `process` is given. A node near ONE block is heavy, not broken:
/// a plugin measured at 1.3-2.0ms against a 1.33ms block crossed the line on 127 of 57330 blocks
/// and cost no underrun at all, so a budget with no margin ejects on the scheduler's noise.
pub const BUDGET: u32 = 4;

/// Publish one complete frame, or drop it if the ring is full: the header and the samples land
/// in one reserved chunk, so no partial frame is ever visible.
fn publish(ring: &mut rtrb::Producer<f32>, header: &[f32], samples: &[f32]) {
    let Ok(mut chunk) = ring.write_chunk(header.len() + samples.len()) else { return };
    let (first, second) = chunk.as_mut_slices();
    let mut values = header.iter().chain(samples).copied();
    for slot in first.iter_mut().chain(second.iter_mut()) {
        *slot = values.next().unwrap_or(0.0);
    }
    chunk.commit_all();
}

pub struct Slot {
    pub uid: Uid,
    pub type_name: &'static str,
    /// Which occupant of the index this is — what a stage compiled for another one checks.
    pub serial: u64,
    pub node: Box<dyn AudioNode>,
    /// The scalar per param, `f64` bits, written by the node's control half.
    pub params: Arc<[AtomicU64]>,
    /// One per Array input, in declaration order.
    pub inboxes: Vec<Frames>,
    /// One per output: what the control half publishes to whoever subscribes.
    pub taps: Vec<rtrb::Producer<f32>>,
    /// One per output: every block, whole, for the recorder.
    pub recs: Vec<rtrb::Producer<f32>>,
    /// Out of the plan: it panicked or the watchdog took it, and its outputs are zero until the
    /// settle that re-plans without it.
    pub dead: bool,
    pub overruns: u8,
    /// What the watchdog takes one `process` to cost in place of the wall time it took, when stated.
    pub cost: Option<Duration>,
}

/// The audio thread's end of a device's or a file's feed: chunks of interleaved samples, each
/// headed by its channel count and length, read one sample per sample, the last one held.
pub struct Inbox {
    ring: rtrb::Consumer<f32>,
    chans: usize,
    left: usize,
    last: [f32; MAX_CHANNELS as usize],
    /// Whether a pile-up is latency to drop. A live source has no past to keep; a file does, and
    /// dropping there would skip through it.
    catch_up: bool,
}

/// Chunks a block may leave queued behind the one in hand. What a stalled clock let pile up is
/// dropped past this, so a period rendered late is latency dropped, never latency kept.
const QUEUED: usize = 2;

impl Inbox {
    pub fn new(ring: rtrb::Consumer<f32>, catch_up: bool) -> Inbox {
        Inbox { ring, chans: 0, left: 0, last: [0.0; MAX_CHANNELS as usize], catch_up }
    }

    /// Skip to the last `QUEUED` chunks when more than that wait behind the one in hand.
    fn catch_up(&mut self) {
        if keep_newest(&mut self.ring, self.left * self.chans, QUEUED) {
            self.left = 0;
        }
    }

    /// One block: per channel the next sample entered, or the last one held.
    pub fn fill(&mut self, out: &mut PortMut<'_>) {
        if self.catch_up {
            self.catch_up();
        }
        let channels = out.channels();
        for i in 0..BLOCK {
            if self.left == 0 {
                if let Ok(head) = self.ring.read_chunk(2) {
                    let mut head = head.into_iter();
                    self.chans = head.next().unwrap_or(0.0) as usize;
                    self.left = head.next().unwrap_or(0.0) as usize;
                }
            }
            if self.left > 0 {
                match self.ring.read_chunk(self.chans) {
                    Ok(sample) => {
                        // `chans` is read off the RING, so it is the producer's word and not this
                        // side's: a device wider than `MAX_CHANNELS` — an ASIO card answers with
                        // eighteen — would index past `last` and panic in the audio callback. The
                        // extra channels are dropped here rather than trusted.
                        for (c, v) in sample.into_iter().enumerate().take(self.last.len()) {
                            self.last[c] = v;
                        }
                        self.left -= 1;
                    }
                    Err(_) => self.left = 0,
                }
            }
            for c in 0..channels as usize {
                out.chan_mut(c)[i] = match (self.chans, c) {
                    (1, _) => self.last[0],
                    (n, c) if c < n => self.last[c],
                    _ => 0.0,
                };
            }
        }
    }
}

/// Drop all but the newest `keep` (at most `QUEUED`) chunks queued `from` samples on; answers
/// whether any went.
fn keep_newest(ring: &mut rtrb::Consumer<f32>, from: usize, keep: usize) -> bool {
    let Ok(readable) = ring.read_chunk(ring.slots()) else { return false };
    let (a, b) = readable.as_slices();
    let at = |i: usize| if i < a.len() { a[i] } else { b[i - a.len()] };
    let len = a.len() + b.len();
    let (mut count, mut recent) = (0, [0; QUEUED]);
    let mut i = from;
    while i + 2 <= len {
        recent[count % keep] = i;
        count += 1;
        i += 2 + at(i) as usize * at(i + 1) as usize;
    }
    if count > keep {
        readable.commit(recent[count % keep]);
    }
    count > keep
}

/// Where a node's `mode` of [`cross::PLAYBACK`] and the `smoothing` beside it sit in its params.
#[derive(Clone, Copy, Default)]
pub struct Playback {
    mode: Option<usize>,
    smoothing: Option<usize>,
}

impl Playback {
    pub fn of(manifest: &NodeManifest) -> Playback {
        let params = &manifest.params;
        let mode = params.iter().position(|d| matches!(d.spec, ParamSpec::Str { options, .. } if options == cross::PLAYBACK));
        let smoothing = mode.and_then(|m| params.iter().position(|d| d.group == params[m].group && d.name == "smoothing"));
        Playback { mode, smoothing }
    }

    pub fn mode(&self) -> Option<usize> {
        self.mode
    }

    /// The option of [`cross::PLAYBACK`] in force.
    pub fn playing(&self, params: &[AtomicU64]) -> &'static str {
        let i = self.mode.map_or(0.0, |m| f64::from_bits(params[m].load(Ordering::Relaxed))).round();
        cross::PLAYBACK.get(i.max(0.0) as usize).copied().unwrap_or(cross::PLAYBACK[0])
    }

    fn smoothing(&self, params: &[AtomicU64]) -> f64 {
        self.smoothing.map_or(0.0, |s| f64::from_bits(params[s].load(Ordering::Relaxed)).max(0.0))
    }
}

/// One frame in hand: interleaved samples, `len` per channel, and where the loop reads next.
struct Voice {
    buf: Vec<f32>,
    chans: usize,
    len: usize,
    pos: usize,
}

impl Voice {
    /// Room for the largest chunk the ring holds, so a take never allocates.
    fn new() -> Voice {
        Voice { buf: Vec::with_capacity(crate::control::INBOX_RING), chans: 0, len: 0, pos: 0 }
    }

    fn sample(&self, c: usize) -> f32 {
        match (self.chans, c) {
            (0, _) => 0.0,
            (1, _) => self.buf[self.pos],
            (n, c) if c < n => self.buf[self.pos * n + c],
            _ => 0.0,
        }
    }

    fn advance(&mut self) {
        if self.len > 0 {
            self.pos = (self.pos + 1) % self.len;
        }
    }

    /// The next whole chunk off `ring` in place of this one; the control half commits each at once.
    fn take(&mut self, ring: &mut rtrb::Consumer<f32>) -> bool {
        let Ok(head) = ring.read_chunk(2) else { return false };
        let (a, b) = head.as_slices();
        let at = |i: usize| if i < a.len() { a[i] } else { b[i - a.len()] };
        let (chans, len) = (at(0) as usize, at(1) as usize);
        head.commit_all();
        let Ok(body) = ring.read_chunk(chans * len) else { return false };
        let (a, b) = body.as_slices();
        self.buf.clear();
        self.buf.extend_from_slice(a);
        self.buf.extend_from_slice(b);
        body.commit_all();
        (self.chans, self.len, self.pos) = (chans, len, 0);
        len > 0
    }
}

/// The audio thread's end of an Array input. Each frame is entered whole and played until the
/// next: looped as a waveform, or as one sine per value, summed on one channel.
pub struct Frames {
    ring: rtrb::Consumer<f32>,
    playback: Playback,
    pub rate: f64,
    now: Voice,
    next: Voice,
    /// Samples into the crossfade from `now` to `next`, of how many; `(0, 0)` while none runs.
    fade: (usize, usize),
    oscillating: bool,
    /// Per sine, one for every value a frame can hold: its pitch and phase offset, gliding, and
    /// its phasor.
    hz: Vec<f32>,
    offset: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Frames {
    pub fn new(ring: rtrb::Consumer<f32>, playback: Playback, rate: f64) -> Frames {
        Frames {
            ring,
            playback,
            rate,
            now: Voice::new(),
            next: Voice::new(),
            fade: (0, 0),
            oscillating: false,
            hz: Vec::with_capacity(crate::control::INBOX_RING),
            offset: Vec::with_capacity(crate::control::INBOX_RING),
            re: Vec::with_capacity(crate::control::INBOX_RING),
            im: Vec::with_capacity(crate::control::INBOX_RING),
        }
    }

    /// Empty the ring and forget the frame in hand — what the previous producer left.
    pub fn flush(&mut self) {
        if let Ok(chunk) = self.ring.read_chunk(self.ring.slots()) {
            chunk.commit_all();
        }
        self.forget();
    }

    fn forget(&mut self) {
        (self.now.len, self.now.chans, self.now.pos, self.fade) = (0, 0, 0, (0, 0));
        self.hz.clear();
        self.offset.clear();
    }

    pub fn fill(&mut self, out: &mut PortMut<'_>, params: &[AtomicU64]) {
        let oscillator = self.playback.playing(params) == "oscillator";
        if oscillator != self.oscillating {
            // What the other mode entered means nothing to this one.
            self.oscillating = oscillator;
            self.forget();
        }
        let smoothing = (self.playback.smoothing(params) * self.rate) as usize;
        match oscillator {
            true => self.oscillate(out, smoothing),
            false => self.loop_frames(out, smoothing),
        }
    }

    /// A new frame takes over at the loop's end, so frames on time play back to back; with
    /// smoothing it is crossfaded in as it arrives, the old one looping underneath.
    fn loop_frames(&mut self, out: &mut PortMut<'_>, smoothing: usize) {
        keep_newest(&mut self.ring, 0, if smoothing > 0 { 1 } else { QUEUED });
        let channels = out.channels() as usize;
        for i in 0..BLOCK {
            if self.fade.1 == 0 && (self.now.pos == 0 || smoothing > 0) && self.next.take(&mut self.ring) {
                match self.now.len == 0 || smoothing == 0 || self.next.chans != self.now.chans {
                    true => std::mem::swap(&mut self.now, &mut self.next),
                    false => self.fade = (0, smoothing),
                }
            }
            let a = if self.fade.1 > 0 { self.fade.0 as f32 / self.fade.1 as f32 } else { 0.0 };
            for c in 0..channels {
                let mixed = match self.fade.1 {
                    0 => self.now.sample(c),
                    _ => self.now.sample(c) * (1.0 - a) + self.next.sample(c) * a,
                };
                out.chan_mut(c)[i] = mixed;
            }
            self.now.advance();
            if self.fade.1 > 0 {
                self.next.advance();
                self.fade.0 += 1;
                if self.fade.0 >= self.fade.1 {
                    std::mem::swap(&mut self.now, &mut self.next);
                    self.fade = (0, 0);
                }
            }
        }
    }

    /// One sine per column of the newest frame, `[n]` pitches in Hz or `[2, n]` pitches over
    /// phases in radians, their mean on every channel. A sine runs on across frames, its phase an offset
    /// on top, and with smoothing both glide. Each is a phasor turned once a sample, so a frame of
    /// thousands costs a few multiplies per sine.
    fn oscillate(&mut self, out: &mut PortMut<'_>, smoothing: usize) {
        keep_newest(&mut self.ring, 0, 1);
        if self.next.take(&mut self.ring) {
            std::mem::swap(&mut self.now, &mut self.next);
        }
        let (width, n) = (self.now.chans, self.now.len);
        let row = |v: usize| (self.now.buf[v], if width == 2 { self.now.buf[n + v] } else { 0.0 });
        // Within the capacity reserved at birth, so nothing here allocates. A new sine starts where
        // its row says, with no glide from a sine it never was.
        let was = self.hz.len().min(n);
        for v in [&mut self.hz, &mut self.offset, &mut self.re, &mut self.im] {
            v.truncate(was);
        }
        for v in was..n {
            let (hz, phase) = row(v);
            self.hz.push(hz);
            self.offset.push(phase);
            self.re.push(1.0);
            self.im.push(0.0);
        }
        let k = if smoothing > 0 { 1.0 - (-(BLOCK as f32) / smoothing as f32).exp() } else { 1.0 };
        let turn = std::f64::consts::TAU / self.rate;
        let mut mix = [0.0f32; BLOCK];
        for v in 0..n {
            let (hz, phase) = row(v);
            self.hz[v] += (hz - self.hz[v]) * k;
            // The short way round, so a phase that wraps past pi does not spin the long way.
            let turned = (phase - self.offset[v] + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            self.offset[v] += turned * k;
            let (s, c) = (turn * f64::from(self.hz[v])).sin_cos();
            let (s, c) = (s as f32, c as f32);
            let (ps, pc) = self.offset[v].sin_cos();
            let (mut x, mut y) = (self.re[v], self.im[v]);
            for m in &mut mix {
                *m += y * pc + x * ps;
                (x, y) = (x * c - y * s, x * s + y * c);
            }
            // Back onto the unit circle, which rounding walks it off.
            let norm = x.hypot(y).max(f32::MIN_POSITIVE);
            (self.re[v], self.im[v]) = (x / norm, y / norm);
        }
        let scale = if n > 0 { 1.0 / n as f32 } else { 0.0 };
        for c in 0..out.channels() as usize {
            for (y, m) in out.chan_mut(c).iter_mut().zip(&mix) {
                *y = m * scale;
            }
        }
    }
}

/// The header a recording ring's block wears: its width, its NUMBER in two exact halves, and the
/// tie its number is read against. A block that never reaches the ring takes its number with it,
/// which is what makes a lost one a gap the recorder counts rather than a silence.
pub const REC_HEADER: usize = 4;

/// One tie between the rendered block count and the patch clock, and what a block is worth after
/// it. Made only where the runtime lock is HELD, so no block can be rendered between the two reads.
#[derive(Clone, Copy)]
struct Tie {
    at: u64,
    time: f64,
    rate: f64,
}

/// Ties kept, so a block still in a ring when the device moved is read against the tie it was
/// rendered under. Eight is more device changes than a one-second ring can outlive.
const TIES: usize = 8;

/// The block count, and the ties that turn a number into an instant. The count is the CLOCK: no
/// clock is read on the audio thread, and none is read where a block is drained.
pub struct Anchor {
    /// Blocks rendered since the engine began.
    pub blocks: AtomicU64,
    /// What the audio thread stamps into every block; a new tie is a new epoch.
    epoch: AtomicU64,
    ties: Mutex<Vec<(u64, Tie)>>,
    time: Arc<goofi_core::time::Time>,
}

impl Anchor {
    pub fn new(time: Arc<goofi_core::time::Time>) -> Anchor {
        let tie = Tie { at: 0, time: time.now(), rate: crate::RATE };
        Anchor {
            blocks: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
            ties: Mutex::new(vec![(0, tie)]),
            time,
        }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Vec<(u64, Tie)>> {
        self.ties.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Tie the clock to the block that will be rendered NEXT. The caller holds the runtime lock, so
    /// no block is rendered between reading the count and reading the clock.
    pub fn tie(&self, time: f64, rate: f64) {
        let tie = Tie { at: self.blocks.load(Ordering::Relaxed), time, rate };
        let epoch = self.epoch.load(Ordering::Relaxed) + 1;
        let mut ties = self.held();
        ties.push((epoch, tie));
        if ties.len() > TIES {
            ties.remove(0);
        }
        drop(ties);
        self.epoch.store(epoch, Ordering::Release);
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// Patch seconds at engine block `n`, read against the tie `epoch` names — the one the block
    /// was rendered under, never whichever is newest. An epoch older than what is held falls to the
    /// oldest, which is a block that outlived eight device changes in a one-second ring.
    pub fn seconds(&self, n: u64, epoch: u64) -> f64 {
        let ties = self.held();
        let tie = ties.iter().find(|(e, _)| *e == epoch).or_else(|| ties.first()).map(|(_, t)| *t);
        let Some(tie) = tie else { return 0.0 };
        tie.time + (n as f64 - tie.at as f64) * BLOCK as f64 / tie.rate
    }

    /// How far this timeline stands ahead of patch time right now. The tie is taken ONCE, so the
    /// device's rate error walks the two apart; the manifest carries this so an analyst can
    /// correct it, rather than a re-tie that would put a seam in the exact block spacing.
    pub fn drift(&self) -> f64 {
        let n = self.blocks.load(Ordering::Relaxed);
        self.seconds(n, self.epoch()) - self.time.now()
    }
}

/// A block's number, split so both halves are exact in an `f32`: 48 bits, which is more blocks than
/// a machine renders in a lifetime.
const HALF: u64 = 1 << 24;

pub fn number_out(n: u64) -> (f32, f32) {
    ((n % HALF) as f32, (n / HALF) as f32)
}

pub fn number_in(lo: f32, hi: f32) -> u64 {
    hi as u64 * HALF + lo as u64
}

pub enum Msg {
    Insert { idx: usize, slot: Slot },
    Remove(usize),
    Plan { plan: Plan, arena: Vec<f32> },
    Grow(Vec<Option<Slot>>),
    RecordBoundary { window: Arc<goofi_core::record::FrameWindow>, begin: bool, done: Arc<AtomicBool> },
}

/// Why a node left the plan.
pub enum Fault {
    Panic(String),
    Overrun,
    NotANumber,
}

/// What comes back to be dropped off the audio thread — and what it put out of the plan.
pub enum Retired {
    Slot(Slot),
    Plan(Plan, Vec<f32>),
    Slab(Vec<Option<Slot>>),
    RecordBoundary(Arc<goofi_core::record::FrameWindow>, Arc<AtomicBool>),
    Faulted { uid: Uid, serial: u64, fault: Fault },
}

pub struct Runtime {
    pub slab: Vec<Option<Slot>>,
    pub plan: Plan,
    pub arena: Vec<f32>,
    pub inbox: rtrb::Consumer<Msg>,
    pub outbox: rtrb::Producer<Retired>,
    /// Rendered output not yet handed to the device, interleaved at `channels()`.
    pub fifo: Vec<f32>,
    /// The device's width while a device is the clock; the summed output's own otherwise.
    device: Option<u16>,
    /// What a node's `process` is held to: `BUDGET` blocks of wall time at the rate.
    pub budget: Duration,
    /// The block count, and its one tie to the clock.
    pub anchor: Arc<Anchor>,
}

impl Runtime {
    pub fn new(slab: usize, inbox: rtrb::Consumer<Msg>, outbox: rtrb::Producer<Retired>, anchor: Arc<Anchor>) -> Runtime {
        Runtime {
            slab: (0..slab).map(|_| None).collect(),
            plan: Plan::default(),
            arena: vec![0.0; BLOCK],
            inbox,
            outbox,
            fifo: Vec::new(),
            device: None,
            budget: Duration::from_secs_f64(BLOCK as f64 / crate::RATE) * BUDGET,
            anchor,
        }
    }

    /// The width the FIFO is interleaved at.
    pub fn channels(&self) -> u16 {
        self.device.unwrap_or(self.plan.output.1)
    }

    pub fn set_device(&mut self, channels: Option<u16>) {
        self.device = channels;
        self.fifo.clear();
    }

    /// Fill one device buffer: whole blocks until enough is rendered, the surplus carried in
    /// place, so the FIFO keeps its allocation.
    pub fn render_into(&mut self, out: &mut [f32]) {
        while self.fifo.len() < out.len() {
            self.render_block();
        }
        out.copy_from_slice(&self.fifo[..out.len()]);
        self.fifo.drain(..out.len());
    }

    fn apply(&mut self, msg: Msg) {
        let retired = match msg {
            Msg::RecordBoundary { window, begin, done } => {
                let block = self.anchor.blocks.load(Ordering::Relaxed);
                if begin { window.begin(block); } else { window.finish(block); }
                done.store(true, Ordering::Release);
                Some(Retired::RecordBoundary(window, done))
            }
            Msg::Insert { idx, slot } => self.slab[idx].replace(slot).map(Retired::Slot),
            Msg::Remove(idx) => self.slab[idx].take().map(Retired::Slot),
            Msg::Plan { plan, arena } => {
                let old = std::mem::replace(&mut self.plan, plan);
                let old_arena = std::mem::replace(&mut self.arena, arena);
                // An inbox the new plan no longer reads is flushed, or what its last producer
                // left would play first when the input is wired again.
                for stage in &old.stages {
                    for src in &stage.ins {
                        let Source::Inbox { inbox, .. } = src else { continue };
                        if !self.plan.reads_inbox(stage.idx, *inbox) {
                            if let Some(slot) = self.slab[stage.idx].as_mut().filter(|s| s.serial == stage.serial) {
                                slot.inboxes[*inbox].flush();
                            }
                        }
                    }
                }
                Some(Retired::Plan(old, old_arena))
            }
            Msg::Grow(mut bigger) => {
                for (i, s) in self.slab.iter_mut().enumerate() {
                    bigger[i] = s.take();
                }
                Some(Retired::Slab(std::mem::replace(&mut self.slab, bigger)))
            }
        };
        if let Some(r) = retired {
            let _ = self.outbox.push(r);
        }
    }

    /// Take every message the control half sent; a pointer move each.
    pub fn apply_pending(&mut self) {
        while let Ok(msg) = self.inbox.pop() {
            self.apply(msg);
        }
    }

    /// One block: drain the inbox, run every stage in plan order, append the output to the fifo.
    /// A stage whose index another occupant took since the plan was compiled waits for its own.
    pub fn render_block(&mut self) {
        self.apply_pending();
        let n = self.anchor.blocks.load(Ordering::Relaxed);
        let epoch = self.anchor.epoch();
        let width = self.channels() as usize;
        let budget = self.budget;
        let arena: &mut [f32] = &mut self.arena;
        for stage in &self.plan.stages {
            let Some(slot) = self.slab[stage.idx].as_mut().filter(|s| s.serial == stage.serial) else { continue };
            for src in stage.params.iter().chain(&stage.ins) {
                match src {
                    Source::Scalar { at, param } => {
                        let v = f64::from_bits(slot.params[*param].load(Ordering::Relaxed)) as f32;
                        let region = carve(arena, &[(*at, BLOCK)]).take(0);
                        if region[0] != v {
                            region.fill(v);
                        }
                    }
                    Source::Sum { at, channels, parts } => {
                        let mut carved = carve(arena, &[(*at, *channels as usize * BLOCK)]);
                        let dst = carved.take(0);
                        dst.fill(0.0);
                        for (part, pc) in parts {
                            let src = Port::new(carved.read(*part, *pc as usize * BLOCK), *pc, true);
                            for c in 0..*channels as usize {
                                let from = src.chan(c);
                                for i in 0..BLOCK {
                                    dst[c * BLOCK + i] += from[i];
                                }
                            }
                        }
                    }
                    Source::Inbox { at, channels, inbox } => {
                        let region = carve(arena, &[(*at, *channels as usize * BLOCK)]).take(0);
                        slot.inboxes[*inbox].fill(&mut PortMut::new(region, *channels), &slot.params);
                    }
                    Source::Silence | Source::Region { .. } => {}
                }
            }
            // The stage writes its outputs and its scalar strip; everything else it reads.
            let mut wants = [(0, 0); MAX_PORTS + 1];
            for (k, (at, channels)) in stage.outs.iter().enumerate() {
                wants[k] = (*at, *channels as usize * BLOCK);
            }
            wants[stage.outs.len()] = (stage.scalars_at, stage.params.len());
            let (ran, started) = {
                let mut carved = carve(arena, &wants[..stage.outs.len() + 1]);
                // Every param's one value for this block, settled after the sources above, so a
                // control rate param reaches the node without costing a port.
                let scalars = carved.take(stage.outs.len());
                for (i, src) in stage.params.iter().enumerate() {
                    scalars[i] = carved.first(src);
                }
                let ins: [Port<'_>; MAX_PORTS] = std::array::from_fn(|i| match stage.ins.get(i) {
                    Some(s) => carved.port(s),
                    None => Port::new(&[], 0, false),
                });
                let params: [Port<'_>; MAX_PORTS] = std::array::from_fn(|i| match stage.params.get(i).filter(|_| i < stage.audio_params) {
                    Some(s) => carved.port(s),
                    None => Port::new(&[], 0, false),
                });
                let mut outs: [PortMut<'_>; MAX_PORTS] = std::array::from_fn(|i| match stage.outs.get(i) {
                    Some((_, channels)) => PortMut::new(carved.take(i), *channels),
                    None => PortMut::new(&mut [], 0),
                });
                let mut block = Block {
                    ins: &ins[..stage.ins.len()],
                    outs: &mut outs[..stage.outs.len()],
                    params: &params[..stage.audio_params],
                    scalars,
                };
                let started = Instant::now();
                (if slot.dead { None } else { Some(catch_unwind(AssertUnwindSafe(|| slot.node.process(&mut block)))) }, started)
            };
            // The outputs again, now the node is done with them: judged, silenced if need be, published.
            let mut outs = carve(arena, &wants[..stage.outs.len()]);
            let fault = match ran {
                None => None,
                Some(Err(p)) => Some(Fault::Panic(goofi_node::panic_message(p))),
                Some(Ok(())) if outs.writes[..stage.outs.len()].iter().flatten().any(|o| o.iter().any(|v| !v.is_finite())) => {
                    Some(Fault::NotANumber)
                }
                Some(Ok(())) if slot.cost.unwrap_or_else(|| started.elapsed()) > budget => {
                    slot.overruns = slot.overruns.saturating_add(1);
                    (slot.overruns >= OVERRUNS).then_some(Fault::Overrun)
                }
                Some(Ok(())) => {
                    slot.overruns = 0;
                    None
                }
            };
            // Dead only once the fault is on its way: a full outbox means it faults again next block.
            let faulted = fault.is_some();
            if let Some(fault) = fault {
                slot.dead = self.outbox.push(Retired::Faulted { uid: slot.uid, serial: slot.serial, fault }).is_ok();
            }
            for (k, (_, channels)) in stage.outs.iter().enumerate() {
                let out = outs.take(k);
                if slot.dead || faulted {
                    out.fill(0.0);
                }
                if let Some(tap) = slot.taps.get_mut(k) {
                    publish(tap, &[*channels as f32], out);
                }
                if let Some(rec) = slot.recs.get_mut(k) {
                    // A block that does not fit is simply not there: its NUMBER is the gap the
                    // recorder counts, so nothing here has to remember that it was lost.
                    let (lo, hi) = number_out(n);
                    let head = [*channels as f32, lo, hi, epoch as f32];
                    publish(rec, &head, out);
                }
            }
        }
        let (at, channels) = self.plan.output;
        {
            let mut carved = carve(arena, &[(at, channels as usize * BLOCK)]);
            let dst = carved.take(0);
            dst.fill(0.0);
            for (input, gain, sel) in &self.plan.sinks {
                let (input, gain) = (carved.port(input), carved.port(gain));
                match sel {
                    // No selection: every device channel is fed by the port's channel of the same
                    // number, and `Port::chan` spreads a narrower port across all of them.
                    None => {
                        for c in 0..channels as usize {
                            let (x, g) = (input.chan(c), gain.chan(c));
                            for i in 0..BLOCK {
                                dst[c * BLOCK + i] += x[i] * g[i];
                            }
                        }
                    }
                    // A selection ROUTES: the sink's own channel `c` lands on the device channel
                    // named `c`th, and every device channel nobody named is left as it was — which
                    // is what lets two AudioOuts hold different pairs of one card without either
                    // clearing the other's. A name past the width the device opened at is dropped,
                    // since the plan's width is capped at `MAX_CHANNELS` and a device may be
                    // narrower than the plan asked for.
                    Some(sel) => {
                        for (c, dev) in sel.iter().enumerate() {
                            let dev = *dev as usize;
                            if dev >= channels as usize {
                                continue;
                            }
                            let (x, g) = (input.chan(c), gain.chan(c));
                            for i in 0..BLOCK {
                                dst[dev * BLOCK + i] += x[i] * g[i];
                            }
                        }
                    }
                }
            }
        }
        self.anchor.blocks.store(n + 1, Ordering::Relaxed);
        let out = Port::new(&arena[at..at + channels as usize * BLOCK], channels, true);
        for i in 0..BLOCK {
            for c in 0..width {
                self.fifo.push(out.chan(c)[i]);
            }
        }
    }
}

/// What a read lands on when the plan put it inside a written region, which it never does.
static QUIET: [f32; MAX_CHANNELS as usize * BLOCK] = [0.0; MAX_CHANNELS as usize * BLOCK];

/// The arena carved for one pass: every region asked for as its own exclusive slice, answered
/// in the order asked, and the gaps between them as the shared slices every read comes from.
struct Carved<'a> {
    writes: [Option<&'a mut [f32]>; MAX_PORTS + 1],
    gaps: [Option<(usize, &'a [f32])>; MAX_PORTS + 2],
}

/// Split `arena` at each wanted `(at, len)`, in address order. The plan lays regions out
/// disjoint, which is what lets this hand out exclusive slices with no pointer arithmetic.
fn carve<'a>(arena: &'a mut [f32], wants: &[(usize, usize)]) -> Carved<'a> {
    let mut order = [(0, 0, 0); MAX_PORTS + 1];
    for (i, (at, len)) in wants.iter().enumerate() {
        order[i] = (*at, *len, i);
    }
    let order = &mut order[..wants.len()];
    order.sort_unstable();
    let mut writes: [Option<&'a mut [f32]>; MAX_PORTS + 1] = std::array::from_fn(|_| None);
    let mut gaps = [None; MAX_PORTS + 2];
    let mut rest: &'a mut [f32] = arena;
    let mut start = 0;
    let mut g = 0;
    for &(at, len, i) in order.iter() {
        debug_assert!(at >= start && at + len <= start + rest.len(), "the plan lays regions out disjoint");
        let (gap, tail) = rest.split_at_mut(at - start);
        let (region, tail) = tail.split_at_mut(len);
        let gap: &'a [f32] = gap;
        gaps[g] = Some((start, gap));
        g += 1;
        writes[i] = Some(region);
        rest = tail;
        start = at + len;
    }
    let rest: &'a [f32] = rest;
    gaps[g] = Some((start, rest));
    Carved { writes, gaps }
}

impl<'a> Carved<'a> {
    /// The `i`th wanted region, once.
    fn take(&mut self, i: usize) -> &'a mut [f32] {
        self.writes[i].take().expect("a region is taken once")
    }

    /// `len` floats from `at`, which lie in one gap.
    fn read(&self, at: usize, len: usize) -> &'a [f32] {
        for (start, gap) in self.gaps.iter().flatten() {
            if at >= *start && at + len <= *start + gap.len() {
                return &gap[at - start..at - start + len];
            }
        }
        debug_assert!(false, "a read region lies inside a written one");
        &QUIET[..len.min(QUIET.len())]
    }

    fn port(&self, src: &Source) -> Port<'a> {
        match src {
            Source::Silence => Port::new(self.read(SILENCE, BLOCK), 1, false),
            Source::Region { at, channels } | Source::Sum { at, channels, .. } | Source::Inbox { at, channels, .. } => {
                Port::new(self.read(*at, *channels as usize * BLOCK), *channels, true)
            }
            Source::Scalar { at, .. } => Port::new(self.read(*at, BLOCK), 1, true),
        }
    }

    /// This block's one value for a param, whatever it is sourced from.
    fn first(&self, src: &Source) -> f32 {
        match src {
            Source::Silence => 0.0,
            Source::Region { at, .. } | Source::Sum { at, .. } | Source::Inbox { at, .. } | Source::Scalar { at, .. } => {
                self.read(*at, BLOCK)[0]
            }
        }
    }
}
