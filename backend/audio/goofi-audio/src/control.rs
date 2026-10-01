//! The audio engine's executor on a node's runtime: what an arrival becomes, what a tap publishes,
//! and the OS handles a node owns (a device, a MIDI port, a file), opened on this thread alone.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use goofi_supervisor::sync::Mutex;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::FromSample;
use goofi_audio_sdk::BLOCK;
use goofi_runtime::{flag, text, Cx, Executor, Out, Ticked};
use goofi_core::{Data, Meta, Param};
use goofi_node::{NodeManifest, ParamKey};

use crate::nodes::midi_in::{Note, NO_PORT};
use crate::nodes::{audio_in, audio_out, audio_playback, midi_in};
use crate::runtime::{Entry, REC_HEADER};
use crate::{wav, Clock, DEFAULT_DEVICE, NO_DEVICE, RATE};

/// An inbox is born this many floats wide and follows the frames that arrive: four of the
/// newest, grown when one does not fit and shrunk when it is sixteen times too wide.
pub const INBOX_SEED: usize = 4096;
/// The longest a frame may play, in seconds: what bounds the ring minted for it.
pub const LONGEST_FRAME: f64 = 10.0;
/// A device's or a file's feed holds a second at sixteen channels; a chunk that does not fit is dropped.
pub const DEVICE_RING: usize = RATE as usize * 16;
/// A tap holds a quarter second of blocks at `width`, what a reader takes between two ticks;
/// what does not fit is dropped, newest first.
pub fn tap_ring(width: u16) -> usize {
    (1 + width.max(1) as usize * BLOCK) * (RATE as usize / BLOCK / 4)
}
/// A recording ring holds one second of blocks at `width`: the control half drains it every tick,
/// and a second is what a parked drain thread may cost before the recorder counts a gap.
pub fn rec_ring(width: u16) -> usize {
    (REC_HEADER + width.max(1) as usize * BLOCK) * (RATE as usize / BLOCK)
}
/// Notes a port may hold between two blocks.
pub const NOTE_RING: usize = 1024;
/// How much of a file one read takes, in frames of the file's own rate.
const READ_CHUNK: usize = 2048;

/// A ring's producer as an OS callback holds it: successive streams on one node share it, and a
/// callback that finds it taken drops that buffer rather than wait.
pub type Feed<T> = Arc<Mutex<rtrb::Producer<T>>>;

/// The control half's ends of a device's or a port's rings — none for a node that owns no OS
/// handle.
#[derive(Default)]
pub struct Ports {
    pub audio_in: Option<(Feed<f32>, Arc<AtomicU16>)>,
    pub midi_in: Option<Feed<Note>>,
    /// The ring an `AudioPlayback` fills from its file, and the width the file answered.
    pub play: Option<(rtrb::Producer<f32>, Arc<AtomicU16>)>,
}

/// What a control half opens on its own thread and never lets cross it: a stream is not `Send`
/// on every host. A device is opened at the clock's rate, so the name AND the rate gate a reopen.
#[derive(Default)]
struct Io {
    stream: Option<goofi_supervisor::scope::Leased<cpal::Stream>>,
    midi: Option<goofi_supervisor::scope::Leased<midir::MidiInputConnection<()>>>,
    /// The device name, the clock's rate and the channel selection — every input the open
    /// depends on, so a move in any one of them reopens the stream.
    device: Option<(String, f64, String)>,
    port: Option<String>,
    /// Raised by the input stream's error callback; the name is then tried once more.
    dead: Arc<AtomicBool>,
}

/// What every audio executor shares, beside the generic `goofi_runtime::Shared`: the clock's
/// rate, what drives it, and what a plugin's own editor wrote.
pub struct AudioShared {
    /// The clock's rate, `f64` bits: what a crossing resamples to and a tap is stamped with.
    pub rate: AtomicU64,
    /// What drives the blocks: a live stream is opened only where the device does.
    pub clock: Clock,
    /// What a plugin's own editor wrote — node, plugin param id, normalized value — for the
    /// worker to put through the param op.
    pub edits: Mutex<Vec<(goofi_node::Uid, u32, f64)>>,
    /// The drain's door, so an edit made on the window thread is taken without waiting for a tick.
    pub waker: Arc<goofi_node::DrainWaker>,
    /// The block count and its one tie to the clock: what dates every recorded block.
    pub anchor: Arc<crate::runtime::Anchor>,
    /// The control halves' ends of rings the engine grew, by node, taken at the half's next tick.
    pub swaps: Mutex<HashMap<goofi_node::Uid, Swap>>,
}

