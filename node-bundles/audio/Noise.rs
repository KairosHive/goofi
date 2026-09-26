use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, Lanes, Tag, BLOCK};

goofi_audio_sdk::params! {
    MODE = ParamDecl {
        group: "noise",
        name: "mode",
        spec: ParamSpec::Str { default: "white", options: &["white", "pink"], refresh: false },
        expression: None,
        doc: Some("`white` is level across the spectrum; `pink` falls with frequency, as most natural sound does"),
        section: 0,
        show: None,
    },
    CHANNELS = ParamDecl {
        group: "noise",
        name: "channels",
        spec: ParamSpec::Int { default: 1, min: 1, max: 256, options: &[] },
        expression: None,
        doc: Some("how many channels to make; no two of them are alike"),
        section: 0,
        show: None,
    },
}

static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator],
    doc: "Noise in [-1, 1], white or pink, on as many channels as are asked for.",
    inputs: &[],
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct Noise {
    /// A fresh lane is zero, which xorshift never leaves; it is seeded by its index on first use.
    seed: Lanes<u32>,
    /// The three one-pole states the pink filter keeps per channel.
    poles: Lanes<[f32; 3]>,
}

/// One uniform sample in [-1, 1], and the state moved on.
fn white(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / 8_388_608.0 - 1.0
}

impl AudioNode for Noise {
    fn channels(&self, _ins: &[u16], params: &[f64], outs: usize) -> Vec<u16> {
        vec![(params[P::CHANNELS] as u16).max(1); outs]
    }

    fn prepare(&mut self, _rate: f64) {
        self.poles.reset();
    }

    fn process(&mut self, b: &mut Block<'_>) {
        let pink = b.params[P::MODE].chan(0)[0] as u8 == 1;
        let out = &mut b.outs[0];
        let width = out.channels() as usize;
        let (seeds, poles) = (self.seed.fit(width), self.poles.fit(width));
        for c in 0..width {
            let (seed, poles) = (&mut seeds[c], &mut poles[c]);
            if *seed == 0 {
                // Each channel starts somewhere else, which is what keeps them uncorrelated.
                *seed = 0x9E37_79B9u32.wrapping_mul(c as u32 + 1) | 1;
            }
            let y = out.chan_mut(c);
            for sample in y.iter_mut().take(BLOCK) {
                let w = white(seed);
                *sample = if pink {
                    // Three one-poles an octave apart sum to a −3 dB per octave slope.
                    poles[0] = 0.997_65 * poles[0] + w * 0.099_046;
                    poles[1] = 0.963_00 * poles[1] + w * 0.296_516;
                    poles[2] = 0.570_00 * poles[2] + w * 1.052_691;
                    (poles[0] + poles[1] + poles[2] + w * 0.184_8) * 0.2
                } else {
                    w
                };
            }
        }
    }
}

goofi_audio_sdk::export!(Noise, MANIFEST);
