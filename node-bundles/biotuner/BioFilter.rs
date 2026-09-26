//! BioFilter — a signal's own peaks as a dynamic EQ: one band per peak, each breathing at the
//! rate that peak IS. The peaks arrive as ONE wire in hertz, and the node reads them twice —
//! raised by `shift` octaves they are where the bands sit, and left alone they are how fast those
//! bands sweep. So an alpha peak at 10 Hz is a band near 640 Hz that moves ten times a second,
//! and nothing outside has to fold or scale anything.
//!
//! The source passes through and the bands are added to it, so this shapes rather than replaces.
//! A negative `gain` subtracts its band instead, which is how a notch is asked for.
//!
//! The band COUNT is the `peaks` wire's channels, and the output's width is the source's, so a
//! stereo input stays stereo and every band filters both sides. `amps` is optional: without it
//! every band is at full amplitude.

use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, SlotDecl, Lanes, Tag, BLOCK};

goofi_audio_sdk::params! {
    SHIFT = ParamDecl {
        group: "band",
        name: "shift",
        spec: ParamSpec::Float { default: 6.0, min: 0.0, max: 12.0 },
        expression: None,
        doc: Some("octaves to raise the peaks by to find the bands: an EEG peak is inaudible, and six octaves puts 10 Hz near 640 Hz"),
        section: 0,
        show: None,
    },
    Q = ParamDecl {
        group: "band",
        name: "q",
        spec: ParamSpec::Float { default: 4.0, min: 0.5, max: 20.0 },
        expression: None,
        doc: Some("how narrow each band is; it rings longer as it climbs"),
        section: 0,
        show: None,
    },
    GAIN = ParamDecl {
        group: "band",
        name: "gain",
        spec: ParamSpec::Float { default: 1.0, min: -4.0, max: 4.0 },
        expression: None,
        doc: Some("how much of each band is added to the source. Negative subtracts it, which is a notch."),
        section: 0,
        show: None,
    },
    DEPTH = ParamDecl {
        group: "band",
        name: "depth",
        spec: ParamSpec::Float { default: 0.5, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("how far the sweep moves that gain: 1 swings it from nothing to twice over"),
        section: 0,
        show: None,
    },
    SWEEP = ParamDecl {
        group: "band",
        name: "sweep",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 10.0 },
        expression: None,
        doc: Some("the sweep as a multiple of the peak's own rate; below 1 to hear a slow breath rather than a tremolo, 0 to hold each band still"),
        section: 0,
        show: None,
    },
    GAINBYAMP = ParamDecl {
        group: "amps",
        name: "gainByAmp",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("how much a peak's amplitude decides its band's gain: at 1 the spectrum of the signal is the spectrum of the filter"),
        section: 0,
        show: None,
    },
    DEPTHBYAMP = ParamDecl {
        group: "amps",
        name: "depthByAmp",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("how much it decides the sweep instead: at 1 a loud peak breathes hard and a quiet one sits still"),
        section: 0,
        show: None,
    },
    FLOOR = ParamDecl {
        group: "amps",
        name: "floor",
        spec: ParamSpec::Float { default: -60.0, min: -120.0, max: -1.0 },
        expression: None,
        doc: Some("the amplitude that reads as nothing; everything from here up to 0 dB spreads across the two amounts above"),
        section: 0,
        show: None,
    },
}

static INS: &[SlotDecl] = &[
    SlotDecl { name: "input", kind: SlotType::Audio, trigger_process: false, multi: true, required: false },
    SlotDecl { name: "peaks", kind: SlotType::Audio, trigger_process: false, multi: false, required: false },
    SlotDecl { name: "amps", kind: SlotType::Audio, trigger_process: false, multi: false, required: false },
];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "A dynamic EQ whose bands are a signal's own spectral peaks.\n\
          Wire `Peaks` in through a SignalIn: raised by `shift` octaves the peaks are where the \
          bands sit, and left alone they are how fast the bands sweep.",
    inputs: INS,
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct BioFilter {
    rate: f32,
    /// One state-variable filter per band per channel, band-major.
    ic1: Lanes<f32>,
    ic2: Lanes<f32>,
    /// Where each band's sweep has reached, in cycles, and what it is worth this sample.
    phase: Lanes<f32>,
    level: Lanes<f32>,
}

