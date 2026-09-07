//! LFO — one oscillator for both planes: a sample per update to modulate a param, or the block of
//! samples real time advanced by. Phase is a function of PATCH TIME, so two LFOs of one frequency
//! run together whenever each was made.

use goofi_core::{Data, Meta, SlotType};
use goofi_signal_sdk::{ExprDecl, ExprMode, Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, Tag};

/// The waveform's value at `t` cycles in `[0, 1)`, in `[-1, 1]`.
fn wave(kind: &str, t: f64, duty: f64) -> f64 {
    match kind {
        "square" => {
            if t < duty {
                1.0
            } else {
                -1.0
            }
        }
        "sawtooth" => 2.0 * t - 1.0,
        "triangle" => {
            if t < 0.25 {
                4.0 * t
            } else if t < 0.75 {
                2.0 - 4.0 * t
            } else {
                4.0 * t - 4.0
            }
        }
        _ => (std::f64::consts::TAU * t).sin(),
    }
}

#[derive(Default)]
struct LFO {
    /// Cycles added to `frequency * now`. Zero unless a pulse re-phased this LFO — a frequency
    /// change does NOT re-solve it, because holding the wave continuous there is what would put
    /// two LFOs of one frequency out of phase.
    phase_offset: f64,
    /// A pulse asked for phase zero, applied at the next process where `now` is known.
    rezero: bool,
    /// `ctx.now` at the first emit — block pacing is measured from here.
    start: Option<f64>,
    /// Samples emitted since `start`; counting keeps the block size drift-free.
    emitted: u64,
}

impl LFO {
    /// Cycles at patch second `now` — a pure function of the patch's time and this frequency.
    fn phase_at(&self, freq: f64, now: f64) -> f64 {
        (freq * now + self.phase_offset).rem_euclid(1.0)
    }

    /// A pulse asked for phase zero here, which is the one thing that takes an LFO off the shared
    /// phase, and only until the next one.
    fn anchor(&mut self, freq: f64, now: f64) {
        if self.rezero {
            self.phase_offset = -freq * now;
            self.rezero = false;
        }
    }
}

impl Node for LFO {
    fn process(
        &mut self,
        _inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let freq = p.f64("lfo", "frequency").unwrap_or(1.0);
        let amp = p.f64("lfo", "amplitude").unwrap_or(1.0);
        let offset = p.f64("lfo", "offset").unwrap_or(0.0);
        // Refuse non-finite BEFORE it reaches `self.phase`: nothing but a reset clears a NaN.
        if !freq.is_finite() || !amp.is_finite() || !offset.is_finite() {
            return Err(format!("non-finite drive: frequency={freq}, amplitude={amp}, offset={offset}").into());
        }
        let kind = p.str("lfo", "waveform").unwrap_or("sine");
        let duty = p.f64("lfo", "duty").unwrap_or(0.5).clamp(0.0, 1.0);
        let skew = p.f64("lfo", "phase").unwrap_or(0.0);
        let sfreq = p.f64("output", "sfreq").unwrap_or(250.0).max(1.0);
        let block = p.str("output", "mode").unwrap_or("value") == "block";

        self.anchor(freq, c.now);
        let mut sample = |phase: f64| ((wave(kind, (phase + skew).rem_euclid(1.0), duty) * amp + offset) as f32).to_le_bytes();

        let (shape, buf, meta) = if block {
            let start = *self.start.get_or_insert(c.now);
            let total = (sfreq * (c.now - start)).round().max(0.0) as u64;
            let n = total.saturating_sub(self.emitted) as usize;
            if n == 0 {
                return Ok(());
            }
            let first = self.emitted;
            self.emitted = total;
            let mut buf = Vec::with_capacity(n * 4);
            for k in 0..n {
                // Each sample sits at its OWN patch second, so a block draws the same wave the
                // value mode would at those instants.
                buf.extend_from_slice(&sample(self.phase_at(freq, start + (first + k as u64) as f64 / sfreq)));
            }
            (vec![n], buf, Meta::new().with_sfreq(Some(sfreq)))
        } else {
            (vec![1], sample(self.phase_at(freq, c.now)).to_vec(), Meta::new())
        };
        out.set("out", Data::array_f32(shape, buf, meta).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        // A new rate or mode re-anchors the block pacing; the old count measures a gone clock.
        if key.group == "output" {
            self.start = None;
            self.emitted = 0;
        }
        Ok(())
    }

    fn on_pulse(&mut self, _key: &ParamKey, _p: &Params<'_>) -> NodeResult {
        self.rezero = true;
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "lfo",
        name: "waveform",
        spec: ParamSpec::Str { default: "sine", options: &["sine", "triangle", "sawtooth", "square"], refresh: false },
        expression: None,
        doc: Some("Shape of one cycle."),
    },
    ParamDecl {
        group: "lfo",
        name: "frequency",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 1000.0 },
        expression: None,
        doc: Some("Cycles per second."),
    },
    ParamDecl {
        group: "lfo",
        name: "amplitude",
        spec: ParamSpec::Float { default: 1.0, min: -1.0e6, max: 1.0e6 },
        expression: None,
        doc: Some("Peak value: the wave swings between minus this and plus this, before `offset`."),
    },
    ParamDecl {
        group: "lfo",
        name: "offset",
        spec: ParamSpec::Float { default: 0.0, min: -1.0e6, max: 1.0e6 },
        expression: None,
        doc: Some("Added to every sample, so the wave can swing around a value other than zero."),
    },
    ParamDecl {
        group: "lfo",
        name: "phase",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("Where in the cycle the wave reads, in cycles; 0.25 is a quarter turn ahead."),
    },
    ParamDecl {
        group: "lfo",
        name: "duty",
        spec: ParamSpec::Float { default: 0.5, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("Fraction of the cycle a square wave spends high; the other waveforms ignore it."),
    },
    ParamDecl {
        group: "lfo",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Put the phase back to the start of the cycle."),
    },
    ParamDecl {
        group: "output",
        name: "mode",
        spec: ParamSpec::Str { default: "value", options: &["value", "block"], refresh: false },
        expression: None,
        doc: Some(
            "`value` emits one sample per update, which a param reference reads as a number; \
             `block` emits the samples elapsed at `sfreq`, which is a signal.",
        ),
    },
    ParamDecl {
        group: "output",
        name: "sfreq",
        spec: ParamSpec::Float { default: 250.0, min: 1.0, max: 10_000.0 },
        expression: None,
        doc: Some("Sample rate within an emitted block, in Hz. `value` mode ignores it."),
    },
    // A manifest's own `common.*` is never overwritten by the universal declaration, and the
    // universal default is uncapped — which makes a block one sample long.
    ParamDecl {
        group: "common",
        name: "max_frequency",
        spec: ParamSpec::Float { default: 30.0, min: 0.0, max: 1000.0 },
        expression: Some(ExprDecl { source: "globals.system.default_ufreq", mode: ExprMode::On, trigger: true }),
        doc: Some(
            "How many frames a second to emit. Bound to the patch's `default_ufreq` global, so \
             editing that global re-rates every generator at once.",
        ),
    },
];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator],
    doc: "A low-frequency oscillator.\n\
          One sample per update to modulate a param, or a block of samples to feed a signal.",
    inputs: &[],
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(LFO, MANIFEST);
