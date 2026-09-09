//! NeuralMass — a population of neurons as one variable, and a network of those populations wired
//! by a connectome. Jansen-Rit is the model an alpha rhythm comes out of.

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

    fn signed(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
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

fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// How many state variables one population carries.
fn rank(model: &str) -> usize {
    match model {
        "jansenrit" => 6,
        "wongwang" => 1,
        _ => 2,
    }
}

/// The variable the network is coupled through, and what the node reports.
fn observe(model: &str, s: &[f64]) -> f64 {
    match model {
        "jansenrit" => s[1] - s[2],
        "wongwang" => s[0],
        _ => s[0] - s[1],
    }
}

fn sigmoid(slope: f64, threshold: f64, x: f64) -> f64 {
    1.0 / (1.0 + (-slope * (x - threshold)).exp())
}

/// One population's derivative, given what the network is sending it.
fn flow(model: &str, s: &[f64], gain: f64, drive: f64) -> Vec<f64> {
    match model {
        "jansenrit" => {
            let (big_a, big_b, a, b, cc) = (3.25, 22.0, 100.0, 50.0, 135.0);
            let sigm = |v: f64| 5.0 / (1.0 + (0.56 * gain * (6.0 - v)).exp());
            vec![
                s[3],
                s[4],
                s[5],
                big_a * a * sigm(s[1] - s[2]) - 2.0 * a * s[3] - a * a * s[0],
                big_a * a * (drive + 0.8 * cc * sigm(cc * s[0])) - 2.0 * a * s[4] - a * a * s[1],
                big_b * b * 0.25 * cc * sigm(0.25 * cc * s[0]) - 2.0 * b * s[5] - b * b * s[2],
            ]
        }
        "wongwang" => {
            let (tau, gamma, a, b, d) = (0.1, 0.641, 270.0, 108.0, 0.154 * gain);
            let x = 0.9 * 0.2609 * s[0] + 0.3 + drive;
            let num = a * x - b;
            let h = if num.abs() < 1e-9 { 1.0 / d } else { num / (1.0 - (-d * num).exp()) };
            vec![-s[0] / tau + (1.0 - s[0]) * gamma * h]
        }
        _ => {
            let (tau_e, tau_i) = (0.010, 0.020);
            let excite = sigmoid(1.3 * gain, 4.0, 16.0 * s[0] - 12.0 * s[1] + drive);
            let inhibit = sigmoid(2.0 * gain, 3.7, 15.0 * s[0] - 3.0 * s[1]);
            vec![(-s[0] + (1.0 - s[0]) * excite) / tau_e, (-s[1] + (1.0 - s[1]) * inhibit) / tau_i]
        }
    }
}

/// Where one population starts, nudged so a network is not born identical.
fn start(model: &str, rng: &mut Rng) -> Vec<f64> {
    let jitter = 0.05 * rng.signed();
    match model {
        "jansenrit" => vec![0.1 + jitter, 20.0 + jitter, 15.0 + jitter, 0.0, 0.0, 0.0],
        "wongwang" => vec![(0.15 + jitter).clamp(0.0, 1.0)],
        _ => vec![(0.1 + jitter).clamp(0.0, 1.0), (0.05 + jitter).clamp(0.0, 1.0)],
    }
}

#[derive(Default)]
struct NeuralMass {
    /// Every population's variables, node-major: population `i` owns `rank` slots from `i * rank`.
    state: Vec<f64>,
    running: String,
    rng: Rng,
    clock: Clock,
    stale: bool,
}

impl NeuralMass {
    fn reseed(&mut self, model: &str, n: usize, p: &Params<'_>) {
        let mut rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
        self.state = (0..n).flat_map(|_| start(model, &mut rng)).collect();
        self.rng = rng;
        self.running = model.to_string();
        self.clock = Clock::default();
        self.stale = false;
    }
}

impl Node for NeuralMass {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let model = p.str("mass", "model").unwrap_or("jansenrit").to_string();
        let vars = rank(&model);
        let matrix = inp.get("connectivity").map(floats).transpose()?;
        if let Some((shape, _)) = &matrix {
            if shape.len() != 2 || shape[0] != shape[1] {
                return Err(format!("connectivity needs a square matrix, got {shape:?}").into());
            }
        }
        let n = match &matrix {
            Some((shape, _)) => shape[0],
            None => p.i64("mass", "size").unwrap_or(1).clamp(1, 512) as usize,
        };
        if self.stale || self.running != model || self.state.len() != n * vars {
            self.reseed(&model, n, p);
        }

        let g = p.f64("mass", "coupling").unwrap_or(1.0);
        let gain = p.f64("mass", "gain").unwrap_or(1.0).max(0.01);
        let base = p.f64("mass", "drive").unwrap_or(1.5);
        let noise = p.f64("mass", "noise").unwrap_or(0.7).max(0.0);
        let dt = p.f64("sim", "dt").unwrap_or(0.001).max(1e-6);
        let sfreq = p.f64("output", "sfreq").unwrap_or(1000.0).max(1.0);
        let block = p.str("output", "mode").unwrap_or("value") == "block";
        let external = inp.get("input").map(floats).transpose()?.map(|(_, v)| v).unwrap_or_default();
        if !external.is_empty() && external.len() != 1 && external.len() != n {
            return Err(format!("input is {} long; it must be one value or one per population ({n})", external.len()).into());
        }
        // Jansen-Rit's own drive is a pulse rate in the hundreds; the others live near 1.
        let scale = if model == "jansenrit" { 150.0 } else { 1.0 };

        let steps = self.clock.due(c.now, sfreq);
        if block && steps == 0 {
            return Ok(());
        }
        let cols = if block { steps } else { 1 };
        let mut values = vec![0f32; n * cols];
        let mut seen = vec![0.0; n];
        let mut next = self.state.clone();
        for t in 0..steps {
            for (i, o) in seen.iter_mut().enumerate() {
                *o = observe(&model, &self.state[i * vars..(i + 1) * vars]);
            }
            for i in 0..n {
                let network: f64 = match &matrix {
                    Some((_, m)) => (0..n).map(|j| m[i * n + j] * seen[j]).sum::<f64>(),
                    None => seen.iter().sum::<f64>() - seen[i],
                };
                let own = match external.len() {
                    0 => 0.0,
                    1 => external[0],
                    _ => external[i],
                };
                // The noise belongs in the INPUT, not in a state variable: these rhythms are a
                // resonance driven by a fluctuating input, and a constant one gives a slower cycle.
                let jitter = noise * self.rng.signed();
                let drive = scale * (base + own + jitter) + g * network / n as f64 * scale;
                let d = flow(&model, &self.state[i * vars..(i + 1) * vars], gain, drive);
                for (k, d) in d.iter().enumerate() {
                    next[i * vars + k] = self.state[i * vars + k] + dt * d;
                }
            }
            self.state.copy_from_slice(&next);
            if !self.state.iter().all(|v| v.is_finite()) {
                self.stale = true;
                return Err(format!("{model} ran away — lower `sim.dt`, `mass.coupling` or `mass.drive`").into());
            }
            if block {
                for i in 0..n {
                    values[i * cols + t] = observe(&model, &self.state[i * vars..(i + 1) * vars]) as f32;
                }
            }
        }
        if !block {
            for i in 0..n {
                values[i] = observe(&model, &self.state[i * vars..(i + 1) * vars]) as f32;
            }
        }

        let names: Vec<Coord> = (0..n).map(|i| Coord::Str(format!("region{i}").into())).collect();
        let shape = if block { vec![n, cols] } else { vec![n] };
        let meta = Meta::new().with_sfreq(block.then_some(sfreq)).with_channels(Axes::new().with(0, Axis::coords(names)));
        out.set("out", Data::array_f32(shape, bytes(&values), meta).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        if matches!(key.name.as_str(), "seed" | "size") {
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

static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "mass",
        name: "model",
        spec: ParamSpec::Str { default: "jansenrit", options: &["jansenrit", "wilsoncowan", "wongwang"], refresh: false },
        expression: None,
        doc: Some(
            "`jansenrit` makes a cortical rhythm in the alpha band, `wilsoncowan` an excitatory and \
             inhibitory pair, `wongwang` the slow decision variable used in whole-brain work.",
        ),
    },
    ParamDecl {
        group: "mass",
        name: "size",
        spec: ParamSpec::Int { default: 1, min: 1, max: 512, options: &[] },
        expression: None,
        doc: Some("How many populations, when nothing is wired. A `connectivity` matrix decides instead."),
    },
    ParamDecl {
        group: "mass",
        name: "coupling",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 50.0 },
        expression: None,
        doc: Some("How hard the network drives each population. With one population it does nothing."),
    },
    ParamDecl {
        group: "mass",
        name: "drive",
        spec: ParamSpec::Float { default: 1.5, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("Background input every population gets, added to whatever the `input` slot carries."),
    },
    ParamDecl {
        group: "mass",
        name: "gain",
        spec: ParamSpec::Float { default: 1.0, min: 0.1, max: 3.0 },
        expression: None,
        doc: Some("How steep the model's firing-rate curve is. Steeper swings harder for the same input."),
    },
    ParamDecl {
        group: "mass",
        name: "noise",
        spec: ParamSpec::Float { default: 0.7, min: 0.0, max: 2.0 },
        expression: None,
        doc: Some(
            "Jitter on the input, as a fraction of `drive`. These rhythms are a resonance driven \
             by a fluctuating input: at 0 the model falls onto a slower cycle of its own.",
        ),
    },
    ParamDecl {
        group: "sim",
        name: "dt",
        spec: ParamSpec::Float { default: 0.001, min: 1.0e-6, max: 0.01 },
        expression: None,
        doc: Some("Model seconds per step. Jansen-Rit needs a millisecond or less."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000, options: &[] },
        expression: None,
        doc: Some("Seeds the starting state and the noise. Negative takes a fresh one from the clock."),
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Start the network over from the seed."),
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
        spec: ParamSpec::Float { default: 1000.0, min: 1.0, max: 20_000.0 },
        expression: None,
        doc: Some("Steps per second of real time, and the sample rate of an emitted block."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation, Tag::Eeg, Tag::Connectivity],
    doc: "Neural populations, wired into a network.\n\
          Each is a whole population as one variable; a connectome on the `connectivity` input \
          makes the set a whole-brain model.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(NeuralMass, MANIFEST);
