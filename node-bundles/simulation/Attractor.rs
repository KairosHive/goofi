//! Attractor — the classical chaotic systems, continuous and discrete, on one clock. `a`, `b` and
//! `c` scale each system's own canonical constants, so 1 is always the textbook figure.

use goofi_core::{Axes, Axis, Coord, Data, Meta, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, Tag};

/// xorshift64*, which needs no dependency and is far beyond what a model asks of it.
struct Rng(u64);

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

/// What one system IS: its rank, where it starts, the size it grows to, and whether time is
/// continuous. The constants are folded into `flow` and `map` below.
struct System {
    rank: usize,
    start: [f64; 3],
    scale: f64,
    flow: bool,
}

fn system(name: &str) -> System {
    match name {
        "rossler" => System { rank: 3, start: [1.0, 1.0, 1.0], scale: 12.0, flow: true },
        "chua" => System { rank: 3, start: [0.7, 0.0, 0.0], scale: 3.0, flow: true },
        "thomas" => System { rank: 3, start: [1.0, 0.0, 0.0], scale: 4.0, flow: true },
        "aizawa" => System { rank: 3, start: [0.1, 0.0, 0.0], scale: 1.5, flow: true },
        "halvorsen" => System { rank: 3, start: [1.0, 0.0, 0.0], scale: 8.0, flow: true },
        "henon" => System { rank: 2, start: [0.1, 0.1, 0.0], scale: 1.5, flow: false },
        "ikeda" => System { rank: 2, start: [0.1, 0.1, 0.0], scale: 2.0, flow: false },
        "logistic" => System { rank: 1, start: [0.5, 0.0, 0.0], scale: 1.0, flow: false },
        "standard" => System { rank: 2, start: [0.1, 0.1, 0.0], scale: std::f64::consts::TAU, flow: false },
        _ => System { rank: 3, start: [1.0, 1.0, 1.0], scale: 25.0, flow: true },
    }
}

/// The derivative of a continuous system at `s`, with `k` scaling its canonical constants.
fn flow(name: &str, s: [f64; 3], k: [f64; 3]) -> [f64; 3] {
    let (x, y, z) = (s[0], s[1], s[2]);
    match name {
        "rossler" => {
            let (a, b, cc) = (0.2 * k[0], 0.2 * k[1], 5.7 * k[2]);
            [-y - z, x + a * y, b + z * (x - cc)]
        }
        "chua" => {
            let (alpha, beta) = (15.6 * k[0], 28.0 * k[1]);
            let (m0, m1) = (-1.143 * k[2], -0.714 * k[2]);
            let h = m1 * x + 0.5 * (m0 - m1) * ((x + 1.0).abs() - (x - 1.0).abs());
            [alpha * (y - x - h), x - y + z, -beta * y]
        }
        "thomas" => {
            let b = 0.208186 * k[0];
            [y.sin() - b * x, z.sin() - b * y, x.sin() - b * z]
        }
        "aizawa" => {
            let (a, b, cc) = (0.95 * k[0], 0.7 * k[1], 0.6 * k[2]);
            let (d, e, f) = (3.5, 0.25, 0.1);
            [
                (z - b) * x - d * y,
                d * x + (z - b) * y,
                cc + a * z - z * z * z / 3.0 - (x * x + y * y) * (1.0 + e * z) + f * z * x * x * x,
            ]
        }
        "halvorsen" => {
            let a = 1.4 * k[0];
            [-a * x - 4.0 * y - 4.0 * z - y * y, -a * y - 4.0 * z - 4.0 * x - z * z, -a * z - 4.0 * x - 4.0 * y - x * x]
        }
        _ => {
            let (sigma, rho, beta) = (10.0 * k[0], 28.0 * k[1], 8.0 / 3.0 * k[2]);
            [sigma * (y - x), x * (rho - z) - y, x * y - beta * z]
        }
    }
}

/// One iteration of a discrete system.
fn map(name: &str, s: [f64; 3], k: [f64; 3]) -> [f64; 3] {
    let (x, y) = (s[0], s[1]);
    match name {
        "ikeda" => {
            let u = 0.9 * k[0];
            let t = 0.4 - 6.0 / (1.0 + x * x + y * y);
            [1.0 + u * (x * t.cos() - y * t.sin()), u * (x * t.sin() + y * t.cos()), 0.0]
        }
        "logistic" => {
            let r = 3.9 * k[0];
            [(r * x * (1.0 - x)).clamp(0.0, 1.0), 0.0, 0.0]
        }
        "standard" => {
            let big = 1.0 * k[0];
            let p = (y + big * x.sin()).rem_euclid(std::f64::consts::TAU);
            [(x + p).rem_euclid(std::f64::consts::TAU), p, 0.0]
        }
        _ => {
            let (a, b) = (1.4 * k[0], 0.3 * k[1]);
            [1.0 - a * x * x + y, b * x, 0.0]
        }
    }
}

