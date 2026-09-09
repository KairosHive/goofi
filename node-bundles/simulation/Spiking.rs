//! Spiking — a network of spiking neurons with conduction delays, in Rust and with no dependency.
//! Neurons sit in space, wire preferentially to their neighbours, and a spike takes time to arrive.

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

/// Each model in its own units: where it rests, where it fires, and the current that means "one".
struct Cell {
    rest: f64,
    fire: f64,
    reset: f64,
    current: f64,
}

fn cell(model: &str) -> Cell {
    match model {
        "izhikevich" => Cell { rest: -65.0, fire: 30.0, reset: -65.0, current: 10.0 },
        "adex" => Cell { rest: -70.0, fire: 0.0, reset: -58.0, current: 50.0 },
        _ => Cell { rest: 0.0, fire: 1.0, reset: 0.0, current: 1.0 },
    }
}

/// The synapses leaving each neuron, so a spike scatters instead of every neuron polling.
#[derive(Default)]
struct Wiring {
    head: Vec<u32>,
    target: Vec<u32>,
    weight: Vec<f32>,
    delay: Vec<u16>,
}

#[derive(Default)]
struct Spiking {
    v: Vec<f64>,
    /// The adaptation variable; `lif` leaves it at zero.
    w: Vec<f64>,
    /// Milliseconds of refractory left.
    quiet: Vec<f64>,
    wiring: Wiring,
    /// Current arriving in the next `slots` steps, `[slots][n]`, read and cleared at its own step.
    pending: Vec<f32>,
    slots: usize,
    at: usize,
    rng: Rng,
    clock: Clock,
    /// The model the state belongs to; a different one is a different network.
    running: String,
    stale: bool,
}

impl Spiking {
    fn build(&mut self, n: usize, model: &str, dt_ms: f64, p: &Params<'_>) {
        let mut rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
        let dims = p.i64("network", "dims").unwrap_or(2).clamp(1, 3) as usize;
        let fan = p.i64("network", "fan_in").unwrap_or(12).clamp(1, 64) as usize;
        let inhibitory = p.f64("network", "inhibitory").unwrap_or(0.2).clamp(0.0, 1.0);
        let locality = p.f64("network", "locality").unwrap_or(4.0).max(0.0);
        let span = p.f64("network", "delay").unwrap_or(5.0).max(0.0);

        let pos: Vec<f64> = (0..n * dims).map(|_| rng.unit()).collect();
        let sign: Vec<f64> = (0..n).map(|_| if rng.unit() < inhibitory { -1.0 } else { 1.0 }).collect();
        let distance = |a: usize, b: usize| {
            (0..dims).map(|k| (pos[a * dims + k] - pos[b * dims + k]).powi(2)).sum::<f64>().sqrt()
        };

        self.slots = ((span / dt_ms).ceil() as usize + 2).min(512);
        let mut edges: Vec<Vec<(u32, f32, u16)>> = vec![Vec::new(); n];
        for target in 0..n {
            for _ in 0..fan {
                let mut source = target;
                // Rejection sampling: a near neighbour is accepted sooner, and 32 tries is a floor
                // on progress rather than a distribution — `locality` 0 makes it uniform anyway.
                for _ in 0..32 {
                    let pick = (rng.unit() * n as f64) as usize % n;
                    if pick == target {
                        continue;
                    }
                    source = pick;
                    if rng.unit() < (-locality * distance(target, pick)).exp() {
                        break;
                    }
                }
                if source == target {
                    continue;
                }
                let delay = ((distance(target, source) * span / dt_ms).round() as usize).clamp(1, self.slots - 1);
                edges[source].push((target as u32, (sign[source] * (0.5 + 0.5 * rng.unit())) as f32, delay as u16));
            }
        }

        self.wiring = Wiring::default();
        self.wiring.head.push(0);
        for out in &edges {
            for (target, weight, delay) in out {
                self.wiring.target.push(*target);
                self.wiring.weight.push(*weight);
                self.wiring.delay.push(*delay);
            }
            self.wiring.head.push(self.wiring.target.len() as u32);
        }

        let c = cell(model);
        self.v = (0..n).map(|_| c.rest + 0.1 * (c.fire - c.rest) * rng.unit()).collect();
        self.w = vec![0.0; n];
        self.quiet = vec![0.0; n];
        self.pending = vec![0.0; self.slots * n];
        self.at = 0;
        self.rng = rng;
        self.running = model.to_string();
        self.clock = Clock::default();
        self.stale = false;
    }
}

impl Node for Spiking {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let model = p.str("neuron", "model").unwrap_or("lif").to_string();
        let n = p.i64("network", "size").unwrap_or(200).clamp(2, 20_000) as usize;
        let dt = p.f64("sim", "dt").unwrap_or(0.0005).max(1e-6);
        let dt_ms = dt * 1000.0;
        if self.stale || self.running != model || self.v.len() != n {
            self.build(n, &model, dt_ms, p);
        }

