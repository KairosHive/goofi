//! Scale a signal from windowed or running statistics.

use goofi_core::normalize::{apply, scale_of, validate, Running, Scale};
use goofi_core::{resolve_axis, stream, Data, SlotType, Stream};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, SlotDecl, Tag};

#[derive(Default)]
struct Normalize {
    past: Stream,
    running: Vec<Running>,
    /// The statistics `hold` froze, kept so the freeze survives the next frame.
    held: Option<Vec<Scale>>,
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
        let mode = p.str("normalize", "mode").unwrap_or("zscore");
        let dim = resolve_axis(p.i64("window", "axis").unwrap_or(-1), a.shape().len())?;
        let size = p.f64("window", "size").unwrap_or(1000.0);
        let unit = p.str("window", "unit").unwrap_or("samples");
        let hold = p.bool("window", "hold").unwrap_or(false);
        let n = a.shape()[dim];

        let width = goofi_core::stream::window_count(size, unit, d.meta())?;

        validate(mode, width)?;

        let (shape, stitched, at) = if width == 0 {
            (a.shape().to_vec(), a.as_bytes().to_vec(), 0usize)
        } else {
            self.past.push(a.shape(), dim, a.as_bytes(), width)
        };
        let lanes = stream::lanes(&shape, dim, &stitched);

        let scales: Vec<Scale> = match &self.held {
            Some(held) if held.len() == lanes.len() => held.clone(),
            _ => {
                if width == 0 {
                    if self.running.len() != lanes.len() {
                        self.running = vec![Running::default(); lanes.len()];
                    }
                    for (lane, run) in lanes.iter().zip(&mut self.running) {
                        for x in lane {
                            run.push(*x as f64);
                        }
                    }
                    self.running.iter().map(|r| r.scale(mode)).collect::<Result<_, _>>()?
                } else {
                    lanes
                        .iter()
                        .map(|lane| scale_of(mode, &lane[lane.len().saturating_sub(width)..]))
                        .collect()
                }
            }
        };
        self.held = hold.then(|| scales.clone());

        let scaled: Vec<Vec<f32>> = lanes
            .iter()
            .zip(&scales)
            .map(|(lane, scale)| lane[at..at + n].iter().map(|x| apply(*scale, *x)).collect())
            .collect();
        let buf = stream::unlanes(a.shape(), dim, &scaled);
        out.set("out", Data::array_f32(a.shape().to_vec(), buf, d.meta().clone()).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_pulse(&mut self, _key: &ParamKey, _p: &Params<'_>) -> NodeResult {
        self.past.reset();
        self.running.clear();
        self.held = None;
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "normalize",
        name: "mode",
        spec: ParamSpec::Str { default: "zscore", options: &["zscore", "minmax", "robust"], refresh: false },
        expression: None,
        doc: Some(
            "`zscore` measures in standard deviations from the mean, `minmax` maps the window onto \
             0 to 1, and `robust` uses the median and the middle half, which an outlier cannot move.",
        ),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "window",
        name: "size",
        spec: ParamSpec::Float { default: 1000.0, min: 0.0, max: 1.0e7 },
        expression: None,
        doc: Some(
            "How much of the past the statistics are taken over. 0 keeps running statistics over \
             everything the node has seen. Robust mode needs a positive size.",
        ),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "window",
        name: "unit",
        spec: ParamSpec::Str { default: "samples", options: &["samples", "seconds", "seconds (ufreq)"], refresh: false },
        expression: None,
        doc: Some("What `size` counts."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "window",
        name: "axis",
        spec: ParamSpec::Int { default: -1, min: -8, max: 7, options: &[-2, -1, 0, 1, 2] },
        expression: None,
        doc: Some("Which axis the statistics are taken along. -1 is time."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "window",
        name: "hold",
        spec: ParamSpec::Bool { default: false },
        expression: None,
        doc: Some("Freeze the statistics where they are, so later data is measured against them."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "window",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Forget the past and any frozen statistics."),
        section: 0,
        show: None,
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
    doc: "Put a signal on a common scale, from statistics over a window of its own past.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Normalize, MANIFEST);