/// The control half's ends of grown rings, by inbox or output index.
#[derive(Default)]
pub struct Swap {
    pub inboxes: Vec<(usize, rtrb::Producer<f32>)>,
    pub taps: Vec<(usize, rtrb::Consumer<f32>)>,
    pub recs: Vec<(usize, rtrb::Consumer<f32>)>,
}

impl AudioShared {
    pub fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::Relaxed))
    }
}

/// The key of one declared param, off the `'static` manifest — never off `&self`, which a caller
/// mid-borrow of its own fields cannot take.
fn key_of(manifest: &NodeManifest, param: usize) -> ParamKey {
    ParamKey::new(manifest.params[param].group, manifest.params[param].name)
}

/// The audio plane's half of a control thread.
pub struct AudioHalf {
    uid: goofi_node::Uid,
    manifest: &'static NodeManifest,
    /// The node's scalar params, which say how its inboxes play what enters them.
    params: Arc<[AtomicU64]>,
    playback: crate::runtime::Playback,
    /// Why the last frame an oscillator was handed could not sound, shown on its `mode`.
    refused: Option<String>,
    /// The pulse params raised since the last run, by index.
    pulses: Vec<usize>,
    inboxes: Vec<Inbox>,
    /// One per output: the ring the audio thread fills after every block.
    taps: Vec<rtrb::Consumer<f32>>,
    ports: Ports,
    io: Io,
    /// One per output: the blocks the audio thread left, each wearing its own number.
    recs: Vec<rtrb::Consumer<f32>>,
    /// An `AudioPlayback`'s file; every other node has none.
    play: Option<Play>,
    audio: Arc<AudioShared>,
}

/// What the engine hands a birth for its half; the half itself is built on the control thread.
pub struct Birth {
    pub uid: goofi_node::Uid,
    pub manifest: &'static NodeManifest,
    pub params: Arc<[AtomicU64]>,
    pub inboxes: Vec<Inbox>,
    pub taps: Vec<rtrb::Consumer<f32>>,
    /// Per output: the recording ring's consumer.
    pub recs: Vec<rtrb::Consumer<f32>>,
    pub ports: Ports,
    pub audio: Arc<AudioShared>,
}

impl AudioHalf {
    /// The cells of each Array input: the channel count the plan sizes its port by, and the ring
    /// size the engine mints for it. Read before the half moves to its thread.
    pub fn cells(inboxes: &[Inbox]) -> (Vec<Arc<AtomicU16>>, Vec<Arc<AtomicUsize>>) {
        (inboxes.iter().map(|i| i.chans.clone()).collect(), inboxes.iter().map(|i| i.wanted.clone()).collect())
    }

    pub fn new(birth: Birth) -> AudioHalf {
        let mut ports = birth.ports;
        AudioHalf {
            uid: birth.uid,
            manifest: birth.manifest,
            params: birth.params,
            playback: crate::runtime::Playback::of(birth.manifest),
            refused: None,
            pulses: Vec::new(),
            inboxes: birth.inboxes,
            taps: birth.taps,
            recs: birth.recs,
            play: ports.play.take().map(Play::new),
            ports,
            io: Io::default(),
            audio: birth.audio,
        }
    }

    /// The type's one refreshable list, off the graph lock: every host's devices behind the
    /// platform default, or the MIDI ports behind `none`.
    fn enumerate(&self) -> Option<Vec<String>> {
        let named = |kind: crate::host::Kind| {
            let mut names = vec![DEFAULT_DEVICE.to_string()];
            names.extend(crate::host::named(kind).into_iter().map(|(n, _)| n));
            names
        };
        match self.manifest.type_name {
            audio_out::TYPE => Some(named(crate::host::Kind::Output)),
            audio_in::TYPE => Some(named(crate::host::Kind::Input)),
            midi_in::TYPE => {
                let mut names = vec![NO_PORT.to_string()];
                if let Ok(input) = midir::MidiInput::new("goofi") {
                    names.extend(input.ports().iter().filter_map(|p| input.port_name(p).ok()));
                }
                Some(names)
            }
            _ => None,
        }
    }