impl AudioNode for BioFilter {
    /// The bands come from `peaks`; the WIDTH comes from the source, so a stereo input stays
    /// stereo rather than becoming as wide as there are peaks.
    fn channels(&self, ins: &[u16], _params: &[f64], outs: usize) -> Vec<u16> {
        vec![ins.first().copied().unwrap_or(1).max(1); outs]
    }

    fn prepare(&mut self, rate: f64) {
        self.rate = (rate as f32).max(1.0);
    }

    fn process(&mut self, b: &mut Block<'_>) {
        let (input, peaks, amps) = (&b.ins[0], &b.ins[1], &b.ins[2]);
        let octaves = b.params[P::SHIFT].chan(0)[0].exp2();
        let k = 1.0 / b.params[P::Q].chan(0)[0].max(0.5);
        let gain = b.params[P::GAIN].chan(0)[0];
        let depth = b.params[P::DEPTH].chan(0)[0].clamp(0.0, 1.0);
        let sweep = b.params[P::SWEEP].chan(0)[0].max(0.0);
        let by_gain = b.params[P::GAINBYAMP].chan(0)[0].clamp(0.0, 1.0);
        let by_depth = b.params[P::DEPTHBYAMP].chan(0)[0].clamp(0.0, 1.0);
        let floor = b.params[P::FLOOR].chan(0)[0].min(-1.0);
        // Nothing wired to `peaks` is no bank at all, and the source passes untouched.
        let bands = if peaks.wired() { peaks.channels() as usize } else { 0 };
        // Unwired `amps` reads 0 dB, which is full amplitude — the right answer for no answer.
        let loudest = if amps.wired() { amps.channels() as usize } else { 0 };
        let out = &mut b.outs[0];
        let width = out.channels() as usize;
        let rate = self.rate;
        let (phase, level) = (self.phase.fit(bands), self.level.fit(bands));
        let (ic1s, ic2s) = (self.ic1.fit(bands * width), self.ic2.fit(bands * width));

        for i in 0..BLOCK {
            // Each band's sweep advances once a sample, not once a channel.
            for (bd, g) in level.iter_mut().enumerate() {
                let hz = peaks.chan(bd)[i].max(0.0);
                let db = if loudest > 0 { amps.chan(bd.min(loudest - 1))[i] } else { 0.0 };
                let loud = ((db - floor) / -floor).clamp(0.0, 1.0);
                phase[bd] = (phase[bd] + hz * sweep / rate).rem_euclid(1.0);
                let swing = depth * (1.0 - by_depth + by_depth * loud);
                let amount = gain * (1.0 - by_gain + by_gain * loud);
                *g = amount * (1.0 + swing * (std::f32::consts::TAU * phase[bd]).sin());
            }
            for c in 0..width {
                let x = input.chan(c)[i];
                let mut y = x;
                for (bd, g) in level.iter().enumerate() {
                    // Nyquist is the ceiling `tan` needs: at it the warp is infinite.
                    let f = (peaks.chan(bd)[i] * octaves).clamp(1.0, 0.45 * rate);
                    let w = (std::f32::consts::PI * f / rate).tan();
                    let a1 = 1.0 / (1.0 + w * (w + k));
                    let (a2, a3) = (w * a1, w * w * a1);
                    let (ic1, ic2) = (&mut ic1s[bd * width + c], &mut ic2s[bd * width + c]);
                    let v3 = x - *ic2;
                    let v1 = a1 * *ic1 + a2 * v3;
                    let v2 = *ic2 + a2 * *ic1 + a3 * v3;
                    *ic1 = 2.0 * v1 - *ic1;
                    *ic2 = 2.0 * v2 - *ic2;
                    y += v1 * *g;
                }
                out.chan_mut(c)[i] = y;
            }
        }
    }
}

goofi_audio_sdk::export!(BioFilter, MANIFEST);
