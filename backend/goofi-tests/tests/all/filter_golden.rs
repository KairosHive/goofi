//! The filter against scipy, in both phases. `sosfiltfilt` is the authority for what a Butterworth
//! band run both ways does to a signal and `sosfilt` for what one causal pass does, and the node is
//! measured against each sample by sample — then against itself, fed the same samples in chunks.
//!
//! Lowpass and highpass only. A Butterworth cascade of second-order sections is the bilinear
//! transform of one analog prototype, so the two designs agree exactly there. A bandpass does not:
//! scipy transforms the prototype to a band, and the node cascades a highpass with a lowpass.
//!
//! `tests/gen_filter_golden.py` writes the fixture; the case list here mirrors it.

use goofi_tests::{f32s, hex, install, j, require_python, shape, Goofi, Uid};

const GOLDEN: &str = include_str!("../fixtures/filter_golden.json");

/// Emit one `size` chunk per pulse. Optional initial silence proves the full chain is connected.
fn chunks_of(input: &[f32], size: usize, silence: bool) -> String {
    format!(
        "import goofi\nimport numpy as np\n\n\nclass Chunks(goofi.Node):\n    \
         \"\"\"Emits the filter golden's next chunk on each pulse.\"\"\"\n\n    \
         TAGS = [\"generator\"]\n    OUTPUTS = {{\"out\": goofi.DataType.ARRAY}}\n    \
         PARAMS = {{\"chunks\": {{\"start\": goofi.PulseParam()}}}}\n    \
         PRODUCER = True\n    SAMPLES = {input:?}\n    SIZE = {size}\n    SILENCE = {silence}\n\n    \
         def setup(self):\n        \
         self.at = 0\n        self.pending = False\n\n    \
         def pulse_chunks_start(self):\n        \
         if self.at >= len(self.SAMPLES):\n            self.at = 0\n        self.pending = True\n\n    \
         def process(self):\n        \
         if not self.pending:\n            \
         if self.SILENCE and self.at == 0:\n                \
         return np.zeros(self.SIZE, dtype=np.float32), {{\"sfreq\": 256.0}}\n            \
         return None\n        \
         self.pending = False\n        \
         chunk = self.SAMPLES[self.at : self.at + self.SIZE]\n        \
         self.at += self.SIZE\n        \
         return np.array(chunk, dtype=np.float32), {{\"sfreq\": 256.0}}\n",
        silence = if silence { "True" } else { "False" },
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
    install(&g, "golden.py", &chunks_of(&input, input.len(), false));
    let src = g.add("Golden");
    let service = goofi_bridge::output_service_of(&g.graph(), src, "out");
    let readers = goofi_transport::PortBundle::open(&g.state.iox, |node| {
        goofi_transport::stream_service(node, &service, goofi_transport::ServiceKind::Data)
    }).unwrap();
    g.ready(src);

    // Born configured: each golden is the first pass, not another pass with a previous block's state.
    let filter = |case: &serde_json::Value, phase: &str| {
        let mode = case["mode"].as_str().unwrap();
        let cutoff = case["cutoff"].as_f64().unwrap();
        // The edge the mode does not use is pushed out of the way, so it cannot narrow the band.
        let (low, high) = if mode == "highpass" { (cutoff, 128.0) } else { (0.0, cutoff) };
        let added = g.call("node add", j!({ "type": "signal:Filter", "param": [
            { "name": "filter/mode", "value": mode },
            { "name": "filter/order", "value": case["order"] },
            { "name": "filter/low", "value": low },
            { "name": "filter/high", "value": high },
            { "name": "filter/phase", "value": phase },
        ] }));
        Uid::from_hex(added["uid"].as_str().unwrap()).unwrap()
    };

    for case in golden["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().unwrap();
        // The node computes in f32 where scipy computes in f64, so the gate is a thousandth of the
        // signal's own swing rather than an equality.
        for (phase, key) in [("zero-phase", "expected"), ("causal", "causal")] {
            let want = numbers(&case[key]);
            let flt = filter(case, phase);
            let probe = g.probe(flt, "out");
            g.link(src, "out", flt, "input");
            g.ready(flt);
            g.until("the filter to subscribe before the one block", |_| {
                (goofi_transport::subscribers(&readers) == 1).then_some(())
            });
            g.call("node param request", j!({ "node": hex(src), "param": "chunks/start", "request": "pulse" }));
            let out = g.until(&format!("{name} {phase}"), |_| probe.latest());
            assert_eq!(f32s(&out).len(), input.len(), "{name} {phase}: the whole span comes back");
            let worst = f32s(&out).iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(worst < 2.0e-3, "{name} {phase}: largest sample error {worst}");
            assert!(g.error(flt).is_none(), "{name} {phase}: {:?}", g.error(flt));
            g.call("node remove", j!({ "node": hex(flt) }));
            g.until("the old filter to release its input", |_| {
                (goofi_transport::subscribers(&readers) == 0).then_some(())
            });
        }
    }

    // A causal pass carries its state across frames, so the stream a live source arrives in cannot
    // change the answer: the same input in 64-sample chunks makes the same 512 samples.
    let case = &golden["cases"].as_array().expect("cases")[0];
    // The silence the source opens with leaves the filter at rest, which is the state scipy's
    // `zi` of zeros means — so the stream is measured against that pass, not the primed one.
    let want = numbers(&case["causal_from_rest"]);
    install(&g, "chunks.py", &chunks_of(&input, 64, true));
    let stream = g.add("Chunks");
    let live = filter(case, "causal");
    let buffer = g.add("signal:Buffer");
    g.set_param(buffer, "buffer", "unit", "samples");
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
    for end in (64..=input.len()).step_by(64) {
        g.call("node param request", j!({ "node": hex(stream), "param": "chunks/start", "request": "pulse" }));
        g.until("the next chunk to reach the end before emitting another", |_| {
            held.latest().filter(|d| {
                shape(d) == vec![input.len()] && f32s(d)[input.len() - end..].iter().zip(&want[..end])
                    .map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max) < 2.0e-3
            })
        });
    }
    let out = held.latest().expect("the input reached the buffer");
    assert_eq!(f32s(&out).len(), input.len(), "the chunked stream fills the buffer");
    assert!(g.error(live).is_none(), "{:?}", g.error(live));
}