    /// Open the device or port a param names when the name, the clock's rate or the stream moves;
    /// a name that failed stands as an error on that param until it moves.
    fn open_io(&mut self, consts: &[Param], errors: &mut Vec<(ParamKey, Option<String>)>) -> bool {
        let manifest = self.manifest;
        let (rate, clock) = (self.audio.rate(), self.audio.clock);
        let (io, ports) = (&mut self.io, &self.ports);
        let mut replan = false;
        if io.dead.swap(false, Ordering::Acquire) {
            io.stream = None;
            io.device = None;
        }
        if let Some((producer, chans)) = ports.audio_in.clone() {
            let wanted = (text(consts, audio_in::P::DEVICE), rate, text(consts, audio_in::P::CHANNELS));
            if io.device.as_ref() != Some(&wanted) {
                io.stream = None;
                // A selection that does not parse is the CHANNELS param's error, not the device's.
                let (stream, error, sel_error) = match crate::chanmap::parse(&wanted.2) {
                    Err(why) => (None, None, Some(why)),
                    Ok(sel) => match open_input(&wanted.0, wanted.1, sel.as_deref(), producer, io.dead.clone(), clock) {
                        Ok(Some((stream, c))) => {
                            chans.store(c, Ordering::Relaxed);
                            (Some(stream), None, None)
                        }
                        Ok(None) => (None, Some(NO_DEVICE.to_string()), None),
                        Err(e) => (None, Some(e), None),
                    },
                };
                replan = true;
                io.stream = stream.map(|s| {
                    goofi_supervisor::scope::leased(goofi_supervisor::scope::Kind::Device, format!("audio in {}", wanted.0), s)
                });
                io.device = Some(wanted);
                errors.push((key_of(manifest, audio_in::P::DEVICE), error));
                errors.push((key_of(manifest, audio_in::P::CHANNELS), sel_error));
            }
        }
        if let Some(producer) = ports.midi_in.clone() {
            let wanted = text(consts, midi_in::P::PORT);
            if io.port.as_deref() != Some(wanted.as_str()) {
                io.midi = None;
                let error = if wanted == NO_PORT {
                    None
                } else {
                    match open_port(&wanted, producer) {
                        Ok(connection) => {
                            io.midi = Some(goofi_supervisor::scope::leased(
                                goofi_supervisor::scope::Kind::Device,
                                format!("midi in {wanted}"),
                                connection,
                            ));
                            None
                        }
                        Err(e) => Some(e),
                    }
                };
                io.port = Some(wanted);
                errors.push((key_of(manifest, midi_in::P::PORT), error));
            }
        }
        replan
    }

    /// The file, from settled state: a moved name opens, a moved `position` or a `reset` skips, and
    /// the ring is kept a second ahead. A name that will not open stands as an error until it moves.
    fn playback(&mut self, cx: &Cx<'_>) -> (Option<String>, bool) {
        let rate = self.audio.rate();
        let named = text(cx.consts, audio_playback::P::FILE);
        let position = f64::from_bits(cx.params[audio_playback::P::POSITION].load(Ordering::Relaxed));
        let reset = self.pulses.contains(&audio_playback::P::RESET);
        let looping = flag(cx.consts, audio_playback::P::LOOPING);
        let Some(play) = self.play.as_mut() else { return (None, false) };
        let mut moved = false;
        if play.named.as_deref() != Some(named.as_str()) {
            play.named = Some(named.clone());
            play.file = None;
            play.error = None;
            play.ended = false;
            play.position = position;
            play.inbox.pos = 0.0;
            let name = named.trim();
            if !name.is_empty() {
                match wav::Reader::open(&source_path(name)) {
                    Ok(f) if f.frames == 0 => play.error = Some(format!("{} holds no samples", f.path.display())),
                    Ok(f) => {
                        moved = play.inbox.chans.swap(f.channels, Ordering::Relaxed) != f.channels;
                        play.file = Some(f);
                    }
                    Err(e) => play.error = Some(e),
                }
            }
        }
        let mut dead = None;
        if let Some(file) = play.file.as_mut() {
            if reset || position != play.position {
                play.position = position;
                let at = if reset { 0 } else { (position * file.frames as f64) as u64 };
                dead = file.seek(at).err();
                play.ended = false;
                play.inbox.pos = 0.0;
            }
            // An eighth of a second ahead: enough over a tick, and what a skip waits out.
            let capacity = play.inbox.ring.buffer().capacity();
            let want = (rate as usize / 8).saturating_mul(file.channels as usize).min(capacity / 2);
            while dead.is_none() && capacity - play.inbox.ring.slots() < want {
                let (got, planar) = match file.read(READ_CHUNK) {
                    Ok(chunk) => chunk,
                    Err(e) => {
                        dead = Some(e);
                        break;
                    }
                };
                if got == 0 {
                    if looping {
                        dead = file.seek(0).err();
                        // A wrap is not an end: leaving this set costs the quiet chunk the NEXT
                        // end needs, and the DSP half then holds the file's last sample for good.
                        play.ended = false;
                        continue;
                    }
                    if !play.ended {
                        play.ended = true;
                        // One quiet chunk, or `fill` holds the last sample it had as an offset.
                        let quiet = vec![0.0; file.channels as usize * BLOCK];
                        moved |= enter_planar(&mut play.inbox, file.channels, BLOCK, &quiet, file.rate as f64, rate);
                    }
                    break;
                }
                moved |= enter_planar(&mut play.inbox, file.channels, got, &planar, file.rate as f64, rate);
            }
        }
        if let Some(e) = dead {
            play.error = Some(e);
            play.file = None;
        }
        (play.error.clone(), moved)
    }
}

