//! Swarm — N particles, a rule between every pair, and one integrator. Flocking, self-propelled
//! alignment, gravity, oscillators that swarm, and asymmetric attraction between colours.

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

fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Whether the box wraps for this model. Gravity and the swarmalators need real distance, so they
/// bounce off the walls instead.
fn wraps(model: &str) -> bool {
    !matches!(model, "gravity" | "swarmalators")
}

/// The knobs one step reads, gathered so the pair loop takes one argument.
struct Rules {
    model: String,
    dims: usize,
    radius: f64,
    separation: f64,
    alignment: f64,
    cohesion: f64,
    coupling: f64,
    speed: f64,
    noise: f64,
    friction: f64,
}

#[derive(Default)]
struct Swarm {
    pos: Vec<f64>,
    vel: Vec<f64>,
    phase: Vec<f64>,
    species: Vec<usize>,
    /// The attraction between every pair of species, drawn when nothing is wired.
    drawn: Vec<f64>,
    kinds: usize,
    running: String,
    rng: Rng,
    clock: Clock,
    stale: bool,
}

impl Swarm {
    fn reseed(&mut self, n: usize, dims: usize, kinds: usize, p: &Params<'_>) {
        let mut rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
        self.pos = (0..n * dims).map(|_| rng.unit()).collect();
        self.vel = (0..n * dims).map(|_| 0.1 * rng.signed()).collect();
        self.phase = (0..n).map(|_| rng.unit() * std::f64::consts::TAU).collect();
        self.species = (0..n).map(|i| i % kinds).collect();
        self.drawn = (0..kinds * kinds).map(|_| rng.signed()).collect();
        self.kinds = kinds;
        self.rng = rng;
        self.clock = Clock::default();
        self.stale = false;
    }

    /// The acceleration on every particle, and the phase change beside it.
    fn forces(&self, r: &Rules, matrix: &[f64], accel: &mut [f64], dphase: &mut [f64]) {
        let n = self.phase.len();
        let d = r.dims;
        accel.fill(0.0);
        dphase.fill(0.0);
        let mut offset = vec![0.0; d];
        for i in 0..n {
            let (mut near, mut centre, mut heading) = (0.0, vec![0.0; d], vec![0.0; d]);
            for j in 0..n {
                if i == j {
                    continue;
                }
                let mut dist2 = 0.0;
                for (k, o) in offset.iter_mut().enumerate() {
                    let mut delta = self.pos[j * d + k] - self.pos[i * d + k];
                    if wraps(&r.model) {
                        // One box, seen through its nearest image: a flock must not tear at the seam.
                        delta -= delta.round();
                    }
                    *o = delta;
                    dist2 += delta * delta;
                }
                let dist = dist2.sqrt().max(1e-6);
                match r.model.as_str() {
                    "vicsek" => {
                        if dist < r.radius {
                            near += 1.0;
                            for (k, h) in heading.iter_mut().enumerate() {
                                *h += self.vel[j * d + k];
                            }
                        }
                    }
                    "gravity" => {
                        let pull = r.coupling * 1e-3 / (dist2 + r.radius * r.radius).powf(1.5);
                        for (k, a) in accel[i * d..(i + 1) * d].iter_mut().enumerate() {
                            *a += pull * offset[k];
                        }
                    }
                    "swarmalators" => {
                        let attract = (1.0 + r.cohesion * (self.phase[j] - self.phase[i]).cos()) / dist;
                        let repel = r.separation / dist2;
                        for (k, a) in accel[i * d..(i + 1) * d].iter_mut().enumerate() {
                            *a += (attract - repel) * offset[k] / dist * 0.1;
                        }
                        dphase[i] += r.coupling * (self.phase[j] - self.phase[i]).sin() / dist;
                    }
                    "particlelife" => {
                        let scaled = dist / r.radius.max(1e-6);
                        if scaled >= 1.0 {
                            continue;
                        }
                        let core = r.separation.clamp(0.05, 0.9);
                        let strength = if scaled < core {
                            scaled / core - 1.0
                        } else {
                            let pair = matrix[self.species[i] * self.kinds + self.species[j]];
                            pair * (1.0 - (2.0 * scaled - 1.0 - core).abs() / (1.0 - core))
                        };
                        for (k, a) in accel[i * d..(i + 1) * d].iter_mut().enumerate() {
                            *a += strength * offset[k] / dist;
                        }
                    }
                    _ => {
                        if dist < r.radius {
                            near += 1.0;
                            for k in 0..d {
                                centre[k] += offset[k];
                                heading[k] += self.vel[j * d + k];
                                accel[i * d + k] -= r.separation * offset[k] / (dist * dist);
                            }
                        }
                    }
                }
            }
            if near > 0.0 {
                for k in 0..d {
                    if r.model == "vicsek" {
                        accel[i * d + k] += r.alignment * (heading[k] / near - self.vel[i * d + k]);
                    } else {
                        accel[i * d + k] += r.cohesion * centre[k] / near;
                        accel[i * d + k] += r.alignment * (heading[k] / near - self.vel[i * d + k]);
                    }
                }
            }
        }
    }
}

