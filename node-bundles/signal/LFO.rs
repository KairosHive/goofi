//! LFO — one oscillator for both planes: a sample per update to modulate a param, or the block of
//! samples real time advanced by. The phase is a running sum on the patch clock, so a frequency
//! change bends the wave from where it sits and never jumps it.

use goofi_core::{Data, Meta, SlotType};
use goofi_signal_sdk::{Face, ExprDecl, ExprMode, Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, Show, Tag};

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
    /// Where the oscillator sits, in cycles within `[0, 1)`. Only the elapsed time at the
    /// frequency of the moment advances it, so a frequency edit continues the wave in place.
    phase: f64,
    /// `ctx.now` at the last `value` emit — what the next advance measures from.
    last: Option<f64>,
    /// `ctx.now` at the first block — block pacing is measured from here.
    start: Option<f64>,
    /// Samples emitted since `start`; counting keeps the block size drift-free.
    emitted: u64,
}

impl LFO {
    /// Move the phase on by `cycles` and answer where it sits now.
    fn advance(&mut self, cycles: f64) -> f64 {
        self.phase = (self.phase + cycles).rem_euclid(1.0);
        self.phase
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
        // Refuse non-finite BEFORE it reaches `self.phase`: nothing would clear a NaN from it.
        if !freq.is_finite() || !amp.is_finite() || !offset.is_finite() {
            return Err(format!("non-finite drive: frequency={freq}, amplitude={amp}, offset={offset}").into());
        }
        let kind = p.str("lfo", "waveform").unwrap_or("sine");
        let duty = p.f64("lfo", "duty").unwrap_or(0.5).clamp(0.0, 1.0);
        let skew = p.f64("lfo", "phase").unwrap_or(0.0);
        let sfreq = p.f64("lfo", "sfreq").unwrap_or(250.0).max(1.0);
        let block = p.str("lfo", "mode").unwrap_or("value") == "block";

        let sample = |phase: f64| ((wave(kind, (phase + skew).rem_euclid(1.0), duty) * amp + offset) as f32).to_le_bytes();

        let (shape, buf, meta) = if block {
            let start = *self.start.get_or_insert(c.now);
            let total = (sfreq * (c.now - start)).round().max(0.0) as u64;
            let n = total.saturating_sub(self.emitted) as usize;
            if n == 0 {
                return Ok(());
            }
            self.emitted = total;
            // Each sample advances the phase by one sample period at the frequency of the moment.
            let mut buf = Vec::with_capacity(n * 4);
            for _ in 0..n {
                buf.extend_from_slice(&sample(self.advance(freq / sfreq)));
            }
            (vec![n], buf, Meta::new().with_sfreq(Some(sfreq)))
        } else {
            let elapsed = c.now - self.last.unwrap_or(c.now);
            self.last = Some(c.now);
            (vec![1], sample(self.advance(freq * elapsed)).to_vec(), Meta::new())
        };
        out.set("out", Data::array_f32(shape, buf, meta).map_err(|e| e.to_string())?);
        Ok(())
    }

    fn on_param_changed(&mut self, key: &ParamKey, _v: &goofi_core::Param) -> NodeResult {
        // A new rate or mode re-anchors the pacing; the old count measures a gone clock. The
        // phase stays where it is, so the wave continues across the switch.
        if key.group == "lfo" && (key.name == "mode" || key.name == "sfreq") {
            self.start = None;
            self.emitted = 0;
            self.last = None;
        }
        Ok(())
    }
}

const SQUARE: &[Show] = &[Show { param: "waveform", any_of: &["square"] }];
const BLOCK: &[Show] = &[Show { param: "mode", any_of: &["block"] }];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "lfo",
        name: "waveform",
        spec: ParamSpec::Str { default: "sine", options: &["sine", "triangle", "sawtooth", "square"], refresh: false },
        expression: None,
        doc: Some("Shape of one cycle."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "lfo",
        name: "frequency",
        spec: ParamSpec::Num { default: &[1.0], min: 0.1, max: 100.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Cycles per second. A change bends the wave from where it sits; it does not restart."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "lfo",
        name: "amplitude",
        spec: ParamSpec::Num { default: &[1.0], min: 0.1, max: 10.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Peak value: the wave swings between minus this and plus this, before `offset`."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "lfo",
        name: "offset",
        spec: ParamSpec::Num { default: &[0.0], min: -10.0, max: 10.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Added to every sample, so the wave can swing around a value other than zero."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "lfo",
        name: "phase",
        spec: ParamSpec::Num { default: &[0.0], min: 0.0, max: 1.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Shift of the read point, in cycles; 0.25 reads a quarter turn ahead of the oscillator."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "lfo",
        name: "duty",
        spec: ParamSpec::Num { default: &[0.5], min: 0.0, max: 1.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Fraction of the cycle the square wave spends high."),
        section: 0,
        show: SQUARE,
        role: None,
    },
    ParamDecl {
        group: "lfo",
        name: "mode",
        spec: ParamSpec::Str { default: "value", options: &["value", "block"], refresh: false },
        expression: None,
        doc: Some(
            "`value` emits one sample per update, which a param reference reads as a number; \
             `block` emits the samples elapsed at `sfreq`, which is a signal.",
        ),
        section: 1,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "lfo",
        name: "sfreq",
        spec: ParamSpec::Num { default: &[250.0], min: 1.0, max: 10_000.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Sample rate within an emitted block, in Hz."),
        section: 1,
        show: BLOCK,
        role: None,
    },
    // A manifest's own `common.*` is never overwritten by the universal declaration, and the
    // universal default is uncapped — which makes a block one sample long.
    ParamDecl {
        group: "common",
        name: "max_frequency",
        spec: ParamSpec::Num { default: &[30.0], min: 0.0, max: 1000.0, int: false, options: &[], face: Face::Plain },
        expression: Some(ExprDecl { source: "vars.signal.default_ufreq", mode: ExprMode::On }),
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
    doc: "A low-frequency oscillator.\n\
          One sample per update to modulate a param, or a block of samples to feed a signal.",
    inputs: &[],
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(LFO, MANIFEST);
