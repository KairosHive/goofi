//! Branching — the neuronal avalanche model. One active unit wakes `branching` others on average;
//! at exactly 1 the avalanches have no size of their own, and that is criticality.

use goofi_core::{Data, Meta, SlotType};
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
struct Branching {
    active: Vec<bool>,
    /// Steps of enforced silence left per unit, which is what stops one unit sustaining itself.
    quiet: Vec<u32>,
    /// Which units each one can wake, row-major `size * fan`.
    wired: Vec<u32>,
    fan: usize,
    /// Units woken since the network was last completely silent.
    running: u64,
    /// The size of the last avalanche that finished, which is what the statistics are taken over.
    last: f64,
    rng: Rng,
    clock: Clock,
    stale: bool,
}

impl Branching {
    fn reseed(&mut self, n: usize, fan: usize, p: &Params<'_>) {
        let mut rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
        self.wired = (0..n * fan).map(|_| (rng.unit() * n as f64) as u32 % n as u32).collect();
        self.active = vec![false; n];
        self.quiet = vec![0; n];
        self.fan = fan;
        self.running = 0;
        self.last = 0.0;
        self.rng = rng;
        self.clock = Clock::default();
        self.stale = false;
    }
}

impl Node for Branching {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let n = p.i64("branching", "size").unwrap_or(1024).clamp(2, 100_000) as usize;
        let fan = p.i64("branching", "connections").unwrap_or(8).clamp(1, 64) as usize;
        if self.stale || self.active.len() != n || self.fan != fan {
            self.reseed(n, fan, p);
        }

        let sigma = p.f64("branching", "branching").unwrap_or(1.0).max(0.0);
        let mut drive = p.f64("branching", "drive").unwrap_or(0.0005).clamp(0.0, 1.0);
        let refractory = p.i64("branching", "refractory").unwrap_or(1).clamp(0, 100) as u32;
        let sfreq = p.f64("output", "sfreq").unwrap_or(60.0).max(1.0);
        let block = p.str("output", "mode").unwrap_or("value") == "block";
        if let Some((_, v)) = inp.get("drive").map(floats).transpose()? {
            drive = (drive + v.first().copied().unwrap_or(0.0)).clamp(0.0, 1.0);
        }

        let steps = self.clock.due(c.now, sfreq);
        if block && steps == 0 {
            return Ok(());
        }
        let cols = if block { steps } else { 1 };
        let mut activity = vec![0f32; cols];
        let mut avalanche = vec![0f32; cols];
        let mut next = vec![false; n];
        let chance = sigma / fan as f64;
        for t in 0..steps {
            next.iter_mut().for_each(|v| *v = false);
            for i in 0..n {
                if self.active[i] {
                    for k in 0..fan {
                        if self.rng.unit() < chance {
                            next[self.wired[i * fan + k] as usize] = true;
                        }
                    }
                } else if self.quiet[i] == 0 && self.rng.unit() < drive {
                    next[i] = true;
                }
            }
            let mut woken = 0u64;
            for i in 0..n {
                if self.quiet[i] > 0 {
                    self.quiet[i] -= 1;
                    next[i] = false;
                }
                if next[i] {
                    woken += 1;
                    self.quiet[i] = refractory;
                }
            }
            self.active.copy_from_slice(&next);
            if woken == 0 && self.running > 0 {
                self.last = self.running as f64;
                self.running = 0;
            } else {
                self.running += woken;
            }
            let at = if block { t } else { 0 };
            activity[at] = woken as f32 / n as f32;
            avalanche[at] = self.last as f32;
        }

        let states: Vec<f32> = self.active.iter().map(|on| *on as u8 as f32).collect();
        out.set("state", Data::array_f32(vec![n], bytes(&states), Meta::new()).map_err(|e| e.to_string())?);
        let meta = Meta::new().with_sfreq(block.then_some(sfreq));
        out.set("activity", Data::array_f32(vec![cols], bytes(&activity), meta.clone()).map_err(|e| e.to_string())?);
        out.set("avalanche", Data::array_f32(vec![cols], bytes(&avalanche), meta).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        if matches!(key.name.as_str(), "seed" | "size" | "connections") {
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

static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "state", kind: SlotType::Array },
    OutputDecl { name: "activity", kind: SlotType::Array },
    OutputDecl { name: "avalanche", kind: SlotType::Array },
];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "branching",
        name: "branching",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 3.0 },
        expression: None,
        doc: Some(
            "How many units one active unit wakes on average. Below 1 every avalanche dies out, \
             above 1 it takes the network, and at 1 the sizes follow a power law.",
        ),
    },
    ParamDecl {
        group: "branching",
        name: "size",
        spec: ParamSpec::Int { default: 1024, min: 2, max: 100_000, options: &[] },
        expression: None,
        doc: Some("How many units. A larger network shows the power law over more decades."),
    },
    ParamDecl {
        group: "branching",
        name: "connections",
        spec: ParamSpec::Int { default: 8, min: 1, max: 64, options: &[] },
        expression: None,
        doc: Some("How many units one can wake. `branching` is shared out between them."),
    },
    ParamDecl {
        group: "branching",
        name: "drive",
        spec: ParamSpec::Float { default: 0.0005, min: 0.0, max: 0.1 },
        expression: None,
        doc: Some("Chance a silent unit wakes on its own, which is what starts each avalanche. The `drive` input adds to it."),
    },
    ParamDecl {
        group: "branching",
        name: "refractory",
        spec: ParamSpec::Int { default: 1, min: 0, max: 100, options: &[] },
        expression: None,
        doc: Some("Steps a unit stays silent after it fires. Zero lets one unit sustain itself for ever."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000, options: &[] },
        expression: None,
        doc: Some("Seeds the wiring and the draws. Negative takes a fresh one from the clock."),
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Wire the network again and silence it."),
    },
    ParamDecl {
        group: "output",
        name: "mode",
        spec: ParamSpec::Str { default: "value", options: &["value", "block"], refresh: false },
        expression: None,
        doc: Some("`value` emits the step now; `block` emits every step since the last frame, which is a signal."),
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 60.0, min: 1.0, max: 2000.0 },
        expression: None,
        doc: Some("Steps per second of real time, and the sample rate of an emitted block."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation, Tag::Eeg],
    doc: "Avalanches of activity, tuned to the critical point.\n\
          The neuronal-avalanche model: `avalanche` reports the size of each cascade once it has \
          finished, and at a branching ratio of 1 those sizes follow a power law.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Branching, MANIFEST);