        let unit = cell(&model);
        let fire = unit.rest + (unit.fire - unit.rest) * p.f64("neuron", "threshold").unwrap_or(1.0).max(0.05);
        let tau = p.f64("neuron", "tau").unwrap_or(20.0).max(0.1);
        let refractory = p.f64("neuron", "refractory").unwrap_or(2.0).max(0.0);
        let adaptation = p.f64("neuron", "adaptation").unwrap_or(1.0);
        let noise = p.f64("neuron", "noise").unwrap_or(0.05).max(0.0);
        let background = p.f64("neuron", "drive").unwrap_or(1.0);
        let synapse = p.f64("network", "weight").unwrap_or(1.0);
        let sfreq = p.f64("output", "sfreq").unwrap_or(2000.0).max(1.0);
        let block = p.str("output", "mode").unwrap_or("value") == "block";

        let external = inp.get("input").map(floats).transpose()?.map(|(_, v)| v).unwrap_or_default();
        if !external.is_empty() && external.len() != 1 && external.len() != n {
            return Err(format!("input is {} long; it must be one value or one per neuron ({n})", external.len()).into());
        }

        let steps = self.clock.due(c.now, sfreq);
        if block && steps == 0 {
            return Ok(());
        }
        let cols = if block { steps } else { 1 };
        let (mut potentials, mut spikes) = (vec![0f32; n * cols], vec![0f32; n * cols]);
        let mut rate = vec![0f32; cols];
        let mut fired = Vec::new();

        for t in 0..steps {
            let slot = self.at * n;
            fired.clear();
            for i in 0..n {
                let own = match external.len() {
                    0 => 0.0,
                    1 => external[0],
                    _ => external[i],
                };
                let arriving = self.pending[slot + i] as f64 * synapse;
                let current = unit.current * (background + own + noise * self.rng.signed()) + unit.current * arriving;
                if self.quiet[i] > 0.0 {
                    self.quiet[i] -= dt_ms;
                    continue;
                }
                let (dv, dw) = match model.as_str() {
                    "izhikevich" => {
                        let v = self.v[i];
                        (0.04 * v * v + 5.0 * v + 140.0 - self.w[i] + current, 0.02 * (0.2 * v - self.w[i]))
                    }
                    "adex" => {
                        let v = self.v[i];
                        let exponential = 2.0 * ((v + 50.0) / 2.0).min(20.0).exp();
                        ((-(v + 70.0) + exponential - self.w[i] + current) / tau, (2.0 * (v + 70.0) - self.w[i]) / 100.0)
                    }
                    _ => ((-self.v[i] + current) / tau, 0.0),
                };
                self.v[i] += dt_ms * dv;
                self.w[i] += dt_ms * dw;
                if self.v[i] >= fire {
                    self.v[i] = unit.reset;
                    self.w[i] += if model == "lif" { 0.0 } else { adaptation * 8.0 };
                    self.quiet[i] = refractory;
                    fired.push(i);
                }
            }

            for i in self.pending[slot..slot + n].iter_mut() {
                *i = 0.0;
            }
            for i in &fired {
                let (from, to) = (self.wiring.head[*i] as usize, self.wiring.head[*i + 1] as usize);
                for e in from..to {
                    let ahead = (self.at + self.wiring.delay[e] as usize) % self.slots;
                    self.pending[ahead * n + self.wiring.target[e] as usize] += self.wiring.weight[e];
                }
            }
            self.at = (self.at + 1) % self.slots;

            if block {
                for (i, v) in self.v.iter().enumerate() {
                    potentials[i * cols + t] = ((v - unit.rest) / (unit.fire - unit.rest)) as f32;
                }
                for i in &fired {
                    spikes[i * cols + t] = 1.0;
                }
                rate[t] = (fired.len() as f64 / (n as f64 * dt)) as f32;
            }
        }
        if !block {
            for (i, v) in self.v.iter().enumerate() {
                potentials[i] = ((v - unit.rest) / (unit.fire - unit.rest)) as f32;
            }
            for i in &fired {
                spikes[*i] = 1.0;
            }
            rate[0] = (fired.len() as f64 / (n as f64 * dt)) as f32;
        }

