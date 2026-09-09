//! Hopfield — an associative memory: store patterns, show it a fragment, and watch it fall into
//! the whole one. `modern` is the dense form, whose update is attention's own.

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

#[derive(Default)]
struct Hopfield {
    /// The stored memories, row-major `count * width`, drawn when nothing is wired.
    drawn: Vec<f64>,
    count: usize,
    width: usize,
    state: Vec<f64>,
    rng: Rng,
    clock: Clock,
    stale: bool,
}

impl Hopfield {
    fn reseed(&mut self, count: usize, width: usize, p: &Params<'_>) {
        let mut rng = Rng::new(p.i64("sim", "seed").unwrap_or(-1));
        self.drawn = (0..count * width).map(|_| if rng.unit() < 0.5 { -1.0 } else { 1.0 }).collect();
        self.state = (0..width).map(|_| 0.1 * rng.signed()).collect();
        self.count = count;
        self.width = width;
        self.rng = rng;
        self.clock = Clock::default();
        self.stale = false;
    }
}

impl Node for Hopfield {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let wired = inp.get("patterns").map(floats).transpose()?;
        if let Some((shape, _)) = &wired {
            if shape.len() != 2 {
                return Err(format!("patterns needs one row per memory, got {shape:?}").into());
            }
        }
        let (count, width) = match &wired {
            Some((shape, _)) => (shape[0], shape[1]),
            None => (
                p.i64("hopfield", "patterns").unwrap_or(4).clamp(1, 256) as usize,
                p.i64("hopfield", "size").unwrap_or(64).clamp(2, 4096) as usize,
            ),
        };
        if self.stale || self.count != count || self.width != width {
            self.reseed(count, width, p);
        }
        let memories = wired.as_ref().map(|(_, v)| v.as_slice()).unwrap_or(&self.drawn);

        let modern = p.str("hopfield", "mode").unwrap_or("classic") == "modern";
        let beta = p.f64("hopfield", "beta").unwrap_or(4.0).max(0.0);
        let clamp = p.f64("hopfield", "clamp").unwrap_or(0.1).clamp(0.0, 1.0);
        let noise = p.f64("hopfield", "noise").unwrap_or(0.0).max(0.0);
        let sfreq = p.f64("output", "sfreq").unwrap_or(30.0).max(1.0);

        let cue = inp.get("cue").map(floats).transpose()?.map(|(_, v)| v);
        if let Some(v) = &cue {
            if v.len() != width {
                return Err(format!("cue is {} long and a memory is {width} wide", v.len()).into());
            }
        }

        let steps = self.clock.due(c.now, sfreq);
        let mut overlap = vec![0.0; count];
        for _ in 0..steps {
            for (m, o) in overlap.iter_mut().enumerate() {
                *o = memories[m * width..(m + 1) * width].iter().zip(&self.state).map(|(a, b)| a * b).sum::<f64>() / width as f64;
            }
            let mut next = vec![0.0; width];
            if modern {
                // Dense associative memory: softmax over the memories, then their weighted sum.
                let top = overlap.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let weights: Vec<f64> = overlap.iter().map(|o| (beta * width as f64 * (o - top)).exp()).collect();
                let total = weights.iter().sum::<f64>().max(1e-12);
                for (m, w) in weights.iter().enumerate() {
                    for (i, slot) in next.iter_mut().enumerate() {
                        *slot += w / total * memories[m * width + i];
                    }
                }
            } else {
                // Hebbian weights, applied without ever forming the N by N matrix.
                for (m, o) in overlap.iter().enumerate() {
                    for (i, slot) in next.iter_mut().enumerate() {
                        *slot += o * memories[m * width + i];
                    }
                }
                for (i, slot) in next.iter_mut().enumerate() {
                    // The self term is what a zero diagonal removes, and it is one subtraction.
                    let own: f64 = (0..count).map(|m| memories[m * width + i].powi(2)).sum::<f64>() / width as f64;
                    *slot = (beta * (*slot - own * self.state[i])).tanh();
                }
            }
            for (i, slot) in self.state.iter_mut().enumerate() {
                let pulled = match &cue {
                    Some(v) => (1.0 - clamp) * next[i] + clamp * v[i],
                    None => next[i],
                };
                *slot = (pulled + noise * self.rng.signed()).clamp(-1.0, 1.0);
            }
        }
        for (m, o) in overlap.iter_mut().enumerate() {
            *o = memories[m * width..(m + 1) * width].iter().zip(&self.state).map(|(a, b)| a * b).sum::<f64>() / width as f64;
        }
        let energy = -0.5 * width as f64 * overlap.iter().map(|o| o * o).sum::<f64>();

