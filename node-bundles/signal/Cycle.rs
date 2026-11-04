//! Cycle — the mean shape of the last `count` cycles of a periodic signal along one axis.
//! Cycles are cut on one grid from the first frame on, so the mean keeps its phase from run to run.

use goofi_core::{resolve_axis, stream, Axis, Data, SlotType};
use goofi_signal_sdk::{Face, Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, Show,
    SlotDecl, Tag,
};

const G: &str = "cycle";
const HZ: Show = Show { param: "unit", any_of: &["Hz"] };
const SAMPLES: Show = Show { param: "unit", any_of: &["samples"] };

#[derive(Default)]
struct Cycle {
    past: stream::Stream,
    /// Samples since the last cycle boundary.
    phase: usize,
    /// What the past was gathered under: the frame's shape with the axis at 0, the cycle length, the count.
    key: (Vec<usize>, usize, usize),
}

impl Node for Cycle {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let a = d.assert_ndims().at_least(1)?;
        let dim = resolve_axis(p.i64(G, "axis").unwrap_or(-1), a.shape().len())?;
        let n = match p.str(G, "unit").unwrap_or("Hz") {
            "Hz" => stream::window_count(1.0 / p.f64(G, "frequency").unwrap_or(10.0), "seconds", d.meta())?,
            _ => stream::window_count(p.f64(G, "length").unwrap_or(100.0), "samples", d.meta())?,
        };
        if n == 0 {
            return Err("a cycle must span at least one sample".into());
        }
        let count = p.i64(G, "count").unwrap_or(10).max(1) as usize;
        let steps = a.shape()[dim];
        if a.as_bytes().is_empty() {
            return Err(format!("needs a nonempty frame, got {:?}", a.shape()).into());
        }
        let history = n.checked_mul(count + 1)
            .filter(|h| h.checked_mul(a.as_bytes().len() / steps).is_some_and(|b| b <= isize::MAX as usize))
            .ok_or("frequency and count ask for more history than can be held")?;

        let mut layout = a.shape().to_vec();
        layout[dim] = 0;
        let key = (layout, n, count);
        if key != self.key {
            self.past.reset();
            self.phase = 0;
            self.key = key;
        }
        let (shape, held, _) = self.past.push(a.shape(), dim, a.as_bytes(), history);
        self.phase = (self.phase + steps) % n;
        let Some(start) = (shape[dim] - self.phase).checked_sub(n * count) else {
            return Ok(());
        };

        let mean: Vec<Vec<f32>> = stream::lanes(&shape, dim, &held)
            .iter()
            .map(|lane| {
                (0..n)
                    .map(|j| ((0..count).map(|k| lane[start + k * n + j] as f64).sum::<f64>() / count as f64) as f32)
                    .collect()
            })
            .collect();
        let mut shape_out = a.shape().to_vec();
        shape_out[dim] = n;
        // The axis' labels name the samples of one frame, not the positions in a cycle.
        let mut meta = d.meta().clone();
        if meta.channels().get(dim).is_some_and(|x| !x.is_empty()) {
            let axes = meta.channels().clone().with(dim, Axis::default());
            meta.set_channels(axes);
        }
        let buf = stream::unlanes(&shape_out, dim, &mean);
        out.set("out", Data::array_f32(shape_out, buf, meta).map_err(|e| e.to_string())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: G,
        name: "unit",
        spec: ParamSpec::Str { default: "Hz", options: &["Hz", "samples"], refresh: false },
        expression: None,
        doc: Some(
            "How the cycle is given: `Hz` as the signal's frequency, which uses sfreq, or ufreq if no \
             sample rate is set; `samples` as its length.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "frequency",
        spec: ParamSpec::Num { default: &[10.0], min: 0.01, max: 1000.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("The frequency of the cycle, in Hz. One cycle is the rate divided by it, rounded to whole samples."),
        section: 0,
        show: &[HZ],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "length",
        spec: ParamSpec::Num { default: &[100.0], min: 1.0, max: 100_000.0, int: true, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("The length of one cycle, in samples."),
        section: 0,
        show: &[SAMPLES],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "count",
        spec: ParamSpec::Num { default: &[10.0], min: 1.0, max: 1000.0, int: true, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("How many of the latest complete cycles to average. Nothing comes out until that many have arrived."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "axis",
        spec: ParamSpec::Num { default: &[-1.0], min: -8.0, max: 7.0, int: true, options: &[-2, -1, 0, 1, 2], face: Face::Plain },
        expression: None,
        doc: Some("Which axis to cut into cycles, negative from the end. -1 is time."),
        section: 1,
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
    tags: &[Tag::Analysis],
    doc: "The average shape of one cycle of a periodic signal.\n\
          Cuts the stream along `axis` into cycles of one length and averages the latest `count` of them.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Cycle, MANIFEST);
