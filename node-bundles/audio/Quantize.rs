use goofi_audio_sdk::goofi_core::scale::{Recipe, MASK_BITS, MAX_DEGREES, METHODS, NOTES, SCALES};
use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{Face, AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, SlotDecl, Tag, BLOCK};

goofi_audio_sdk::params! {
    SCALE = ParamDecl {
        doc: Some("a named scale, or `custom` to build one from the params below"),
        ..ParamDecl::new("scale", "scale", ParamSpec::Str { default: "major", options: &SCALES, refresh: false })
    },
    ROOT = ParamDecl {
        doc: Some("the note the scale starts on"),
        ..ParamDecl::new("scale", "root", ParamSpec::Str { default: "C", options: NOTES, refresh: false })
    },
    METHOD = ParamDecl {
        doc: Some("a custom scale stacked from a generator, read off the harmonic series, or picked from an equal division"),
        ..ParamDecl::new("scale", "method", ParamSpec::Str { default: "generator", options: METHODS, refresh: false })
    },
    GENERATOR = ParamDecl {
        doc: Some("the interval stacked, in cents: 700 is a tempered fifth, 701.955 a pure one"),
        ..ParamDecl::new("scale", "generator", ParamSpec::Num { default: &[700.0], min: 0.0, max: 4800.0, int: false, options: &[], face: Face::Plain })
    },
    STEPS = ParamDecl {
        doc: Some("notes in the octave: generators stacked, harmonics read (partials steps to 2 steps - 1), or equal parts"),
        ..ParamDecl::new("scale", "steps", ParamSpec::Num { default: &[7.0], min: 1.0, max: 53.0, int: true, options: &[], face: Face::Plain })
    },
    MASK = ParamDecl {
        doc: Some("which of a division's first 24 parts are admitted, bit k for part k; zero admits every part"),
        ..ParamDecl::new("scale", "mask", ParamSpec::Num { default: &[0.0], min: 0.0, max: ((1 << MASK_BITS) - 1) as f64, int: true, options: &[], face: Face::Plain })
    },
    MODE = ParamDecl {
        doc: Some("the degree the scale is read from: seven fifths read from 4 are major, from 0 lydian, from 2 minor"),
        ..ParamDecl::new("scale", "mode", ParamSpec::Num { default: &[4.0], min: 0.0, max: 52.0, int: true, options: &[], face: Face::Plain })
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
