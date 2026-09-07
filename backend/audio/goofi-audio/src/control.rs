//! The audio engine's half of a node's control thread. The thread, the door, the desired state
//! and the bindings are `goofi-control`'s, shared with every scheduled engine; what is here is
//! what an ARRIVAL becomes on the audio plane, what a tap publishes, and the OS handles a node
//! owns — a device, a MIDI port, a file — which are opened on this thread and never leave it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::FromSample;
use goofi_audio_sdk::{BLOCK, MAX_CHANNELS};
use goofi_control::{flag, text, Cx, Half, Ticked};
use goofi_core::{Data, Meta, Param};
use goofi_node::{NodeManifest, ParamKey};

use crate::nodes::midi_in::{Note, NO_PORT};
use crate::nodes::{audio_in, audio_out, audio_playback, midi_in};
use crate::runtime::REC_HEADER;
use crate::{wav, Clock, DEFAULT_DEVICE, NO_DEVICE, RATE};

/// A tap holds this many blocks of the widest output; what does not fit is dropped, newest first.
pub const TAP_RING: usize = (1 + MAX_CHANNELS as usize * BLOCK) * 16;
/// An inbox holds one second of the widest frame at the rate; a frame that does not fit is
/// dropped whole.
pub const INBOX_RING: usize = RATE as usize * MAX_CHANNELS as usize;
/// A recording ring holds one second of the widest block, as an inbox holds one second of a frame:
/// the control half drains it every tick, so a second is what a stalled thread may cost.
pub const REC_RING: usize = (REC_HEADER + MAX_CHANNELS as usize * BLOCK) * (RATE as usize / BLOCK);
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
    stream: Option<cpal::Stream>,
    midi: Option<midir::MidiInputConnection<()>>,
    device: Option<(String, f64)>,
    port: Option<String>,
    /// Raised by the input stream's error callback; the name is then tried once more.
    dead: Arc<AtomicBool>,
}

/// What every audio control half shares, beside the generic `goofi_control::Shared`: the clock's
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

/// One output's tap: the ring the audio thread fills after every block.
struct Tap {
    ring: rtrb::Consumer<f32>,
}

/// The audio plane's half of a control thread.
pub struct AudioHalf {
    manifest: &'static NodeManifest,
    inboxes: Vec<Inbox>,
    taps: Vec<Tap>,
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
    pub manifest: &'static NodeManifest,
    pub inboxes: Vec<Inbox>,
    pub taps: Vec<rtrb::Consumer<f32>>,
    /// Per output: the recording ring's consumer.
    pub recs: Vec<rtrb::Consumer<f32>>,
    pub ports: Ports,
    pub audio: Arc<AudioShared>,
}

impl AudioHalf {
    /// The channel cell of each Array input, which the plan sizes its inbox by. Read before the
    /// half moves to its thread.
    pub fn channels(inboxes: &[Inbox]) -> Vec<Arc<AtomicU16>> {
        inboxes.iter().map(|i| i.chans.clone()).collect()
    }

    pub fn new(birth: Birth) -> AudioHalf {
        let mut ports = birth.ports;
        AudioHalf {
            manifest: birth.manifest,
            inboxes: birth.inboxes,
            taps: birth.taps.into_iter().map(|ring| Tap { ring }).collect(),
            recs: birth.recs,
            play: ports.play.take().map(Play::new),
            ports,
            io: Io::default(),
            audio: birth.audio,
        }
    }

    /// The one refreshable list a type has — the graph refuses a refresh on any other param —
    /// enumerated here rather than under the graph lock: every host's devices behind the platform
    /// default, or the MIDI ports behind `none`.
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

