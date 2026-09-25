//! Quantize — every value pulled onto the nearest of an allowed set, and `index` says which.
//! `mode` names the set, and each set has a tab of its own: `scale`, the notes of a musical
//! scale with every value a pitch in Hz; `count`, values spread evenly between two ends; or
//! `levels`, a set wired in — a tuning's own ratios — repeated every `repeat` times over, so a
//! scale that spans one octave quantizes a pitch four octaves up.
//!
//! NaN in `levels` is padding, not a level: a `Tuning` output feeds straight in.

use goofi_core::scale::{Recipe, C4_HZ, MASK_BITS, MAX_DEGREES, METHODS, NOTES, OCTAVE, SCALES};
use goofi_core::{Data, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};

/// Where `v` sits in an ascending set: the index of the nearest entry.
fn nearest(set: &[f32], v: f32) -> usize {
    let i = set.partition_point(|s| *s < v);
    if i == 0 {
        0
    } else if i >= set.len() {
        set.len() - 1
    } else if v - set[i - 1] <= set[i] - v {
        i - 1
    } else {
        i
    }
}

/// The scale the `scale` tab describes: a preset, or the custom recipe below it.
fn recipe(p: &Params<'_>) -> Recipe {
    let chosen = |name: &str, options: &[&str]| {
        p.str("scale", name).and_then(|s| options.iter().position(|o| *o == s)).unwrap_or(0)
    };
    let int = |name: &str, default: i64| p.i64("scale", name).unwrap_or(default) as f64;
    let custom = Recipe::from_scalars(
        chosen("method", METHODS) as f64,
        p.f64("scale", "generator").unwrap_or(700.0),
        int("steps", 7),
        int("mask", 0),
        int("mode", 4),
    );
    Recipe::chosen(chosen("scale", &SCALES), custom)
}

