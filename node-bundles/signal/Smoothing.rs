//! Smoothing — across ticks, one value at a time (`sample`), or along one axis of each frame
//! (`buffer`), as an exponential or a simple moving average, or as a kernel convolution.

use goofi_core::{resolve_axis, stream, Data, SlotType};
use goofi_signal_sdk::{
    Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, Show,
    SlotDecl, Tag,
};

const G: &str = "smoothing";
const SAMPLE: Show = Show { param: "mode", any_of: &["sample"] };
const BUFFER: Show = Show { param: "mode", any_of: &["buffer"] };
const EXPONENTIAL: Show = Show { param: "method", any_of: &["exponential"] };
const SPAN: Show = Show { param: "method", any_of: &["simple", "convolution"] };
const AVERAGE: Show = Show { param: "method", any_of: &["exponential", "simple"] };
const CONVOLUTION: Show = Show { param: "method", any_of: &["convolution"] };
const GAUSSIAN: Show = Show { param: "kernel", any_of: &["gaussian"] };

#[derive(Default)]
struct Smoothing {
    /// The exponential average so far, one per element of the frame.
    level: Vec<f32>,
    /// The last `size` frames, for the simple average across ticks.
    frames: std::collections::VecDeque<Vec<f32>>,
}

fn values(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|x| f32::from_le_bytes(x.try_into().expect("four bytes"))).collect()
}

fn bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A one-pole average run forward over a lane, starting from its first sample.
fn exponential(lane: &[f32], alpha: f32) -> Vec<f32> {
    let mut y = lane.first().copied().unwrap_or(0.0);
    lane.iter()
        .map(|v| {
            y += alpha * (v - y);
            y
        })
        .collect()
}

/// The mean of the `size` values ending at each position, the head clamped to what exists.
fn simple(lane: &[f32], size: usize) -> Vec<f32> {
    let mut sum = 0.0f64;
    lane.iter()
        .enumerate()
        .map(|(i, v)| {
            sum += *v as f64;
            if i >= size {
                sum -= lane[i - size] as f64;
            }
            (sum / (i + 1).min(size) as f64) as f32
        })
        .collect()
}

/// `pass` forward and, when `both`, backward over its answer, so nothing comes out shifted in time.
fn average(lane: &[f32], both: bool, pass: impl Fn(&[f32]) -> Vec<f32>) -> Vec<f32> {
    let ahead = pass(lane);
    if !both {
        return ahead;
    }
    let mut back: Vec<f32> = ahead.into_iter().rev().collect();
    back = pass(&back);
    back.reverse();
    back
}

/// A `width`-tap kernel of the named shape, centred and normalized to sum to one.
fn kernel(shape: &str, width: usize, sigma: f32) -> Result<Vec<f32>, String> {
    let c = (width as f32 - 1.0) / 2.0;
    let taps: Vec<f32> = (0..width)
        .map(|i| {
            let d = i as f32 - c;
            match shape {
                "box" => 1.0,
                "triangle" => 1.0 - d.abs() / (c + 1.0),
                "gaussian" => (-d * d / (2.0 * sigma * sigma)).exp(),
                "hann" => 0.5 * (1.0 - (std::f32::consts::TAU * (i as f32 + 1.0) / (width as f32 + 1.0)).cos()),
                _ => f32::NAN,
            }
        })
        .collect();
    let sum: f32 = taps.iter().sum();
    if !sum.is_finite() || sum <= 0.0 {
        return Err(format!("unknown kernel: {shape}"));
    }
    Ok(taps.iter().map(|t| t / sum).collect())
}

/// The lane's value at `i`, which may lie past either end, continued as `edge` says.
fn at(lane: &[f32], i: isize, edge: &str) -> Result<f32, String> {
    let n = lane.len() as isize;
    if (0..n).contains(&i) {
        return Ok(lane[i as usize]);
    }
    Ok(match edge {
        "zero" => 0.0,
        "hold" => lane[i.clamp(0, n - 1) as usize],
        "reflect" => {
            // d c b a | a b c d | d c b a — the edge sample repeats, as a mirror on the boundary.
            let period = 2 * n;
            let m = i.rem_euclid(period);
            lane[if m < n { m } else { period - 1 - m } as usize]
        }
        _ => return Err(format!("unknown edge mode: {edge}")),
    })
}

/// The centred convolution of `lane` with `taps`, the same length as `lane`.
fn convolve(lane: &[f32], taps: &[f32], edge: &str) -> Result<Vec<f32>, String> {
    let half = (taps.len() / 2) as isize;
    (0..lane.len() as isize)
        .map(|i| {
            taps.iter().enumerate().try_fold(0.0f32, |acc, (k, t)| {
                Ok(acc + t * at(lane, i + k as isize - half, edge)?)
            })
        })
        .collect()
}

impl Smoothing {
    fn across_ticks(&mut self, x: Vec<f32>, p: &Params<'_>) -> Result<Vec<f32>, String> {
        let stale = |held: usize| held != 0 && held != x.len();
        if stale(self.level.len()) || stale(self.frames.front().map_or(0, Vec::len)) {
            self.reset();
        }
        match p.str(G, "method").unwrap_or("exponential") {
            "simple" => {
                let size = p.i64(G, "size").unwrap_or(10).max(1) as usize;
                self.frames.push_back(x);
                while self.frames.len() > size {
                    self.frames.pop_front();
                }
                let n = self.frames.len() as f32;
                Ok((0..self.frames[0].len()).map(|i| self.frames.iter().map(|f| f[i]).sum::<f32>() / n).collect())
            }
            "exponential" => {
                let alpha = p.f64(G, "alpha").unwrap_or(0.1).clamp(0.0, 1.0) as f32;
                if self.level.is_empty() {
                    self.level = x.clone();
                }
                for (y, v) in self.level.iter_mut().zip(&x) {
                    *y += alpha * (v - *y);
                }
                Ok(self.level.clone())
            }
            other => Err(format!("`{other}` smooths along an axis: it needs `buffer` mode")),
        }
    }

