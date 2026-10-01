//! One plugin instance behind [`AudioNode`]: instantiated at `prepare` on the window thread, its
//! params and note events queued per block, its buses staged from the arena.

use std::path::PathBuf;
use std::sync::Arc;

use goofi_audio_sdk::{high, AudioNode, Block, BLOCK};

/// A voice is a MIDI channel, and MIDI has sixteen: the protocol's bound, not the engine's.
const MIDI_CHANNELS: usize = 16;
use goofi_node::Stamp;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{ComPtr, ComWrapper};

use super::editor;
use super::host::{Changes, Events, Host, Stream};
use super::module;
use super::ok;
use crate::control::AudioShared;
use goofi_window::{self as ui, Ui};
use goofi_node::Uid;

/// The tempo the host reports: a constant, as goofi has no transport, but a synced plugin stops
/// at zero.
const TEMPO: f64 = 120.0;

/// What a note-on's `tuning` is denominated in: VST3 spells the per-note offset in cents where
/// goofi's pitch is volts per octave, and a round's residual is never more than half a semitone.
const CENTS_PER_SEMITONE: f32 = 100.0;

/// The bend a wheel at full travel is taken to mean: the MIDI default, an ASSUMPTION a plugin may
/// not share. `roadmap/vst3-per-note-tuning.md` holds what was measured.
const BEND_SEMITONES: f32 = 2.0;

/// One voice's gate, pitch or velocity for one sample, whichever source is carrying it.
type Reader<'a> = dyn Fn(usize, usize, usize) -> f32 + 'a;
/// Whether a gate reading means the note is down — a level for a param, a velocity for the cable.
type Gate = dyn Fn(f32) -> bool;

/// What the scan derived for one class: enough to stage a block and to instantiate.
pub struct Derived {
    pub binary: PathBuf,
    pub stamp: Stamp,
    pub cid: TUID,
    pub inputs: Vec<u16>,
    pub outputs: Vec<u16>,
    /// Per plugin param: its id, and the steps a goofi scalar is divided by to normalize it (1
    /// for a continuous one).
    pub params: Vec<(ParamID, f64)>,
}

pub struct Plugin {
    class: Arc<Derived>,
    ui: Option<Ui>,
    uid: Option<Uid>,
    shared: Option<Arc<AudioShared>>,
    live: Option<Live>,
    /// What instantiation refused; every `process` raises it, because the runtime marks a node
    /// dead only once the fault is on its way and expects the next block to fault again.
    failed: Option<String>,
    /// What `load` was handed. Kept, so a failed instantiation cannot answer `save` with nothing
    /// and have the engine delete the preset behind it.
    blob: Vec<u8>,
}

impl Plugin {
    pub fn new(class: Arc<Derived>, ui: Option<Ui>, uid: Option<Uid>, shared: Option<Arc<AudioShared>>) -> Plugin {
        Plugin { class, ui, uid, shared, live: None, failed: None, blob: Vec::new() }
    }
}

/// On the window thread where there is one — a JUCE plugin holds the thread that loaded it to be
/// its message thread — and inline, with no host, where there is none.
fn on<T: Send>(ui: &Option<Ui>, f: impl FnOnce(Option<&mut ui::Host>) -> T + Send) -> T {
    match ui {
        Some(ui) => ui.run(|host| f(Some(host))),
        None => f(None),
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        let (live, uid) = (self.live.take(), self.uid);
        on(&self.ui, move |host| {
            if let (Some(host), Some(uid)) = (host, uid) {
                editor::unregister(host, uid);
            }
            drop(live);
        });
    }
}

impl AudioNode for Plugin {
    fn channels(&self, _ins: &[u16], _params: &[f64], outs: usize) -> Vec<u16> {
        (0..outs).map(|i| self.class.outputs.get(i).copied().unwrap_or(1).max(1)).collect()
    }

