//! Kuramoto — N phase oscillators pulling each other into step. The coupling is a matrix off the
//! graph, so a connectome or a correlation matrix decides who hears whom.

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

    fn normal(&mut self) -> f64 {
        let u = self.unit().max(1e-12);
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * self.unit()).cos()
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

/// One input frame as its shape and its values.
fn floats(d: &Data) -> Result<(Vec<usize>, Vec<f64>), String> {
    let a = d.as_array()?;
    let v = a.as_bytes().chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four bytes")) as f64).collect();
    Ok((a.shape().to_vec(), v))
}

fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[derive(Default)]
struct Kuramoto {
    /// Phase per oscillator, in radians, bounded to `[0, TAU)`.
    theta: Vec<f64>,
    /// Natural frequencies drawn from `frequency` and `spread`, in Hz. A wired input wins over it.
    drawn: Vec<f64>,
    rng: Rng,
    clock: Clock,
    /// Set at birth and by any edit to the draw, so the next run seeds again.
    stale: bool,
}

impl Kuramoto {
    fn reseed(&mut self, n: usize, p: &Params<'_>) {
        let mut rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
        let mean = p.f64("kuramoto", "frequency").unwrap_or(1.0);
        let spread = p.f64("kuramoto", "spread").unwrap_or(0.2);
        self.theta = (0..n).map(|_| rng.unit() * std::f64::consts::TAU).collect();
        self.drawn = (0..n).map(|_| mean + spread * rng.normal()).collect();
        self.rng = rng;
        self.clock = Clock::default();
        self.stale = false;
    }

    /// Column `t` of each output, read off the phases as they now stand.
    fn write(&self, phases: &mut [f32], waves: &mut [f32], order: &mut [f32], t: usize, cols: usize) {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, theta) in self.theta.iter().enumerate() {
            phases[i * cols + t] = *theta as f32;
            waves[i * cols + t] = theta.sin() as f32;
            re += theta.cos();
            im += theta.sin();
        }
        order[t] = (re.hypot(im) / self.theta.len() as f64) as f32;
    }
}

impl Node for Kuramoto {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let matrix = inp.get("coupling").map(floats).transpose()?;
        let wired = inp.get("frequencies").map(floats).transpose()?;
        if let Some((shape, _)) = &matrix {
            if shape.len() != 2 || shape[0] != shape[1] {
                return Err(format!("coupling needs a square matrix, got {shape:?}").into());
            }
        }
        let n = match (&matrix, &wired) {
            (Some((shape, _)), _) => shape[0],
            (None, Some((_, v))) => v.len(),
            (None, None) => p.i64("kuramoto", "size").unwrap_or(8).clamp(1, 1024) as usize,
        };
        if n == 0 {
            return Err("no oscillators to run".to_string().into());
        }
        if self.stale || self.theta.len() != n {
            self.reseed(n, p);
        }
        let hz = match &wired {
            Some((_, v)) if v.len() != n => {
                return Err(format!("frequencies is {} long and the coupling matrix is {n} wide", v.len()).into())
            }
            Some((_, v)) => v.as_slice(),
            None => self.drawn.as_slice(),
        };

        let gain = p.f64("kuramoto", "coupling").unwrap_or(1.0);
        let noise = p.f64("kuramoto", "noise").unwrap_or(0.0).max(0.0);
        let dt = p.f64("sim", "dt").unwrap_or(0.005).max(1e-6);
        let sfreq = p.f64("output", "sfreq").unwrap_or(200.0).max(1.0);
        let block = p.str("output", "mode").unwrap_or("value") == "block";

        let steps = self.clock.due(c.now, sfreq);
        if block && steps == 0 {
            return Ok(());
        }
        let cols = if block { steps } else { 1 };
        let (mut phases, mut waves) = (vec![0f32; n * cols], vec![0f32; n * cols]);
        let mut order = vec![0f32; cols];

