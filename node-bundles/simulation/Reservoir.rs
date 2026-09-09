//! Reservoir — an echo state network: a pool of neurons wired at random, driven by an input and
//! left to ring. `spectral_radius` is the one knob that sets how long it remembers.

use goofi_core::{Axes, Axis, Coord, Data, Meta, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, SlotDecl, Tag};

/// xorshift64*, which needs no dependency and is far beyond what a model asks of it.
struct Rng(u64);

impl Default for Rng {
    fn default() -> Rng {
        Rng::new(0)
    }
}

impl Rng {
    fn new(seed: i64) -> Rng {
        let base = if seed < 0 {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x2545_f491, |d| d.as_nanos() as u64)
        } else {
            seed as u64
        };
        Rng(base.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn signed(&mut self) -> f64 {
        self.unit() * 2.0 - 1.0
    }
}

/// The steps a frame owes, counted from the start so the model cannot drift, and capped at a
/// second's worth so a stalled frame does not become a freeze.
#[derive(Default)]
struct Clock {
    start: Option<f64>,
    done: u64,
}

impl Clock {
    fn due(&mut self, now: f64, sfreq: f64) -> usize {
        let start = *self.start.get_or_insert(now);
        let total = (sfreq * (now - start)).round().max(0.0) as u64;
        let n = total.saturating_sub(self.done).min(sfreq.ceil() as u64);
        self.done = total;
        n as usize
    }
}

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

fn activate(kind: &str, x: f64) -> f64 {
    match kind {
        "sigmoid" => 1.0 / (1.0 + (-x).exp()),
        "relu" => x.max(0.0),
        _ => x.tanh(),
    }
}

#[derive(Default)]
struct Reservoir {
    /// The pool's state, `size` long.
    x: Vec<f64>,
    /// Recurrent weights, row-major `size * size`, already rescaled to the spectral radius.
    w: Vec<f64>,
    /// Input weights, row-major `size * fan`, redrawn when the input's width changes.
    win: Vec<f64>,
    fan: usize,
    bias: Vec<f64>,
    rng: Rng,
    clock: Clock,
    stale: bool,
}

impl Reservoir {
    /// The largest absolute eigenvalue, by power iteration — enough digits to rescale by.
    fn radius(w: &[f64], n: usize) -> f64 {
        let mut v = vec![1.0 / (n as f64).sqrt(); n];
        let mut scale = 0.0;
        for _ in 0..40 {
            let mut next = vec![0.0; n];
            for (i, out) in next.iter_mut().enumerate() {
                *out = (0..n).map(|j| w[i * n + j] * v[j]).sum();
            }
            let norm = next.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm < 1e-12 {
                return 0.0;
            }
            for (dst, src) in v.iter_mut().zip(&next) {
                *dst = src / norm;
            }
            scale = norm;
        }
        scale
    }

    fn reseed(&mut self, n: usize, wired: Option<&[f64]>, p: &Params<'_>) {
        let mut rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
        let density = p.f64("reservoir", "density").unwrap_or(0.1).clamp(0.0, 1.0);
        let mut w = match wired {
            Some(m) => m.to_vec(),
            None => (0..n * n).map(|_| if rng.unit() < density { rng.signed() } else { 0.0 }).collect(),
        };
        for (i, row) in w.chunks_exact_mut(n).enumerate() {
            row[i] = 0.0;
        }
        let radius = Self::radius(&w, n);
        let want = p.f64("reservoir", "spectral_radius").unwrap_or(0.95);
        if radius > 1e-12 {
            let scale = want / radius;
            for v in &mut w {
                *v *= scale;
            }
        }
        self.w = w;
        self.x = vec![0.0; n];
        self.bias = (0..n).map(|_| rng.signed()).collect();
        self.win = Vec::new();
        self.fan = 0;
        self.rng = rng;
        self.clock = Clock::default();
        self.stale = false;
    }
}

impl Node for Reservoir {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let matrix = inp.get("connectivity").map(floats).transpose()?;
        if let Some((shape, _)) = &matrix {
            if shape.len() != 2 || shape[0] != shape[1] {
                return Err(format!("connectivity needs a square matrix, got {shape:?}").into());
            }
        }
        let n = match &matrix {
            Some((shape, _)) => shape[0],
            None => p.i64("reservoir", "size").unwrap_or(100).clamp(2, 1000) as usize,
        };
        if self.stale || self.x.len() != n {
            self.reseed(n, matrix.as_ref().map(|(_, m)| m.as_slice()), p);
        }

        let drive = inp.get("input").map(floats).transpose()?.map(|(shape, v)| newest(&shape, &v)).unwrap_or_default();
        if self.fan != drive.len() {
            self.fan = drive.len();
            self.win = (0..n * self.fan).map(|_| self.rng.signed()).collect();
        }

