//! Normalize — put a signal on a common scale from statistics over a set of axes: of a buffer
//! the node keeps across ticks, of the frame itself, or of a buffer the frame already is.

use goofi_core::normalize::{apply, scale_with, Scale};
use goofi_core::{resolve_axis, Data, SlotType};
use goofi_signal_sdk::{
    Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, Show,
    SlotDecl, Tag,
};

const BUFFERED: &str = "sample (buffered)";
const INSTANT: &str = "sample (instant)";
const BUFFER: &str = "buffer";

const ZSCORE: &[Show] = &[Show { param: "strategy", any_of: &["zscore"] }];
const ROBUST: &[Show] = &[Show { param: "strategy", any_of: &["robust"] }];
const RANGED: &[Show] = &[Show { param: "strategy", any_of: &["minmax", "robust"] }];
const KEPT: &[Show] = &[Show { param: "mode", any_of: &[BUFFERED] }];
const TIMED: &[Show] = &[Show { param: "mode", any_of: &[BUFFERED, BUFFER] }];
const BY_AXES: &[Show] = &[Show { param: "elementwise", any_of: &["false"] }];

#[derive(Default)]
struct Normalize {
    /// The frames kept for `sample (buffered)`, oldest first, all of one shape.
    frames: std::collections::VecDeque<Vec<f32>>,
    /// The statistics `hold` froze, kept so the freeze survives the next frame.
    held: Option<Vec<Scale>>,
}

fn values(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|x| f32::from_le_bytes(x.try_into().expect("four bytes"))).collect()
}

/// Which of `ndim` axes `spec` names, as comma-separated numbers; empty names `all` of them.
fn axes(spec: &str, ndim: usize, all: &[bool]) -> Result<Vec<bool>, String> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Ok(all.to_vec());
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

/// The scale of every group of `array`, by the strategy and its own params.
fn scales(array: &[f32], group: &[usize], count: usize, p: &Params<'_>) -> Vec<Scale> {
    let strategy = p.str("normalization", "strategy").unwrap_or("zscore");
    let quantile = p.f64("normalization", "quantile").unwrap_or(0.25);
    let mut members = vec![Vec::new(); count];
    for (x, g) in array.iter().zip(group) {
        members[*g].push(*x);
    }
    members.iter().map(|m| scale_with(strategy, m, quantile)).collect()
}

/// `x` on its group's scale, then onto the strategy's own range: `range` is where 0 and 1 land.
fn place(x: f32, scale: Scale, p: &Params<'_>) -> f32 {
    let z = apply(scale, x);
    match p.str("normalization", "strategy").unwrap_or("zscore") {
        "minmax" | "robust" => match p.vec("normalization", "range") {
            Some([low, high, ..]) => (*low + z as f64 * (high - low)) as f32,
            _ => z,
        },
        "zscore" => match p.f64("normalization", "clip").unwrap_or(0.0) as f32 {
            c if c > 0.0 => z.clamp(-c, c),
            _ => z,
        },
        _ => z,
    }
}

impl Node for Normalize {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let a = d.assert_ndims().at_least(1)?;
        let frame = values(a.as_bytes());
        let mode = p.str("window", "mode").unwrap_or(BUFFERED);
        let spec = p.str("window", "axes").unwrap_or("");

        // The array the statistics are taken over, and where the frame sits inside it.
        let (shape, array, at): (Vec<usize>, Vec<f32>, usize) = if mode == BUFFERED {
            let unit = if p.str("window", "unit").unwrap_or("seconds") == "seconds" { "seconds (ufreq)" } else { "samples" };
            let keep = goofi_core::stream::window_count(p.f64("window", "size").unwrap_or(10.0), unit, d.meta())?;
            if keep == 0 {
                return Err("the buffer needs a positive length".to_string().into());
            }
            if self.frames.front().is_some_and(|f| f.len() != frame.len()) {
                self.frames.clear();
            }
            self.frames.push_back(frame.clone());
            while self.frames.len() > keep {
                self.frames.pop_front();
            }
            let mut shape = vec![self.frames.len()];
            shape.extend_from_slice(a.shape());
            (shape, self.frames.iter().flatten().copied().collect(), (self.frames.len() - 1) * frame.len())
        } else {
            self.frames.clear();
            (a.shape().to_vec(), frame.clone(), 0)
        };