impl Node for Swarm {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let model = p.str("swarm", "model").unwrap_or("boids").to_string();
        let n = p.i64("swarm", "count").unwrap_or(300).clamp(2, 2000) as usize;
        let dims = p.i64("swarm", "dims").unwrap_or(2).clamp(2, 3) as usize;
        let wired = inp.get("attraction").map(floats).transpose()?;
        if let Some((shape, _)) = &wired {
            if shape.len() != 2 || shape[0] != shape[1] {
                return Err(format!("attraction needs a square matrix, got {shape:?}").into());
            }
        }
        let kinds = match &wired {
            Some((shape, _)) => shape[0].max(1),
            None => p.i64("swarm", "species").unwrap_or(4).clamp(1, 16) as usize,
        };
        if self.stale || self.phase.len() != n || self.pos.len() != n * dims || self.kinds != kinds || self.running != model {
            self.reseed(n, dims, kinds, p);
            self.running = model.clone();
        }

        let r = Rules {
            model: model.clone(),
            dims,
            radius: p.f64("swarm", "radius").unwrap_or(0.08).max(1e-4),
            separation: p.f64("swarm", "separation").unwrap_or(0.3),
            alignment: p.f64("swarm", "alignment").unwrap_or(1.0),
            cohesion: p.f64("swarm", "cohesion").unwrap_or(1.0),
            coupling: p.f64("swarm", "coupling").unwrap_or(1.0),
            speed: p.f64("swarm", "speed").unwrap_or(0.3).max(0.0),
            noise: p.f64("swarm", "noise").unwrap_or(0.05).max(0.0),
            friction: p.f64("swarm", "friction").unwrap_or(0.1).clamp(0.0, 1.0),
        };
        let matrix = wired.as_ref().map(|(_, m)| m.as_slice()).unwrap_or(&self.drawn);
        let dt = p.f64("sim", "dt").unwrap_or(0.02).max(1e-6);
        let sfreq = p.f64("output", "sfreq").unwrap_or(60.0).max(1.0);

        let steps = self.clock.due(c.now, sfreq);
        let mut accel = vec![0.0; n * dims];
        let mut dphase = vec![0.0; n];
        for _ in 0..steps {
            self.forces(&r, matrix, &mut accel, &mut dphase);
            for i in 0..n {
                for k in 0..dims {
                    let at = i * dims + k;
                    self.vel[at] += dt * (accel[at] - r.friction * self.vel[at] + r.noise * self.rng.signed());
                }
                // Vicsek moves at a FIXED speed; every other model only has a ceiling.
                let speed = (0..dims).map(|k| self.vel[i * dims + k].powi(2)).sum::<f64>().sqrt();
                let fixed = r.model == "vicsek";
                if speed > 1e-9 && r.speed > 0.0 && (fixed || speed > r.speed) {
                    for k in 0..dims {
                        self.vel[i * dims + k] *= r.speed / speed;
                    }
                }
                self.phase[i] = (self.phase[i] + dt * dphase[i] / n as f64).rem_euclid(std::f64::consts::TAU);
                for k in 0..dims {
                    let at = i * dims + k;
                    self.pos[at] += dt * self.vel[at];
                    if wraps(&r.model) {
                        self.pos[at] = self.pos[at].rem_euclid(1.0);
                    } else if !(0.0..=1.0).contains(&self.pos[at]) {
                        self.pos[at] = self.pos[at].clamp(0.0, 1.0);
                        self.vel[at] = -self.vel[at];
                    }
                }
            }
            if !self.pos.iter().all(|v| v.is_finite()) {
                self.stale = true;
                return Err(format!("{model} ran away — lower `sim.dt` or the interaction strengths").into());
            }
        }

        let mut mean = vec![0.0; dims];
        for i in 0..n {
            let speed = (0..dims).map(|k| self.vel[i * dims + k].powi(2)).sum::<f64>().sqrt().max(1e-12);
            for (k, m) in mean.iter_mut().enumerate() {
                *m += self.vel[i * dims + k] / speed;
            }
        }
        let order = (mean.iter().map(|m| m * m).sum::<f64>().sqrt() / n as f64) as f32;

