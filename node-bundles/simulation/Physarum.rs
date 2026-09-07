//! Physarum — slime-mould agents that follow their own trail. Each senses ahead, turns towards
//! the strongest scent, and leaves more behind; the trail spreads and fades. Networks grow.

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

#[derive(Default)]
struct Physarum {
    /// Agent position and heading: `x`, `y` in cells, `heading` in radians.
    x: Vec<f32>,
    y: Vec<f32>,
    heading: Vec<f32>,
    /// The scent field, row-major `side * side`, row 0 at the top as everywhere else in goofi.
    trail: Vec<f32>,
    scratch: Vec<f32>,
    side: usize,
    rng: Rng,
    clock: Clock,
    stale: bool,
}

impl Physarum {
    fn reseed(&mut self, agents: usize, side: usize, p: &Params<'_>) {
        let mut rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
        let half = side as f64 / 2.0;
        // Started on a disc rather than the whole field: a ring is what grows the first network.
        self.x = (0..agents)
            .map(|_| {
                let (r, a) = (rng.unit().sqrt() * half * 0.4, rng.unit() * std::f64::consts::TAU);
                (half + r * a.cos()) as f32
            })
            .collect();
        self.y = (0..agents)
            .map(|_| {
                let (r, a) = (rng.unit().sqrt() * half * 0.4, rng.unit() * std::f64::consts::TAU);
                (half + r * a.sin()) as f32
            })
            .collect();
        self.heading = (0..agents).map(|_| (rng.unit() * std::f64::consts::TAU) as f32).collect();
        self.trail = vec![0.0; side * side];
        self.scratch = vec![0.0; side * side];
        self.side = side;
        self.rng = rng;
        self.clock = Clock::default();
        self.stale = false;
    }

    /// The scent under a point, wrapped, so the field has no edge for an agent to pile up on.
    fn scent(&self, x: f32, y: f32) -> f32 {
        let side = self.side as i64;
        let col = (x.round() as i64).rem_euclid(side) as usize;
        let row = (y.round() as i64).rem_euclid(side) as usize;
        self.trail[row * self.side + col]
    }
}

impl Node for Physarum {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let agents = p.i64("physarum", "agents").unwrap_or(5000).clamp(1, 200_000) as usize;
        let side = p.i64("physarum", "size").unwrap_or(128).clamp(16, 512) as usize;
        if self.stale || self.heading.len() != agents || self.side != side {
            self.reseed(agents, side, p);
        }

        let speed = p.f64("physarum", "speed").unwrap_or(1.0) as f32;
        let sensor = p.f64("physarum", "sensor_distance").unwrap_or(6.0) as f32;
        let angle = p.f64("physarum", "sensor_angle").unwrap_or(0.6) as f32;
        let turn = p.f64("physarum", "turn").unwrap_or(0.5) as f32;
        let deposit = p.f64("physarum", "deposit").unwrap_or(1.0) as f32;
        let decay = p.f64("physarum", "decay").unwrap_or(0.08).clamp(0.0, 1.0) as f32;
        let diffuse = p.f64("physarum", "diffuse").unwrap_or(0.5).clamp(0.0, 1.0) as f32;
        let wander = p.f64("physarum", "wander").unwrap_or(0.1) as f32;
        let sfreq = p.f64("output", "sfreq").unwrap_or(30.0).max(1.0);

        // A wired field is added to the trail before the agents read it, so a biosignal is scent.
        if let Some((shape, v)) = inp.get("attract").map(floats).transpose()? {
            if shape.iter().product::<usize>() != side * side {
                return Err(format!("attract is {shape:?}; it must hold {side} by {side} cells").into());
            }
            for (cell, add) in self.trail.iter_mut().zip(&v) {
                *cell += *add as f32;
            }
        }

