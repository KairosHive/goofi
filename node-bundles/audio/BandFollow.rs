use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{
    band_partial, band_volts, hz_of, AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, SlotDecl, Tag, BLOCK,
    MAX_CHANNELS,
};

goofi_audio_sdk::params! {
    PITCH = ParamDecl {
        group: "band",
        name: "pitch",
        spec: ParamSpec::Float { default: 0.0, min: -6.0, max: 6.5 },
        expression: None,
        doc: Some("in `harmonic`, what the bands stand on, in volts per octave; an audio reference is one voice per channel"),
    },
    BANDS = ParamDecl {
        group: "band",
        name: "bands",
        spec: ParamSpec::Int { default: 16, min: 2, max: MAX_CHANNELS as i64 },
        expression: None,
        doc: Some("how many bands leave, one per channel; `BandFilter` needs the same count"),
    },
    LOW = ParamDecl {
        group: "band",
        name: "low",
        spec: ParamSpec::Float { default: -1.5, min: -6.0, max: 6.5 },
        expression: None,
        doc: Some("in `spread`, the lowest band's centre in volts per octave, 0 at C4 — the same units as `Osc.pitch`"),
    },
    HIGH = ParamDecl {
        group: "band",
        name: "high",
        spec: ParamSpec::Float { default: 4.25, min: -6.0, max: 6.5 },
        expression: None,
        doc: Some("in `spread`, the highest band's centre; the rest sit evenly between, so a band is a fixed interval"),
    },
    Q = ParamDecl {
        group: "band",
        name: "q",
        spec: ParamSpec::Float { default: 4.0, min: 0.5, max: 20.0 },
        expression: None,
        doc: Some("how narrow one band is; near 4 the default sixteen meet without a gap"),
    },
    ATTACK = ParamDecl {
        group: "band",
        name: "attack",
        spec: ParamSpec::Float { default: 0.005, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("seconds to follow a band that gets louder; short keeps the consonants"),
    },
    RELEASE = ParamDecl {
        group: "band",
        name: "release",
        spec: ParamSpec::Float { default: 0.05, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("seconds to follow a band that gets quieter; long smears one word into the next"),
    },
    LAYOUT = ParamDecl {
        group: "band",
        name: "layout",
        spec: ParamSpec::Str { default: "spread", options: &["spread", "harmonic"], refresh: false },
        expression: None,
        doc: Some("where the bands sit: `spread` evenly from `low` to `high`, or `harmonic` on the partials of `pitch`"),
    },
}

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Audio, trigger_process: false, multi: true, required: false }];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Analysis],
    doc: "How loud its input is in each band, one band per channel.\n\
          The half of a vocoder that listens: wire `out` to `BandFilter.gains` and the shape of \
          this signal lands on that one. Every channel of the input is heard as one signal. In \
          `harmonic` the bands stand on the partials of `pitch` and take its voices in turn, so \
          what it measures is a chord rather than a spread.",
    inputs: INS,
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct BandFollow {
    rate: f32,
    ic1: [f32; MAX_CHANNELS as usize],
    ic2: [f32; MAX_CHANNELS as usize],
    level: [f32; MAX_CHANNELS as usize],
}

/// The one-pole step for a time constant; a zero time is a wire.
fn coef(seconds: f32, rate: f32) -> f32 {
    match seconds > 0.0 {
        true => 1.0 - (-1.0 / (seconds * rate)).exp(),
        false => 1.0,
    }
}

impl AudioNode for BandFollow {
    fn channels(&self, _ins: &[u16], params: &[f64], outs: usize) -> Vec<u16> {
        vec![(params[P::BANDS] as u16).clamp(2, MAX_CHANNELS); outs]
    }

    fn audio_params(&self, _declared: usize) -> usize {
        1
    }

    fn prepare(&mut self, rate: f64) {
        self.rate = rate as f32;
    }

    fn process(&mut self, b: &mut Block<'_>) {
        let rate = self.rate;
        let (low, high) = (b.scalars[P::LOW], b.scalars[P::HIGH]);
        let k = 1.0 / b.scalars[P::Q].max(0.5);
        let (attack, release) = (coef(b.scalars[P::ATTACK], rate), coef(b.scalars[P::RELEASE], rate));

        let input = &b.ins[0];
        let mut heard = [0.0f32; BLOCK];
        for c in 0..input.channels() as usize {
            let x = input.chan(c);
            for i in 0..BLOCK {
                heard[i] += x[i];
            }
        }

        let pitch = &b.params[P::PITCH];
        let harmonic = b.scalars[P::LAYOUT] as u8 == 1;
        let voices = pitch.channels() as usize;
        let out = &mut b.outs[0];
        let bands = out.channels() as usize;
        for band in 0..bands {
            // The bank re-tunes once a block: a note lands on a block edge, and a tan per sample
            // per band buys nothing for it.
            let volts = match harmonic {
                true => {
                    let (voice, partial) = band_partial(band, voices);
                    pitch.chan(voice)[0] + partial
                }
                false => band_volts(band, bands, low, high),
            };
            let f = hz_of(volts).clamp(1.0, 0.45 * rate);
            let g = (std::f32::consts::PI * f / rate).tan();
            let a1 = 1.0 / (1.0 + g * (g + k));
            let (a2, a3) = (g * a1, g * g * a1);
            let (ic1, ic2, level) = (&mut self.ic1[band], &mut self.ic2[band], &mut self.level[band]);
            let y = out.chan_mut(band);
            for i in 0..BLOCK {
                let v3 = heard[i] - *ic2;
                let v1 = a1 * *ic1 + a2 * v3;
                let v2 = *ic2 + a2 * *ic1 + a3 * v3;
                *ic1 = 2.0 * v1 - *ic1;
                *ic2 = 2.0 * v2 - *ic2;
                // `k * v1` is the band at unity, where `Filter`'s own band output peaks at `q`.
                let magnitude = (k * v1).abs();
                *level += (magnitude - *level) * if magnitude > *level { attack } else { release };
                y[i] = *level;
            }
        }
    }
}

goofi_audio_sdk::export!(BandFollow, MANIFEST);