        let axis = ["x", "y", "z"][..dims].iter().map(|n| Coord::Str((*n).into())).collect::<Vec<_>>();
        let meta = Meta::new().with_channels(Axes::new().with(1, Axis::coords(axis)));
        let cast = |v: &[f64]| bytes(&v.iter().map(|x| *x as f32).collect::<Vec<f32>>());
        out.set("positions", Data::array_f32(vec![n, dims], cast(&self.pos), meta.clone()).map_err(|e| e.to_string())?);
        out.set("velocities", Data::array_f32(vec![n, dims], cast(&self.vel), meta).map_err(|e| e.to_string())?);
        out.set("phases", Data::array_f32(vec![n], cast(&self.phase), Meta::new()).map_err(|e| e.to_string())?);
        out.set("order", Data::array_f32(vec![1], bytes(&[order]), Meta::new()).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        if matches!(key.name.as_str(), "seed" | "count" | "dims" | "species") {
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
    &[SlotDecl { name: "attraction", kind: SlotType::Array, trigger_process: false, multi: false, required: false }];

static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "positions", kind: SlotType::Array },
    OutputDecl { name: "velocities", kind: SlotType::Array },
    OutputDecl { name: "phases", kind: SlotType::Array },
    OutputDecl { name: "order", kind: SlotType::Array },
];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "swarm",
        name: "model",
        spec: ParamSpec::Str {
            default: "boids",
            options: &["boids", "vicsek", "gravity", "swarmalators", "particlelife"],
            refresh: false,
        },
        expression: None,
        doc: Some(
            "`boids` flocks, `vicsek` aligns and nothing else, `gravity` pulls, `swarmalators` \
             couples position to phase, `particlelife` reads a matrix of attractions between colours.",
        ),
    },
    ParamDecl {
        group: "swarm",
        name: "count",
        spec: ParamSpec::Int { default: 300, min: 2, max: 2000 },
        expression: None,
        doc: Some("How many particles. Every pair is considered, so the cost grows with the square."),
    },
    ParamDecl {
        group: "swarm",
        name: "dims",
        spec: ParamSpec::Int { default: 2, min: 2, max: 3 },
        expression: None,
        doc: Some("Two dimensions or three. Positions always live in the unit box."),
    },
    ParamDecl {
        group: "swarm",
        name: "radius",
        spec: ParamSpec::Float { default: 0.08, min: 0.001, max: 1.0 },
        expression: None,
        doc: Some("How far a particle sees, as a fraction of the box. `gravity` reads it as its softening length."),
    },
    ParamDecl {
        group: "swarm",
        name: "separation",
        spec: ParamSpec::Float { default: 0.3, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("How hard a particle pushes off its neighbours. `particlelife` reads it as the size of the repelling core."),
    },
    ParamDecl {
        group: "swarm",
        name: "alignment",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("How much a particle matches its neighbours' heading. `boids` and `vicsek` read it."),
    },
    ParamDecl {
        group: "swarm",
        name: "cohesion",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("Pull towards the neighbours' centre. `swarmalators` reads it as how much phase decides attraction."),
    },
    ParamDecl {
        group: "swarm",
        name: "coupling",
        spec: ParamSpec::Float { default: 1.0, min: -5.0, max: 5.0 },
        expression: None,
        doc: Some("Phase coupling for `swarmalators` — negative splits them by phase — and the gravitational constant for `gravity`."),
    },
    ParamDecl {
        group: "swarm",
        name: "species",
        spec: ParamSpec::Int { default: 4, min: 1, max: 16 },
        expression: None,
        doc: Some("How many colours `particlelife` deals out. A wired `attraction` matrix decides instead."),
    },
    ParamDecl {
        group: "swarm",
        name: "speed",
        spec: ParamSpec::Float { default: 0.3, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("Speed limit, and the fixed speed `vicsek` moves at."),
    },
    ParamDecl {
        group: "swarm",
        name: "friction",
        spec: ParamSpec::Float { default: 0.1, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("How much of the velocity is lost each step. `particlelife` needs some to settle."),
    },
    ParamDecl {
        group: "swarm",
        name: "noise",
        spec: ParamSpec::Float { default: 0.05, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("Jitter on the motion. In `vicsek` this is the knob the order-disorder transition sits on."),
    },
    ParamDecl {
        group: "sim",
        name: "dt",
        spec: ParamSpec::Float { default: 0.02, min: 1.0e-4, max: 0.5 },
        expression: None,
        doc: Some("Model seconds per step."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000 },
        expression: None,
        doc: Some("Seeds the layout, the colours and the drawn attraction matrix. Negative takes a fresh one from the clock."),
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Scatter the particles again from the seed."),
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 60.0, min: 1.0, max: 1000.0 },
        expression: None,
        doc: Some("Steps per second of real time. Particle state is a snapshot, so there is no block mode."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation],
    doc: "Particles with a rule between every pair.\n\
          Flocking, alignment, gravity, oscillators that swarm, and asymmetric attraction between \
          colours — one integrator, five rules.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Swarm, MANIFEST);
