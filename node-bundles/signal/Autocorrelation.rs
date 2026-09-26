//! Autocorrelation — how much a signal resembles itself shifted by each lag, along one axis of
//! the frame it is handed. It keeps no past: put a `Buffer` in front for the window it should see.

use goofi_core::{resolve_axis, stream, Axis, Data, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};
use rustfft::{num_complex::Complex32, FftPlanner};

struct Autocorrelation {
    planner: FftPlanner<f32>,
}

impl Default for Autocorrelation {
    fn default() -> Autocorrelation {
        Autocorrelation { planner: FftPlanner::new() }
    }
}

impl Autocorrelation {
    /// `r[k] = Σ x[i]·x[i+k]` for `lags` lags, by the Wiener–Khinchin route: the power spectrum
    /// of the zero-padded lane, transformed back. Padding to twice the length keeps it linear.
    fn lags(&mut self, lane: &[f32], lags: usize) -> Vec<f64> {
        let t = lane.len();
        let n = (2 * t).next_power_of_two();
        let mut buf: Vec<Complex32> = lane.iter().map(|v| Complex32::new(*v, 0.0)).collect();
        buf.resize(n, Complex32::default());
        self.planner.plan_fft_forward(n).process(&mut buf);
        for c in &mut buf {
            *c = Complex32::new(c.norm_sqr(), 0.0);
        }
        self.planner.plan_fft_inverse(n).process(&mut buf);
        buf[..lags].iter().map(|c| c.re as f64 / n as f64).collect()
    }
}

impl Node for Autocorrelation {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, _c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let a = d.assert_ndims().at_least(1)?;
        let dim = resolve_axis(p.i64("autocorrelation", "axis").unwrap_or(-1), a.shape().len())?;
        let t = a.shape()[dim];
        if t < 2 {
            return Err(format!("needs at least 2 samples along the axis, got {t}").into());
        }
        let asked = p.i64("autocorrelation", "lags").unwrap_or(0).max(0) as usize;
        let lags = if asked == 0 { t } else { asked.min(t) };
        let estimate = p.str("autocorrelation", "estimate").unwrap_or("coefficient");
        let detrend = p.bool("autocorrelation", "detrend").unwrap_or(true);

        let lanes = stream::lanes(a.shape(), dim, a.as_bytes());
        let mut answered = Vec::with_capacity(lanes.len());
        for lane in &lanes {
            let mean = if detrend { lane.iter().map(|v| *v as f64).sum::<f64>() / t as f64 } else { 0.0 };
            let centred: Vec<f32> = lane.iter().map(|v| (*v as f64 - mean) as f32).collect();
            let r = self.lags(&centred, lags);
            let scaled: Vec<f32> = match estimate {
                // Lag zero is one; a lane of nothing has no resemblance to state.
                "coefficient" => {
                    let r0 = r[0];
                    r.iter().map(|v| if r0 > 0.0 { (v / r0) as f32 } else { 0.0 }).collect()
                }
                "unbiased" => r.iter().enumerate().map(|(k, v)| (v / (t - k) as f64) as f32).collect(),
                _ => r.iter().map(|v| (v / t as f64) as f32).collect(),
            };
            answered.push(scaled);
        }
        let mut shape = a.shape().to_vec();
        shape[dim] = lags;
        let buf = stream::unlanes(&shape, dim, &answered);
        // A lag is spaced like a sample, so the rate still reads as the lag axis's spacing; any
        // coordinates the time axis wore do not.
        let meta = d.meta().clone().with_channels(d.meta().channels().clone().with(dim, Axis::default()));
        out.set("out", Data::array_f32(shape, buf, meta).map_err(|e| e.to_string())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "autocorrelation",
        name: "lags",
        spec: ParamSpec::Int { default: 0, min: 0, max: 100_000, options: &[] },
        expression: None,
        doc: Some("How many lags to answer, from zero up. 0 is every lag the axis has."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "autocorrelation",
        name: "estimate",
        spec: ParamSpec::Str { default: "coefficient", options: &["coefficient", "biased", "unbiased"], refresh: false },
        expression: None,
        doc: Some(
            "`coefficient` is one at lag zero; `biased` divides every lag by the length, \
             `unbiased` by the overlap the lag leaves.",
        ),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "autocorrelation",
        name: "detrend",
        spec: ParamSpec::Bool { default: true },
        expression: None,
        doc: Some("Take the mean out first, so an offset does not read as resemblance."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "autocorrelation",
        name: "axis",
        spec: ParamSpec::Int { default: -1, min: -8, max: 7, options: &[-2, -1, 0, 1, 2] },
        expression: None,
        doc: Some("Which axis to correlate along. -1 is time."),
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
    doc: "How much a signal resembles itself shifted by each lag.\n\
          The frame as handed is the window: a `Buffer` in front sets how much is seen. Each lane \
          along `axis` becomes its autocorrelation over `lags` lags, so a periodic signal peaks \
          again at its period.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Autocorrelation, MANIFEST);