impl Executor for AudioHalf {
    /// One frame into the crossing that resamples it to the clock's rate, or that enters its
    /// values as they are for an oscillator to sound.
    fn arrive(&mut self, inbox: usize, _wire: usize, frame: &Data) -> bool {
        let rate = self.audio.rate();
        let entry = self.playback.entry(&self.params);
        self.refused = None;
        if let goofi_core::Value::Array(a) = frame.value() {
            match entry {
                Entry::Pitches { .. } if pitch_width(a.shape()).is_none() => {
                    self.refused = Some(format!("an oscillator takes [n] pitches or [2, n] pitches and phases, not {:?}", a.shape()));
                    return false;
                }
                Entry::Waveform { mix: false, .. } if layout(a.shape()).is_some_and(|l| l.0 > crate::plan::CEILING as usize) => {
                    let most = crate::plan::CEILING;
                    self.refused = Some(format!("a waveform plays at most {most} channels, not {:?}: mix them", a.shape()));
                    return false;
                }
                Entry::Waveform { .. } if lasts(a.shape(), frame.meta().sfreq(), rate) > LONGEST_FRAME => {
                    let (t, sf) = (layout(a.shape()).map_or(0, |l| l.1), frame.meta().sfreq().unwrap_or(rate));
                    self.refused = Some(format!("a frame of {t} samples at {sf} Hz lasts too long to play: at most {LONGEST_FRAME} s"));
                    return false;
                }
                _ => {}
            }
        }
        self.inboxes[inbox].enter(frame, rate, entry).unwrap_or(false)
    }

    fn rewire(&mut self, inbox: usize, wires: &[(String, String)]) {
        if wires.is_empty() {
            self.inboxes[inbox].pos = 0.0;
        }
    }

    fn pulse(&mut self, param: usize) -> Ticked {
        self.pulses.push(param);
        Ticked::default()
    }

    fn run(&mut self, cx: &Cx<'_>, publish: &mut dyn FnMut(usize, Out<'_>)) -> Ticked {
        let mut ticked = Ticked::default();
        ticked.replan |= self.open_io(cx.consts, &mut ticked.errors);
        if let Some(mode) = self.playback.mode() {
            ticked.errors.push((key_of(self.manifest, mode), self.refused.clone()));
        }
        if self.play.is_some() {
            let (error, moved) = self.playback(cx);
            let key = key_of(self.manifest, audio_playback::P::FILE);
            ticked.errors.push((key, error));
            ticked.replan |= moved;
        }
        self.pulses.clear();
        let rate = self.audio.rate();
        let anchor = self.audio.anchor.clone();
        for (i, ring) in self.recs.iter_mut().enumerate() {
            record_out(ring, cx, i, rate, &anchor, publish);
        }
        for (i, tap) in self.taps.iter_mut().enumerate() {
            let Some((c, planar)) = drain_blocks(tap) else { continue };
            if !cx.readers[i] {
                continue;
            }
            let t = planar.len() / c;
            let bytes: Vec<u8> = planar.iter().flat_map(|v| v.to_le_bytes()).collect();
            let frame = Data::array_f32(vec![c, t], bytes, Meta::new().with_sfreq(Some(rate)));
            if let Some(bytes) = frame.ok().and_then(|f| goofi_codec::encode(&f).ok()) {
                publish(i, Out::Bytes(&bytes));
            }
        }
        // After the drains, so what the old rings still held went out before they go.
        let swap = self.audio.swaps.lock().remove(&self.uid);
        if let Some(swap) = swap {
            let entry = self.playback.entry(&self.params);
            for (i, ring) in swap.inboxes {
                self.inboxes[i].ring = ring;
                // The frame the old ring could not take enters the one minted for it.
                if let Some(frame) = self.inboxes[i].pending.take() {
                    ticked.replan |= self.inboxes[i].enter(&frame, rate, entry).unwrap_or(false);
                }
            }
            for (i, ring) in swap.taps {
                self.taps[i] = ring;
            }
            for (i, ring) in swap.recs {
                self.recs[i] = ring;
            }
        }
        ticked
    }

    fn refresh(&mut self, _: usize) -> Option<Vec<String>> {
        self.enumerate()
    }
}

