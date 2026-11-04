use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{Face, cross, AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, Show, SlotDecl, Tag};

const WAVEFORM: &[Show] = &[Show { param: "mode", any_of: &["waveform"] }];
const OSCILLATOR: &[Show] = &[Show { param: "mode", any_of: &["oscillator"] }];

goofi_audio_sdk::params! {
    MODE = ParamDecl {
        group: "signal",
        name: "mode",
        spec: ParamSpec::Str { default: "waveform", options: cross::PLAYBACK, refresh: false },
        expression: None,
        doc: Some("the frame's samples looped until the next frame, or each column a sine: [n] Hz, or [2, n] Hz over phase"),
        section: 0,
        show: &[],
        role: None,
    },
    PITCH = ParamDecl {
        group: "signal",
        name: "pitch",
        spec: ParamSpec::Str { default: "hz", options: &["hz", "v/oct"], refresh: false },
        expression: None,
        doc: Some("what a column's pitch is: cycles a second, or volts per octave with 0 V at C4"),
        section: 0,
        show: OSCILLATOR,
        role: None,
    },
    MIX = ParamDecl {
        group: "signal",
        name: "mix",
        spec: ParamSpec::Bool { default: false },
        expression: None,
        doc: Some("the rows of a [C, T] frame played together as one channel, their mean, rather than one channel each"),
        section: 0,
        show: WAVEFORM,
        role: None,
    },
    LOW = ParamDecl {
        group: "signal",
        name: "low",
        spec: ParamSpec::Num { default: &[-1.0], min: -10.0, max: 10.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("the input value that is full scale low, -1 on the audio plane"),
        section: 0,
        show: WAVEFORM,
        role: None,
    },
    HIGH = ParamDecl {
        group: "signal",
        name: "high",
        spec: ParamSpec::Num { default: &[1.0], min: -10.0, max: 10.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("the input value that is full scale high, 1 on the audio plane"),
        section: 0,
        show: WAVEFORM,
        role: None,
    },
    SPEED = ParamDecl {
        group: "signal",
        name: "speed",
        spec: ParamSpec::Num { default: &[1.0], min: 0.001, max: 100.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("frame samples played per audio sample: 1 is one for one, 2 twice as fast"),
        section: 1,
        show: WAVEFORM,
        role: None,
    },
    RESAMPLE = ParamDecl {
        group: "signal",
        name: "resample",
        spec: ParamSpec::Str { default: "linear", options: cross::RESAMPLE, refresh: false },
        expression: None,
        doc: Some("how samples between a frame's own are made: a line, the nearest, or a cubic through four"),
        section: 1,
        show: WAVEFORM,
        role: None,
    },
    MORPH = ParamDecl {
        group: "signal",
        name: "morph",
        spec: ParamSpec::Bool { default: false },
        expression: None,
        doc: Some("glide from the previous frame to a new one over the time between their arrivals"),
        section: 2,
        show: WAVEFORM,
        role: None,
    },
    SEAM = ParamDecl {
        group: "signal",
        name: "seam",
        spec: ParamSpec::Num { default: &[0.0], min: 0.0, max: 3000.0, int: true, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("audio samples on each side of the loop point that blend across it, 0 for none"),
        section: 2,
        show: WAVEFORM,
        role: None,
    },
    SIGMA = ParamDecl {
        group: "signal",
        name: "sigma",
        spec: ParamSpec::Num { default: &[300.0], min: 1.0, max: 3000.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("the width of the seam's gaussian falloff, in audio samples"),
        section: 2,
        show: WAVEFORM,
        role: None,
    },
}

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Array, trigger_process: false, multi: false, required: false }];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Control],
    doc: "A signal frame crossing into the audio plane.\n\
          As a `waveform`, a `[C, T]` frame enters as `C` audio channels, `speed` frame samples \
          to an audio sample. The newest frame loops until the next one takes over at its end, so \
          frames on time play back to back and a late one never leaves a gap; with `morph`, it \
          takes over as it arrives and the loop glides into it. `seam` blends the loop's two ends \
          where they meet. With `mix`, the `C` rows play together as one channel, their mean. \
          `low..high` are the values that are full scale. As an `oscillator`, an `[n]` frame is \
          n sines at those pitches, Hz or volts per octave by `pitch`, and a `[2, n]` frame n \
          sines at row 0 with row 1 as their phase in radians; their mean is one channel.",
    inputs: INS,
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct SignalIn;

impl AudioNode for SignalIn {
    fn prepare(&mut self, _rate: f64) {}

    /// The engine plays the frames by the `signal` params; what reaches here is already sound.
    fn process(&mut self, b: &mut Block<'_>) {
        let input = &b.ins[0];
        let out = &mut b.outs[0];
        for c in 0..out.channels() as usize {
            out.chan_mut(c).copy_from_slice(input.chan(c));
        }
    }
}

goofi_audio_sdk::export!(SignalIn, MANIFEST);
