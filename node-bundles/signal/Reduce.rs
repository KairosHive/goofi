//! Reduce — a set of axes becomes one number per position on the others, so those axes and their
//! labels go.

use goofi_core::{resolve_axis, Data, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};

#[derive(Default)]
struct Reduce;

/// `v` is one axis' worth of values, gathered in order.
fn reduce(mode: &str, v: &mut [f32]) -> f32 {
    let n = v.len() as f32;
    let sum: f32 = v.iter().sum();
    match mode {
        "sum" => sum,
        "min" => v.iter().copied().fold(f32::INFINITY, f32::min),
        "max" => v.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        "argmin" => position(v, |x, best| x < best),
        "argmax" => position(v, |x, best| x > best),
        "norm" => v.iter().map(|x| x * x).sum::<f32>().sqrt(),
        "std" => {
            let mean = sum / n;
            (v.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / n).sqrt()
        }
        "median" => {
            v.sort_by(f32::total_cmp);
            let mid = v.len() / 2;
            if v.len() % 2 == 0 {
                (v[mid - 1] + v[mid]) / 2.0
            } else {
                v[mid]
            }
        }
        _ => sum / n,
    }
}

/// The flat index of the first value no other `beats`, NaN skipped as `min` and `max` skip it;
/// NaN when every value is NaN.
fn position(v: &[f32], beats: fn(f32, f32) -> bool) -> f32 {
    let mut best: Option<usize> = None;
    for (i, &x) in v.iter().enumerate() {
        if !x.is_nan() && best.is_none_or(|b| beats(x, v[b])) {
            best = Some(i);
        }
    }
    best.map_or(f32::NAN, |b| b as f32)
}

/// Which of `ndim` axes `spec` names, as comma-separated numbers; empty names every axis.
fn axes(spec: &str, ndim: usize) -> Result<Vec<bool>, String> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Ok(vec![true; ndim]);
    }
    let mut over = vec![false; ndim];
    for part in parts {
        let axis: i64 = part.parse().map_err(|_| format!("`axes` takes axis numbers, not `{part}`"))?;
        over[resolve_axis(axis, ndim)?] = true;
    }
    Ok(over)
}

/// Each element's group — its position on the axes NOT in `over`, flattened — and the group count.
fn groups(shape: &[usize], over: &[bool]) -> (Vec<usize>, usize) {
    let len: usize = shape.iter().product();
    let strides: Vec<usize> = (0..shape.len()).map(|d| shape[d + 1..].iter().product()).collect();
    let count = (0..shape.len()).filter(|&d| !over[d]).map(|d| shape[d]).product::<usize>().max(1);
    let of = |i: usize| {
        (0..shape.len()).filter(|&d| !over[d]).fold(0, |key, d| key * shape[d] + (i / strides[d]) % shape[d])
    };
    ((0..len).map(of).collect(), count)
}

impl Node for Reduce {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let a = d.assert_ndims().at_least(1)?;
        let shape = a.shape();
        let over = axes(p.str("reduce", "axes").unwrap_or("-1"), shape.len())?;
        let mode = p.str("reduce", "mode").unwrap_or("mean");
        if shape.iter().zip(&over).any(|(n, o)| *o && *n == 0) {
            return Err("cannot reduce an axis of no length".to_string().into());
        }

        let (group, count) = groups(shape, &over);
        let mut members = vec![Vec::new(); count];
        for (bytes, g) in a.as_bytes().chunks_exact(4).zip(&group) {
            members[*g].push(f32::from_le_bytes(bytes.try_into().expect("four bytes")));
        }
        let mut buf = Vec::with_capacity(count * 4);
        for lane in &mut members {
            buf.extend_from_slice(&reduce(mode, lane).to_le_bytes());
        }

        // Axes go from the last down, so each one's number still holds when it is dropped.
        let mut shape_out = shape.to_vec();
        let mut meta = d.meta().clone();
        for dim in (0..shape.len()).rev().filter(|&d| over[d]) {
            meta = meta.drop_axis(dim, shape_out.len());
            shape_out.remove(dim);
        }
        out.set("out", Data::array_f32(shape_out, buf, meta).map_err(|e| e.to_string())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "reduce",
        name: "mode",
        spec: ParamSpec::Str {
            default: "mean",
            options: &["mean", "median", "min", "max", "argmin", "argmax", "std", "sum", "norm"],
            refresh: false,
        },
        expression: None,
        doc: Some(
            "How the values along the axis become one: an average, a spread, an extreme, where an \
             extreme lies or a total. `argmin` and `argmax` count over the collapsed axes flattened \
             in order, the first one on a tie.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "reduce",
        name: "axes",
        spec: ParamSpec::Str { default: "-1", options: &[], refresh: false },
        expression: None,
        doc: Some(
            "The axes to collapse, as numbers separated by commas, negative from the end. `-1` is \
             time, `-2` channels, `0,1` the first two. Empty collapses the whole array to one value.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
];
static INPUTS: &[SlotDecl] = &[SlotDecl {
    name: "input",
    kind: SlotType::Array,
    trigger_process: true,
    multi: false,
    required: true,
}];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "Collapse a set of axes to a single value.\n\
          One value per position on the other axes: a mean, a spread, an extreme, where an extreme lies or a total.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Reduce, MANIFEST);