    /// Only the voice params are ports, read per sample; a plugin parameter is one value per
    /// block, which lets a synth declare thousands.
    fn audio_params(&self, declared: usize) -> usize {
        declared.saturating_sub(self.class.params.len())
    }

    fn prepare(&mut self, rate: f64) {
        let Plugin { class, ui, uid, shared, live, failed, blob } = self;
        *failed = on(ui, |host| match live {
            Some(live) => live.retune(rate),
            None => Live::open(class, rate).map(|opened| {
                opened.load(blob);
                if let (Some(_), Some(uid), Some(shared), Some(controller)) = (host, *uid, shared.clone(), opened.controller()) {
                    editor::register(uid, controller, shared);
                }
                *live = Some(opened)
            }),
        })
        .err();
    }

    fn process(&mut self, b: &mut Block<'_>) {
        if let Some(text) = &self.failed {
            std::panic::resume_unwind(Box::new(text.clone()));
        }
        match self.live.as_mut() {
            Some(live) => live.block(&self.class, b),
            None => {
                for out in b.outs.iter_mut() {
                    for c in 0..out.channels() as usize {
                        out.chan_mut(c).fill(0.0);
                    }
                }
            }
        }
    }

    fn save(&self) -> Vec<u8> {
        let Some(live) = &self.live else { return self.blob.clone() };
        let stream = ComWrapper::new(Stream::default());
        let ptr = stream.to_com_ptr::<IBStream>().expect("a stream is an IBStream");
        if unsafe { live.component.getState(ptr.as_ptr()) } != kResultOk {
            return self.blob.clone();
        }
        stream.bytes.take()
    }

    fn load(&mut self, bytes: &[u8]) {
        self.blob = bytes.to_vec();
        if let Some(live) = self.live.as_mut() {
            live.load(bytes);
        }
    }
}

struct Live {
    _host: ComPtr<FUnknown>,
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    /// The plugin's other half, connected for as long as this instance lives: a plugin whose
    /// halves were never introduced can sit muted.
    controller: Option<ComPtr<IEditController>>,
    wired: Option<Wire>,
    changes: ComWrapper<Changes>,
    changes_ptr: ComPtr<IParameterChanges>,
    events: ComWrapper<Events>,
    events_ptr: ComPtr<IEventList>,
    context: Box<ProcessContext>,
    ins: Buses,
    outs: Buses,
    /// The normalized value last handed over per plugin param; NaN sends it at the next block.
    sent: Vec<f64>,
    /// Per voice, the note sounding and the cents it was detuned by.
    held: [Option<(i16, f32)>; MIDI_CHANNELS],
    /// Per voice, where its channel's pitch wheel sits in `changes` — `None` where the plugin
    /// maps none, which is what decides between the wheel and the note's own `tuning`.
    bend: [Option<usize>; MIDI_CHANNELS],
    /// Whether `setupProcessing` has run: what a re-prepare must undo and a first one must not.
    prepared: bool,
}

// The one deviation from "no unsafe impl": the VST3 contract lets `process` run on a thread of
// the host's choosing once `setProcessing` was called, and these pointers cross exactly once.
unsafe impl Send for Live {}

