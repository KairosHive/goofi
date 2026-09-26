use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{
    band_partial, band_volts, hz_of, AudioNode, Block, Lanes, Manifest, OutputDecl, ParamDecl, ParamSpec, SlotDecl, Tag,
    BLOCK,
};

goofi_audio_sdk::params! {
    GAINS = ParamDecl {
        group: "band",
        name: "gains",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 4.0 },
        expression: None,
        doc: Some("what each band is worth; an audio reference is one band per channel, and one number opens them all"),
        section: 0,
        show: None,
    },
    PITCH = ParamDecl {
        group: "band",
        name: "pitch",
        spec: ParamSpec::Float { default: 0.0, min: -6.0, max: 6.5 },
        expression: None,
        doc: Some("in `harmonic`, what the bands stand on, in volts per octave; an audio reference is one voice per channel"),
        section: 0,
        show: None,
    },
    GATE = ParamDecl {
        group: "band",
        name: "gate",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("in `harmonic`, what one voice's partials are worth — a released note keeps its pitch, so this is what lets it go; `MidiIn.gate` drops it at once and an `Env` fades it"),
        section: 0,
        show: None,
    },
    BANDS = ParamDecl {
        group: "band",
        name: "bands",
        spec: ParamSpec::Int { default: 16, min: 2, max: 128, options: &[] },
        expression: None,
        doc: Some("how many bands the input is split into, while nothing drives `gains`; a shape that does is as wide as it is"),
        section: 0,
        show: None,
    },
    LOW = ParamDecl {
        group: "band",
        name: "low",
        spec: ParamSpec::Float { default: -1.5, min: -6.0, max: 6.5 },
        expression: None,
        doc: Some("in `spread`, the lowest band's centre in volts per octave, 0 at C4 — the same units as `Osc.pitch`"),
        section: 0,
        show: None,
    },
    HIGH = ParamDecl {
        group: "band",
        name: "high",
        spec: ParamSpec::Float { default: 4.25, min: -6.0, max: 6.5 },
        expression: None,
        doc: Some("in `spread`, the highest band's centre; the rest sit evenly between, so a band is a fixed interval"),
        section: 0,
        show: None,
    },
    Q = ParamDecl {
        group: "band",
        name: "q",
        spec: ParamSpec::Float { default: 4.0, min: 0.5, max: 20.0 },
        expression: None,
        doc: Some("how narrow one band is; near 4 the default sixteen meet without a gap"),
        section: 0,
        show: None,
    },
    LAYOUT = ParamDecl {
        group: "band",
        name: "layout",
        spec: ParamSpec::Str { default: "spread", options: &["spread", "harmonic"], refresh: false },
        expression: None,
        doc: Some("where the bands sit: `spread` evenly from `low` to `high`, or `harmonic` on the partials of `pitch`"),
        section: 0,
        show: None,
    },
}

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Audio, trigger_process: false, multi: true, required: false }];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "Its input split into bands, each one at its own gain, added back up.\n\
          The half of a vocoder that speaks: `gains` from a `BandFollow` puts that signal's shape \
          on this one, and a shape is as many bands wide as it says, so the two banks cannot \
          disagree about the count. With nothing behind `gains` every band is open, which is a \
          few dB louder than the input rather than equal to it, because neighbouring bands \
          overlap and add. One voice per channel of \
          the input, so a chord goes through as a chord. In `harmonic` the bands stand on the \
          partials of `pitch` and take its voices in turn, so what passes is a chord rather than \
          a spread.",
    inputs: INS,
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct BandFilter {
    rate: f32,
    /// One state-variable filter per band per channel, channel-major.
    ic1: Lanes<f32>,
    ic2: Lanes<f32>,
}

impl AudioNode for BandFilter {
    /// The bands are the gains' axis, never the output's: what leaves is what arrived, per voice.
    fn channels(&self, ins: &[u16], _params: &[f64], outs: usize) -> Vec<u16> {
        vec![ins.first().copied().unwrap_or(1).max(1); outs]
    }

    fn audio_params(&self, _declared: usize) -> usize {
        3
    }

    fn prepare(&mut self, rate: f64) {
        self.rate = rate as f32;
    }

    fn process(&mut self, b: &mut Block<'_>) {
        let rate = self.rate;
        let (low, high) = (b.scalars[P::LOW], b.scalars[P::HIGH]);
        let k = 1.0 / b.scalars[P::Q].max(0.5);
        let (input, gains, pitch) = (&b.ins[0], &b.params[P::GAINS], &b.params[P::PITCH]);
        let gate = &b.params[P::GATE];
        // A shape says how many bands it holds, so a driven bank cannot disagree with its follower.
        let bands = match gains.channels() > 1 {
            true => gains.channels() as usize,
            false => (b.scalars[P::BANDS] as usize).max(1),
        };
        let harmonic = b.scalars[P::LAYOUT] as u8 == 1;
        let voices = pitch.channels() as usize;
        let out = &mut b.outs[0];
        let width = out.channels() as usize;
        let (ic1s, ic2s) = (self.ic1.fit(bands * width), self.ic2.fit(bands * width));
        for c in 0..width {
            let x = input.chan(c);
            let (ic1s, ic2s) = (&mut ic1s[c * bands..(c + 1) * bands], &mut ic2s[c * bands..(c + 1) * bands]);
            let y = out.chan_mut(c);
            y.fill(0.0);
            for band in 0..bands {
                // The bank re-tunes once a block: a note lands on a block edge, and a tan per
                // sample per band buys nothing for it.
                let (volts, voiced) = match harmonic {
                    true => {
                        let (voice, partial) = band_partial(band, voices);
                        (pitch.chan(voice)[0] + partial, gate.chan(voice)[0])
                    }
                    false => (band_volts(band, bands, low, high), 1.0),
                };
                let f = hz_of(volts).clamp(1.0, 0.45 * rate);
                let g = (std::f32::consts::PI * f / rate).tan();
                let a1 = 1.0 / (1.0 + g * (g + k));
                let (a2, a3) = (g * a1, g * g * a1);
                let (ic1, ic2, gain) = (&mut ic1s[band], &mut ic2s[band], gains.chan(band));
                for i in 0..BLOCK {
                    let v3 = x[i] - *ic2;
                    let v1 = a1 * *ic1 + a2 * v3;
                    let v2 = *ic2 + a2 * *ic1 + a3 * v3;
                    *ic1 = 2.0 * v1 - *ic1;
                    *ic2 = 2.0 * v2 - *ic2;
                    // `k * v1` is the band at unity, where `Filter`'s own band output peaks at `q`.
                    y[i] += k * v1 * gain[i] * voiced;
                }
            }
        }
    }
}

goofi_audio_sdk::export!(BandFilter, MANIFEST);
