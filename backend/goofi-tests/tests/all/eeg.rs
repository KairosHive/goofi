//! The EEG bundle's analysis chain, run as a user gets it: a montage is re-referenced, related
//! channel by channel, and read as a graph — plus the two nodes that measure a signal rather than
//! relate it. Every node here is judged on a property of the answer, never on a pinned number.

use std::path::Path;
use std::time::{Duration, Instant};

use goofi_tests::{f32s, hex, install, labels, require_python, shape, Goofi, OutputProbe, Uid};
use serde_json::json;

/// One of a bundle's files, installed through the same seam a user's own file takes.
fn bundled(g: &Goofi, bundle: &str, file: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../node-bundles").join(bundle).join(file);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    install(g, file, &source)
}

/// A required slot still empty is a node WAITING for its producer, not a node that failed.
fn waiting(e: &str) -> bool {
    e.contains("has no data")
}

/// The first frame on `slot` that `keep` accepts. An error is NOT fatal on sight: a `Buffer` emits
/// growing windows while it fills, so everything downstream is briefly the wrong shape and says so.
/// What matters is whether it is STILL saying it at the end, which is what the timeout reports.
fn frame(
    g: &Goofi,
    what: &str,
    node: Uid,
    probe: &OutputProbe,
    mut keep: impl FnMut(&goofi_core::Data) -> bool,
) -> goofi_core::Data {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut said = None;
    loop {
        if let Some(e) = g.error(node).filter(|e| !waiting(e)) {
            said = Some(e);
        }
        if let Some(d) = probe.latest().filter(|d| keep(d)) {
            return d;
        }
        if Instant::now() >= deadline {
            let tail = said.map_or(String::new(), |e| format!("; the node last said: {e}"));
            panic!("timed out waiting for {what}{tail}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A window of `size` off one producer, which is what makes every frame downstream one shape.
fn window(g: &Goofi, from: Uid, size: i64) -> Uid {
    let buf = g.add("Buffer");
    g.set_param(buf, "buffer", "unit", "samples");
    g.set_param(buf, "buffer", "size", size);
    g.link(from, "out", buf, "input");
    buf
}

#[test]
fn a_montage_is_rereferenced_related_and_read_as_a_graph() {
    let _py = require_python();
    let g = Goofi::new();

    // Four channels off ONE producer, buffered to one window before anything reads them. Two block
    // producers each pace themselves, so a join of two of them is never the same length twice and
    // the chain never settles — every stage here descends from a single source for that reason.
    let noise = g.add("signal:Noise");
    g.set_param(noise, "output", "sfreq", 250.0);
    g.set_param(noise, "output", "mode", "block");
    g.set_param(noise, "output", "channels", 4);
    // Named, because propagating the names onto BOTH axes of the matrix is part of what the
    // connectivity node owes, and a source with no names could not show whether it does.
    let named = g.add("Meta");
    g.set_param(named, "meta", "labels", "Fz, Cz, Pz, Oz");
    g.set_param(named, "meta", "axis", 0);
    g.link(window(&g, noise, 1024), "out", named, "input");
    let montage = named;

    let reference = g.add(&bundled(&g, "eeg", "reference.py"));
    let connectivity = g.add(&bundled(&g, "eeg", "connectivity.py"));
    let graph = g.add(&bundled(&g, "eeg", "graph_metrics.py"));
    let leaders = g.add(&bundled(&g, "complexity", "wavelet_leaders.py"));

    let referenced = g.probe(reference, "data");
    let matrix = g.probe(connectivity, "matrix");
    let degree = g.probe(graph, "degree");
    let clustering = g.probe(graph, "clustering");
    let c2 = g.probe(leaders, "c2");

    g.link(montage, "out", reference, "data");
    g.link(reference, "data", connectivity, "data");
    g.link(connectivity, "matrix", graph, "matrix");
    g.link(montage, "out", leaders, "data");

    // Step: an average reference takes the montage's own mean out of every channel, so what is
    // left has nothing in common — every column sums to zero, whatever was in it.
    let after = frame(&g, "a re-referenced montage", reference, &referenced, |d| shape(d) == vec![4, 1024]);
    let v = f32s(&after);
    let worst = (0..1024).map(|t| (0..4).map(|c| v[c * 1024 + t]).sum::<f32>().abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-3, "an average reference leaves nothing common: worst column sums to {worst}");

    // Step: the matrix a connectivity node owes, whatever the numbers in it are.
    g.set_param(connectivity, "connectivity", "method", "plv");
    g.set_param(connectivity, "connectivity", "low", 8.0);
    g.set_param(connectivity, "connectivity", "high", 13.0);
    let m = frame(&g, "a connectivity matrix", connectivity, &matrix, |d| shape(d) == vec![4, 4]);
    let w = f32s(&m);
    let at = |i: usize, j: usize| w[i * 4 + j];
    for i in 0..4 {
        assert!((at(i, i) - 1.0).abs() < 1e-3, "a channel is perfectly locked to itself, not {}", at(i, i));
        for j in 0..4 {
            assert!((at(i, j) - at(j, i)).abs() < 1e-4, "the matrix is symmetric");
            assert!((0.0..=1.0001).contains(&at(i, j)), "phase locking is a fraction: {}", at(i, j));
        }
    }
    assert_eq!(labels(&m, "dim0"), ["Fz", "Cz", "Pz", "Oz"], "the matrix keeps the channel names");
    assert_eq!(labels(&m, "dim1"), ["Fz", "Cz", "Pz", "Oz"], "on BOTH axes, so it reads either way round");
    let strongest = (0..4)
        .flat_map(|i| (0..4).filter(move |j| *j != i).map(move |j| (i, j)))
        .map(|(i, j)| at(i, j))
        .fold(0.0f32, f32::max);
    assert!(strongest < 0.95, "independent channels do not lock: the strongest pair reads {strongest}");

    // Step: the graph node answers per node and for the whole graph, and keeps the channel names.
    let d = frame(&g, "a degree per channel", graph, &degree, |d| shape(d) == vec![4]);
    assert!(f32s(&d).iter().all(|x| x.is_finite()), "every degree is a number: {:?}", f32s(&d));
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz", "Pz", "Oz"], "a per-node measure is labelled by node");
    let c = frame(&g, "one clustering coefficient", graph, &clustering, |d| shape(d) == vec![1]);
    assert!((0.0..=1.0001).contains(&f32s(&c)[0]), "clustering is a fraction: {}", f32s(&c)[0]);

    // Step: c2 near zero is what MONOFRACTAL means, and noise is monofractal.
    let got = frame(&g, "c2 per channel", leaders, &c2, |d| shape(d) == vec![4]);
    let values = f32s(&got);
    assert!(values.iter().all(|x| x.is_finite()), "c2 is a number for every channel: {values:?}");
    assert!(values.iter().all(|x| x.abs() < 0.6), "noise is not strongly multifractal: {values:?}");

    capture_windows(&g);
}

#[test]
fn a_connectivity_matrix_tells_a_shared_rhythm_from_two_that_are_not() {
    let _py = require_python();
    let g = Goofi::new();

    // Two channels cut from ONE oscillator: 2048 samples reshaped to two rows of 1024 is the same
    // 10 Hz rhythm twice, offset in time — perfectly phase-locked, and built with no second
    // producer to fall out of step with. A pair of independent channels could not fail the claim
    // this test makes, and so could not test it.
    let osc = g.add("LFO");
    g.set_param(osc, "output", "sfreq", 250.0);
    g.set_param(osc, "output", "mode", "block");
    g.set_param(osc, "lfo", "frequency", 10.0);
    let split = g.add("Reshape");
    g.set_param(split, "reshape", "shape", "2, 1024");
    g.link(window(&g, osc, 2048), "out", split, "input");
    // `Reshape` does not carry the sample rate across a change of shape, and a band needs one.
    let rated = g.add("Meta");
    g.set_param(rated, "meta", "sfreq", 250.0);
    g.link(split, "out", rated, "input");

    let locking = |source: Uid| {
        let node = g.add(&bundled(&g, "eeg", "connectivity.py"));
        g.set_param(node, "connectivity", "method", "plv");
        g.set_param(node, "connectivity", "low", 8.0);
        g.set_param(node, "connectivity", "high", 13.0);
        let probe = g.probe(node, "matrix");
        g.link(source, "out", node, "data");
        (node, probe)
    };
    let (paired, locked) = locking(rated);
    let m = frame(&g, "the rhythm's own matrix", paired, &locked, |d| shape(d) == vec![2, 2]);
    let shared = f32s(&m)[1];
    assert!(shared > 0.9, "two cuts of one rhythm are locked, and read {shared}");

    // The same node and the same settings on noise instead, which is the comparison that makes the
    // number above mean something rather than merely be large.
    let noise = g.add("signal:Noise");
    g.set_param(noise, "output", "sfreq", 250.0);
    g.set_param(noise, "output", "mode", "block");
    let cut = g.add("Reshape");
    g.set_param(cut, "reshape", "shape", "2, 1024");
    g.link(window(&g, noise, 2048), "out", cut, "input");
    let quiet = g.add("Meta");
    g.set_param(quiet, "meta", "sfreq", 250.0);
    g.link(cut, "out", quiet, "input");
    let (apart, scattered) = locking(quiet);

    let n = frame(&g, "the noise matrix", apart, &scattered, |d| shape(d) == vec![2, 2]);
    let independent = f32s(&n)[1];
    assert!(
        shared > independent + 0.3,
        "the shared rhythm locks and the noise does not: {shared:.3} against {independent:.3}"
    );
}

fn capture_windows(g: &Goofi) {
    let source_type = install(g, "epoch_windows.py", r#"
import goofi
import numpy as np

class EpochWindows(goofi.Node):
    OUTPUTS = {"data": goofi.DataType.ARRAY, "trigger": goofi.DataType.ARRAY}
    PRODUCER = True
    PARAMS = {"emit": {
        "data": goofi.PulseParam(), "trigger": goofi.PulseParam(),
        "value": goofi.FloatParam(1.0, -100.0, 100.0), "width": goofi.IntParam(3, 1, 32),
        "level": goofi.FloatParam(1.0, 0.0, 1.0), "flat": goofi.BoolParam(False),
    }}

    def setup(self):
        self.pending = {}

    def pulse_emit_data(self):
        x = np.arange(self.params.emit.width, dtype=np.float32) + self.params.emit.value
        if self.params.emit.flat:
            self.pending["data"] = (x, {"channels": {"dim0": [str(i) for i in range(x.size)]}})
        else:
            self.pending["data"] = (x.reshape(1, -1), {"channels": {"dim0": ["Cz"]}})

    def pulse_emit_trigger(self):
        self.pending["trigger"] = np.array([self.params.emit.level], dtype=np.float32)

    def process(self):
        result, self.pending = self.pending, {}
        return result
"#);
    let source = g.add(&source_type);
    g.set_param(source, "common", "autotrigger", true);
    g.set_param(source, "common", "max_frequency", 100.0);
    let epoch = g.add(&bundled(g, "eeg", "epoch.py"));
    let count = g.probe(epoch, "count");
    let latest = g.probe(epoch, "latest");
    let average = g.probe(epoch, "erp");
    let sent = g.probe(source, "data");
    g.link(source, "data", epoch, "data");
    g.link(source, "trigger", epoch, "trigger");
    g.ready(source);
    g.ready(epoch);
    let pulse = |param: &str| {
        g.call("node param pulse", json!({"node": hex(source), "param": format!("emit/{param}")}));
    };
    let held = |n: f32| {
        assert!(g.stays(|_| count.latest().is_some_and(|d| f32s(&d) == [n])), "capture count must remain {n}");
    };
    let capture = |n: f32| {
        pulse("trigger");
        frame(g, "one accepted epoch", epoch, &count, |d| f32s(d) == [n]);
    };
    let supply = |value: f64, width: i64| {
        g.set_param(source, "emit", "value", value);
        g.set_param(source, "emit", "width", width);
        pulse("data");
        frame(g, "the supplied window", source, &sent, |d| {
            shape(d) == vec![1, width as usize] && f32s(d)[0] == value as f32
        });
    };

    pulse("trigger");
    assert!(g.stays(|_| count.latest().is_none()), "a trigger without data captures nothing");
    supply(1.0, 3);
    assert!(g.stays(|_| count.latest().is_none()), "data must not reuse an old trigger");
    capture(1.0);
    let first = frame(g, "the complete first window", epoch, &latest, |d| f32s(d) == [1.0, 2.0, 3.0]);
    assert_eq!(labels(&first, "dim0"), ["Cz"]);
    pulse("trigger");
    held(1.0);
    supply(3.0, 3);
    held(1.0);
    capture(2.0);
    frame(g, "the mean of two windows", epoch, &average, |d| f32s(d) == [2.0, 3.0, 4.0]);

    g.set_param(source, "emit", "level", 0.0);
    supply(5.0, 3);
    held(2.0);
    pulse("trigger");
    held(2.0);
    g.set_param(source, "emit", "level", 1.0);
    capture(3.0);
    frame(g, "a low trigger leaves the window available", epoch, &average, |d| f32s(d) == [3.0, 4.0, 5.0]);

    g.set_param(epoch, "epoch", "reject", 1.0);
    supply(7.0, 3);
    held(3.0);
    pulse("trigger");
    held(3.0);
    g.set_param(epoch, "epoch", "reject", 0.0);
    pulse("trigger");
    held(3.0);

    g.set_param(epoch, "epoch", "baseline", "whole");
    supply(9.0, 3);
    held(3.0);
    capture(4.0);
    frame(g, "whole-window baseline correction", epoch, &average, |d| f32s(d) == [-1.0, 0.0, 1.0]);

    g.set_param(epoch, "epoch", "baseline", "none");
    supply(11.0, 2);
    held(4.0);
    capture(1.0);
    frame(g, "a changed shape starts a new average", epoch, &average, |d| f32s(d) == [11.0, 12.0]);
    g.call("node param pulse", json!({"node": hex(epoch), "param": "epoch/reset"}));
    supply(21.0, 2);
    held(1.0);
    pulse("trigger");
    frame(g, "a reset forgets previous windows", epoch, &average, |d| f32s(d) == [21.0, 22.0]);
    held(1.0);
    g.set_param(source, "emit", "flat", true);
    g.set_param(source, "emit", "value", 31.0);
    g.set_param(epoch, "epoch", "baseline", "whole");
    pulse("data");
    frame(g, "a labeled one-dimensional window", source, &sent, |d| shape(d) == vec![2]);
    held(1.0);
    pulse("trigger");
    for probe in [&average, &latest] {
        let flat = frame(g, "baseline preserves the supplied dimensions", epoch, probe, |d| {
            shape(d) == vec![2] && f32s(d) == [-0.5, 0.5]
        });
        assert_eq!(labels(&flat, "dim0"), ["0", "1"]);
    }
    held(1.0);
    assert!(g.error(epoch).is_none(), "window capture leaves no error");
}