impl Live {
    fn open(class: &Derived, rate: f64) -> Result<Live, String> {
        let factory = module::factory(&class.binary)?;
        let component: ComPtr<IComponent> = factory.create(&class.cid)?;
        let host = ComWrapper::new(Host).to_com_ptr::<FUnknown>().expect("a host is an FUnknown");
        unsafe { ok(component.initialize(host.as_ptr()), "initialize")? };
        let Some(processor) = component.cast::<IAudioProcessor>() else {
            unsafe { component.terminate() };
            return Err("the component is no IAudioProcessor".into());
        };
        let (controller, wired) = unsafe { pair(&factory, &component, &host) };
        let (ins, outs) = unsafe { arrange(&component, &processor, class) };
        // A per-note offset prefers each channel's pitch-wheel parameter, which every plugin
        // honours; the note-on's optional `tuning` is the fallback.
        let (bend, bend_ids) = unsafe { wheels(controller.as_ref(), &component, class.params.len()) };
        let changes = ComWrapper::new(Changes::new(class.params.iter().map(|(id, _)| *id).chain(bend_ids)));
        let changes_ptr = changes.to_com_ptr().expect("changes are an IParameterChanges");
        let events = ComWrapper::new(Events::with_capacity(BLOCK * MIDI_CHANNELS));
        let events_ptr = events.to_com_ptr().expect("events are an IEventList");
        let mut live = Live {
            _host: host,
            component,
            processor,
            controller,
            wired,
            changes,
            changes_ptr,
            events,
            events_ptr,
            context: Box::new(unsafe { std::mem::zeroed() }),
            ins,
            outs,
            sent: vec![f64::NAN; class.params.len()],
            held: [None; MIDI_CHANNELS],
            bend,
            prepared: false,
        };
        live.retune(rate)?;
        Ok(live)
    }