        // Time is axis 0 of the kept buffer and the last axis of a buffer frame. Elementwise
        // statistics run along time alone; otherwise empty `axes` is the whole array, except that
        // a buffer frame's statistics run along its time.
        let mut time = vec![false; shape.len()];
        time[if mode == BUFFERED { 0 } else { shape.len() - 1 }] = true;
        let elementwise = mode != INSTANT && p.bool("window", "elementwise").unwrap_or(true);
        let over = if elementwise {
            time
        } else {
            axes(spec, shape.len(), &if mode == BUFFER { time } else { vec![true; shape.len()] })?
        };
        let (group, count) = groups(&shape, &over);
        let scales = match &self.held {
            Some(held) if held.len() == count => held.clone(),
            _ => scales(&array, &group, count, p),
        };
        self.held = p.bool("window", "hold").unwrap_or(false).then(|| scales.clone());

        let buf: Vec<u8> = frame
            .iter()
            .enumerate()
            .flat_map(|(j, x)| place(*x, scales[group[at + j]], p).to_le_bytes())
            .collect();
        out.set("out", Data::array_f32(a.shape().to_vec(), buf, d.meta().clone()).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_pulse(&mut self, _key: &ParamKey, _p: &Params<'_>) -> NodeResult {
        self.frames.clear();
        self.held = None;
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "normalization",
        name: "strategy",
        spec: ParamSpec::Str { default: "zscore", options: &["zscore", "robust", "minmax"], refresh: false },
        expression: None,
        doc: Some(
            "`zscore` measures in standard deviations from the mean, `robust` in quantile spreads \
             from the median, which an outlier cannot move, and `minmax` in the distance between \
             the extremes.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "normalization",
        name: "clip",
        spec: ParamSpec::Num { default: &[0.0], min: 0.0, max: 100.0, int: false, options: &[], color: false },
        expression: None,
        doc: Some("Limit the result to this many standard deviations either side of zero. 0 is no limit."),
        section: 0,
        show: ZSCORE,
        role: None,
    },
    ParamDecl {
        group: "normalization",
        name: "quantile",
        spec: ParamSpec::Num { default: &[0.25], min: 0.0, max: 0.5, int: false, options: &[], color: false },
        expression: None,
        doc: Some(
            "The spread is the distance between this quantile and its mirror above the median: 0.25 \
             is the middle half, 0 the full range.",
        ),
        section: 0,
        show: ROBUST,
        role: None,
    },
    ParamDecl {
        group: "normalization",
        name: "range",
        spec: ParamSpec::Num { default: &[0.0, 1.0], min: -1000.0, max: 1000.0, int: false, options: &[], color: false },
        expression: None,
        doc: Some(
            "Where 0 and 1 of the measure land: for `minmax` the minimum and the maximum, for \
             `robust` the median and one spread above it.",
        ),
        section: 0,
        show: RANGED,
        role: None,
    },
    ParamDecl {
        group: "window",
        name: "mode",
        spec: ParamSpec::Str { default: BUFFERED, options: &[BUFFERED, INSTANT, BUFFER], refresh: false },
        expression: None,
        doc: Some(
            "Where the statistics come from. `sample (buffered)` keeps the last `size` of input and \
             measures each frame against that buffer, time as its axis 0. `sample (instant)` \
             measures each frame against itself. `buffer` takes a frame that is a buffer already \
             and measures it along its own time.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "window",
        name: "elementwise",
        spec: ParamSpec::Bool { default: true },
        expression: None,
        doc: Some("Give every element its own statistics, over time alone."),
        section: 0,
        show: TIMED,
        role: None,
    },
    ParamDecl {
        group: "window",
        name: "axes",
        spec: ParamSpec::Str { default: "", options: &[], refresh: false },
        expression: None,
        doc: Some(
            "The axes the statistics are taken over, as numbers separated by commas; every other \
             axis gets statistics of its own. Empty is the whole array, or in `buffer` mode its \
             last axis. `-1` is the last axis; `0,1` the first two.",
        ),
        section: 0,
        show: BY_AXES,
        role: None,
    },
    ParamDecl {
        group: "window",
        name: "size",
        spec: ParamSpec::Num { default: &[10.0], min: 0.0, max: 1.0e6, int: false, options: &[], color: false },
        expression: None,
        doc: Some("How much of the past the buffer keeps, in `unit`."),
        section: 0,
        show: KEPT,
        role: None,
    },
    ParamDecl {
        group: "window",
        name: "unit",
        spec: ParamSpec::Str { default: "seconds", options: &["seconds", "samples"], refresh: false },
        expression: None,
        doc: Some("What `size` counts: seconds at the input's update rate, or whole frames."),
        section: 0,
        show: KEPT,
        role: None,
    },
    ParamDecl {
        group: "window",
        name: "hold",
        spec: ParamSpec::Bool { default: false },
        expression: None,
        doc: Some("Freeze the statistics where they are, so later data is measured against them."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "window",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Forget the buffer and any frozen statistics."),
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
    doc: "Put a signal on a common scale from its past, its frame, or a buffer.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Normalize, MANIFEST);