pub struct Inbox {
    ring: rtrb::Producer<f32>,
    chans: Arc<AtomicU16>,
    /// The ring size the frames ask for, in floats; the engine mints a ring of it at the settle.
    wanted: Arc<AtomicUsize>,
    /// A frame the ring could not take, entered once the ring minted for it arrives.
    pending: Option<Data>,
    /// The fractional input position the next output sample reads, carried across frames.
    pos: f64,
}

impl Inbox {
    pub fn new(ring: rtrb::Producer<f32>) -> Inbox {
        let wanted = Arc::new(AtomicUsize::new(ring.buffer().capacity()));
        Inbox { ring, chans: Arc::new(AtomicU16::new(1)), wanted, pending: None, pos: 0.0 }
    }

    /// Whether a chunk of `need` floats fits, and whether the ring must be re-minted: for a chunk
    /// that does not fit, which is held until then, or for one sixteen times smaller than the ring.
    fn room(&mut self, need: usize, frame: &Data) -> (bool, bool) {
        let capacity = self.ring.buffer().capacity();
        let fits = need <= capacity;
        if !fits {
            self.pending = Some(frame.clone());
        }
        let resize = !fits || need.saturating_mul(16) < capacity;
        let wanted = need.checked_mul(4).filter(|_| resize);
        if let Some(wanted) = wanted {
            self.wanted.store(wanted, Ordering::Relaxed);
        }
        (fits, wanted.is_some())
    }

    /// Resample one frame linearly from its `sfreq` to the rate and enter it as one chunk headed by
    /// its channel count and length. Answers whether the channel count moved or the ring must grow.
    fn enter(&mut self, frame: &Data, rate: f64, entry: Entry) -> Option<bool> {
        let goofi_core::Value::Array(a) = frame.value() else { return None };
        let (mix, range) = match entry {
            Entry::Waveform { mix, range } => (mix, range),
            Entry::Pitches { volts } => {
                let width = pitch_width(a.shape())?;
                let n = a.as_bytes().len() / 4;
                let (fits, resize) = self.room(n + 2, frame);
                if !fits {
                    return Some(true);
                }
                let chunk = self.ring.write_chunk_uninit(n + 2).ok()?;
                let values = a.values();
                let head = [width as f32, (n / width) as f32];
                // Row 0 is the pitch; row 1, where there is one, stays the phase.
                let values = values.enumerate().map(|(i, v)| match v.is_finite() {
                    true if volts && i < n / width => goofi_audio_sdk::hz_of(v),
                    true => v,
                    false => 0.0,
                });
                chunk.fill_from_iter(head.into_iter().chain(values));
                self.pos = 0.0;
                return Some(self.chans.swap(1, Ordering::Relaxed) != 1 || resize);
            }
        };
        let (c, t, lane, stride) = layout(a.shape())?;
        if c == 0 || t == 0 {
            return None;
        }
        let mut x: Vec<f32> = a.values().collect();
        // Any number of rows mix into one channel; unmixed, a port carries at most the ceiling.
        let (c, lane, stride) = match mix && c > 1 {
            true => {
                x = (0..t).map(|i| (0..c).map(|ch| x[ch * lane + i * stride]).sum::<f32>() / c as f32).collect();
                (1, t, 1)
            }
            false if c > crate::plan::CEILING as usize => return None,
            false => (c, lane, stride),
        };
        let step = frame.meta().sfreq().filter(|sf| *sf > 0.0).map_or(1.0, |sf| sf / rate);
        let moved = self.chans.swap(c as u16, Ordering::Relaxed) != c as u16;
        if moved {
            self.pos = 0.0;
        }
        let pos = self.pos;
        let n = ((t as f64 - pos) / step).ceil().max(0.0) as usize;
        let Some(need) = n.checked_mul(c).and_then(|s| s.checked_add(2)) else { return Some(moved) };
        let (fits, resize) = self.room(need, frame);
        if !fits {
            return Some(true);
        }
        if let Ok(chunk) = self.ring.write_chunk_uninit(need) {
            let at = |ch: usize, i: usize| {
                let v = x[ch * lane + i.min(t - 1) * stride];
                // A linear map, not a clamp: a pitch in volts past the range is still a pitch.
                match range {
                    _ if !v.is_finite() => 0.0,
                    Some((lo, hi)) if hi > lo => (v - lo) / (hi - lo) * 2.0 - 1.0,
                    _ => v,
                }
            };
            let samples = (0..n).flat_map(|k| {
                let p = pos + k as f64 * step;
                let i = p.floor();
                let f = (p - i) as f32;
                let i = i as usize;
                (0..c).map(move |ch| at(ch, i) + (at(ch, i + 1) - at(ch, i)) * f)
            });
            chunk.fill_from_iter([c as f32, n as f32].into_iter().chain(samples));
        }
        self.pos = pos + n as f64 * step - t as f64;
        Some(moved || resize)
    }
}