    fn retune(&mut self, rate: f64) -> Result<(), String> {
        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            maxSamplesPerBlock: BLOCK as int32,
            sampleRate: rate,
        };
        unsafe {
            if self.prepared {
                self.processor.setProcessing(0);
                self.component.setActive(0);
            }
            self.prepared = false;
            ok(self.processor.setupProcessing(&mut setup), "setupProcessing")?;
            activate_buses(&self.component, self.ins.buses.len(), self.outs.buses.len());
            ok(self.component.setActive(1), "setActive")?;
            // OPTIONAL in the SDK: a plugin that does not distinguish processing from active
            // answers kNotImplemented, which is an answer rather than a refusal.
            let processing = self.processor.setProcessing(1);
            if processing != kNotImplemented {
                ok(processing, "setProcessing")?;
            }
        }
        self.prepared = true;
        self.context.sampleRate = rate;
        // A playing, advancing transport: a zeroed context tells a plugin the host is stopped at
        // 0 BPM, and a tempo-synced engine then correctly produces nothing.
        self.context.state = (ProcessContext_::StatesAndFlags_::kPlaying
            | ProcessContext_::StatesAndFlags_::kTempoValid
            | ProcessContext_::StatesAndFlags_::kTimeSigValid
            | ProcessContext_::StatesAndFlags_::kProjectTimeMusicValid
            | ProcessContext_::StatesAndFlags_::kContTimeValid) as uint32;
        self.context.tempo = TEMPO;
        self.context.timeSigNumerator = 4;
        self.context.timeSigDenominator = 4;
        // The reactivation dropped the plugin's voices, so a gate still HIGH must note again.
        self.sent.fill(f64::NAN);
        self.held = [None; MIDI_CHANNELS];
        Ok(())
    }

    /// The half a view is asked of: the separate controller, or the component where it is both.
    fn controller(&self) -> Option<ComPtr<IEditController>> {
        self.controller.clone().or_else(|| self.component.cast())
    }

    fn load(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let stream = ComWrapper::new(Stream::of(bytes));
        let ptr = stream.to_com_ptr::<IBStream>().expect("a stream is an IBStream");
        unsafe { self.component.setState(ptr.as_ptr()) };
    }

    fn block(&mut self, class: &Derived, b: &mut Block<'_>) {
        // The voice params, if any, are the ones the manifest carries beyond the plugin's own.
        let voice = b.scalars.len() - class.params.len();
        self.changes.clear();
        for (i, (_, steps)) in class.params.iter().enumerate() {
            let value = (b.scalars[voice + i] as f64 / steps).clamp(0.0, 1.0);
            if self.sent[i].to_bits() != value.to_bits() {
                self.sent[i] = value;
                self.changes.set(i, value);
            }
        }
        self.events.clear();
        if voice == 3 {
            // The `voice` cable is the last input: pitches then velocities, no gate, as a
            // velocity of zero is the note off.
            let cable = b.ins.last().filter(|p| p.wired() && p.channels() >= 2);
            // A param gate crosses at GATE_HIGH; the cable's gate is MIDI's, any velocity above
            // zero.
            let (voices, read, held): (usize, &Reader<'_>, &Gate) = match cable {
                    Some(p) => {
                        let n = p.channels() as usize / 2;
                        (n, &move |which, c, s| if which == 1 { p.chan(c)[s] } else { p.chan(n + c)[s] }, &|v| v > 0.0)
                    }
                    None => {
                        let n = (b.params[0].channels() as usize).min(MIDI_CHANNELS);
                        (n, &|which, c, s| b.params[which].chan(c)[s], &high)
                    }
                };
            for c in 0..voices.min(MIDI_CHANNELS) {
                for s in 0..BLOCK {
                    match (held(read(0, c, s)), self.held[c]) {
                        (true, None) => {
                            // A plugin's note is an integer; what the round threw away rides as
                            // `tuning`.
                            let want = (60.0 + 12.0 * read(1, c, s)).clamp(0.0, 127.0);
                            let note = want.round();
                            let off = want - note;
                            // One spelling or the other, never both: a plugin honouring each would
                            // detune twice.
                            let detune = match self.bend[c] {
                                Some(slot) => {
                                    let wheel = 0.5 + 0.5 * (off / BEND_SEMITONES).clamp(-1.0, 1.0);
                                    self.changes.set(slot, wheel as f64);
                                    0.0
                                }
                                None => off * CENTS_PER_SEMITONE,
                            };
                            self.held[c] = Some((note as i16, detune));
                            self.events.push(note_on(c, s, note as i16, detune, read(2, c, s)));
                        }
                        (false, Some((note, detune))) => {
                            self.held[c] = None;
                            self.events.push(note_off(c, s, note, detune));
                        }
                        _ => {}
                    }
                }
            }
        }
        for (bus, port) in self.ins.buses.iter_mut().zip(b.ins.iter()) {
            bus.silenceFlags = if port.wired() { 0 } else { u64::MAX };
        }
        for (i, port) in b.ins.iter().enumerate().take(self.ins.pointers.len()) {
            for (c, dst) in self.ins.pointers[i].iter().enumerate() {
                unsafe { std::ptr::copy_nonoverlapping(port.chan(c).as_ptr(), *dst, BLOCK) };
            }
        }
        for lanes in &self.outs.pointers {
            for dst in lanes {
                unsafe { std::ptr::write_bytes(*dst, 0, BLOCK) };
            }
        }
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: BLOCK as int32,
            numInputs: self.ins.buses.len() as int32,
            numOutputs: self.outs.buses.len() as int32,
            inputs: self.ins.buses.as_mut_ptr(),
            outputs: self.outs.buses.as_mut_ptr(),
            inputParameterChanges: self.changes_ptr.as_ptr(),
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: self.events_ptr.as_ptr(),
            outputEvents: std::ptr::null_mut(),
            processContext: &mut *self.context,
        };
        unsafe { self.processor.process(&mut data) };
        // Advanced AFTER the block it described, so the plugin's clock runs at the rate its own
        // audio does. A transport that is valid but frozen is a host paused on the first sample.
        self.context.projectTimeSamples += BLOCK as i64;
        self.context.continousTimeSamples += BLOCK as i64;
        self.context.projectTimeMusic += BLOCK as f64 / self.context.sampleRate * (TEMPO / 60.0);
        for (i, out) in b.outs.iter_mut().enumerate().take(self.outs.pointers.len()) {
            let lanes = &self.outs.pointers[i];
            for c in 0..out.channels() as usize {
                let src = lanes[c.min(lanes.len() - 1)];
                unsafe { std::ptr::copy_nonoverlapping(src, out.chan_mut(c).as_mut_ptr(), BLOCK) };
            }
        }
    }
}