        let steps = self.clock.due(c.now, sfreq);
        for _ in 0..steps {
            for i in 0..agents {
                let (x, y, h) = (self.x[i], self.y[i], self.heading[i]);
                let look = |a: f32| self.scent(x + sensor * (h + a).cos(), y + sensor * (h + a).sin());
                let (left, ahead, right) = (look(-angle), look(0.0), look(angle));
                let jitter = (self.rng.unit() as f32 - 0.5) * wander;
                self.heading[i] = h + jitter
                    + if ahead >= left && ahead >= right {
                        0.0
                    } else if left > right {
                        -turn
                    } else {
                        turn
                    };
                let side_f = side as f32;
                self.x[i] = (x + speed * self.heading[i].cos()).rem_euclid(side_f);
                self.y[i] = (y + speed * self.heading[i].sin()).rem_euclid(side_f);
                let col = (self.x[i] as usize).min(side - 1);
                let row = (self.y[i] as usize).min(side - 1);
                self.trail[row * side + col] += deposit;
            }

            for row in 0..side {
                for col in 0..side {
                    let mut sum = 0.0;
                    for dr in 0..3 {
                        for dc in 0..3 {
                            let r = (row + side + dr - 1) % side;
                            let cc = (col + side + dc - 1) % side;
                            sum += self.trail[r * side + cc];
                        }
                    }
                    let here = self.trail[row * side + col];
                    self.scratch[row * side + col] = (here + diffuse * (sum / 9.0 - here)) * (1.0 - decay);
                }
            }
            std::mem::swap(&mut self.trail, &mut self.scratch);
        }

        let mut positions = Vec::with_capacity(agents * 2);
        for i in 0..agents {
            positions.push(self.x[i] / side as f32);
            positions.push(self.y[i] / side as f32);
        }
        let field = Meta::new();
        out.set("trail", Data::array_f32(vec![side, side], bytes(&self.trail), field).map_err(|e| e.to_string())?);
        let axis = vec![Coord::Str("x".into()), Coord::Str("y".into())];
        let meta = Meta::new().with_channels(Axes::new().with(1, Axis::coords(axis)));
        out.set("positions", Data::array_f32(vec![agents, 2], bytes(&positions), meta).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        if matches!(key.name.as_str(), "seed" | "agents" | "size") {
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
    &[SlotDecl { name: "attract", kind: SlotType::Array, trigger_process: false, multi: false, required: false }];

static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "trail", kind: SlotType::Array },
    OutputDecl { name: "positions", kind: SlotType::Array },
];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "physarum",
        name: "agents",
        spec: ParamSpec::Int { default: 5000, min: 1, max: 200_000 },
        expression: None,
        doc: Some("How many agents crawl the field."),
    },
    ParamDecl {
        group: "physarum",
        name: "size",
        spec: ParamSpec::Int { default: 128, min: 16, max: 512 },
        expression: None,
        doc: Some("Width and height of the scent field in cells. The cost of spreading it grows with the square."),
    },
    ParamDecl {
        group: "physarum",
        name: "speed",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 8.0 },
        expression: None,
        doc: Some("Cells an agent moves each step."),
    },
    ParamDecl {
        group: "physarum",
        name: "sensor_distance",
        spec: ParamSpec::Float { default: 6.0, min: 0.5, max: 64.0 },
        expression: None,
        doc: Some("How far ahead an agent smells. Larger makes coarser, straighter veins."),
    },
    ParamDecl {
        group: "physarum",
        name: "sensor_angle",
        spec: ParamSpec::Float { default: 0.6, min: 0.0, max: 1.6 },
        expression: None,
        doc: Some("How wide apart the left and right senses are, in radians."),
    },
    ParamDecl {
        group: "physarum",
        name: "turn",
        spec: ParamSpec::Float { default: 0.5, min: 0.0, max: 1.6 },
        expression: None,
        doc: Some("How sharply an agent turns towards the stronger side, in radians per step."),
    },
    ParamDecl {
        group: "physarum",
        name: "deposit",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 10.0 },
        expression: None,
        doc: Some("How much scent an agent leaves where it lands."),
    },
    ParamDecl {
        group: "physarum",
        name: "decay",
        spec: ParamSpec::Float { default: 0.08, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("How much of the field fades each step. This is what stops the network filling in."),
    },
    ParamDecl {
        group: "physarum",
        name: "diffuse",
        spec: ParamSpec::Float { default: 0.5, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("How much the scent spreads to its neighbours each step."),
    },
    ParamDecl {
        group: "physarum",
        name: "wander",
        spec: ParamSpec::Float { default: 0.1, min: 0.0, max: 2.0 },
        expression: None,
        doc: Some("Random turn added every step, which is what breaks a symmetric field."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000 },
        expression: None,
        doc: Some("Seeds where the agents start and how they wander. Negative takes a fresh one from the clock."),
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Clear the field and scatter the agents again."),
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 30.0, min: 1.0, max: 240.0 },
        expression: None,
        doc: Some("Steps per second of real time. The field is a picture, so there is no block mode."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation, Tag::Image],
    doc: "Slime-mould agents growing a transport network.\n\
          Each follows the trail it and the others left; `trail` is a field an image viewer or \
          `ArrayIn` draws directly.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Physarum, MANIFEST);