/// A waveform frame's channels, samples per channel, and where lane `ch` sample `i` sits
/// (`ch * lane + i * stride`): planar `[T]` or `[C, T]`, or `[H, W, C]` texels in scan order.
fn layout(shape: &[usize]) -> Option<(usize, usize, usize, usize)> {
    match *shape {
        [t] => Some((1, t, t, 1)),
        [c, t] => Some((c, t, t, 1)),
        [h, w, c] => Some((c, h * w, 1, c)),
        _ => None,
    }
}

/// How long a frame plays at `sfreq`, in seconds; one without a rate plays a sample per sample.
fn lasts(shape: &[usize], sfreq: Option<f64>, rate: f64) -> f64 {
    layout(shape).map_or(0, |l| l.1) as f64 / sfreq.filter(|sf| *sf > 0.0).unwrap_or(rate)
}

/// How many rows an oscillator's frame has: pitches, or pitches over phases.
fn pitch_width(shape: &[usize]) -> Option<usize> {
    match *shape {
        [_] => Some(1),
        [2, _] => Some(2),
        _ => None,
    }
}

/// Everything the audio thread pushed into a ring since the last read, as one planar `[C, T]`
/// frame, up to a block whose channel count differs.
fn drain_blocks(ring: &mut rtrb::Consumer<f32>) -> Option<(usize, Vec<f32>)> {
    let mut chans = 0;
    let mut planar: Vec<Vec<f32>> = Vec::new();
    while let Ok(head) = ring.read_chunk(1) {
        let c = head.as_slices().0.first().copied().unwrap_or(0.0) as usize;
        if c == 0 || (chans != 0 && c != chans) {
            if c == 0 {
                head.commit_all();
            }
            break;
        }
        head.commit_all();
        let Ok(block) = ring.read_chunk(c * BLOCK) else { break };
        if chans == 0 {
            chans = c;
            planar = vec![Vec::new(); c];
        }
        let (a, b) = block.as_slices();
        let samples: Vec<f32> = a.iter().chain(b).copied().collect();
        for (ch, lane) in planar.iter_mut().enumerate() {
            lane.extend_from_slice(&samples[ch * BLOCK..(ch + 1) * BLOCK]);
        }
        block.commit_all();
    }
    (chans != 0).then(|| (chans, planar.concat()))
}

/// One block off a recording ring: its width, its own number, the tie it is read against, and its
/// planar samples. `None` until a whole block is there.
fn take_block(ring: &mut rtrb::Consumer<f32>) -> Option<(usize, u64, u64, Vec<f32>)> {
    // `as_slices` rather than an iterator: iterating a chunk COMMITS what it yields, so a header
    // read that way is eaten — and a partial block would then leave the reader between two blocks.
    let head = {
        let peek = ring.read_chunk(REC_HEADER).ok()?;
        let (a, b) = peek.as_slices();
        let at = |i: usize| if i < a.len() { a[i] } else { b[i - a.len()] };
        [at(0), at(1), at(2), at(3)]
    };
    let c = head[0] as usize;
    if c == 0 || ring.slots() < REC_HEADER + c * BLOCK {
        return None;
    }
    let whole = ring.read_chunk(REC_HEADER + c * BLOCK).ok()?;
    let (a, b) = whole.as_slices();
    let planar: Vec<f32> = a.iter().chain(b).skip(REC_HEADER).copied().collect();
    whole.commit_all();
    Some((c, crate::runtime::number_in(head[1], head[2]), head[3] as u64, planar))
}

