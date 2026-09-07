//! Readout — recursive least squares over a reservoir's state, trained while it runs. This is
//! what turns `Reservoir` from a decoration into a predictor, and the error is itself a signal.

use goofi_core::{Data, Meta, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, SlotDecl, Tag};

fn floats(d: &Data) -> Result<(Vec<usize>, Vec<f64>), String> {
    let a = d.as_array()?;
    let v = a.as_bytes().chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four bytes")) as f64).collect();
    Ok((a.shape().to_vec(), v))
}

/// The frame as it stands NOW: the last column of the last axis, so a block reads as its newest
/// sample and a vector reads as itself.
fn newest(shape: &[usize], v: &[f64]) -> Vec<f64> {
    match shape.len() {
        0 | 1 => v.to_vec(),
        _ => {
            let t = (*shape.last().expect("a shape with a last axis")).max(1);
            (0..v.len() / t).map(|i| v[i * t + t - 1]).collect()
        }
    }
}

fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[derive(Default)]
struct Readout {
    /// The trained weights, row-major `outputs * width`.
    w: Vec<f64>,
    /// The inverse correlation matrix recursive least squares carries, `width * width`.
    p: Vec<f64>,
    width: usize,
    outputs: usize,
    stale: bool,
}

impl Readout {
    fn reset(&mut self, width: usize, outputs: usize, alpha: f64) {
        self.w = vec![0.0; outputs * width];
        self.p = vec![0.0; width * width];
        for i in 0..width {
            self.p[i * width + i] = 1.0 / alpha.max(1e-9);
        }
        self.width = width;
        self.outputs = outputs;
        self.stale = false;
    }
}

impl Node for Readout {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, _c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let (shape, raw) = inp.get("state").map(floats).transpose()?.ok_or("state has no data")?;
        let x = newest(&shape, &raw);
        if x.is_empty() {
            return Err("state is empty".to_string().into());
        }
        if x.len() > 2048 {
            return Err(format!("state is {} wide; recursive least squares carries its square", x.len()).into());
        }
        let target = inp.get("target").map(floats).transpose()?.map(|(shape, v)| newest(&shape, &v));
        let outputs = target.as_ref().map_or(self.outputs.max(1), |t| t.len());
        let alpha = p.f64("readout", "regularization").unwrap_or(1.0);
        if self.stale || self.width != x.len() || self.outputs != outputs {
            self.reset(x.len(), outputs, alpha);
        }
        let n = self.width;

        let mut prediction = vec![0.0; outputs];
        for (o, slot) in prediction.iter_mut().enumerate() {
            *slot = self.w[o * n..(o + 1) * n].iter().zip(&x).map(|(w, x)| w * x).sum();
        }
        let error: Vec<f64> = match &target {
            Some(t) => prediction.iter().zip(t).map(|(a, b)| a - b).collect(),
            None => vec![0.0; outputs],
        };

        if target.is_some() && p.bool("readout", "learning").unwrap_or(true) {
            let mut px = vec![0.0; n];
            for (i, slot) in px.iter_mut().enumerate() {
                *slot = self.p[i * n..(i + 1) * n].iter().zip(&x).map(|(p, x)| p * x).sum();
            }
            let denom = 1.0 + x.iter().zip(&px).map(|(x, p)| x * p).sum::<f64>();
            if denom.abs() < 1e-12 {
                return Err("the state is degenerate — raise `regularization`".to_string().into());
            }
            let gain: Vec<f64> = px.iter().map(|v| v / denom).collect();
            for (o, e) in error.iter().enumerate() {
                for (i, k) in gain.iter().enumerate() {
                    self.w[o * n + i] -= e * k;
                }
            }
            for i in 0..n {
                for j in 0..n {
                    self.p[i * n + j] -= gain[i] * px[j];
                }
            }
        }

        let cast = |v: &[f64]| bytes(&v.iter().map(|x| *x as f32).collect::<Vec<f32>>());
        out.set("prediction", Data::array_f32(vec![outputs], cast(&prediction), Meta::new()).map_err(|e| e.to_string())?);
        out.set("error", Data::array_f32(vec![outputs], cast(&error), Meta::new()).map_err(|e| e.to_string())?);
        out.set("weights", Data::array_f32(vec![outputs, n], cast(&self.w), Meta::new()).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        if key.name == "regularization" {
            self.stale = true;
        }
        Ok(())
    }

    fn on_pulse(&mut self, _key: &ParamKey, _p: &Params<'_>) -> NodeResult {
        self.stale = true;
        Ok(())
    }
}

static INPUTS: &[SlotDecl] = &[
    SlotDecl { name: "state", kind: SlotType::Array, trigger_process: true, multi: false, required: true },
    SlotDecl { name: "target", kind: SlotType::Array, trigger_process: false, multi: false, required: false },
];

static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "prediction", kind: SlotType::Array },
    OutputDecl { name: "error", kind: SlotType::Array },
    OutputDecl { name: "weights", kind: SlotType::Array },
];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "readout",
        name: "learning",
        spec: ParamSpec::Bool { default: true },
        expression: None,
        doc: Some("Keep training. Turn it off to freeze what it learned and watch the prediction run on alone."),
    },
    ParamDecl {
        group: "readout",
        name: "regularization",
        spec: ParamSpec::Float { default: 1.0, min: 1.0e-6, max: 1000.0 },
        expression: None,
        doc: Some("How cautious the first steps are. Larger learns slower and is steadier on a state with few directions."),
    },
    ParamDecl {
        group: "readout",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Forget the weights and start training again."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform, Tag::Simulation, Tag::Ml],
    doc: "Trains a linear readout on a live state, one step at a time.\n\
          Wire a reservoir to `state` and what it should say to `target`; recursive least squares \
          does the rest, and `error` is how wrong it still is.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Readout, MANIFEST);
