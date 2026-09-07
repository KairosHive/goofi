//! Quantum — a statevector simulator of a few qubits, with the rotation angles driven by the
//! patch. Exact, not approximate: ten qubits is a thousand amplitudes and costs nothing.

use goofi_core::{Axes, Axis, Coord, Data, Meta, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, SlotDecl, Tag};

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

    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
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

/// The statevector, split into its real and imaginary halves so no complex type is needed.
struct State {
    re: Vec<f64>,
    im: Vec<f64>,
    qubits: usize,
}

impl State {
    fn ground(qubits: usize) -> State {
        let mut re = vec![0.0; 1 << qubits];
        re[0] = 1.0;
        State { re, im: vec![0.0; 1 << qubits], qubits }
    }

    /// Every index where qubit `q` is 0, paired with the index where it is 1.
    fn pairs(&self, q: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
        let bit = 1usize << q;
        (0..self.re.len()).filter(move |i| i & bit == 0).map(move |i| (i, i | bit))
    }

    fn ry(&mut self, q: usize, angle: f64) {
        let (c, s) = ((angle / 2.0).cos(), (angle / 2.0).sin());
        for (lo, hi) in self.pairs(q).collect::<Vec<_>>() {
            let (a, b) = (self.re[lo], self.re[hi]);
            self.re[lo] = c * a - s * b;
            self.re[hi] = s * a + c * b;
            let (a, b) = (self.im[lo], self.im[hi]);
            self.im[lo] = c * a - s * b;
            self.im[hi] = s * a + c * b;
        }
    }

    fn rz(&mut self, q: usize, angle: f64) {
        let (c, s) = ((angle / 2.0).cos(), (angle / 2.0).sin());
        for (lo, hi) in self.pairs(q).collect::<Vec<_>>() {
            let (re, im) = (self.re[lo], self.im[lo]);
            self.re[lo] = c * re + s * im;
            self.im[lo] = c * im - s * re;
            let (re, im) = (self.re[hi], self.im[hi]);
            self.re[hi] = c * re - s * im;
            self.im[hi] = c * im + s * re;
        }
    }

    fn cnot(&mut self, control: usize, target: usize) {
        let (cbit, tbit) = (1usize << control, 1usize << target);
        for i in 0..self.re.len() {
            if i & cbit != 0 && i & tbit == 0 {
                self.re.swap(i, i | tbit);
                self.im.swap(i, i | tbit);
            }
        }
    }

    /// The three Pauli expectations for one qubit, which is its whole reduced state.
    fn bloch(&self, q: usize) -> [f64; 3] {
        let bit = 1usize << q;
        let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
        for i in 0..self.re.len() {
            let weight = self.re[i] * self.re[i] + self.im[i] * self.im[i];
            z += if i & bit == 0 { weight } else { -weight };
            if i & bit == 0 {
                let (ar, ai) = (self.re[i], self.im[i]);
                let (br, bi) = (self.re[i | bit], self.im[i | bit]);
                x += 2.0 * (ar * br + ai * bi);
                y += 2.0 * (ar * bi - ai * br);
            }
        }
        [x, y, z]
    }
}

#[derive(Default)]
struct Quantum {
    /// A per-gate offset drawn once, so a fresh circuit is not every qubit doing the same thing.
    offsets: Vec<f64>,
    seeded: i64,
}

impl Node for Quantum {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, _c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let qubits = p.i64("quantum", "qubits").unwrap_or(6).clamp(1, 12) as usize;
        let layers = p.i64("quantum", "layers").unwrap_or(3).clamp(1, 12) as usize;
        let seed = p.i64("sim", "seed").unwrap_or(-1);
        let gates = layers * qubits * 2;
        if self.offsets.len() != gates || self.seeded != seed {
            let mut rng = Rng::new(seed);
            self.offsets = (0..gates).map(|_| rng.unit() * std::f64::consts::TAU).collect();
            self.seeded = seed;
        }

        let scale = p.f64("quantum", "angle").unwrap_or(1.0);
        let entangle = p.bool("quantum", "entangle").unwrap_or(true);
        let driven = inp.get("angles").map(floats).transpose()?.map(|(_, v)| v).unwrap_or_default();

