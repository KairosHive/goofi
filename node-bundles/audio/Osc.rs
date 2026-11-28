use goofi_sdk::audio::goofi_core::SlotType;
use goofi_sdk::audio::{Face, hz_of, AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, Lanes, Tag, BLOCK};

goofi_sdk::audio::params! {
    PITCH = ParamDecl {
        doc: Some("volts per octave, 0 at C4 (0.75 is A4); an audio reference is one voice per channel"),
        ..ParamDecl::new("osc", "pitch", ParamSpec::Num { default: &[0.75], min: -10.0, max: 10.0, int: false, options: &[], face: Face::Plain })
    },
    WAVEFORM = ParamDecl {
        doc: Some("the shape one cycle traces, from the roundest to the brightest"),
        ..ParamDecl::new("osc", "waveform", ParamSpec::Str { default: "sine", options: &["sine", "triangle", "sawtooth", "square"], refresh: false, })
    },
}

static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator],
    doc: "An oscillator in [-1, 1], one channel per channel of `pitch`.",
    inputs: &[],
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct Osc {
    phase: Lanes<f32>,
    step: f32,
}

impl AudioNode for Osc {
    fn prepare(&mut self, rate: f64) {
        self.step = 1.0 / rate as f32;
    }

    fn process(&mut self, b: &mut Block<'_>) {
        let pitch = &b.params[P::PITCH];
        let waveform = b.params[P::WAVEFORM].chan(0)[0] as u8;
        let out = &mut b.outs[0];
        let width = out.channels() as usize;
        let phases = self.phase.fit(width);
        for (c, phase) in phases.iter_mut().enumerate() {
            let p = pitch.chan(c);
            let samples = out.chan_mut(c);
            for i in 0..BLOCK {
                samples[i] = match waveform {
                    1 => 1.0 - 4.0 * (*phase - 0.5).abs(),
                    2 => 2.0 * *phase - 1.0,
                    3 => if *phase < 0.5 { 1.0 } else { -1.0 },
                    _ => (std::f32::consts::TAU * *phase).sin(),
                };
                *phase = (*phase + hz_of(p[i]) * self.step).fract();
            }
        }
    }
}

goofi_sdk::audio::export!(Osc, MANIFEST);
