//! Population — the textbook ecology and epidemic models: predator and prey, an outbreak running
//! its course, and a single population folding into chaos.

use goofi_core::{Axes, Axis, Coord, Data, Meta, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, Tag};

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

/// What each model tracks, and where it starts.
fn start(model: &str) -> (&'static [&'static str], Vec<f64>) {
    match model {
        "rosenzweig" => (&["prey", "predator"], vec![0.5, 0.2]),
        "sir" => (&["susceptible", "infected", "recovered"], vec![0.99, 0.01, 0.0]),
        "seir" => (&["susceptible", "exposed", "infected", "recovered"], vec![0.99, 0.0, 0.01, 0.0]),
        "ricker" => (&["population"], vec![0.1]),
        _ => (&["prey", "predator"], vec![1.0, 0.5]),
    }
}

/// One rate, per model. `ricker` is a map and its "rate" is the whole next value.
struct Rates {
    growth: f64,
    capacity: f64,
    predation: f64,
    efficiency: f64,
    mortality: f64,
    handling: f64,
    incubation: f64,
}

fn flow(model: &str, s: &[f64], r: &Rates) -> Vec<f64> {
    match model {
        "rosenzweig" => {
            let (x, y) = (s[0], s[1]);
            let eaten = r.predation * x / (1.0 + r.predation * r.handling * x);
            vec![r.growth * x * (1.0 - x / r.capacity.max(1e-9)) - eaten * y, r.efficiency * eaten * y - r.mortality * y]
        }
        "sir" => {
            let (sus, inf) = (s[0], s[1]);
            vec![-r.predation * sus * inf, r.predation * sus * inf - r.mortality * inf, r.mortality * inf]
        }
        "seir" => {
            let (sus, exp, inf) = (s[0], s[1], s[2]);
            vec![
                -r.predation * sus * inf,
                r.predation * sus * inf - r.incubation * exp,
                r.incubation * exp - r.mortality * inf,
                r.mortality * inf,
            ]
        }
        _ => {
            let (x, y) = (s[0], s[1]);
            vec![r.growth * x - r.predation * x * y, r.efficiency * r.predation * x * y - r.mortality * y]
        }
    }
}

#[derive(Default)]
struct Population {
    state: Vec<f64>,
    running: String,
    clock: Clock,
    stale: bool,
}

impl Node for Population {
    fn process(&mut self, _inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let model = p.str("population", "model").unwrap_or("lotkavolterra").to_string();
        let (names, first) = start(&model);
        if self.stale || self.running != model {
            self.state = first;
            self.running = model.clone();
            self.clock = Clock::default();
            self.stale = false;
        }

        let r = Rates {
            growth: p.f64("population", "growth").unwrap_or(1.0),
            capacity: p.f64("population", "capacity").unwrap_or(1.0),
            predation: p.f64("population", "predation").unwrap_or(1.0),
            efficiency: p.f64("population", "efficiency").unwrap_or(0.5),
            mortality: p.f64("population", "mortality").unwrap_or(0.5),
            handling: p.f64("population", "handling").unwrap_or(0.5),
            incubation: p.f64("population", "incubation").unwrap_or(0.2),
        };
        let dt = p.f64("sim", "dt").unwrap_or(0.01).max(1e-6);
        let sfreq = p.f64("output", "sfreq").unwrap_or(200.0).max(1.0);
        let block = p.str("output", "mode").unwrap_or("value") == "block";

        let steps = self.clock.due(c.now, sfreq);
        if block && steps == 0 {
            return Ok(());
        }
        let rank = self.state.len();
        let cols = if block { steps } else { 1 };
        let mut values = vec![0f32; rank * cols];
        for t in 0..steps {
            if model == "ricker" {
                let n = self.state[0];
                self.state[0] = (n * (r.growth * (1.0 - n / r.capacity.max(1e-9))).exp()).clamp(0.0, 1.0e6);
            } else {
                let d = flow(&model, &self.state, &r);
                for (v, d) in self.state.iter_mut().zip(&d) {
                    *v = (*v + dt * d).max(0.0);
                }
            }
            if !self.state.iter().all(|v| v.is_finite()) {
                self.stale = true;
                return Err(format!("{model} ran away — lower the rates or `sim.dt`").into());
            }
            if block {
                for (i, v) in self.state.iter().enumerate() {
                    values[i * cols + t] = *v as f32;
                }
            }
        }
        if !block {
            for (i, v) in self.state.iter().enumerate() {
                values[i] = *v as f32;
            }
        }

        let coords: Vec<Coord> = names.iter().map(|n| Coord::Str((*n).into())).collect();
        let shape = if block { vec![rank, cols] } else { vec![rank] };
        let meta = Meta::new().with_sfreq(block.then_some(sfreq)).with_channels(Axes::new().with(0, Axis::coords(coords)));
        out.set("out", Data::array_f32(shape, bytes(&values), meta).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_pulse(&mut self, _key: &ParamKey, _p: &Params<'_>) -> NodeResult {
        self.stale = true;
        Ok(())
    }
}

static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "population",
        name: "model",
        spec: ParamSpec::Str {
            default: "lotkavolterra",
            options: &["lotkavolterra", "rosenzweig", "sir", "seir", "ricker"],
            refresh: false,
        },
        expression: None,
        doc: Some("Predator and prey, the same with a carrying capacity, two epidemics, or one population as a map."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "population",
        name: "growth",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 4.0 },
        expression: None,
        doc: Some("How fast the prey or the population grows. `ricker` folds into chaos above about 2.7."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "population",
        name: "capacity",
        spec: ParamSpec::Float { default: 1.0, min: 0.01, max: 10.0 },
        expression: None,
        doc: Some("How much the environment holds. `rosenzweig` and `ricker` read it."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "population",
        name: "predation",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 10.0 },
        expression: None,
        doc: Some("Attack rate for the predator models, and the infection rate for the epidemics."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "population",
        name: "efficiency",
        spec: ParamSpec::Float { default: 0.5, min: 0.0, max: 2.0 },
        expression: None,
        doc: Some("How much of what is eaten becomes predator. The epidemics ignore it."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "population",
        name: "mortality",
        spec: ParamSpec::Float { default: 0.5, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("Predator death rate, and the recovery rate for the epidemics."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "population",
        name: "handling",
        spec: ParamSpec::Float { default: 0.5, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("How long a predator spends on each catch, which is what saturates it. `rosenzweig` alone reads it."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "population",
        name: "incubation",
        spec: ParamSpec::Float { default: 0.2, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("How fast the exposed become infectious. `seir` alone reads it."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "sim",
        name: "dt",
        spec: ParamSpec::Float { default: 0.01, min: 1.0e-6, max: 0.5 },
        expression: None,
        doc: Some("Model seconds per step. `ricker` is a map and ignores it."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Start the model over."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "output",
        name: "mode",
        spec: ParamSpec::Str { default: "value", options: &["value", "block"], refresh: false },
        expression: None,
        doc: Some("`value` emits the state now; `block` emits every step since the last frame, which is a signal."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 200.0, min: 1.0, max: 10_000.0 },
        expression: None,
        doc: Some("Steps per second of real time, and the sample rate of an emitted block."),
        section: 0,
        show: None,
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation],
    doc: "Populations that rise, crash and cycle.\n\
          Predator and prey, an epidemic running its course, and the map that folds one population \
          into chaos.",
    inputs: &[],
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Population, MANIFEST);
