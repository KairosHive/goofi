//! Morph — one frame picked along the list of wired frames: a straight mix of the two nearest,
//! or a closed Catmull-Rom curve through all of them. `position` counts in frames and loops.

use goofi_core::{Data, SlotType};
use goofi_signal_sdk::{Face, Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};

/// A frame named by its sender's stamps: the run index and when that run ended.
type Stamp = (Option<u64>, Option<f64>);

/// What the last mix was made of: each frame by its stamp, and the point along them.
#[derive(Default)]
struct Morph {
    made_of: Option<(Vec<Stamp>, f64, String)>,
}

fn floats(bytes: &[u8]) -> impl Iterator<Item = f32> + '_ {
    bytes.chunks_exact(4).map(|x| f32::from_le_bytes(x.try_into().expect("four bytes")))
}

/// The weights of the frame before, the two around and the one after a point `s` into a gap.
fn weights(curve: &str, s: f32) -> [f32; 4] {
    if curve == "spline" {
        let (s2, s3) = (s * s, s * s * s);
        [
            0.5 * (-s + 2.0 * s2 - s3),
            0.5 * (2.0 - 5.0 * s2 + 3.0 * s3),
            0.5 * (s + 4.0 * s2 - 3.0 * s3),
            0.5 * (-s2 + s3),
        ]
    } else {
        [0.0, 1.0 - s, s, 0.0]
    }
}

impl Node for Morph {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        // Not required: the node runs on its own clock, and before the first wire there is nothing to say.
        let wires = inp.get_multi("input");
        if wires.is_empty() {
            return Ok(());
        }
        let first = wires[0].1.as_array()?;
        let shape = first.shape().to_vec();
        for (source, d) in wires {
            let a = d.as_array()?;
            if a.shape() != shape.as_slice() {
                return Err(format!("`{source}` is {:?} where the first wire is {shape:?}", a.shape()).into());
            }
        }
        let position = p.f64("morph", "position").unwrap_or(0.0);
        if !position.is_finite() {
            return Err(format!("`position` is not a number: {position}").into());
        }
        let curve = p.str("morph", "curve").unwrap_or("linear");
        // The node runs on its own clock; the same frames at the same point are the mix already sent.
        let frames: Vec<Stamp> = wires.iter().map(|(_, d)| (d.meta().index(), d.meta().procend())).collect();
        if self.made_of.as_ref().is_some_and(|(held, at, how)| *at == position && how == curve && *held == frames) {
            return Ok(());
        }
        let n = wires.len();
        let x = position.rem_euclid(n as f64);
        let k = x.floor() as i64;
        let weights = weights(curve, (x - k as f64) as f32);

        let mut mix = vec![0f32; shape.iter().product()];
        for (step, weight) in (-1i64..=2).zip(weights) {
            if weight == 0.0 {
                continue;
            }
            let a = wires[(k + step).rem_euclid(n as i64) as usize].1.as_array()?;
            for (m, v) in mix.iter_mut().zip(floats(a.as_bytes())) {
                *m += weight * v;
            }
        }
        let bytes = mix.iter().flat_map(|v| v.to_le_bytes()).collect();
        out.set("out", Data::array_f32(shape, bytes, wires[0].1.meta().clone()).map_err(|e| e.to_string())?);
        self.made_of = Some((frames, position, curve.to_owned()));
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "morph",
        name: "position",
        spec: ParamSpec::Num { default: &[0.0], min: 0.0, max: 64.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Where along the wired frames to be, in frames: 1.5 is halfway from the second to the third, and the list loops."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "morph",
        name: "curve",
        spec: ParamSpec::Str { default: "linear", options: &["linear", "spline"], refresh: false },
        expression: None,
        doc: Some("`linear` mixes the two nearest frames; `spline` runs one smooth Catmull-Rom loop through all of them."),
        section: 0,
        show: &[],
        role: None,
    },
    // On by default: the frames are held, and it is `position` that moves between them.
    ParamDecl {
        group: "common",
        name: "autotrigger",
        spec: ParamSpec::Bool { default: true },
        expression: None,
        doc: Some("Run on the node's own clock, so a moving `position` morphs between the held frames."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "common",
        name: "max_frequency",
        spec: ParamSpec::Num { default: &[30.0], min: 0.1, max: 240.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Rate cap: frames a second at most."),
        section: 0,
        show: &[],
        role: None,
    },
];
static INPUTS: &[SlotDecl] = &[SlotDecl {
    name: "input",
    kind: SlotType::Array,
    trigger_process: true,
    multi: true,
    required: false,
}];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "One frame picked along the wired frames by `position`.\n\
          A straight mix of the two nearest, or a Catmull-Rom loop through all of them.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Morph, MANIFEST);