    fn reset(&mut self) {
        self.level.clear();
        self.frames.clear();
    }
}

fn along_axis(lane: &[f32], p: &Params<'_>) -> Result<Vec<f32>, String> {
    let both = p.bool(G, "bidirectional").unwrap_or(false);
    let size = p.i64(G, "size").unwrap_or(5).max(1) as usize;
    match p.str(G, "method").unwrap_or("exponential") {
        "exponential" => {
            let alpha = p.f64(G, "alpha").unwrap_or(0.1).clamp(0.0, 1.0) as f32;
            Ok(average(lane, both, |l| exponential(l, alpha)))
        }
        "simple" => Ok(average(lane, both, |l| simple(l, size))),
        "convolution" => {
            let sigma = p.f64(G, "sigma").unwrap_or(1.0).max(1e-3) as f32;
            let taps = kernel(p.str(G, "kernel").unwrap_or("gaussian"), size, sigma)?;
            convolve(lane, &taps, p.str(G, "edge").unwrap_or("reflect"))
        }
        other => Err(format!("unknown method: {other}")),
    }
}

impl Node for Smoothing {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let a = d.assert_ndims().at_least(1)?;
        let buf = if p.str(G, "mode").unwrap_or("sample") == "buffer" {
            self.reset();
            let dim = resolve_axis(p.i64(G, "axis").unwrap_or(-1), a.shape().len())?;
            let lanes = stream::lanes(a.shape(), dim, a.as_bytes())
                .iter()
                .map(|lane| along_axis(lane, p))
                .collect::<Result<Vec<_>, _>>()?;
            stream::unlanes(a.shape(), dim, &lanes)
        } else {
            bytes(&self.across_ticks(values(a.as_bytes()), p)?)
        };
        out.set("out", Data::array_f32(a.shape().to_vec(), buf, d.meta().clone()).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_pulse(&mut self, _key: &ParamKey, _p: &Params<'_>) -> NodeResult {
        self.reset();
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: G,
        name: "mode",
        spec: ParamSpec::Str { default: "sample", options: &["sample", "buffer"], refresh: false },
        expression: None,
        doc: Some(
            "`sample` smooths each position of the frame across ticks, whatever the frame's shape. \
             `buffer` smooths along one axis inside each frame, and keeps nothing between frames.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "method",
        spec: ParamSpec::Str {
            default: "exponential",
            options: &["exponential", "simple", "convolution"],
            refresh: false,
        },
        expression: None,
        doc: Some(
            "`exponential` lets an old value fade away, by `alpha` a step. `simple` is the mean of \
             the last `size` steps. `convolution`, in `buffer` mode only, weighs the `size` samples \
             around each one by a `kernel`, centred, so it shifts nothing in time.",
        ),
        section: 1,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "axis",
        spec: ParamSpec::Num { default: &[-1.0], min: -8.0, max: 7.0, int: true, options: &[-2, -1, 0, 1, 2], color: false },
        expression: None,
        doc: Some("Which axis to smooth along. -1 is time."),
        section: 1,
        show: &[BUFFER],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "alpha",
        spec: ParamSpec::Num { default: &[0.1], min: 0.0, max: 1.0, int: false, options: &[], color: false },
        expression: None,
        doc: Some(
            "How far each new value pulls the average towards itself: 1 follows the input exactly, \
             and a small value changes slowly.",
        ),
        section: 1,
        show: &[EXPONENTIAL],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "size",
        spec: ParamSpec::Num { default: &[5.0], min: 1.0, max: 1001.0, int: true, options: &[], color: false },
        expression: None,
        doc: Some(
            "How many steps the mean, or the kernel, spans: frames across ticks, samples along an \
             axis. An odd kernel sits on its sample.",
        ),
        section: 1,
        show: &[SPAN],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "bidirectional",
        spec: ParamSpec::Bool { default: false },
        expression: None,
        doc: Some(
            "Run the average forward and then backward over its own answer, so nothing comes out \
             shifted in time. Off, it lags like a live stream would.",
        ),
        section: 1,
        show: &[BUFFER, AVERAGE],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "kernel",
        spec: ParamSpec::Str { default: "gaussian", options: &["box", "triangle", "gaussian", "hann"], refresh: false },
        expression: None,
        doc: Some("The shape of the weights: flat, a tent, a bell of width `sigma`, or a raised cosine."),
        section: 1,
        show: &[CONVOLUTION],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "sigma",
        spec: ParamSpec::Num { default: &[1.0], min: 0.1, max: 100.0, int: false, options: &[], color: false },
        expression: None,
        doc: Some("The width of the gaussian bell, in samples."),
        section: 1,
        show: &[CONVOLUTION, GAUSSIAN],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "edge",
        spec: ParamSpec::Str { default: "reflect", options: &["reflect", "hold", "zero"], refresh: false },
        expression: None,
        doc: Some(
            "What the kernel reads past either end: the lane mirrored back on itself, its end \
             sample held, or zeros.",
        ),
        section: 1,
        show: &[CONVOLUTION],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Forget the past, so the node starts again from the next frame."),
        section: 1,
        show: &[SAMPLE],
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
    doc: "Take the edge off a signal across ticks or along an axis.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Smoothing, MANIFEST);