        let mut state = State::ground(qubits);
        let mut gate = 0;
        for _ in 0..layers {
            for q in 0..qubits {
                let outside = if driven.is_empty() { 0.0 } else { driven[gate % driven.len()] };
                state.ry(q, scale * (self.offsets[gate] + std::f64::consts::TAU * outside));
                gate += 1;
                let outside = if driven.is_empty() { 0.0 } else { driven[gate % driven.len()] };
                state.rz(q, scale * (self.offsets[gate] + std::f64::consts::TAU * outside));
                gate += 1;
            }
            if entangle && qubits > 1 {
                for q in 0..qubits {
                    state.cnot(q, (q + 1) % qubits);
                }
            }
        }

        let probabilities: Vec<f32> =
            state.re.iter().zip(&state.im).map(|(re, im)| (re * re + im * im) as f32).collect();
        let mut bloch = Vec::with_capacity(qubits * 3);
        let mut entropy = 0.0;
        for q in 0..qubits {
            let v = state.bloch(q);
            bloch.extend(v.iter().map(|x| *x as f32));
            // A pure whole is a mixed part: the Bloch length alone gives the reduced eigenvalues.
            let r = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().min(1.0);
            for lambda in [(1.0 + r) / 2.0, (1.0 - r) / 2.0] {
                if lambda > 1e-12 {
                    entropy -= lambda * lambda.log2();
                }
            }
        }
        entropy /= qubits as f64;

        let basis: Vec<Coord> = (0..1usize << qubits).map(|i| Coord::Str(format!("{i:0width$b}", width = qubits).into())).collect();
        let axis = vec![Coord::Str("x".into()), Coord::Str("y".into()), Coord::Str("z".into())];
        let states = Meta::new().with_channels(Axes::new().with(0, Axis::coords(basis)));
        out.set("probabilities", Data::array_f32(vec![1 << qubits], bytes(&probabilities), states).map_err(|e| e.to_string())?);
        let per_qubit = Meta::new().with_channels(Axes::new().with(1, Axis::coords(axis)));
        out.set("bloch", Data::array_f32(vec![qubits, 3], bytes(&bloch), per_qubit).map_err(|e| e.to_string())?);
        out.set("entropy", Data::array_f32(vec![1], bytes(&[entropy as f32]), Meta::new()).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_pulse(&mut self, _key: &ParamKey, _p: &Params<'_>) -> NodeResult {
        self.offsets.clear();
        Ok(())
    }
}

static INPUTS: &[SlotDecl] =
    &[SlotDecl { name: "angles", kind: SlotType::Array, trigger_process: true, multi: false, required: false }];

static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "probabilities", kind: SlotType::Array },
    OutputDecl { name: "bloch", kind: SlotType::Array },
    OutputDecl { name: "entropy", kind: SlotType::Array },
];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "quantum",
        name: "qubits",
        spec: ParamSpec::Int { default: 6, min: 1, max: 12 },
        expression: None,
        doc: Some("How many qubits. The statevector is two to this power, so 12 is four thousand amplitudes."),
    },
    ParamDecl {
        group: "quantum",
        name: "layers",
        spec: ParamSpec::Int { default: 3, min: 1, max: 12 },
        expression: None,
        doc: Some("How many rounds of rotations. More layers reach further into the space."),
    },
    ParamDecl {
        group: "quantum",
        name: "angle",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 4.0 },
        expression: None,
        doc: Some("Scales every rotation. At 0 the circuit does nothing and the state stays at the ground."),
    },
    ParamDecl {
        group: "quantum",
        name: "entangle",
        spec: ParamSpec::Bool { default: true },
        expression: None,
        doc: Some("Put a ring of controlled-nots between the layers. Without it every qubit stays its own, and `entropy` stays 0."),
    },
    ParamDecl {
        group: "sim",
        name: "seed",
        spec: ParamSpec::Int { default: -1, min: -1, max: 1_000_000 },
        expression: None,
        doc: Some("Seeds the per-gate offsets, which is what makes one circuit differ from another."),
    },
    ParamDecl {
        group: "sim",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Draw the circuit again."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator, Tag::Simulation],
    doc: "A few qubits, simulated exactly, with the patch turning the knobs.\n\
          `angles` drives every rotation; `probabilities` is what a measurement would find, and \
          `entropy` is how entangled the qubits have become.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Quantum, MANIFEST);
