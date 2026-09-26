use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, SlotDecl, Lanes, Tag, BLOCK};

goofi_audio_sdk::params! {
    TIME = ParamDecl {
        group: "delay",
        name: "time",
        spec: ParamSpec::Float { default: 0.25, min: 0.0, max: 5.0 },
        expression: None,
        doc: Some("seconds behind the input; move it while it sounds and the echoes bend with it"),
        section: 0,
        show: None,
    },
    FEEDBACK = ParamDecl {
        group: "delay",
        name: "feedback",
        spec: ParamSpec::Float { default: 0.3, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("how much of each echo goes back in; at 1 it repeats without fading"),
        section: 0,
        show: None,
    },
    MIX = ParamDecl {
        group: "delay",
        name: "mix",
        spec: ParamSpec::Float { default: 0.5, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("how much of what leaves is the echo rather than the input"),
        section: 0,
        show: None,
    },
}

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Audio, trigger_process: false, multi: true, required: false }];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "An echo, one line per channel, at a delay that may move while it sounds.",
    inputs: INS,
    outputs: OUTS,
    params: PARAMS,
};

const MAX_SECONDS: f32 = 5.0;

#[derive(Default)]
struct Delay {
    rate: f32,
    len: usize,
    lines: Lanes<Vec<f32>>,
    write: usize,
}

/// The line read `back` samples behind the write head, between the two samples that straddle it.
fn tap(line: &[f32], write: usize, back: f32) -> f32 {
    let len = line.len();
    let back = back.clamp(1.0, len as f32 - 2.0);
    let whole = back.floor();
    let frac = back - whole;
    let at = (write + len - whole as usize) % len;
    let before = (at + len - 1) % len;
    line[at] * (1.0 - frac) + line[before] * frac
}

impl AudioNode for Delay {
    fn prepare(&mut self, rate: f64) {
        self.rate = rate as f32;
        self.len = (MAX_SECONDS * self.rate) as usize + 2;
        self.lines.reset();
        self.write = 0;
    }

    fn process(&mut self, b: &mut Block<'_>) {
        let (input, time, feedback, mix) =
            (&b.ins[0], &b.params[P::TIME], &b.params[P::FEEDBACK], &b.params[P::MIX]);
        let out = &mut b.outs[0];
        let width = out.channels() as usize;
        let (len, lines) = (self.len, self.lines.fit(width));
        // A line is allocated when its lane is new or the rate moved, never on a steady block.
        for line in lines.iter_mut().filter(|line| line.len() != len) {
            *line = vec![0.0; len];
        }
        let start = self.write;
        let mut write = start;
        for (c, line) in lines.iter_mut().enumerate() {
            let (x, t, fb, wet) = (input.chan(c), time.chan(c), feedback.chan(c), mix.chan(c));
            let y = out.chan_mut(c);
            write = start;
            for i in 0..BLOCK {
                let echo = tap(line, write, t[i].clamp(0.0, MAX_SECONDS) * self.rate);
                line[write] = x[i] + echo * fb[i].clamp(0.0, 1.0);
                y[i] = x[i] * (1.0 - wet[i]) + echo * wet[i];
                write = (write + 1) % line.len();
            }
        }
        self.write = write;
    }
}

goofi_audio_sdk::export!(Delay, MANIFEST);