        let names: Vec<Coord> = (0..count).map(|m| Coord::Str(format!("pattern{m}").into())).collect();
        let cast = |v: &[f64]| bytes(&v.iter().map(|x| *x as f32).collect::<Vec<f32>>());
        out.set("state", Data::array_f32(vec![width], cast(&self.state), Meta::new()).map_err(|e| e.to_string())?);
        let meta = Meta::new().with_channels(Axes::new().with(0, Axis::coords(names)));
        out.set("overlap", Data::array_f32(vec![count], cast(&overlap), meta).map_err(|e| e.to_string())?);
        out.set("energy", Data::array_f32(vec![1], bytes(&[energy as f32]), Meta::new()).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        if matches!(key.name.as_str(), "seed" | "size" | "patterns") {
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
    SlotDecl { name: "patterns", kind: SlotType::Array, trigger_process: false, multi: false, required: false },
    SlotDecl { name: "cue", kind: SlotType::Array, trigger_process: false, multi: false, required: false },
];

static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "state", kind: SlotType::Array },
    OutputDecl { name: "overlap", kind: SlotType::Array },
    OutputDecl { name: "energy", kind: SlotType::Array },
];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "hopfield",
        name: "mode",
        spec: ParamSpec::Str { default: "classic", options: &["classic", "modern"], refresh: false },
        expression: None,
        doc: Some(
            "`classic` is the 1982 network, which holds about 0.14 memories per unit before they \
             blur; `modern` is the dense associative form, which holds far more and whose update \
             is a softmax over the memories.",
        ),
    },
    ParamDecl {
        group: "hopfield",
        name: "size",
        spec: ParamSpec::Int { default: 64, min: 2, max: 4096, options: &[] },
        expression: None,
        doc: Some("How wide a memory is, when none is wired. A wired `patterns` matrix decides instead."),
    },
    ParamDecl {
        group: "hopfield",
        name: "patterns",
        spec: ParamSpec::Int { default: 4, min: 1, max: 256, options: &[] },
        expression: None,
        doc: Some("How many memories to draw, when none is wired."),
    },
    ParamDecl {
        group: "hopfield",
        name: "beta",
        spec: ParamSpec::Float { default: 4.0, min: 0.0, max: 50.0 },
        expression: None,
        doc: Some("How sharply the network commits. High makes it snap to one memory; low leaves it between them."),
    },
    ParamDecl {
        group: "hopfield",
        name: "clamp",
        spec: ParamSpec::Float { default: 0.1, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("How hard the `cue` holds the state. 1 pins it to the cue; 0 lets the network run free."),
    },
    ParamDecl {
        group: "hopfield",
        name: "noise",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("Jitter on the state, which shakes it out of a shallow memory."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000, options: &[] },
        expression: None,
        doc: Some("Seeds the drawn memories and the starting state. Negative takes a fresh one from the clock."),
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Draw the memories again and start over."),
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 30.0, min: 1.0, max: 1000.0 },
        expression: None,
        doc: Some("Updates per second of real time."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation, Tag::Ml],
    doc: "An associative memory that completes what it is shown.\n\
          Store patterns, cue it with a fragment, and `overlap` says which memory it settled into.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Hopfield, MANIFEST);