        let leak = p.f64("reservoir", "leak").unwrap_or(0.3).clamp(0.0, 1.0);
        let gain = p.f64("reservoir", "input_scale").unwrap_or(1.0);
        let bias = p.f64("reservoir", "bias").unwrap_or(0.1);
        let noise = p.f64("reservoir", "noise").unwrap_or(0.0).max(0.0);
        let kind = p.str("reservoir", "activation").unwrap_or("tanh");
        let sfreq = p.f64("output", "sfreq").unwrap_or(60.0).max(1.0);
        let block = p.str("output", "mode").unwrap_or("value") == "block";

        let steps = self.clock.due(c.now, sfreq);
        if block && steps == 0 {
            return Ok(());
        }
        let cols = if block { steps } else { 1 };
        let mut states = vec![0f32; n * cols];
        let mut next = vec![0.0; n];
        for t in 0..steps {
            for (i, slot) in next.iter_mut().enumerate() {
                let recurrent: f64 = self.w[i * n..(i + 1) * n].iter().zip(&self.x).map(|(w, x)| w * x).sum();
                let driven: f64 = self.win[i * self.fan..(i + 1) * self.fan].iter().zip(&drive).map(|(w, u)| w * u).sum();
                let sum = recurrent + gain * driven + bias * self.bias[i] + noise * self.rng.signed();
                *slot = (1.0 - leak) * self.x[i] + leak * activate(kind, sum);
            }
            self.x.copy_from_slice(&next);
            if block {
                for (i, x) in self.x.iter().enumerate() {
                    states[i * cols + t] = *x as f32;
                }
            }
        }
        if !block {
            for (i, x) in self.x.iter().enumerate() {
                states[i] = *x as f32;
            }
        }

        let names: Vec<Coord> = (0..n).map(|i| Coord::Str(format!("unit{i}").into())).collect();
        let shape = if block { vec![n, cols] } else { vec![n] };
        let meta = Meta::new().with_sfreq(block.then_some(sfreq)).with_channels(Axes::new().with(0, Axis::coords(names)));
        out.set("state", Data::array_f32(shape, bytes(&states), meta).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        // The weights ARE the model: a new draw, a new radius or a new size is a different one.
        if matches!(key.name.as_str(), "seed" | "size" | "density" | "spectral_radius") {
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
    SlotDecl { name: "input", kind: SlotType::Array, trigger_process: false, multi: false, required: false },
    SlotDecl { name: "connectivity", kind: SlotType::Array, trigger_process: false, multi: false, required: false },
];

static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "state", kind: SlotType::Array }];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "reservoir",
        name: "size",
        spec: ParamSpec::Int { default: 100, min: 2, max: 1000, options: &[] },
        expression: None,
        doc: Some("How many units. A wired `connectivity` matrix decides instead. Cost grows with the square."),
    },
    ParamDecl {
        group: "reservoir",
        name: "spectral_radius",
        spec: ParamSpec::Float { default: 0.95, min: 0.0, max: 2.0 },
        expression: None,
        doc: Some("How long the pool remembers. Below 1 it forgets; near 1 is the edge of chaos; above 1 it runs away."),
    },
    ParamDecl {
        group: "reservoir",
        name: "leak",
        spec: ParamSpec::Float { default: 0.3, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("How much of each update is new. Lower makes the pool slower than its input."),
    },
    ParamDecl {
        group: "reservoir",
        name: "density",
        spec: ParamSpec::Float { default: 0.1, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("Fraction of the drawn connections that are not zero."),
    },
    ParamDecl {
        group: "reservoir",
        name: "input_scale",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 20.0 },
        expression: None,
        doc: Some("How hard the wired input drives the pool."),
    },
    ParamDecl {
        group: "reservoir",
        name: "bias",
        spec: ParamSpec::Float { default: 0.1, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("Scale of the per-unit constant offset, which keeps an undriven pool from settling flat."),
    },
    ParamDecl {
        group: "reservoir",
        name: "activation",
        spec: ParamSpec::Str { default: "tanh", options: &["tanh", "sigmoid", "relu"], refresh: false },
        expression: None,
        doc: Some("The unit's nonlinearity."),
    },
    ParamDecl {
        group: "reservoir",
        name: "noise",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("Noise added to every unit before the nonlinearity."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000, options: &[] },
        expression: None,
        doc: Some("Seeds the weights and the biases. Negative takes a fresh one from the clock."),
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Draw the pool again from the seed."),
    },
    ParamDecl {
        group: "output",
        name: "mode",
        spec: ParamSpec::Str { default: "value", options: &["value", "block"], refresh: false },
        expression: None,
        doc: Some("`value` emits the state now; `block` emits every step since the last frame, which is a signal."),
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 60.0, min: 1.0, max: 10_000.0 },
        expression: None,
        doc: Some("Updates per second of real time, and the sample rate of an emitted block."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation, Tag::Ml],
    doc: "A pool of randomly wired units that rings when driven.\n\
          The echo state network: the pool is never trained, only read — wire `Readout` to it to \
          turn what it remembers into a prediction.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Reservoir, MANIFEST);