#[derive(Default)]
struct Quantize {
    /// The allowed values of `count` and `levels`, ascending, rebuilt each frame.
    set: Vec<f32>,
}

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
        let strength = p.f64("quantize", "strength").unwrap_or(1.0).clamp(0.0, 1.0) as f32;
        let mode = p.str("quantize", "mode").unwrap_or("scale");

        let (mut degrees, mut scale) = ([0.0; MAX_DEGREES], None);
        let mut repeat = None;
        self.set.clear();
        match mode {
            "count" => {
                let count = p.i64("count", "values").unwrap_or(12).max(2) as usize;
                let low = p.f64("count", "low").unwrap_or(0.0) as f32;
                let high = p.f64("count", "high").unwrap_or(1.0) as f32;
                let step = (high - low) / (count - 1) as f32;
                self.set.extend((0..count).map(|i| low + step * i as f32));
            }
            "levels" => {
                let wire = inp.get("levels").ok_or("`levels` is required when `mode` is `levels`")?;
                self.set.extend(
                    wire.as_array()?
                        .as_bytes()
                        .chunks_exact(4)
                        .map(|x| f32::from_le_bytes(x.try_into().expect("four bytes")))
                        .filter(|v| v.is_finite()),
                );
                self.set.sort_by(|a, b| a.total_cmp(b));
                self.set.dedup();
                repeat = p.f64("levels", "repeat").map(|r| r as f32).filter(|r| *r > 1.0);
            }
            _ => {
                let r = recipe(p);
                let root = NOTES.iter().position(|n| Some(*n) == p.str("scale", "root")).unwrap_or(0) as f64 * 100.0;
                scale = Some((r, r.degrees(&mut degrees), root));
            }
        }

        // What `v` is pulled to and which entry that is; `None` passes it and lands nowhere, which
        // is what a tuning whose window held no peaks — no set at all — answers for every value.
        let set = &self.set;
        let pull = |v: f32| -> Option<(f32, usize)> {
            if !v.is_finite() {
                return None;
            }
            if let Some((r, n, root)) = scale {
                // A pitch is a positive frequency.
                if v <= 0.0 {
                    return None;
                }
                let (cents, degree) = r.snap(&degrees[..n], OCTAVE * (f64::from(v) / C4_HZ).log2() - root);
                return Some(((C4_HZ * ((cents + root) / OCTAVE).exp2()) as f32, degree));
            }
            let base = *set.first()?;
            match repeat.filter(|_| base > 0.0 && v > 0.0) {
                // Carry the register aside, match inside one repeat, then put the register back.
                Some(r) => {
                    let (mut inside, mut n) = (v, 0i32);
                    while inside >= base * r {
                        inside /= r;
                        n += 1;
                    }
                    while inside < base {
                        inside *= r;
                        n -= 1;
                    }
                    let i = nearest(set, inside);
                    Some((set[i] * r.powi(n), i))
                }
                None => {
                    let i = nearest(set, v);
                    Some((set[i], i))
                }
            }
        };

        let mut values = Vec::with_capacity(a.as_bytes().len());
        let mut index = Vec::with_capacity(a.as_bytes().len());
        for x in a.as_bytes().chunks_exact(4) {
            let v = f32::from_le_bytes(x.try_into().expect("four bytes"));
            let (y, i) = pull(v).map_or((v, f32::NAN), |(to, i)| (v + (to - v) * strength, i as f32));
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
        name: "mode",
        spec: ParamSpec::Str { default: "scale", options: &["scale", "count", "levels"], refresh: false },
        expression: None,
        doc: Some("The set values land on: a musical scale over pitches in Hz, values counted out, or a set wired into `levels`."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "quantize",
        name: "strength",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("How far towards the allowed value each one is pulled. Zero passes the signal through."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "scale",
        name: "scale",
        spec: ParamSpec::Str { default: "major", options: &SCALES, refresh: false },
        expression: None,
        doc: Some("A named scale, or `custom` to build one from the params below."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "scale",
        name: "root",
        spec: ParamSpec::Str { default: "C", options: NOTES, refresh: false },
        expression: None,
        doc: Some("The note the scale starts on."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "scale",
        name: "method",
        spec: ParamSpec::Str { default: "generator", options: METHODS, refresh: false },
        expression: None,
        doc: Some("How `custom` builds its notes: stacking one interval, reading the harmonic series, or picking from equal steps."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "scale",
        name: "steps",
        spec: ParamSpec::Int { default: 7, min: 1, max: 53, options: &[] },
        expression: None,
        doc: Some("Notes in the octave: intervals stacked, harmonics read (partials steps to 2 steps - 1), or equal steps."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "scale",
        name: "generator",
        spec: ParamSpec::Float { default: 700.0, min: 0.0, max: 1200.0 },
        expression: None,
        doc: Some("The interval stacked, in cents: 700 is a tempered fifth, 701.955 a pure one."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "scale",
        name: "mask",
        spec: ParamSpec::Int { default: 0, min: 0, max: (1 << MASK_BITS) - 1, options: &[] },
        expression: None,
        doc: Some("Which of the first 24 equal steps are notes, bit k for step k; zero keeps every step."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "scale",
        name: "mode",
        spec: ParamSpec::Int { default: 4, min: 0, max: 52, options: &[] },
        expression: None,
        doc: Some("The note the scale is read from: seven fifths read from 4 are major, from 0 lydian, from 2 minor."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "count",
        name: "values",
        spec: ParamSpec::Int { default: 12, min: 2, max: 4096, options: &[] },
        expression: None,
        doc: Some("How many values are allowed, spread evenly from `low` to `high` inclusive."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "count",
        name: "low",
        spec: ParamSpec::Float { default: 0.0, min: -1.0e9, max: 1.0e9 },
        expression: None,
        doc: Some("The first of the counted values."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "count",
        name: "high",
        spec: ParamSpec::Float { default: 1.0, min: -1.0e9, max: 1.0e9 },
        expression: None,
        doc: Some("The last of them."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "levels",
        name: "repeat",
        spec: ParamSpec::Float { default: 2.0, min: 0.0, max: 16.0 },
        expression: None,
        doc: Some("The ratio the wired set repeats at, so a few numbers cover every register; 2 is the octave, 1 or less repeats nothing."),
        section: 0,
        show: None,
    },
];
static INPUTS: &[SlotDecl] = &[
    SlotDecl { name: "input", kind: SlotType::Array, trigger_process: true, multi: false, required: true },
    SlotDecl { name: "levels", kind: SlotType::Array, trigger_process: false, multi: false, required: false },
];
static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "out", kind: SlotType::Array },
    OutputDecl { name: "index", kind: SlotType::Array },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "Pull every value onto the nearest of an allowed set.\n\
          By default a pitch in Hz lands on the nearest note of a scale: a named one, or any scale \
          built by stacking an interval, reading the harmonic series, or picking from equal steps. \
          The set can instead be counted out, or wired in as a tuning's own ratios. `index` says \
          which one each value landed on. Shape and metadata are the input's.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Quantize, MANIFEST);