/// Every whole block the audio thread left, as one frame each at the instant its own number gives;
/// an unarmed slot's blocks are dropped, so the ring never fills while nobody records.
fn record_out(ring: &mut rtrb::Consumer<f32>, cx: &Cx<'_>, out: usize, rate: f64, anchor: &crate::runtime::Anchor, publish: &mut dyn FnMut(usize, Out<'_>)) {
    let drift = anchor.drift();
    while let Some((c, n, epoch, planar)) = take_block(ring) {
        if !cx.recorded[out] {
            continue;
        }
        let bytes: Vec<u8> = planar.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut meta = Meta::new().with_sfreq(Some(rate)).with_index(Some(n));
        meta.set_time(Some(anchor.seconds(n, epoch)));
        meta.set(goofi_core::META_DRIFT, goofi_core::MetaValue::Float(drift));
        if let Some(bytes) = Data::array_f32(vec![c, BLOCK], bytes, meta).ok().and_then(|f| goofi_codec::encode(&f).ok()) {
            publish(out, Out::Record(&bytes));
        }
    }
}

/// An `AudioPlayback`'s file: the reader, the crossing that resamples it into the DSP half's
/// ring, and what the params last asked for.
struct Play {
    inbox: Inbox,
    file: Option<wav::Reader>,
    named: Option<String>,
    position: f64,
    ended: bool,
    error: Option<String>,
}

impl Play {
    fn new((ring, chans): (rtrb::Producer<f32>, Arc<AtomicU16>)) -> Play {
        let wanted = Arc::new(AtomicUsize::new(ring.buffer().capacity()));
        let inbox = Inbox { ring, chans, wanted, pending: None, pos: 0.0 };
        Play { inbox, file: None, named: None, position: 0.0, ended: false, error: None }
    }
}

/// One planar chunk of a file through the crossing every Array input enters by, so a file at its
/// own rate arrives at the engine's.
fn enter_planar(inbox: &mut Inbox, channels: u16, frames: usize, planar: &[f32], from: f64, rate: f64) -> bool {
    let bytes: Vec<u8> = planar.iter().flat_map(|v| v.to_le_bytes()).collect();
    match Data::array_f32(vec![channels as usize, frames], bytes, Meta::new().with_sfreq(Some(from))) {
        Ok(frame) => inbox.enter(&frame, rate, Entry::Waveform { mix: false, range: None }).unwrap_or(false),
        Err(_) => false,
    }
}

/// Whether a name names a place of its own. `has_root` as well as `is_absolute`, because on
/// Windows `/x.wav` is rooted and NOT absolute — and `join` on a rooted path drops the folder.
fn rooted(p: &Path) -> bool {
    p.is_absolute() || p.has_root()
}

/// Where a name is looked for: the recordings folder for a bare one, an absolute path as it is,
/// and `.wav` joined on where it is not already there.
fn source_path(name: &str) -> PathBuf {
    let name = name.trim();
    let name = if name.to_ascii_lowercase().ends_with(".wav") { name.to_string() } else { format!("{name}.wav") };
    if rooted(Path::new(&name)) {
        PathBuf::from(name)
    } else {
        goofi_supervisor::home::recordings().join(name)
    }
}