    /// A device or a port a param names is opened here, on this thread, when the name moves — or
    /// the clock's rate, or the stream died; a name that failed stands as an error on that param
    /// until it moves.
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
            let wanted = (text(consts, audio_in::P::DEVICE), rate);
            if io.device.as_ref() != Some(&wanted) {
                io.stream = None;
                let (stream, error) = match open_input(&wanted.0, wanted.1, producer, io.dead.clone(), clock) {
                    Ok(Some((stream, c))) => {
                        chans.store(c, Ordering::Relaxed);
                        (Some(stream), None)
                    }
                    Ok(None) => (None, Some(NO_DEVICE.to_string())),
                    Err(e) => (None, Some(e)),
                };
                replan = true;
                io.stream = stream;
                io.device = Some(wanted);
                errors.push((key_of(manifest, audio_in::P::DEVICE), error));
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
                            io.midi = Some(connection);
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

    /// The file, driven from settled state: a name that moved is opened, a `position` that moved
    /// or a `reset` skips, and the ring is kept a second ahead so the DSP half never runs dry. A
    /// name that will not open stands as an error on it until it moves.
    fn playback(&mut self, cx: &Cx<'_>) -> (Option<String>, bool) {
        let rate = self.audio.rate();
        let named = text(cx.consts, audio_playback::P::FILE);
        let position = f64::from_bits(cx.params[audio_playback::P::POSITION].load(Ordering::Relaxed));
        let reset = cx.pulses.contains(&audio_playback::P::RESET);
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
            let want = (rate as usize / 8).saturating_mul(file.channels as usize).min(INBOX_RING / 2);
            while dead.is_none() && INBOX_RING - play.inbox.ring.slots() < want {
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

impl Half for AudioHalf {
    /// One frame into the crossing that resamples it to the clock's rate.
    fn arrive(&mut self, inbox: usize, frame: &Data) -> bool {
        let rate = self.audio.rate();
        self.inboxes[inbox].enter(frame, rate).unwrap_or(false)
    }

    fn unwired(&mut self, inbox: usize) {
        self.inboxes[inbox].pos = 0.0;
    }

    fn tick(&mut self, cx: &Cx<'_>, publish: &mut dyn FnMut(usize, &[u8])) -> Ticked {
        let mut ticked = Ticked::default();
        ticked.replan |= self.open_io(cx.consts, &mut ticked.errors);
        if self.play.is_some() {
            let (error, moved) = self.playback(cx);
            let key = key_of(self.manifest, audio_playback::P::FILE);
            ticked.errors.push((key, error));
            ticked.replan |= moved;
        }
        let rate = self.audio.rate();
        let anchor = self.audio.anchor.clone();
        for (i, ring) in self.recs.iter_mut().enumerate() {
            record_out(ring, cx, i, rate, &anchor);
        }
        for (i, tap) in self.taps.iter_mut().enumerate() {
            let Some((c, planar)) = drain_blocks(&mut tap.ring) else { continue };
            if !cx.readers[i] {
                continue;
            }
            let t = planar.len() / c;
            let bytes: Vec<u8> = planar.iter().flat_map(|v| v.to_le_bytes()).collect();
            if let Ok(frame) = Data::array_f32(vec![c, t], bytes, Meta::new().with_sfreq(Some(rate))) {
                publish(i, &goofi_codec::encode(&frame));
            }
        }
        ticked
    }

    fn refresh(&mut self) -> Option<Vec<String>> {
        self.enumerate()
    }
}

pub struct Inbox {
    ring: rtrb::Producer<f32>,
    chans: Arc<AtomicU16>,
    /// The fractional input position the next output sample reads, carried across frames.
    pos: f64,
}

impl Inbox {
    pub fn new(ring: rtrb::Producer<f32>) -> Inbox {
        Inbox { ring, chans: Arc::new(AtomicU16::new(1)), pos: 0.0 }
    }

    /// Resample one `[T]` or `[C, T]` frame linearly from its `sfreq` to the rate and enter it
    /// whole, as one chunk headed by its channel count and length. A frame with no `sfreq` enters
    /// one sample per sample, so a control value is held until the next. Answers whether the
    /// channel count moved.
    fn enter(&mut self, frame: &Data, rate: f64) -> Option<bool> {
        let goofi_core::Value::Array(a) = frame.value() else { return None };
        let (c, t) = match *a.shape() {
            [t] => (1, t),
            [c, t] => (c, t),
            _ => return None,
        };
        if c == 0 || t == 0 || c > MAX_CHANNELS as usize {
            return None;
        }
        let x: Vec<f32> = a.as_bytes().chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four bytes"))).collect();
        let step = frame.meta().sfreq().filter(|sf| *sf > 0.0).map_or(1.0, |sf| sf / rate);
        let moved = self.chans.swap(c as u16, Ordering::Relaxed) != c as u16;
        if moved {
            self.pos = 0.0;
        }
        let pos = self.pos;
        let n = ((t as f64 - pos) / step).ceil().max(0.0) as usize;
        let Some(need) = n.checked_mul(c).and_then(|s| s.checked_add(2)) else { return Some(moved) };
        if let Ok(chunk) = self.ring.write_chunk_uninit(need) {
            let at = |ch: usize, i: usize| {
                let v = x[ch * t + i.min(t - 1)];
                if v.is_finite() { v } else { 0.0 }
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
        Some(moved)
    }
}

/// Everything the audio thread pushed into a ring since the last read, as one planar `[C, T]`
/// frame — up to a block whose channel count differs, which the next read starts from. The
/// framing a tap and a take share, read in one place.
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

/// Every whole block the audio thread left, as one frame each. The BLOCK carries its own number, so
/// a block that never reached the ring leaves a gap the recorder counts, and the blocks around it
/// keep the instants they were rendered at. An unarmed slot's blocks are dropped here, so the ring
/// never fills while nobody records.
fn record_out(ring: &mut rtrb::Consumer<f32>, cx: &Cx<'_>, out: usize, rate: f64, anchor: &crate::runtime::Anchor) {
    while let Some((c, n, epoch, planar)) = take_block(ring) {
        if !cx.recorded[out] {
            continue;
        }
        let bytes: Vec<u8> = planar.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut meta = Meta::new().with_sfreq(Some(rate)).with_index(Some(n));
        meta.set_time(Some(anchor.seconds(n, epoch)));
        if let Ok(frame) = Data::array_f32(vec![c, BLOCK], bytes, meta) {
            (cx.record)(out, &goofi_codec::encode(&frame));
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
        let inbox = Inbox { ring, chans, pos: 0.0 };
        Play { inbox, file: None, named: None, position: 0.0, ended: false, error: None }
    }
}

/// One planar chunk of a file through the crossing every Array input enters by, so a file at its
/// own rate arrives at the engine's.
fn enter_planar(inbox: &mut Inbox, channels: u16, frames: usize, planar: &[f32], from: f64, rate: f64) -> bool {
    let bytes: Vec<u8> = planar.iter().flat_map(|v| v.to_le_bytes()).collect();
    match Data::array_f32(vec![channels as usize, frames], bytes, Meta::new().with_sfreq(Some(from))) {
        Ok(frame) => inbox.enter(&frame, rate).unwrap_or(false),
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
        goofi_core::home::recordings().join(name)
    }
}

/// The device's input stream, opened AT the clock's rate — a device that cannot is the error —
/// its callback entering interleaved frames into the node's inbox as the Array crossing does.
/// The name is resolved whatever the clock, so an absent one is still named; only the device
/// clock opens what it resolved to.
fn open_input(
    name: &str,
    rate: f64,
    producer: Feed<f32>,
    dead: Arc<AtomicBool>,
    clock: Clock,
) -> Result<Option<(cpal::Stream, u16)>, String> {
    let device = crate::host::device(crate::host::Kind::Input, name)?;
    if !clock.owns_devices() {
        return Ok(None);
    }
    let supported = device.default_input_config().map_err(|e| format!("`{name}`: {e}"))?;
    let format = supported.sample_format();
    let mut config = supported.config();
    config.sample_rate = rate as u32;
    // The engine carries `MAX_CHANNELS`, and a device may be wider: an ASIO card answers with every
    // channel the interface has — eighteen on a Scarlett 4pre — where WASAPI answers with the pair
    // an endpoint is. Ask for what can be carried rather than for everything, so the extra channels
    // are never opened instead of being read and dropped.
    config.channels = config.channels.min(MAX_CHANNELS);
    let channels = config.channels;
    if let Ok(configs) = device.supported_input_configs() {
        let ranges: Vec<(u32, u32)> = configs.map(|c| (c.min_sample_rate(), c.max_sample_rate())).collect();
        if let Some(why) = rate_refusal(config.sample_rate, &ranges) {
            return Err(format!("`{name}`: {why}"));
        }
    }
    // The word the DRIVER speaks, not the one goofi would prefer. A shared-mode host reformats to
    // `f32` for every client, so demanding it cost nothing and was never wrong there; a host that
    // hands over the device's own word — a Focusrite's is `i32` — failed outright on a format goofi
    // never asked about. Reading it and converting in the callback is the whole of the difference.
    let refused = |f| format!("the driver's sample format {f} is one goofi does not read");
    let open = |f| {
        crate::by_format!(f, input_stream, refused, &device, config, channels, producer.clone(), dead.clone())
    };
    let stream = open(format).map_err(|e| format!("`{name}`: {e}"))?;
    stream.play().map_err(|e| format!("`{name}`: {e}"))?;
    Ok(Some((stream, channels)))
}

/// Why `wanted` Hz cannot be had from a device offering `ranges`, or `None` when it can.
///
/// A device that cannot run at the clock's rate is still the error — one rate crosses the graph —
/// but the refusal should say what the device DOES offer. What a card is set to is set somewhere
/// else, in a driver's own control panel or by another application holding it, so "unsupported"
/// almost always means "go and change it", and the message is worth nothing if it does not say to
/// what. A host that will not enumerate says nothing here: `ranges` is empty and the open is left
/// to fail on its own terms rather than be refused on a guess.
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
    channels: u16,
    producer: Feed<f32>,
    dead: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _| {
                let Ok(mut inbox) = producer.try_lock() else { return };
                let frames = data.len() / channels as usize;
                if let Ok(chunk) = inbox.write_chunk_uninit(2 + data.len()) {
                    chunk.fill_from_iter(
                        [f32::from(channels), frames as f32]
                            .into_iter()
                            .chain(data.iter().map(|s| f32::from_sample_(*s))),
                    );
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
                if let (Some(note), Ok(mut notes)) = (Note::parse(bytes), producer.try_lock()) {
                    let _ = notes.push(note);
                }
            },
            (),
        )
        .map_err(|e| format!("`{name}`: {e}"))
}

#[cfg(test)]
mod tests {
    use super::rate_refusal;

    /// The case this was written for: a card pinned to one rate by its own control panel, or by
    /// another application already holding it, against a graph clocked from somewhere else. The
    /// old message named neither number, so it read as a defect in goofi rather than a setting.
    #[test]
    fn a_refusal_names_the_rate_wanted_and_the_rates_offered() {
        let why = rate_refusal(48_000, &[(44_100, 44_100)]).expect("44100-only device refuses 48000");
        assert!(why.contains("48000"), "the rate wanted is named: {why}");
        assert!(why.contains("44100"), "the rate offered is named: {why}");
    }

    #[test]
    fn every_offered_rate_is_named_once_and_in_order() {
        let ranges = [(48_000, 48_000), (44_100, 44_100), (96_000, 96_000), (44_100, 44_100)];
        let why = rate_refusal(22_050, &ranges).expect("22050 is offered by none of them");
        assert!(why.contains("44100, 48000, 96000"), "sorted and deduplicated: {why}");
    }

    #[test]
    fn a_rate_the_device_has_is_no_refusal() {
        assert!(rate_refusal(48_000, &[(44_100, 44_100), (48_000, 48_000)]).is_none());
        // A continuous range is a range, not two points: a host reporting 8k–192k accepts 48k.
        assert!(rate_refusal(48_000, &[(8_000, 192_000)]).is_none());
    }

    /// A host that will not enumerate must not be turned into a refusal on a guess — the open is
    /// left to fail on its own terms, with whatever the backend actually says.
    #[test]
    fn a_device_that_names_no_rate_is_not_refused() {
        assert!(rate_refusal(48_000, &[]).is_none());
    }
}
