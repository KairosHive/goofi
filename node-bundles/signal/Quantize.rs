//! Quantize — a pitch pulled onto the nearest note of a scale. `mode` says what a value is: a
//! frequency in Hz, or volts per octave with C4 at 0 V, the pitch the audio plane speaks.

use goofi_core::scale::{Recipe, C4_HZ, MAX_DEGREES, NOTES, OCTAVE, PRESET_NAMES};
use goofi_core::{Data, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};

#[derive(Default)]
struct Quantize;

impl Node for Quantize {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let a = d.as_array()?;
        let volts = p.str("quantize", "mode") == Some("v/oct");
        let strength = p.f64("quantize", "strength").unwrap_or(1.0).clamp(0.0, 1.0) as f32;
        let recipe = Recipe::preset(p.str("quantize", "scale").unwrap_or("major"));
        let root = NOTES.iter().position(|n| Some(*n) == p.str("quantize", "root")).unwrap_or(0) as f64 * 100.0;
        let mut all = [0.0; MAX_DEGREES];
        let n = recipe.degrees(&mut all);
        let degrees = &all[..n];

        // A value as cents above C4, and back; a frequency that is not positive is no pitch and
        // passes through, landing nowhere.
        let cents_of = |v: f32| -> Option<f64> {
            if !v.is_finite() {
                return None;
            }
            if volts {
                Some(OCTAVE * f64::from(v))
            } else {
                (v > 0.0).then(|| OCTAVE * (f64::from(v) / C4_HZ).log2())
            }
        };
        let value_of = |cents: f64| -> f32 {
            if volts { (cents / OCTAVE) as f32 } else { (C4_HZ * (cents / OCTAVE).exp2()) as f32 }
        };

        let mut values = Vec::with_capacity(a.as_bytes().len());
        let mut index = Vec::with_capacity(a.as_bytes().len());
        for x in a.as_bytes().chunks_exact(4) {
            let v = f32::from_le_bytes(x.try_into().expect("four bytes"));
            let (y, i) = match cents_of(v) {
                Some(cents) => {
                    let (snapped, degree) = recipe.snap(degrees, cents - root);
                    let to = value_of(snapped + root);
                    (v + (to - v) * strength, degree as f32)
                }
                None => (v, f32::NAN),
            };
            values.extend_from_slice(&y.to_le_bytes());
            index.extend_from_slice(&i.to_le_bytes());
        }
        let shape = a.shape().to_vec();
        out.set("out", Data::array_f32(shape.clone(), values, d.meta().clone()).map_err(|e| e.to_string())?);
        out.set("index", Data::array_f32(shape, index, d.meta().clone()).map_err(|e| e.to_string())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "quantize",
        name: "scale",
        spec: ParamSpec::Str { default: "major", options: &PRESET_NAMES, refresh: false },
        expression: None,
        doc: Some("The scale whose notes a pitch may land on."),
        section: 0,
        show: None,
        role: None,
    },
    ParamDecl {
        group: "quantize",
        name: "root",
        spec: ParamSpec::Str { default: "C", options: NOTES, refresh: false },
        expression: None,
        doc: Some("The note the scale starts on."),
        section: 0,
        show: None,
        role: None,
    },
    ParamDecl {
        group: "quantize",
        name: "mode",
        spec: ParamSpec::Str { default: "hz", options: &["hz", "v/oct"], refresh: false },
        expression: None,
        doc: Some("What a value is: a frequency in Hz, or volts per octave with C4 at 0 V."),
        section: 0,
        show: None,
        role: None,
    },
    ParamDecl {
        group: "quantize",
        name: "strength",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("How far towards the note each pitch is pulled. Zero passes the signal through."),
        section: 0,
        show: None,
        role: None,
    },
];
static INPUTS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Array, trigger_process: true, multi: false, required: true }];
static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "out", kind: SlotType::Array },
    OutputDecl { name: "index", kind: SlotType::Array },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "Pull every pitch onto the nearest note of a scale.\n\
          A value is a frequency in Hz, or volts per octave with C4 at 0 V. `index` says which \
          degree of the scale each one landed on. Shape and metadata are the input's.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Quantize, MANIFEST);
