//! Boolean — Kauffman's random Boolean network. Every unit reads K others through a truth table
//! drawn once; `connections` is the knob the edge of chaos sits on, at about 2.

use goofi_core::{Axes, Axis, Coord, Data, Meta, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, Tag};

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

fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[derive(Default)]
struct Boolean {
    state: Vec<bool>,
    /// Which units each one reads, row-major `size * connections`.
    wired: Vec<u32>,
    /// One truth table per unit as a bitmask over its `2^connections` input combinations.
    table: Vec<u64>,
    fan: usize,
    rng: Rng,
    clock: Clock,
    stale: bool,
}

impl Boolean {
    fn reseed(&mut self, n: usize, fan: usize, p: &Params<'_>) {
        let mut rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
        let bias = p.f64("boolean", "bias").unwrap_or(0.5).clamp(0.0, 1.0);
        self.wired = (0..n * fan).map(|_| (rng.unit() * n as f64) as u32 % n as u32).collect();
        self.table = (0..n)
            .map(|_| {
                let mut bits = 0u64;
                for row in 0..(1usize << fan) {
                    if rng.unit() < bias {
                        bits |= 1 << row;
                    }
                }
                bits
            })
            .collect();
        self.state = (0..n).map(|_| rng.unit() < 0.5).collect();
        self.fan = fan;
        self.rng = rng;
        self.clock = Clock::default();
        self.stale = false;
    }
}

impl Node for Boolean {
    fn process(&mut self, _inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let n = p.i64("boolean", "size").unwrap_or(64).clamp(2, 4096) as usize;
        let fan = p.i64("boolean", "connections").unwrap_or(2).clamp(1, 6) as usize;
        if self.stale || self.state.len() != n || self.fan != fan {
            self.reseed(n, fan, p);
        }

        let noise = p.f64("boolean", "noise").unwrap_or(0.0).clamp(0.0, 1.0);
        let sfreq = p.f64("output", "sfreq").unwrap_or(20.0).max(1.0);
        let block = p.str("output", "mode").unwrap_or("value") == "block";

        let steps = self.clock.due(c.now, sfreq);
        if block && steps == 0 {
            return Ok(());
        }
        let cols = if block { steps } else { 1 };
        let mut states = vec![0f32; n * cols];
        let mut activity = vec![0f32; cols];
        let mut next = vec![false; n];
        for t in 0..steps {
            let mut changed = 0;
            for (i, slot) in next.iter_mut().enumerate() {
                let mut row = 0usize;
                for k in 0..fan {
                    if self.state[self.wired[i * fan + k] as usize] {
                        row |= 1 << k;
                    }
                }
                let mut on = self.table[i] >> row & 1 == 1;
                if noise > 0.0 && self.rng.unit() < noise {
                    on = !on;
                }
                if on != self.state[i] {
                    changed += 1;
                }
                *slot = on;
            }
            self.state.copy_from_slice(&next);
            if block {
                for (i, on) in self.state.iter().enumerate() {
                    states[i * cols + t] = *on as u8 as f32;
                }
                activity[t] = changed as f32 / n as f32;
            } else {
                activity[0] = changed as f32 / n as f32;
            }
        }
        if !block {
            for (i, on) in self.state.iter().enumerate() {
                states[i] = *on as u8 as f32;
            }
        }

        let names: Vec<Coord> = (0..n).map(|i| Coord::Str(format!("unit{i}").into())).collect();
        let shape = if block { vec![n, cols] } else { vec![n] };
        let meta = Meta::new().with_sfreq(block.then_some(sfreq)).with_channels(Axes::new().with(0, Axis::coords(names)));
        out.set("state", Data::array_f32(shape, bytes(&states), meta).map_err(|e| e.to_string())?);
        let scalar = Meta::new().with_sfreq(block.then_some(sfreq));
        out.set("activity", Data::array_f32(vec![cols], bytes(&activity), scalar).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        // The wiring and the tables are drawn once; every knob but `noise` changes what was drawn.
        if matches!(key.name.as_str(), "seed" | "size" | "connections" | "bias") {
            self.stale = true;
        }
        Ok(())
    }

    fn on_pulse(&mut self, _key: &ParamKey, _p: &Params<'_>) -> NodeResult {
        self.stale = true;
        Ok(())
    }
}

static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "state", kind: SlotType::Array },
    OutputDecl { name: "activity", kind: SlotType::Array },
];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "boolean",
        name: "size",
        spec: ParamSpec::Int { default: 64, min: 2, max: 4096 },
        expression: None,
        doc: Some("How many units."),
    },
    ParamDecl {
        group: "boolean",
        name: "connections",
        spec: ParamSpec::Int { default: 2, min: 1, max: 6 },
        expression: None,
        doc: Some("How many units each one reads. 1 freezes, 3 and up is chaos, and 2 is the edge between them."),
    },
    ParamDecl {
        group: "boolean",
        name: "bias",
        spec: ParamSpec::Float { default: 0.5, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("How often a drawn rule answers on. Away from 0.5 the network freezes even at high `connections`."),
    },
    ParamDecl {
        group: "boolean",
        name: "noise",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 0.5 },
        expression: None,
        doc: Some("Chance a unit flips against its rule, which is what shakes a frozen network."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000 },
        expression: None,
        doc: Some("Seeds the wiring, the rules and the starting state. Negative takes a fresh one from the clock."),
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Draw the network again from the seed."),
    },
    ParamDecl {
        group: "output",
        name: "mode",
        spec: ParamSpec::Str { default: "value", options: &["value", "block"], refresh: false },
        expression: None,
        doc: Some("`value` emits the state now; `block` emits every step since the last frame, which is a raster."),
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 20.0, min: 1.0, max: 1000.0 },
        expression: None,
        doc: Some("Updates per second of real time, and the sample rate of an emitted block."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation],
    doc: "A random Boolean network, frozen or chaotic on one knob.\n\
          Every unit reads a few others through a rule drawn once; `activity` says how much of the \
          network is still changing.",
    inputs: &[],
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Boolean, MANIFEST);
