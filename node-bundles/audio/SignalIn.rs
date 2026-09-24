use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{cross, AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, SlotDecl, Tag};

goofi_audio_sdk::params! {
    MODE = ParamDecl {
        group: "signal",
        name: "mode",
        spec: ParamSpec::Str { default: "waveform", options: cross::PLAYBACK, refresh: false },
        expression: None,
        doc: Some("the frame's samples looped until the next frame, or each value a sine at that many Hz"),
    },
    SMOOTHING = ParamDecl {
        group: "signal",
        name: "smoothing",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 10.0 },
        expression: None,
        doc: Some("seconds a new frame crossfades in over, or each sine glides to its new pitch in"),
    },
}

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Array, trigger_process: false, multi: false, required: false }];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Control],
    doc: "A signal frame crossing into the audio plane.\n\
          As a `waveform`, a `[C, T]` frame with `sfreq` enters as `C` audio channels, resampled \
          to the rate; one with no `sfreq` enters one sample per sample. The newest frame loops \
          until the next one takes over at its end, so frames on time play back to back and a \
          late one never leaves a gap. As an `oscillator`, every value is a sine at that many Hz, \
          and their mean is one channel. `smoothing` morphs one frame into the next.",
    inputs: INS,
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct SignalIn;

impl AudioNode for SignalIn {
    fn prepare(&mut self, _rate: f64) {}

    /// The engine plays the frames by `mode` and `smoothing`; what reaches here is already sound.
    fn process(&mut self, b: &mut Block<'_>) {
        let input = &b.ins[0];
        let out = &mut b.outs[0];
        for c in 0..out.channels() as usize {
            out.chan_mut(c).copy_from_slice(input.chan(c));
        }
    }
}

goofi_audio_sdk::export!(SignalIn, MANIFEST);