#[derive(Default)]
struct Attractor {
    state: [f64; 3],
    /// The system the state belongs to; a different one starts over.
    running: String,
    clock: Clock,
    stale: bool,
}

impl Node for Attractor {
    fn process(&mut self, _inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let name = p.str("attractor", "system").unwrap_or("lorenz").to_string();
        let s = system(&name);
        if self.stale || self.running != name {
            let mut rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
            self.state = s.start;
            for v in &mut self.state {
                *v += 1e-3 * rng.signed();
            }
            self.running = name.clone();
            self.clock = Clock::default();
            self.stale = false;
        }

        let k = [
            p.f64("attractor", "a").unwrap_or(1.0),
            p.f64("attractor", "b").unwrap_or(1.0),
            p.f64("attractor", "c").unwrap_or(1.0),
        ];
        let dt = p.f64("sim", "dt").unwrap_or(0.01).max(1e-6);
        let sfreq = p.f64("output", "sfreq").unwrap_or(500.0).max(1.0);
        let block = p.str("output", "mode").unwrap_or("value") == "block";
        let scale = if p.bool("output", "normalize").unwrap_or(true) { s.scale } else { 1.0 };

        let steps = self.clock.due(c.now, sfreq);
        if block && steps == 0 {
            return Ok(());
        }
        let cols = if block { steps } else { 1 };
        let mut values = vec![0f32; s.rank * cols];
        for t in 0..steps {
            self.state = if s.flow { advance(&name, self.state, k, dt) } else { map(&name, self.state, k) };
            if !self.state.iter().all(|v| v.is_finite()) {
                self.stale = true;
                return Err(format!("{name} ran away with a={}, b={}, c={} — the constants left its basin", k[0], k[1], k[2]).into());
            }
            if block {
                for i in 0..s.rank {
                    values[i * cols + t] = (self.state[i] / scale) as f32;
                }
            }
        }
        if !block {
            for i in 0..s.rank {
                values[i] = (self.state[i] / scale) as f32;
            }
        }

        let names: Vec<Coord> = ["x", "y", "z"][..s.rank].iter().map(|n| Coord::Str((*n).into())).collect();
        let shape = if block { vec![s.rank, cols] } else { vec![s.rank] };
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

/// One RK4 step of a continuous system.
fn advance(name: &str, s: [f64; 3], k: [f64; 3], dt: f64) -> [f64; 3] {
    let add = |s: [f64; 3], d: [f64; 3], h: f64| [s[0] + h * d[0], s[1] + h * d[1], s[2] + h * d[2]];
    let k1 = flow(name, s, k);
    let k2 = flow(name, add(s, k1, dt / 2.0), k);
    let k3 = flow(name, add(s, k2, dt / 2.0), k);
    let k4 = flow(name, add(s, k3, dt), k);
    let mut out = s;
    for i in 0..3 {
        out[i] += dt / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
    }
    out
}

static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "attractor",
        name: "system",
        spec: ParamSpec::Str {
            default: "lorenz",
            options: &["lorenz", "rossler", "chua", "thomas", "aizawa", "halvorsen", "henon", "ikeda", "logistic", "standard"],
            refresh: false,
        },
        expression: None,
        doc: Some("Which system. The first six are flows integrated at `dt`; the last four are maps, which ignore it."),
    },
    ParamDecl {
        group: "attractor",
        name: "a",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 2.0 },
        expression: None,
        doc: Some("Scales the system's first canonical constant. 1 is the textbook figure."),
    },
    ParamDecl {
        group: "attractor",
        name: "b",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 2.0 },
        expression: None,
        doc: Some("Scales the second canonical constant; a system with only one ignores it."),
    },
    ParamDecl {
        group: "attractor",
        name: "c",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 2.0 },
        expression: None,
        doc: Some("Scales the third canonical constant; a system with fewer ignores it."),
    },
    ParamDecl {
        group: "sim",
        name: "dt",
        spec: ParamSpec::Float { default: 0.01, min: 1.0e-6, max: 0.1 },
        expression: None,
        doc: Some("Model seconds per step, for the flows. Too large and the integration leaves the attractor."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000, options: &[] },
        expression: None,
        doc: Some("Nudges the starting point. Negative takes a fresh one from the clock."),
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
        doc: Some("`value` emits the point now; `block` emits every step since the last frame, which is a signal."),
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 500.0, min: 1.0, max: 20_000.0 },
        expression: None,
        doc: Some("Steps per second of real time, and the sample rate of an emitted block."),
    },
    ParamDecl {
        group: "output",
        name: "normalize",
        spec: ParamSpec::Bool { default: true },
        expression: None,
        doc: Some("Divide by the system's own size, so every system reads roughly within -1 to 1 and modulates a param directly."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation],
    doc: "The classical chaotic systems.\n\
          Six flows and four maps, each on its own canonical constants — a source of motion that \
          never repeats and never jumps.",
    inputs: &[],
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Attractor, MANIFEST);