/// The device's input stream, opened AT the clock's rate, its callback entering frames into the
/// node's inbox. Only the device clock opens it; an absent name is an error under any clock.
fn open_input(
    name: &str,
    rate: f64,
    sel: Option<&[u16]>,
    producer: Feed<f32>,
    dead: Arc<AtomicBool>,
    clock: Clock,
) -> Result<Option<(cpal::Stream, u16)>, String> {
    let device = crate::host::device(crate::host::Kind::Input, name)?;
    if !crate::owns_devices(clock) {
        return Ok(None);
    }
    let supported = device.default_input_config().map_err(|e| format!("`{name}`: {e}"))?;
    let format = supported.sample_format();
    let mut config = supported.config();
    config.sample_rate = rate as u32;
    // A selection opens the stream just wide enough to contain the highest channel it names, and
    // only the selection leaves the callback.
    let device_width = config.channels;
    config.channels = match sel {
        Some(sel) => crate::chanmap::needed_width(sel).min(device_width),
        None => device_width,
    };
    let opened = config.channels;
    // A selection naming a channel the device does not have is the selection's error, and it is
    // worth saying how wide the device actually is — the number is not written on the front panel.
    if let Some(sel) = sel {
        if let Some(past) = sel.iter().copied().find(|c| *c >= device_width) {
            return Err(format!("`{name}` has {device_width} input channels and the selection names channel {}", past + 1));
        }
    }
    let channels = sel.map_or(opened, |s| s.len() as u16);
    if let Ok(configs) = device.supported_input_configs() {
        let ranges: Vec<(u32, u32)> = configs.map(|c| (c.min_sample_rate(), c.max_sample_rate())).collect();
        if let Some(why) = rate_refusal(config.sample_rate, &ranges) {
            return Err(format!("`{name}`: {why}"));
        }
    }
    // The sample format the DRIVER speaks, converted in the callback: an exclusive host hands over
    // the device's own word.
    let refused = |f| format!("the driver's sample format {f} is one goofi does not read");
    let sel = sel.map(<[u16]>::to_vec);
    let open = |f| {
        crate::by_format!(f, input_stream, refused, &device, config, opened, sel.clone(), producer.clone(), dead.clone())
    };
    let stream = open(format).map_err(|e| format!("`{name}`: {e}"))?;
    stream.play().map_err(|e| format!("`{name}`: {e}"))?;
    Ok(Some((stream, channels)))
}

/// Why `wanted` Hz cannot be had from a device offering `ranges`, naming the rates it offers; an
/// empty `ranges` refuses nothing and leaves the open to fail on its own.
fn rate_refusal(wanted: u32, ranges: &[(u32, u32)]) -> Option<String> {
    if ranges.is_empty() || ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&wanted)) {
        return None;
    }
    let mut offered: Vec<u32> = ranges.iter().flat_map(|(lo, hi)| [*lo, *hi]).collect();
    offered.sort_unstable();
    offered.dedup();
    let offered: Vec<String> = offered.iter().map(|r| r.to_string()).collect();
    Some(format!(
        "the clock runs at {wanted} Hz and this device offers only {} Hz. Set the device to \
         {wanted} Hz, or clock the graph from an output that runs at one of those.",
        offered.join(", ")
    ))
}

/// One device callback, entering interleaved frames into the node's inbox as `f32` whatever word
/// the driver hands over. The two numbers ahead of the samples are the crossing's own header.
fn input_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    opened: u16,
    sel: Option<Vec<u16>>,
    producer: Feed<f32>,
    dead: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    // The width the node emits: the selection's length, or the whole stream. A new selection is a
    // new stream, as the callback holds this one by value.
    let emitted = sel.as_ref().map_or(opened, |s| s.len() as u16);
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _| {
                let Some(mut inbox) = producer.try_lock() else { return };
                let frames = data.len() / opened as usize;
                let len = frames * emitted as usize;
                if let Ok(chunk) = inbox.write_chunk_uninit(2 + len) {
                    let header = [f32::from(emitted), frames as f32].into_iter();
                    let _ = match &sel {
                        // No selection: the stream's own interleaving is already the answer.
                        None => chunk.fill_from_iter(header.chain(data.iter().map(|s| f32::from_sample_(*s)))),
                        // A selection gathers the named channels in order, per frame; open checked
                        // it against the opened width.
                        Some(sel) => chunk.fill_from_iter(header.chain((0..frames).flat_map(|f| {
                            sel.iter().map(move |c| f32::from_sample_(data[f * opened as usize + *c as usize]))
                        }))),
                    };
                }
            },
            move |e| {
                if matches!(e.kind(), cpal::ErrorKind::DeviceNotAvailable) {
                    dead.store(true, Ordering::Release);
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}

/// A MIDI port, its callback handing every note to the node's ring.
fn open_port(name: &str, producer: Feed<Note>) -> Result<midir::MidiInputConnection<()>, String> {
    let mut input = midir::MidiInput::new("goofi").map_err(|e| format!("midi: {e}"))?;
    input.ignore(midir::Ignore::All);
    let port = input
        .ports()
        .into_iter()
        .find(|p| input.port_name(p).is_ok_and(|n| n == name))
        .ok_or_else(|| format!("no MIDI port `{name}`"))?;
    input
        .connect(
            &port,
            "goofi-in",
            move |_, bytes, _| {
                if let (Some(note), Some(mut notes)) = (Note::parse(bytes), producer.try_lock()) {
                    let _ = notes.push(note);
                }
            },
            (),
        )
        .map_err(|e| format!("`{name}`: {e}"))
}