        let names: Vec<Coord> = (0..n).map(|i| Coord::Str(format!("n{i}").into())).collect();
        let shape = if block { vec![n, cols] } else { vec![n] };
        let meta = Meta::new().with_sfreq(block.then_some(sfreq)).with_channels(Axes::new().with(0, Axis::coords(names)));
        out.set("potentials", Data::array_f32(shape.clone(), bytes(&potentials), meta.clone()).map_err(|e| e.to_string())?);
        out.set("spikes", Data::array_f32(shape, bytes(&spikes), meta).map_err(|e| e.to_string())?);
        let scalar = Meta::new().with_sfreq(block.then_some(sfreq));
        out.set("rate", Data::array_f32(vec![cols], bytes(&rate), scalar).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        // The wiring is drawn once: a new topology, a new count or a new seed is a new network.
        // `sim.dt` is in here too, because the delay ring is sized in steps and a new step is a
        // new ring.
        if (key.group == "network" && key.name != "weight") || key.group == "sim" {
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
    &[SlotDecl { name: "input", kind: SlotType::Array, trigger_process: false, multi: false, required: false }];

static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "potentials", kind: SlotType::Array },
    OutputDecl { name: "spikes", kind: SlotType::Array },
    OutputDecl { name: "rate", kind: SlotType::Array },
];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "neuron",
        name: "model",
        spec: ParamSpec::Str { default: "lif", options: &["lif", "izhikevich", "adex"], refresh: false },
        expression: None,
        doc: Some(
            "`lif` leaks and fires, `izhikevich` bursts and chatters, `adex` adapts. \
             `potentials` is reported the same way for all three: 0 at rest, 1 at threshold.",
        ),
    },
    ParamDecl {
        group: "neuron",
        name: "drive",
        spec: ParamSpec::Float { default: 1.0, min: -2.0, max: 10.0 },
        expression: None,
        doc: Some("Background current every neuron gets. Around 1 is enough to fire; below it the network needs its input."),
    },
    ParamDecl {
        group: "neuron",
        name: "threshold",
        spec: ParamSpec::Float { default: 1.0, min: 0.05, max: 3.0 },
        expression: None,
        doc: Some("Scales the firing threshold. Lower makes the network twitchier."),
    },
    ParamDecl {
        group: "neuron",
        name: "tau",
        spec: ParamSpec::Float { default: 20.0, min: 0.1, max: 200.0 },
        expression: None,
        doc: Some("Membrane time constant in milliseconds. `izhikevich` sets its own."),
    },
    ParamDecl {
        group: "neuron",
        name: "refractory",
        spec: ParamSpec::Float { default: 2.0, min: 0.0, max: 50.0 },
        expression: None,
        doc: Some("Milliseconds a neuron stays silent after it fires, which caps its rate."),
    },
    ParamDecl {
        group: "neuron",
        name: "adaptation",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("How much each spike tires the neuron. `lif` has no adaptation and ignores it."),
    },
    ParamDecl {
        group: "neuron",
        name: "noise",
        spec: ParamSpec::Float { default: 0.05, min: 0.0, max: 2.0 },
        expression: None,
        doc: Some("Jitter on the current, which is what keeps a quiet network from being exactly still."),
    },
    ParamDecl {
        group: "network",
        name: "size",
        spec: ParamSpec::Int { default: 200, min: 2, max: 20_000, options: &[] },
        expression: None,
        doc: Some("How many neurons. Cost grows with this times `fan_in`."),
    },
    ParamDecl {
        group: "network",
        name: "fan_in",
        spec: ParamSpec::Int { default: 12, min: 1, max: 64, options: &[] },
        expression: None,
        doc: Some("How many neurons each one listens to."),
    },
    ParamDecl {
        group: "network",
        name: "dims",
        spec: ParamSpec::Int { default: 2, min: 1, max: 3, options: &[] },
        expression: None,
        doc: Some("How many dimensions the neurons are laid out in, which is what distance and delay are measured in."),
    },
    ParamDecl {
        group: "network",
        name: "locality",
        spec: ParamSpec::Float { default: 4.0, min: 0.0, max: 30.0 },
        expression: None,
        doc: Some("How strongly a neuron prefers its neighbours. 0 wires the network at random."),
    },
    ParamDecl {
        group: "network",
        name: "inhibitory",
        spec: ParamSpec::Float { default: 0.2, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("Fraction of neurons whose every synapse subtracts instead of adds."),
    },
    ParamDecl {
        group: "network",
        name: "weight",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 10.0 },
        expression: None,
        doc: Some("Gain on every synapse. This is the knob between a silent network and a seizing one."),
    },
    ParamDecl {
        group: "network",
        name: "delay",
        spec: ParamSpec::Float { default: 5.0, min: 0.0, max: 100.0 },
        expression: None,
        doc: Some("Milliseconds a spike takes to cross the whole layout. Distance sets each synapse's share of it."),
    },
    ParamDecl {
        group: "sim",
        name: "dt",
        spec: ParamSpec::Float { default: 0.0005, min: 1.0e-6, max: 0.01 },
        expression: None,
        doc: Some("Model seconds per step. Half a millisecond suits all three models."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000, options: &[] },
        expression: None,
        doc: Some("Seeds the layout, the wiring and the noise. Negative takes a fresh one from the clock."),
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Wire the network again from the seed."),
    },
    ParamDecl {
        group: "output",
        name: "mode",
        spec: ParamSpec::Str { default: "value", options: &["value", "block"], refresh: false },
        expression: None,
        doc: Some("`value` emits the state now; `block` emits every step since the last frame, which is a spike raster."),
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 2000.0, min: 1.0, max: 20_000.0 },
        expression: None,
        doc: Some("Steps per second of real time. At the default `sim.dt` this runs the network in real time."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation, Tag::Eeg],
    doc: "A spiking network laid out in space, with conduction delays.\n\
          Neurons wire to their neighbours, a spike takes time to arrive, and `weight` is the knob \
          between silence and a storm.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Spiking, MANIFEST);