/// Which parameter each voice channel's pitch wheel is, asked of the plugin itself. Answers the
/// slot per channel and the ids to append to `changes`, in that order.
unsafe fn wheels(
    controller: Option<&ComPtr<IEditController>>,
    component: &ComPtr<IComponent>,
    first: usize,
) -> ([Option<usize>; MIDI_CHANNELS], Vec<ParamID>) {
    let mut slots = [None; MIDI_CHANNELS];
    let mut ids = Vec::new();
    let Some(mapping) = controller.cloned().or_else(|| component.cast()).and_then(|c: ComPtr<IEditController>| c.cast::<IMidiMapping>())
    else {
        return (slots, ids);
    };
    for (channel, slot) in slots.iter_mut().enumerate() {
        let mut id: ParamID = 0;
        let asked = mapping.getMidiControllerAssignment(0, channel as i16, ControllerNumbers_::kPitchBend as CtrlNumber, &mut id);
        if asked == kResultOk {
            *slot = Some(first + ids.len());
            ids.push(id);
        }
    }
    (slots, ids)
}

/// A component and its controller, each holding the other's connection point.
type Wire = (ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>);

/// What pairing yields: the other half, and the connection to undo before tearing it down.
type Pair = (Option<ComPtr<IEditController>>, Option<Wire>);

/// The component's separate controller, initialized, connected and seeded with the component's
/// state, for the scan and a live instance alike. Every step is optional; a failure changes nothing.
pub(super) unsafe fn pair(
    factory: &module::Factory,
    component: &ComPtr<IComponent>,
    context: &ComPtr<FUnknown>,
) -> Pair {
    if component.cast::<IEditController>().is_some() {
        return (None, None);
    }
    let mut ccid: TUID = [0; 16];
    if component.getControllerClassId(&mut ccid) != kResultOk {
        return (None, None);
    }
    let Ok(controller) = factory.create::<IEditController>(&ccid) else { return (None, None) };
    if controller.initialize(context.as_ptr()) != kResultOk {
        return (None, None);
    }
    let wired = match (component.cast::<IConnectionPoint>(), controller.cast::<IConnectionPoint>()) {
        (Some(cp), Some(ccp)) if cp.connect(ccp.as_ptr()) == kResultOk && ccp.connect(cp.as_ptr()) == kResultOk => {
            Some((cp, ccp))
        }
        _ => None,
    };
    let state = ComWrapper::new(Stream::default());
    if let Some(s) = state.to_com_ptr::<IBStream>() {
        if component.getState(s.as_ptr()) == kResultOk {
            s.seek(0, IBStream_::IStreamSeekMode_::kIBSeekSet as int32, std::ptr::null_mut());
            controller.setComponentState(s.as_ptr());
        }
    }
    (Some(controller), wired)
}

/// Undo a [`pair`]: the wire both ways BEFORE the controller is terminated, or the component
/// points at a torn-down controller.
pub(super) unsafe fn unpair(controller: Option<&ComPtr<IEditController>>, wired: Option<&Wire>) {
    if let Some((cp, ccp)) = wired {
        ccp.disconnect(cp.as_ptr());
        cp.disconnect(ccp.as_ptr());
    }
    if let Some(c) = controller {
        c.terminate();
    }
}

/// The buses at the plugin's default arrangements, staged at the widths the LIVE instance reports,
/// never the scan's cached counts.
unsafe fn arrange(component: &ComPtr<IComponent>, processor: &ComPtr<IAudioProcessor>, class: &Derived) -> (Buses, Buses) {
    let (input, output) = (BusDirections_::kInput as BusDirection, BusDirections_::kOutput as BusDirection);
    let arrangements = |dir: BusDirection, n: usize| -> Vec<SpeakerArrangement> {
        (0..n as int32)
            .map(|i| {
                let mut arrangement = 0;
                processor.getBusArrangement(dir, i, &mut arrangement);
                arrangement
            })
            .collect()
    };
    let (mut ins, mut outs) = (arrangements(input, class.inputs.len()), arrangements(output, class.outputs.len()));
    processor.setBusArrangements(ins.as_mut_ptr(), ins.len() as int32, outs.as_mut_ptr(), outs.len() as int32);
    let widths = |dir, n: usize| -> Vec<u16> { channel_counts(component, dir, n as int32).map(|c| c.max(1) as u16).collect() };
    (Buses::new(&widths(input, ins.len())), Buses::new(&widths(output, outs.len())))
}

