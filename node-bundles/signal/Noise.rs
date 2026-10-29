//! Noise — one independent stream per channel, paced like the LFO: a sample per update, or the
//! block real time advanced by. Uniform, normal and pink draw a fresh sample each time; value,
//! Perlin, simplex and ridged are smooth functions of the patch clock, layered in octaves.

use goofi_core::{Data, Meta, SlotType};
use goofi_signal_sdk::{Face, ExprDecl, ExprMode, Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec,
    Show, Tag,
};

const COHERENT: Show = Show { param: "mode", any_of: &["value", "perlin", "simplex", "ridged"] };
const LAYERED: Show = Show { param: "octaves", any_of: &["2", "3", "4", "5", "6", "7", "8"] };

/// How a coherent stream is read at one instant: its time scale and its octaves.
struct Shape {
    period: f64,
    octaves: u32,
    lacunarity: f64,
    persistence: f64,
}

/// A lattice point's fixed value in [-1, 1] for a stream `key` (splitmix64 of the two).
fn lattice(key: u64, i: i64) -> f64 {
    let mut z = key ^ (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

/// One octave of a coherent noise at `x` lattice cells, in [-1, 1].
fn octave(mode: &str, key: u64, x: f64) -> f64 {
    let i = x.floor();
    let (f, i) = (x - i, i as i64);
    let fade = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    match mode {
        "value" => lattice(key, i) + fade * (lattice(key, i + 1) - lattice(key, i)),
        "simplex" => {
            // Each lattice point's kernel reaches one cell; 1/0.316 is the peak of two unit gradients.
            let kernel = |d: f64, g: f64| (1.0 - d * d).max(0.0).powi(4) * g * d;
            (kernel(f, lattice(key, i)) + kernel(f - 1.0, lattice(key, i + 1))) / 0.316
        }
        "ridged" => {
            let ridge = 1.0 - octave("perlin", key, x).abs();
            2.0 * ridge * ridge - 1.0
        }
        // Perlin: a gradient at each lattice point, blended; twice the blend spans [-1, 1].
        _ => 2.0 * (lattice(key, i) * f + fade * (lattice(key, i + 1) * (f - 1.0) - lattice(key, i) * f)),
    }
}

/// One channel's generator state: a 64-bit stream, plus the poles pink noise needs.
#[derive(Clone, Copy)]
struct Source {
    /// What the stream was seeded with, which fixes the coherent lattice.
    key: u64,
    state: u64,
    pink: [f64; 3],
    /// The second of a Box-Muller pair, held for the next normal sample.
    spare: Option<f64>,
}

impl Source {
    fn new(seed: u64) -> Source {
        Source { key: seed, state: seed | 1, pink: [0.0; 3], spare: None }
    }

    /// xorshift64*, which needs no dependency and is far beyond what a noise source asks of it.
    fn next(&mut self) -> f64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        (self.state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn uniform(&mut self) -> f64 {
        2.0 * self.next() - 1.0
    }

    fn normal(&mut self) -> f64 {
        if let Some(z) = self.spare.take() {
            return z;
        }
        let (u, v) = (self.next().max(1e-12), self.next());
        let (r, theta) = ((-2.0 * u.ln()).sqrt(), std::f64::consts::TAU * v);
        self.spare = Some(r * theta.sin());
        r * theta.cos()
    }

    /// Three one-pole filters over white noise: about 1/f across the audible decades.
    fn pink(&mut self) -> f64 {
        let w = self.uniform();
        self.pink[0] = 0.99765 * self.pink[0] + w * 0.0990460;
        self.pink[1] = 0.96300 * self.pink[1] + w * 0.2965164;
        self.pink[2] = 0.57000 * self.pink[2] + w * 1.0526913;
        (self.pink[0] + self.pink[1] + self.pink[2] + w * 0.1848) * 0.25
    }

    /// Octaves summed, each `lacunarity` times finer and `persistence` times weaker, kept in [-1, 1].
    fn coherent(&self, mode: &str, t: f64, shape: &Shape) -> f64 {
        let (mut sum, mut weight, mut amp, mut freq) = (0.0, 0.0, 1.0, 1.0 / shape.period);
        for o in 0..shape.octaves {
            // An octave of its own lattice, shifted by part of a cell, so neither the octaves nor the
            // channels meet their lattice points, where Perlin is zero, at the same instants.
            let key = self.key.wrapping_add(u64::from(o).wrapping_mul(0xd1b5_4a32_d192_ed03));
            let shift = (lattice(key, i64::MIN) + 1.0) * 0.5;
            sum += amp * octave(mode, key, t * freq + shift);
            weight += amp;
            amp *= shape.persistence;
            freq *= shape.lacunarity;
        }
        if weight > 0.0 { sum / weight } else { 0.0 }
    }

    /// The stream's sample at patch time `t`.
    fn sample(&mut self, mode: &str, t: f64, shape: &Shape) -> f64 {
        match mode {
            "uniform" => self.uniform(),
            "normal" => self.normal(),
            "pink" => self.pink(),
            _ => self.coherent(mode, t, shape),
        }
    }
}

#[derive(Default)]
struct Noise {
    sources: Vec<Source>,
    /// The seed the sources were built from, so a change rebuilds them.
    seeded: i64,
    start: Option<f64>,
    emitted: u64,
}

impl Noise {
    fn reseed(&mut self, channels: usize, seed: i64) {
        let base = if seed < 0 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9e37_79b9_7f4a_7c15)
        } else {
            seed as u64
        };
        // Each channel walks the golden-ratio stride off the base, so no two streams line up.
        self.sources =
            (0..channels).map(|c| Source::new(base.wrapping_add((c as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15)))).collect();
        self.seeded = seed;
    }
}

impl Node for Noise {
    fn process(
        &mut self,
        _inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let channels = p.i64("output", "channels").unwrap_or(1).clamp(1, 64) as usize;
        let seed = p.i64("noise", "seed").unwrap_or(-1);
        if self.sources.len() != channels || self.seeded != seed {
            self.reseed(channels, seed);
        }
        let amp = p.f64("noise", "amplitude").unwrap_or(1.0);
        let offset = p.f64("noise", "offset").unwrap_or(0.0);
        if !amp.is_finite() || !offset.is_finite() {
            return Err(format!("non-finite drive: amplitude={amp}, offset={offset}").into());
        }
        let mode = p.str("noise", "mode").unwrap_or("uniform");
        let shape = Shape {
            period: p.f64("noise", "period").unwrap_or(1.0),
            octaves: p.i64("noise", "octaves").unwrap_or(1).clamp(1, 8) as u32,
            lacunarity: p.f64("noise", "lacunarity").unwrap_or(2.0),
            persistence: p.f64("noise", "persistence").unwrap_or(0.5),
        };
        if !(shape.period > 0.0 && shape.period.is_finite()) || !shape.lacunarity.is_finite() || !shape.persistence.is_finite() {
            return Err(format!(
                "non-finite or non-positive shape: period={}, lacunarity={}, persistence={}",
                shape.period, shape.lacunarity, shape.persistence
            )
            .into());
        }
        let sfreq = p.f64("output", "sfreq").unwrap_or(250.0).max(1.0);

        if p.str("output", "mode").unwrap_or("value") != "block" {
            let mut buf = Vec::with_capacity(channels * 4);
            for s in &mut self.sources {
                buf.extend_from_slice(&((s.sample(mode, c.now, &shape) * amp + offset) as f32).to_le_bytes());
            }
            out.set("out", Data::array_f32(vec![channels], buf, Meta::new()).map_err(|e| e.to_string())?);
            return Ok(());
        }

        let start = *self.start.get_or_insert(c.now);
        let total = (sfreq * (c.now - start)).round().max(0.0) as u64;
        let n = total.saturating_sub(self.emitted) as usize;
        if n == 0 {
            return Ok(());
        }
        let first = self.emitted;
        self.emitted = total;
        let mut buf = Vec::with_capacity(channels * n * 4);
        for s in &mut self.sources {
            for k in 0..n {
                let t = start + (first + k as u64) as f64 / sfreq;
                buf.extend_from_slice(&((s.sample(mode, t, &shape) * amp + offset) as f32).to_le_bytes());
            }
        }
        let meta = Meta::new().with_sfreq(Some(sfreq));
        out.set("out", Data::array_f32(vec![channels, n], buf, meta).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        if key.group == "output" {
            self.start = None;
            self.emitted = 0;
        }
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "noise",
        name: "mode",
        spec: ParamSpec::Str {
            default: "uniform",
            options: &["uniform", "normal", "pink", "value", "perlin", "simplex", "ridged"],
            refresh: false,
        },
        expression: None,
        doc: Some(
            "`uniform` fills the range evenly, `normal` clusters around the offset and `pink` \
             weights the low frequencies, each a fresh sample every time. `value`, `perlin` and \
             `simplex` drift smoothly over the patch clock; `ridged` folds Perlin into sharp crests.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "noise",
        name: "amplitude",
        spec: ParamSpec::Num { default: &[1.0], min: -1.0e6, max: 1.0e6, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Scales every sample, so the noise spans minus this to plus this."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "noise",
        name: "offset",
        spec: ParamSpec::Num { default: &[0.0], min: -1.0e6, max: 1.0e6, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Added to every sample, so the noise sits around a value other than zero."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "noise",
        name: "seed",
        spec: ParamSpec::Num { default: &[-1.0], min: -1.0, max: i32::MAX as f64, int: true, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Fixes the stream so a patch replays the same noise; -1 takes a fresh one."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "noise",
        name: "period",
        spec: ParamSpec::Num { default: &[1.0], min: 0.01, max: 600.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Seconds between the turns of the first octave: longer drifts slower."),
        section: 1,
        show: &[COHERENT],
        role: None,
    },
    ParamDecl {
        group: "noise",
        name: "octaves",
        spec: ParamSpec::Num { default: &[1.0], min: 1.0, max: 8.0, int: true, options: &[1, 2, 3, 4, 5, 6, 7, 8], face: Face::Plain },
        expression: None,
        doc: Some("Layers of finer detail summed over the first."),
        section: 1,
        show: &[COHERENT],
        role: None,
    },
    ParamDecl {
        group: "noise",
        name: "lacunarity",
        spec: ParamSpec::Num { default: &[2.0], min: 1.0, max: 4.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("How many times faster each octave turns than the one before."),
        section: 1,
        show: &[COHERENT, LAYERED],
        role: None,
    },
    ParamDecl {
        group: "noise",
        name: "persistence",
        spec: ParamSpec::Num { default: &[0.5], min: 0.0, max: 1.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("How much of the one before each octave keeps: lower is smoother."),
        section: 1,
        show: &[COHERENT, LAYERED],
        role: None,
    },
    ParamDecl {
        group: "output",
        name: "mode",
        spec: ParamSpec::Str { default: "value", options: &["value", "block"], refresh: false },
        expression: None,
        doc: Some(
            "`value` emits one sample per channel per update, which a param reference reads as a \
             number; `block` emits the samples elapsed at `sfreq`, which is a signal.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Num { default: &[250.0], min: 1.0, max: 10_000.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Sample rate within an emitted block, in Hz. `value` mode ignores it."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "output",
        name: "channels",
        spec: ParamSpec::Num { default: &[1.0], min: 1.0, max: 64.0, int: true, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("How many independent noise streams to emit, one per channel."),
        section: 0,
        show: &[],
        role: None,
    },
    // A manifest's own `common.*` is never overwritten by the universal declaration, and the
    // universal default is uncapped — which makes a block one sample long.
    ParamDecl {
        group: "common",
        name: "max_frequency",
        spec: ParamSpec::Num { default: &[30.0], min: 0.0, max: 1000.0, int: false, options: &[], face: Face::Plain },
        expression: Some(ExprDecl { source: "variables.signal.default_ufreq", mode: ExprMode::On }),
        doc: Some(
            "How many frames a second to emit. Bound to the patch's `default_ufreq` variable, so \
             editing that variable re-rates every generator at once.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator],
    doc: "Random or smooth noise, one independent stream per channel.\n\
          Uniform, normal or pink samples, or value, Perlin, simplex or ridged noise drifting at a period, in octaves.",
    inputs: &[],
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Noise, MANIFEST);
