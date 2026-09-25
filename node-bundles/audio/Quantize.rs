use goofi_audio_sdk::goofi_core::scale::{Recipe, MASK_BITS, MAX_DEGREES, METHODS, NOTES, SCALES};
use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, SlotDecl, Tag, BLOCK};

goofi_audio_sdk::params! {
    SCALE = ParamDecl {
        group: "scale",
        name: "scale",
        spec: ParamSpec::Str { default: "major", options: &SCALES, refresh: false },
        expression: None,
        doc: Some("a named scale, or `custom` to build one from the params below"),
        section: 0,
        show: None,
    },
    ROOT = ParamDecl {
        group: "scale",
        name: "root",
        spec: ParamSpec::Str { default: "C", options: NOTES, refresh: false },
        expression: None,
        doc: Some("the note the scale starts on"),
        section: 0,
        show: None,
    },
    METHOD = ParamDecl {
        group: "scale",
        name: "method",
        spec: ParamSpec::Str { default: "generator", options: METHODS, refresh: false },
        expression: None,
        doc: Some("a custom scale stacked from a generator, read off the harmonic series, or picked from an equal division"),
        section: 0,
        show: None,
    },
    GENERATOR = ParamDecl {
        group: "scale",
        name: "generator",
        spec: ParamSpec::Float { default: 700.0, min: 0.0, max: 4800.0 },
        expression: None,
        doc: Some("the interval stacked, in cents: 700 is a tempered fifth, 701.955 a pure one"),
        section: 0,
        show: None,
    },
    STEPS = ParamDecl {
        group: "scale",
        name: "steps",
        spec: ParamSpec::Int { default: 7, min: 1, max: 53, options: &[] },
        expression: None,
        doc: Some("notes in the octave: generators stacked, harmonics read (partials steps to 2 steps - 1), or equal parts"),
        section: 0,
        show: None,
    },
    MASK = ParamDecl {
        group: "scale",
        name: "mask",
        spec: ParamSpec::Int { default: 0, min: 0, max: (1 << MASK_BITS) - 1, options: &[] },
        expression: None,
        doc: Some("which of a division's first 24 parts are admitted, bit k for part k; zero admits every part"),
        section: 0,
        show: None,
    },
    MODE = ParamDecl {
        group: "scale",
        name: "mode",
        spec: ParamSpec::Int { default: 4, min: 0, max: 52, options: &[] },
        expression: None,
        doc: Some("the degree the scale is read from: seven fifths read from 4 are major, from 0 lydian, from 2 minor"),
        section: 0,
        show: None,
    },
}

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Audio, trigger_process: false, multi: true, required: false }];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "A pitch in volts per octave pulled to the nearest note of a scale.\n\
          A named scale is a preset; `custom` builds any scale from a stacked generator, the \
          harmonic series, or a chosen subset of an equal division, read from any of its modes.",
    inputs: INS,
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct Quantize;

impl AudioNode for Quantize {
    fn prepare(&mut self, _rate: f64) {}

    fn process(&mut self, b: &mut Block<'_>) {
        let at = |k: usize| f64::from(b.params[k].chan(0)[0]);
        let custom = Recipe::from_scalars(at(P::METHOD), at(P::GENERATOR), at(P::STEPS), at(P::MASK), at(P::MODE));
        let recipe = Recipe::chosen(at(P::SCALE) as usize, custom);
        let mut all = [0.0; MAX_DEGREES];
        let n = recipe.degrees(&mut all);
        let degrees = &all[..n];
        let root = at(P::ROOT) * 100.0;
        let input = &b.ins[0];
        let out = &mut b.outs[0];
        for c in 0..out.channels() as usize {
            let x = input.chan(c);
            let y = out.chan_mut(c);
            for i in 0..BLOCK {
                y[i] = ((recipe.snap(degrees, f64::from(x[i]) * 1200.0 - root).0 + root) / 1200.0) as f32;
            }
        }
    }
}

goofi_audio_sdk::export!(Quantize, MANIFEST);