/// The channel count of each of the first `n` audio buses in `dir`.
pub(super) unsafe fn channel_counts(component: &ComPtr<IComponent>, dir: BusDirection, n: int32) -> impl Iterator<Item = int32> + '_ {
    (0..n).map(move |i| {
        let mut info: BusInfo = std::mem::zeroed();
        component.getBusInfo(MediaTypes_::kAudio as MediaType, dir, i, &mut info);
        info.channelCount
    })
}

/// Activate the audio and event buses AFTER `setupProcessing` and BEFORE `setActive`, as
/// Steinberg's own host does: earlier left some plugins rendering silence.
unsafe fn activate_buses(component: &ComPtr<IComponent>, n_in: usize, n_out: usize) {
    let audio = MediaTypes_::kAudio as MediaType;
    let input = BusDirections_::kInput as BusDirection;
    for i in 0..n_in as int32 {
        component.activateBus(audio, input, i, 1);
    }
    for i in 0..n_out as int32 {
        component.activateBus(audio, BusDirections_::kOutput as BusDirection, i, 1);
    }
    let events = MediaTypes_::kEvent as MediaType;
    for i in 0..component.getBusCount(events, input) {
        component.activateBus(events, input, i, 1);
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        unsafe {
            if self.prepared {
                self.processor.setProcessing(0);
                self.component.setActive(0);
            }
            unpair(self.controller.as_ref(), self.wired.as_ref());
            self.component.terminate();
        }
    }
}

/// Staged bus buffers and the pointer tables a plugin reads. Every access goes through the
/// pointers, so the plugin's writes and ours never race a Rust borrow.
struct Buses {
    _samples: Vec<Vec<[f32; BLOCK]>>,
    pointers: Vec<Vec<*mut f32>>,
    buses: Vec<AudioBusBuffers>,
}

impl Buses {
    fn new(widths: &[u16]) -> Buses {
        let mut samples: Vec<Vec<[f32; BLOCK]>> = widths.iter().map(|&w| vec![[0.0; BLOCK]; w.max(1) as usize]).collect();
        let mut pointers: Vec<Vec<*mut f32>> =
            samples.iter_mut().map(|bus| bus.iter_mut().map(|lane| lane.as_mut_ptr()).collect()).collect();
        let buses = pointers
            .iter_mut()
            .map(|lanes| AudioBusBuffers {
                numChannels: lanes.len() as int32,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 { channelBuffers32: lanes.as_mut_ptr() },
            })
            .collect();
        Buses { _samples: samples, pointers, buses }
    }
}

fn note_on(channel: usize, sample: usize, pitch: i16, tuning: f32, velocity: f32) -> Event {
    Event {
        busIndex: 0,
        sampleOffset: sample as int32,
        ppqPosition: 0.0,
        flags: 0,
        r#type: Event_::EventTypes_::kNoteOnEvent as u16,
        __field0: Event__type0 { noteOn: NoteOnEvent { channel: channel as i16, pitch, tuning, velocity, length: 0, noteId: -1 } },
    }
}

fn note_off(channel: usize, sample: usize, pitch: i16, tuning: f32) -> Event {
    Event {
        busIndex: 0,
        sampleOffset: sample as int32,
        ppqPosition: 0.0,
        flags: 0,
        r#type: Event_::EventTypes_::kNoteOffEvent as u16,
        __field0: Event__type0 { noteOff: NoteOffEvent { channel: channel as i16, pitch, velocity: 0.0, noteId: -1, tuning } },
    }
}
