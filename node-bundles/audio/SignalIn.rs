use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{cross, AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, SlotDecl, Tag, BLOCK};

goofi_audio_sdk::params! {
    MODE = ParamDecl {
        group: "signal",
        name: "mode",
        spec: ParamSpec::Str { default: "direct", options: cross::RANGES, refresh: false },
        expression: None,
        doc: Some("the values themselves, or `min`..`max` mapped onto [-1, 1] or [0, 1]"),
    },
    MIN = ParamDecl {
        group: "signal",
        name: "min",
        spec: ParamSpec::Float { default: 0.0, min: -1.0e9, max: 1.0e9 },
        expression: None,
        doc: Some("the value a mapped range starts at"),
    },
    MAX = ParamDecl {
        group: "signal",
        name: "max",
        spec: ParamSpec::Float { default: 1.0, min: -1.0e9, max: 1.0e9 },
        expression: None,
        doc: Some("the value it ends at"),
    },
}

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Array, trigger_process: false, multi: false, required: false }];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Control],
    doc: "A signal frame crossing into the audio plane.\n\
          A `[C, T]` frame with `sfreq` enters as `C` audio channels, resampled to the rate. A \
          frame with no `sfreq` enters one sample per sample, so a control value is held until \
          the next. `mode` says what its numbers are worth here: a signal in its own units \
          becomes a full-scale signal by naming the range it spans.",
    inputs: INS,
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct SignalIn;

impl AudioNode for SignalIn {
    fn prepare(&mut self, _rate: f64) {}

    fn process(&mut self, b: &mut Block<'_>) {
        let mode = b.params[P::MODE].chan(0)[0] as u8;
        let (input, min, max) = (&b.ins[0], &b.params[P::MIN], &b.params[P::MAX]);
        let out = &mut b.outs[0];
        for c in 0..out.channels() as usize {
            let (x, lo, hi) = (input.chan(c), min.chan(c), max.chan(c));
            let y = out.chan_mut(c);
            for i in 0..BLOCK {
                y[i] = cross::ranged(mode, x[i], lo[i], hi[i]);
            }
        }
    }
}

goofi_audio_sdk::export!(SignalIn, MANIFEST);