        let kick = (2.0 * noise * dt).sqrt();
        let mut delta = vec![0.0; n];
        for t in 0..steps {
            for (i, d) in delta.iter_mut().enumerate() {
                let mut pull = 0.0;
                for (j, other) in self.theta.iter().enumerate() {
                    let weight = match &matrix {
                        Some((_, m)) => m[i * n + j],
                        None => 1.0,
                    };
                    pull += weight * (other - self.theta[i]).sin();
                }
                *d = std::f64::consts::TAU * hz[i] + gain * pull / n as f64;
            }
            for (theta, d) in self.theta.iter_mut().zip(&delta) {
                *theta = (*theta + dt * d + kick * self.rng.normal()).rem_euclid(std::f64::consts::TAU);
            }
            if block {
                self.write(&mut phases, &mut waves, &mut order, t, cols);
            }
        }
        if !block {
            self.write(&mut phases, &mut waves, &mut order, 0, cols);
        }

        let names: Vec<Coord> = (0..n).map(|i| Coord::Str(format!("osc{i}").into())).collect();
        let shape = if block { vec![n, cols] } else { vec![n] };
        let meta = Meta::new().with_sfreq(block.then_some(sfreq)).with_channels(Axes::new().with(0, Axis::coords(names)));
        out.set("phases", Data::array_f32(shape.clone(), bytes(&phases), meta.clone()).map_err(|e| e.to_string())?);
        out.set("waveforms", Data::array_f32(shape, bytes(&waves), meta).map_err(|e| e.to_string())?);
        let scalar = Meta::new().with_sfreq(block.then_some(sfreq));
        out.set("order", Data::array_f32(vec![cols], bytes(&order), scalar).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        // A new draw or a new count is a different model, and the phases it left behind are not its.
        if matches!(key.name.as_str(), "seed" | "size" | "frequency" | "spread") {
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
    SlotDecl { name: "frequencies", kind: SlotType::Array, trigger_process: false, multi: false, required: false },
    SlotDecl { name: "coupling", kind: SlotType::Array, trigger_process: false, multi: false, required: false },
];

static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "phases", kind: SlotType::Array },
    OutputDecl { name: "waveforms", kind: SlotType::Array },
    OutputDecl { name: "order", kind: SlotType::Array },
];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "kuramoto",
        name: "size",
        spec: ParamSpec::Int { default: 8, min: 1, max: 1024, options: &[] },
        expression: None,
        doc: Some("How many oscillators, when neither input says. A wired input decides instead."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "kuramoto",
        name: "coupling",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 50.0 },
        expression: None,
        doc: Some("How hard the oscillators pull on each other. Past a threshold set by `spread`, they lock."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "kuramoto",
        name: "frequency",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 200.0 },
        expression: None,
        doc: Some("Mean natural frequency in Hz. The `frequencies` input replaces this and `spread`."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "kuramoto",
        name: "spread",
        spec: ParamSpec::Float { default: 0.2, min: 0.0, max: 50.0 },
        expression: None,
        doc: Some("Standard deviation of the natural frequencies. A wider set needs stronger coupling to lock."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "kuramoto",
        name: "noise",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 10.0 },
        expression: None,
        doc: Some("Phase diffusion, which fights the coupling."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "sim",
        name: "dt",
        spec: ParamSpec::Float { default: 0.005, min: 1.0e-6, max: 1.0 },
        expression: None,
        doc: Some("Model seconds per step. The model runs at `output.sfreq` times this, relative to real time."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000, options: &[] },
        expression: None,
        doc: Some("Seeds the phases and the drawn frequencies. Negative takes a fresh one from the clock."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Draw the model again from the seed."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "output",
        name: "mode",
        spec: ParamSpec::Str { default: "value", options: &["value", "block"], refresh: false },
        expression: None,
        doc: Some(
            "`value` emits the state now, which a param reference reads as a number; \
             `block` emits every step since the last frame, which is a signal.",
        ),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 200.0, min: 1.0, max: 10_000.0 },
        expression: None,
        doc: Some("Integration steps per second of real time, and the sample rate of an emitted block."),
        section: 0,
        show: None,
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation, Tag::Connectivity],
    doc: "Coupled phase oscillators that fall into step.\n\
          Each runs at its own natural frequency and is pulled towards the others; `order` says \
          how synchronised they are, from 0 to 1.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Kuramoto, MANIFEST);
