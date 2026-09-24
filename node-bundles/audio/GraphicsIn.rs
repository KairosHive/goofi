use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{cross, AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, SlotDecl, Tag, BLOCK};

goofi_audio_sdk::params! {
    MODE = ParamDecl {
        group: "graphics",
        name: "mode",
        spec: ParamSpec::Str { default: "bipolar", options: cross::RANGES, refresh: false },
        expression: None,
        doc: Some("the texels themselves, or `min`..`max` mapped onto [-1, 1] or [0, 1]"),
    },
    MIN = ParamDecl {
        group: "graphics",
        name: "min",
        spec: ParamSpec::Float { default: 0.0, min: -1.0e9, max: 1.0e9 },
        expression: None,
        doc: Some("the value a mapped range starts at; a texture's own window is 0 to 1"),
    },
    MAX = ParamDecl {
        group: "graphics",
        name: "max",
        spec: ParamSpec::Float { default: 1.0, min: -1.0e9, max: 1.0e9 },
        expression: None,
        doc: Some("the value it ends at"),
    },
    CHANNELS = ParamDecl {
        group: "graphics",
        name: "channels",
        spec: ParamSpec::Str { default: "luma", options: &["luma", "all"], refresh: false },
        expression: None,
        doc: Some("the picture's brightness on one channel, or its colour channels each on their own"),
    },
}

/// Rec. 709 luminance. A port narrower than three channels reads its one channel on all three,
/// and the weights sum to one, so a gray texture crosses unchanged.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Texture, trigger_process: false, multi: false, required: false }];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Control],
    doc: "A texture crossing into the audio plane.\n\
          Its texels are read in scan order, row 0 first, one sample per texel — so the picture's \
          SIZE is its length, and a frame arrives whole and loops until the next one does. The \
          inbox holds one second of samples, so a picture past about 435 square does not fit and \
          is dropped: this crossing is for a modest one.",
    inputs: INS,
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct GraphicsIn;

impl AudioNode for GraphicsIn {
    fn channels(&self, ins: &[u16], params: &[f64], outs: usize) -> Vec<u16> {
        let one = params.get(P::CHANNELS).is_none_or(|v| *v < 0.5);
        vec![if one { 1 } else { ins.first().copied().unwrap_or(1).max(1) }; outs]
    }

    fn prepare(&mut self, _rate: f64) {}

    fn process(&mut self, b: &mut Block<'_>) {
        let mode = b.params[P::MODE].chan(0)[0] as u8;
        let luma = b.params[P::CHANNELS].chan(0)[0] < 0.5;
        let (input, min, max) = (&b.ins[0], &b.params[P::MIN], &b.params[P::MAX]);
        let out = &mut b.outs[0];
        for c in 0..out.channels() as usize {
            let (lo, hi) = (min.chan(c), max.chan(c));
            let mut mixed = [0.0f32; BLOCK];
            for i in 0..BLOCK {
                mixed[i] = match luma {
                    true => (0..3).map(|k| LUMA[k] * input.chan(k)[i]).sum(),
                    false => input.chan(c)[i],
                };
            }
            let y = out.chan_mut(c);
            for i in 0..BLOCK {
                y[i] = cross::ranged(mode, mixed[i], lo[i], hi[i]);
            }
        }
    }
}

goofi_audio_sdk::export!(GraphicsIn, MANIFEST);
