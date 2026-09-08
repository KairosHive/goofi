//! The filter against scipy, in both phases. `sosfiltfilt` is the authority for what a Butterworth
//! band run both ways does to a signal and `sosfilt` for what one causal pass does, and the node is
//! measured against each sample by sample — then against itself, fed the same samples in chunks.
//!
//! Lowpass and highpass only. A Butterworth cascade of second-order sections is the bilinear
//! transform of one analog prototype, so the two designs agree exactly there. A bandpass does not:
//! scipy transforms the prototype to a band, and the node cascades a highpass with a lowpass.
//!
//! `tests/gen_filter_golden.py` writes the fixture; the case list here mirrors it.

use goofi_tests::{f32s, hex, install, j, require_python, shape, Goofi};

const GOLDEN: &str = include_str!("fixtures/filter_golden.json");

/// A producer that emits the golden's input, so the node under test sees the same samples scipy did.
fn source_of(input: &[f32]) -> String {
    format!(
        "import goofi\nimport numpy as np\n\n\nclass Golden(goofi.Node):\n    \
         \"\"\"Emits the filter golden's input, at the rate it was generated for.\"\"\"\n\n    \
         TAGS = [\"generator\"]\n    OUTPUTS = {{\"out\": goofi.DataType.ARRAY}}\n    \
         PRODUCER = True\n    SAMPLES = {input:?}\n\n    \
         def process(self):\n        \
         return np.array(self.SAMPLES, dtype=np.float32), {{\"sfreq\": 256.0}}\n",
    )
}

/// The same input, walked in `size` chunks and emitted once each — a live stream, where the whole
/// block a golden holds is a recording. Until the pulse it emits silence, so the walk cannot start
/// before the chain has carried something end to end.
fn chunks_of(input: &[f32], size: usize) -> String {
    format!(
        "import goofi\nimport numpy as np\n\n\nclass Chunks(goofi.Node):\n    \
         \"\"\"Emits silence, then the filter golden's input in chunks once each, on a pulse.\"\"\"\n\n    \
         TAGS = [\"generator\"]\n    OUTPUTS = {{\"out\": goofi.DataType.ARRAY}}\n    \
         PARAMS = {{\"chunks\": {{\"start\": goofi.PulseParam()}}}}\n    \
         PRODUCER = True\n    SAMPLES = {input:?}\n    SIZE = {size}\n\n    \
         def setup(self):\n        \
         self.at = None\n\n    \
         def pulse_chunks_start(self):\n        \
         self.at = 0\n\n    \
         def process(self):\n        \
         if self.at is None:\n            \
         return np.zeros(self.SIZE, dtype=np.float32), {{\"sfreq\": 256.0}}\n        \
         if self.at >= len(self.SAMPLES):\n            \
         return None\n        \
         chunk = self.SAMPLES[self.at : self.at + self.SIZE]\n        \
         self.at += self.SIZE\n        \
         return np.array(chunk, dtype=np.float32), {{\"sfreq\": 256.0}}\n",
    )
}

#[test]
fn the_filter_answers_what_scipy_answers_in_either_phase() {
    let _py = require_python();
    let golden: serde_json::Value = serde_json::from_str(GOLDEN).expect("the golden parses");
    let numbers = |v: &serde_json::Value| -> Vec<f32> {
        v.as_array().expect("an array").iter().map(|x| x.as_f64().expect("a number") as f32).collect()
    };
    let input = numbers(&golden["input"]);

    let g = Goofi::new();
    install(&g, "golden.py", &source_of(&input));
    let src = g.add("Golden");
    let flt = g.add("signal:Filter");
    let probe = g.probe(flt, "out");
    g.link(src, "out", flt, "input");

    // The design a case names, on whichever Filter is asked for it.
    let design = |node, case: &serde_json::Value| {
        let mode = case["mode"].as_str().unwrap();
        let cutoff = case["cutoff"].as_f64().unwrap();
        g.set_param(node, "filter", "mode", mode);
        g.set_param(node, "filter", "order", case["order"].as_i64().unwrap());
        // The edge the mode does not use is pushed out of the way, so it cannot narrow the band.
        let (low, high) = if mode == "highpass" { (cutoff, 128.0) } else { (0.0, cutoff) };
        g.set_param(node, "filter", "low", low);
        g.set_param(node, "filter", "high", high);
    };

    for case in golden["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().unwrap();
        design(flt, case);
        // The node computes in f32 where scipy computes in f64, so the gate is a thousandth of the
        // signal's own swing rather than an equality.
        for (phase, key) in [("zero-phase", "expected"), ("causal", "causal")] {
            let want = numbers(&case[key]);
            g.set_param(flt, "filter", "phase", phase);
            let worst = move |got: &[f32]| {
                got.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max)
            };
            let out = g.until(&format!("{name} {phase}"), |_| {
                probe.latest().filter(|d| shape(d) == vec![input.len()] && worst(&f32s(d)) < 2.0e-3)
            });
            assert_eq!(f32s(&out).len(), input.len(), "{name} {phase}: the whole span comes back");
            assert!(g.error(flt).is_none(), "{name} {phase}: {:?}", g.error(flt));
        }
    }

    // A causal pass carries its state across frames, so the stream a live source arrives in cannot
    // change the answer: the same input in 64-sample chunks makes the same 512 samples.
    let case = &golden["cases"].as_array().expect("cases")[0];
    // The silence the source opens with leaves the filter at rest, which is the state scipy's
    // `zi` of zeros means — so the stream is measured against that pass, not the primed one.
    let want = numbers(&case["causal_from_rest"]);
    install(&g, "chunks.py", &chunks_of(&input, 64));
    let stream = g.add("Chunks");
    let live = g.add("signal:Filter");
    design(live, case);
    g.set_param(live, "filter", "phase", "causal");
    let buffer = g.add("signal:Buffer");
    g.set_param(buffer, "buffer", "size", input.len() as i64);
    let held = g.probe(buffer, "out");
    g.link(stream, "out", live, "input");
    g.link(live, "out", buffer, "input");
    g.ready(stream);
    g.ready(live);
    g.ready(buffer);
    // A producer that emits into a link the plan has not reached yet loses what it starts with,
    // and a lost chunk is a wrong answer rather than a short one. So the walk starts only once
    // silence has travelled the whole chain and filled the buffer.
    g.until("silence reaches the far end of the chain", |_| {
        held.latest().filter(|d| shape(d) == vec![input.len()])
    });
    g.call("node param pulse", j!({ "node": hex(stream), "param": "chunks/start" }));

    let out = g.until("the chunked stream reaches the same 512 samples", |_| {
        held.latest().filter(|d| {
            shape(d) == vec![input.len()]
                && f32s(d).iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max) < 2.0e-3
        })
    });
    assert_eq!(f32s(&out).len(), input.len(), "the chunked stream fills the buffer");
    assert!(g.error(live).is_none(), "{:?}", g.error(live));
}
