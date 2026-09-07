//! Oscillator — the nonlinear oscillators, which carry an amplitude where `Kuramoto` carries only
//! a phase. A limit cycle, an excitable cell, a double well, and the bifurcation between them.

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

fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The newest value of a wired frame, which is what an external forcing reads as.
fn newest(d: &Data) -> Result<f64, String> {
    let a = d.as_array()?;
    let last = a.as_bytes().chunks_exact(4).next_back().ok_or("the drive frame is empty")?;
    Ok(f32::from_le_bytes(last.try_into().expect("four bytes")) as f64)
}

/// Where each model starts, so a reset lands somewhere the trajectory can leave.
fn start(model: &str) -> [f64; 2] {
    match model {
        "fitzhugh" => [-1.2, -0.6],
        "stuartlandau" => [0.1, 0.0],
        "pendulum" => [0.3, 0.0],
        _ => [0.1, 0.0],
    }
}

/// The derivative in the model's own time, before `frequency` scales it.
fn flow(model: &str, s: [f64; 2], nonlinearity: f64, damping: f64, force: f64) -> [f64; 2] {
    let (x, y) = (s[0], s[1]);
    match model {
        "duffing" => [y, -damping * y + x - nonlinearity * x * x * x + force],
        "fitzhugh" => [x - x * x * x / 3.0 - y + force, nonlinearity * 0.1 * (x + 0.7 - 0.8 * y)],
        "stuartlandau" => {
            let r2 = x * x + y * y;
            [(nonlinearity - r2) * x - y + force, (nonlinearity - r2) * y + x]
        }
        "pendulum" => [y, -damping * y - x.sin() + force],
        _ => [y, nonlinearity * (1.0 - x * x) * y - x + force],
    }
}

#[derive(Default)]
struct Oscillator {
    state: [f64; 2],
    /// Model seconds since the start, which the periodic forcing is read off.
    t: f64,
    running: String,
    rng: Rng,
    clock: Clock,
    stale: bool,
}

impl Node for Oscillator {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let model = p.str("oscillator", "model").unwrap_or("vanderpol").to_string();
        if self.stale || self.running != model {
            self.state = start(&model);
            self.state[0] += 1e-3 * Rng::new(p.i64("sim", "seed").unwrap_or(-1)).signed();
            self.rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
            self.t = 0.0;
            self.running = model.clone();
            self.clock = Clock::default();
            self.stale = false;
        }

        let hz = p.f64("oscillator", "frequency").unwrap_or(1.0).max(0.0);
        let nonlinearity = p.f64("oscillator", "nonlinearity").unwrap_or(1.0);
        let damping = p.f64("oscillator", "damping").unwrap_or(0.2);
        let amplitude = p.f64("oscillator", "drive").unwrap_or(0.0);
        let drive_hz = p.f64("oscillator", "drive_frequency").unwrap_or(1.0);
        let noise = p.f64("oscillator", "noise").unwrap_or(0.0).max(0.0);
        let external = inp.get("drive").map(newest).transpose()?.unwrap_or(0.0);
        let dt = p.f64("sim", "dt").unwrap_or(0.005).max(1e-6);
        let sfreq = p.f64("output", "sfreq").unwrap_or(500.0).max(1.0);
        let block = p.str("output", "mode").unwrap_or("value") == "block";

        let steps = self.clock.due(c.now, sfreq);
        if block && steps == 0 {
            return Ok(());
        }
        let cols = if block { steps } else { 1 };
        let mut values = vec![0f32; 2 * cols];
        let omega = std::f64::consts::TAU * hz;
        let kick = (2.0 * noise * dt).sqrt();
        for t in 0..steps {
            let force = external + amplitude * (std::f64::consts::TAU * drive_hz * self.t).cos();
            let d = flow(&model, self.state, nonlinearity, damping, force);
            self.state[0] += dt * omega * d[0] + kick * self.rng.signed();
            self.state[1] += dt * omega * d[1] + kick * self.rng.signed();
            self.t += dt;
            if !self.state.iter().all(|v| v.is_finite()) {
                self.stale = true;
                return Err(format!("{model} ran away — lower `sim.dt` or `oscillator.frequency`").into());
            }
            if block {
                values[t] = self.state[0] as f32;
                values[cols + t] = self.state[1] as f32;
            }
        }
        if !block {
            values[0] = self.state[0] as f32;
            values[1] = self.state[1] as f32;
        }

        let names = vec![Coord::Str("x".into()), Coord::Str("y".into())];
        let shape = if block { vec![2, cols] } else { vec![2] };
        let meta = Meta::new().with_sfreq(block.then_some(sfreq)).with_channels(Axes::new().with(0, Axis::coords(names)));
        out.set("out", Data::array_f32(shape, bytes(&values), meta).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        if key.name == "seed" {
            self.stale = true;
        }
        Ok(())
    }

    fn on_pulse(&mut self, _key: &ParamKey, _p: &Params<'_>) -> NodeResult {
        self.stale = true;
        Ok(())
    }
}

static INPUTS: &[SlotDecl] =
    &[SlotDecl { name: "drive", kind: SlotType::Array, trigger_process: false, multi: false, required: false }];

static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "oscillator",
        name: "model",
        spec: ParamSpec::Str {
            default: "vanderpol",
            options: &["vanderpol", "duffing", "fitzhugh", "stuartlandau", "pendulum"],
            refresh: false,
        },
        expression: None,
        doc: Some(
            "`vanderpol` is a relaxation cycle, `fitzhugh` an excitable neuron, `stuartlandau` the \
             Hopf normal form, `duffing` a double well, `pendulum` the driven damped pendulum.",
        ),
    },
    ParamDecl {
        group: "oscillator",
        name: "frequency",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 200.0 },
        expression: None,
        doc: Some("Roughly how many cycles a second. It scales the whole model's time, not one term."),
    },
    ParamDecl {
        group: "oscillator",
        name: "nonlinearity",
        spec: ParamSpec::Float { default: 1.0, min: -2.0, max: 5.0 },
        expression: None,
        doc: Some(
            "The model's own shape knob: `vanderpol` reads it as mu, `duffing` as the cubic term, \
             `fitzhugh` as how slow the recovery is, `stuartlandau` as lambda — negative for a \
             fixed point, positive for a cycle, and the bifurcation at zero.",
        ),
    },
    ParamDecl {
        group: "oscillator",
        name: "damping",
        spec: ParamSpec::Float { default: 0.2, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("Friction. `duffing` and `pendulum` read it; the others set their own."),
    },
    ParamDecl {
        group: "oscillator",
        name: "drive",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 10.0 },
        expression: None,
        doc: Some("Amplitude of a periodic forcing, added to whatever the `drive` input carries."),
    },
    ParamDecl {
        group: "oscillator",
        name: "drive_frequency",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 20.0 },
        expression: None,
        doc: Some("Frequency of that forcing, relative to the oscillator's own."),
    },
    ParamDecl {
        group: "oscillator",
        name: "noise",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("Jitter on both state variables, which is what makes an excitable model fire on its own."),
    },
    ParamDecl {
        group: "sim",
        name: "dt",
        spec: ParamSpec::Float { default: 0.005, min: 1.0e-6, max: 0.1 },
        expression: None,
        doc: Some("Model seconds per step. Too large and the integration leaves the cycle."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000 },
        expression: None,
        doc: Some("Nudges the start and seeds the noise. Negative takes a fresh one from the clock."),
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Start over from the seed."),
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
        spec: ParamSpec::Float { default: 500.0, min: 1.0, max: 20_000.0 },
        expression: None,
        doc: Some("Steps per second of real time, and the sample rate of an emitted block."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation],
    doc: "Nonlinear oscillators with an amplitude of their own.\n\
          Five models on one set of knobs — a limit cycle, an excitable neuron, a normal form, a \
          double well and a driven pendulum.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Oscillator, MANIFEST);
