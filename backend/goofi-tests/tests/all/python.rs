//! Python nodes, driven the way a user drives them: a `.py` file in the patch's workspace, probed,
//! registered, added to the graph and run — on whichever tier its imports allow. The in-process
//! tier needs `--features embed`, which LINKS libpython.

use goofi_tests::{ep, f32s, hex, install, require_python, j, text, Goofi, Uid};

/// Set a consumer to free-run, so it produces with nothing wired upstream.
fn free_run(g: &Goofi, uid: Uid, hz: f64) {
    g.set_param(uid, "common", "autotrigger", true);
    g.set_param(uid, "common", "max_frequency", hz);
}

const AFFINE: &str = r#"
import goofi
class Affine(goofi.Node):
    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {"gain": {"factor": goofi.IntParam(1, 0, 100)}}
    def setup(self):
        self._base = 10
    def process(self, data):
        return {"out": data.data * self.params.gain.factor + self._base}
"#;

#[test]
fn a_python_file_in_the_workspace_becomes_a_node_that_runs_and_takes_its_params() {
    let _py = require_python();
    let g = Goofi::new();
    let ty = install(&g, "affine.py", AFFINE);
    assert_eq!(ty, "Affine", "the type is named after the file stem");

    let row = g.call("library list", j!({ "full": true }))["types"].as_array().unwrap().iter()
        .find(|t| t["type"] == "signal:Affine").expect("Affine is in the palette").clone();
    assert_eq!(row["input_slots"]["data"], "ARRAY", "{row}");
    assert_eq!(row["output_slots"]["out"], "ARRAY", "{row}");
    assert_eq!(row["params"]["gain"]["factor"]["value"], 1, "{row}");

    let src = g.add("_TestConst");
    g.set_param(src, "constant", "value", 1.0);
    let node = g.add("Affine");
    let probe = g.probe(node, "out");
    g.set_param(node, "gain", "factor", 3);
    g.link(src, "out", node, "data");

    // 1*3 + 10 = 13: the 10 proves `setup` ran in the child, the 3 that a live param crossed.
    g.until("the affine node's frame", |_| probe.latest().filter(|d| f32s(d)[0] == 13.0));

    g.set_param(node, "gain", "factor", 0);
    g.until("the re-parameterized node", |_| {
        probe.latest().filter(|d| f32s(d)[0] == 10.0).map(|_| ())
    });
    assert!(g.error(node).is_none(), "a healthy python node carries no error");
}

#[test]
fn a_python_node_that_raises_reports_it_and_the_child_carries_on() {
    // A raise inside `process` is a per-run error: the SAME child answers the next run.
    let _py = require_python();
    let g = Goofi::new();
    install(&g, "boom.py", r#"
import goofi
import numpy as np
class Boom(goofi.Node):
    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    def setup(self):
        self._runs = 0
    def process(self, data):
        self._runs += 1
        if data.data[0] < 0:
            raise ValueError("the sensor read negative")
        return {"out": np.array([float(self._runs)], dtype="float32")}
"#);
    let src = g.add("_TestConst");
    let node = g.add("Boom");
    let probe = g.probe(node, "out");
    g.set_param(src, "constant", "value", -1.0);
    g.link(src, "out", node, "data");

    let why = g.until("the raise to surface", |g| g.error(node));
    assert!(why.contains("the sensor read negative"), "the Python exception text rides back: {why}");

    // A count above 1 proves the child never died — a respawn would reset `_runs`.
    g.set_param(src, "constant", "value", 1.0);
    let d = g.until("the recovered node", |_| probe.latest().filter(|d| f32s(d)[0] > 1.0));
    assert!(f32s(&d)[0] > 1.0, "the child survived the raise with its state: {:?}", f32s(&d));
    g.until("the error to clear", |g| g.error(node).is_none().then_some(()));

    // Streams its idle zero until the first edit, so a consumer can prove the wire carries before
    // the one-shot sends: a frame published before the consumer's subscribe phase lands is gone.
    install(&g, "once.py", r#"
import goofi
import numpy as np
class Once(goofi.Node):
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PRODUCER = True
    PARAMS = {"send": {"value": goofi.IntParam(0, 0, 100)}}
    def setup(self):
        self.last = 0
    def process(self):
        value = self.params.send.value
        if value == self.last and value != 0:
            return None
        self.last = value
        return np.array([value], dtype=np.float32)
"#);
    install(&g, "consume.py", r#"
import goofi
import numpy as np
class Consume(goofi.Node):
    INPUTS = {"data": goofi.InputSlot(goofi.DataType.ARRAY, multi=True)}
    OUTPUTS = {"out": goofi.DataType.ARRAY, "ran": goofi.DataType.ARRAY}
    PARAMS = {"consume": {"mode": goofi.IntParam(0, 0, 4)}}
    def process(self, data):
        mode = self.params.consume.mode
        ran = np.array([mode], dtype=np.float32)
        if mode:
            self.clear_input("data")
        if mode == 2:
            return {"ran": ran}
        if mode == 3:
            raise ValueError("consume failed")
        if mode == 4:
            self.clear_input("missing")
        held = [d for _, d in data if d is not None]
        return {"out": np.array([mode, sum(float(d.data[0]) for d in held), len(held)], dtype=np.float32), "ran": ran}
"#);
    let first = g.add("Once");
    let second = g.add("Once");
    for source in [first, second] {
        g.set_param(source, "common", "max_frequency", 20.0);
    }
    let consume = g.add("Consume");
    let consumed = g.probe(consume, "out");
    let ran = g.probe(consume, "ran");
    free_run(&g, consume, 20.0);
    g.link(first, "out", consume, "data");
    g.link(second, "out", consume, "data");
    g.ready(first);
    g.ready(second);
    // `[mode, sum of the held inputs, how many are held]`.
    let sees = |mode: f32, sum: f32, held: f32| {
        let before = consumed.latest().and_then(|d| d.meta().index());
        g.until(&format!("the held input readout [{mode}, {sum}, {held}]"), |_| {
            consumed.latest().filter(|d| d.meta().index() > before && f32s(d) == [mode, sum, held])
        });
    };
    // Both wires carry before a value is sent once: what is published before that is lost.
    sees(0.0, 0.0, 2.0);
    g.set_param(first, "send", "value", 2);
    g.set_param(second, "send", "value", 3);
    sees(0.0, 5.0, 2.0);
    g.set_param(consume, "consume", "mode", 1);
    sees(1.0, 0.0, 0.0);
    g.set_param(consume, "consume", "mode", 0);
    sees(0.0, 0.0, 0.0);
    g.set_param(first, "send", "value", 4);
    g.set_param(second, "send", "value", 6);
    sees(0.0, 10.0, 2.0);
    g.set_param(consume, "consume", "mode", 3);
    let why = g.until("a failed consumption", |g| g.error(consume));
    assert!(why.contains("consume failed"), "{why}");
    g.set_param(consume, "consume", "mode", 0);
    sees(0.0, 10.0, 2.0);
    g.until("failed process recovery", |g| g.error(consume).is_none().then_some(()));
    g.set_param(consume, "consume", "mode", 4);
    let why = g.until("an invalid clear request", |g| g.error(consume));
    assert!(why.contains("missing"), "{why}");
    g.set_param(consume, "consume", "mode", 0);
    sees(0.0, 10.0, 2.0);
    g.until("clear failure recovery", |g| g.error(consume).is_none().then_some(()));
    // A run that clears and answers no `out` frame is seen on the slot it does answer: a fixed
    // wait here let a starved child miss the mode before it moved on, and the inputs stayed held.
    g.set_param(consume, "consume", "mode", 2);
    g.until("a clearing run that answers nothing", |_| ran.latest().filter(|d| f32s(d)[0] == 2.0));
    g.set_param(consume, "consume", "mode", 0);
    sees(0.0, 0.0, 0.0);
    g.set_param(second, "send", "value", 7);
    sees(0.0, 7.0, 1.0);
}

#[test]
fn a_python_node_writing_to_stdout_does_not_corrupt_the_transport() {
    // Text streams are captured independently of the shared-memory frames.
    let _py = require_python();
    let g = Goofi::new();
    install(&g, "chatty.py", r#"
import sys
import goofi
class Chatty(goofi.Node):
    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    def process(self, data):
        print("debug from the node", flush=True)
        print("interleaved output", flush=True)
        print("debug from the node", flush=True)
        print("node stderr", file=sys.stderr, flush=True)
        return {"out": data.data * 2.0}
"#);
    let src = g.add("_TestCounter");
    let node = g.add("Chatty");
    let probe = g.probe(node, "out");
    g.link(src, "out", node, "data");

    let first = f32s(&g.until("a frame from the printing node", |_| probe.latest()))[0];
    let second = g.until("a later frame, still in sync", |_| {
        probe.latest().filter(|d| f32s(d)[0] > first)
    });
    assert_eq!(f32s(&second)[0] % 2.0, 0.0, "every frame is the doubled counter: {:?}", f32s(&second));
    assert!(g.error(node).is_none(), "the transport stayed in sync: {:?}", g.error(node));
    g.until("both text streams reach the log with node identity", |g| {
        let snapshot = g.call("log list", j!({}));
        let rows = snapshot["groups"].as_array()?;
        let out = rows.iter().find(|row| row["node"] == hex(node) && row["text"] == "debug from the node" && row["stream"] == "stdout")?;
        rows.iter().find(|row| row["node"] == hex(node) && row["text"] == "node stderr" && row["stream"] == "stderr")?;
        (out["count"].as_u64()? >= 2).then_some(())
    });
}

#[test]
fn a_node_whose_setup_raises_retries_the_whole_initialization_on_the_next_wake() {
    let _py = require_python();
    let g = Goofi::new();
    install(&g, "late_boot.py", r#"
import goofi
import numpy as np
class LateBoot(goofi.Node):
    setups = 0
    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    def setup(self):
        LateBoot.setups += 1
        if LateBoot.setups < 2:
            raise RuntimeError("the device is not open")
    def process(self, data):
        return {"out": np.array([float(LateBoot.setups)], dtype="float32")}
"#);
    let src = g.add("_TestCounter");
    let node = g.add("LateBoot");
    let probe = g.probe(node, "out");
    g.link(src, "out", node, "data");

    // The frame VALUE is the oracle: a 2 says the whole initialization ran again on one instance.
    let d = g.until("the retried initialization", |_| probe.latest());
    assert_eq!(f32s(&d)[0], 2.0, "setup ran a second time and the node came up");
}

#[test]
fn a_node_missing_a_dependency_is_listed_greyed_rather_than_vanishing() {
    let py = require_python();
    let g = Goofi::new();
    // An interpreter of this test's own: the install below touches its site-packages, and a file
    // put in the shared venv's would move the memo key of every probed node for every boot after.
    let own = tempfile::tempdir().unwrap();
    let venv = own.path().join("venv");
    let made = std::process::Command::new(&py.py).args(["-m", "venv", "--without-pip"]).arg(&venv)
        .env_remove("PYTHONPATH").env_remove("PYTHONHOME").status().unwrap();
    assert!(made.success(), "a venv of the test's own");
    let site = goofi_init::site_packages(&venv).expect("the new venv's site-packages");
    let shared = std::path::Path::new(&py.py).parent().unwrap().parent().unwrap();
    let shared = goofi_init::site_packages(shared).expect("the test interpreter is a venv");
    std::fs::write(site.join("goofi_shared.pth"), shared.to_string_lossy().as_bytes()).unwrap();
    let own_py = goofi_init::venv_python(&venv).expect("the new venv's python").to_string_lossy().into_owned();
    let dir = g.state.mount().join("nodes_signal");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("needs_scipy.py");
    // Named per process, so two suites probing at once cannot install for each other.
    let module = format!("definitely_not_installed_{}", std::process::id());
    std::fs::write(
        &path,
        format!("import goofi\nimport {module}\n\nclass NeedsScipy(goofi.Node):\n    OUTPUTS = {{\"out\": goofi.DataType.ARRAY}}\n"),
    )
    .unwrap();

    let memo = tempfile::tempdir().unwrap();
    match goofi_python::subproc::probe(&path, &own_py, memo.path()) {
        goofi_python::Discovery::Unavailable { type_name, reason } => {
            assert_eq!(type_name, "NeedsScipy");
            assert!(reason.contains(&module), "the reason names the module: {reason}");
            let ty = goofi_node::qualify("signal", &type_name);
            g.state.graph.lock().unwrap().register_unavailable(ty, reason);
        }
        goofi_python::Discovery::Found(_) =>
            panic!("a node with a missing import must not probe as loadable"),
        goofi_python::Discovery::Skip => panic!("the file was not taken for a node file at all"),
    }
    let row = g.call("library list", j!({}))["types"].as_array().unwrap().iter()
        .find(|t| t["type"] == "signal:NeedsScipy").expect("the greyed row is in the palette").clone();
    assert_eq!(row["available"], false, "{row}");
    assert!(row["doc"].as_str().unwrap().contains(&module), "{row}");
    g.refuse("node add", j!({ "type": "NeedsScipy" }));

    // Installed into the interpreter's site-packages, the module lights the node up on the next
    // probe: a probe's memo is keyed on that directory, so the install itself moves the key.
    std::fs::write(site.join(format!("{module}.py")), "").unwrap();
    match goofi_python::subproc::probe(&path, &own_py, memo.path()) {
        goofi_python::Discovery::Found(found) => assert_eq!(found.manifest.type_name, "NeedsScipy"),
        goofi_python::Discovery::Unavailable { reason, .. } => panic!("installed, and the probe still answers from before it: {reason}"),
        goofi_python::Discovery::Skip => panic!("the file was not taken for a node file at all"),
    }
    // A loadable row says nothing about availability: the index spends that key on greyed rows alone.
    let row = g.call("library list", j!({}))["types"].as_array().unwrap().iter()
        .find(|t| t["type"] != "signal:NeedsScipy").expect("a loadable row").clone();
    assert!(row.get("available").is_none(), "{row}");
}

#[test]
fn a_nodes_own_python_thread_runs_while_the_child_is_idle() {
    // A GIL held across the child's idle serve loop would starve a `setup()` thread for ever.
    let _py = require_python();
    let g = Goofi::new();
    install(&g, "ticker.py", r#"
import threading, time
import numpy as np
import goofi
class Ticker(goofi.Node):
    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    def setup(self):
        self.count = 0
        def spin():
            while True:
                self.count += 1
                time.sleep(0.001)
        threading.Thread(target=spin, daemon=True).start()
    def process(self, data):
        return {"out": np.array([float(self.count)], dtype="float32")}
"#);
    let node = g.add("Ticker");
    let probe = g.probe(node, "out");
    // Slowly, so most of the window below is genuine idle.
    free_run(&g, node, 2.0);
    let first = f32s(&g.until("the ticker's first frame", |_| probe.latest()))[0];
    g.until("the node's own thread to tick ten times while the child idles", |_| {
        probe.latest().filter(|d| f32s(d)[0] > first + 10.0)
    });
}

const PULSE_COUNTER: &str = include_str!("../fixtures/pulse_counter.py");
const SOURCES: &str = include_str!("../fixtures/sources.py");
const STITCHER: &str = include_str!("../fixtures/stitcher.py");
const AXES: &str = include_str!("../fixtures/axes.py");

#[test]
fn a_python_multi_slot_names_its_senders_and_follows_a_rename() {
    // The probe honours `multi`, and a multi slot reaches `process` as `list[tuple[str, Data]]`
    // in wire order; a rename reaches the child, and a removed wire takes its name with it.
    let _py = require_python();
    let g = Goofi::new();
    install(&g, "sources.py", SOURCES);
    let row = g.call("library list", j!({ "full": true }))["types"].as_array().unwrap().iter()
        .find(|t| t["type"] == "signal:Sources").expect("Sources is in the palette").clone();
    assert_eq!(row["input_multi"], j!(["input"]), "{row}");

    let a = g.add("_TestCounter");
    let b = g.add("_TestCounter");
    let s = g.add("Sources");
    g.call("node edit", j!({ "node": hex(a), "name": "alpha" }));
    g.call("node edit", j!({ "node": hex(b), "name": "beta" }));
    let probe = g.probe(s, "out");
    let probe = &probe;
    let names = |want: &'static str| move |_: &Goofi| probe.latest().filter(|d| text(d) == Some(want)).map(|_| ());
    g.link(b, "out", s, "input");
    g.link(a, "out", s, "input");
    g.until("both senders named, in wire order", names("beta.out,alpha.out"));
    g.call("node edit", j!({ "node": hex(b), "name": "gamma" }));
    g.until("the rename to reach the child", names("gamma.out,alpha.out"));
    g.call("link remove", j!({ "from": ep(hex(b), "out"), "to": ep(hex(s), "input") }));
    g.until("one sender left", names("alpha.out"));
    assert!(g.error(s).is_none(), "a healthy python node carries no error");
}

#[test]
fn a_python_pulse_param_is_a_request_the_node_answers_with_a_hook() {
    // The probe carries the pulse spec through the wheel, the op crosses to the child as a
    // request of its own, and `pulse_count_reset` answers it.
    let _py = require_python();
    let g = Goofi::new();
    install(&g, "pulse_counter.py", PULSE_COUNTER);
    let row = g.call("library list", j!({ "full": true }))["types"].as_array().unwrap().iter()
        .find(|t| t["type"] == "signal:PulseCounter").expect("PulseCounter is in the palette").clone();
    assert_eq!(row["params"]["count"]["reset"]["type"], "pulse", "{row}");
    assert!(row["params"]["count"]["reset"]["value"].is_null(), "{row}");

    let node = g.add("PulseCounter");
    let probe = g.probe(node, "out");
    free_run(&g, node, 50.0);
    let under = |c: f32| probe.latest().map(|d| f32s(&d)[0]).is_some_and(|v| v < c);
    let before = f32s(&g.until("a count past twenty", |_| probe.latest().filter(|d| f32s(d)[0] > 20.0)))[0];
    g.call("node param request", j!({ "node": hex(node), "param": "count/reset", "request": "pulse" }));
    g.until("the count to start over", |_| under(before).then_some(()));
    assert!(g.error(node).is_none(), "a pulse leaves no error");
}

/// The coords on one dimension of a frame, as strings.
fn labels(d: &goofi_core::Data, dim: &str) -> Vec<String> {
    d.meta()
        .channels()
        .dims()
        .find(|(k, _)| k == dim)
        .map(|(_, coords)| {
            coords
                .iter()
                .map(|c| match c {
                    goofi_core::Coord::Str(s) => s.to_string(),
                    goofi_core::Coord::Num(n) => n.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_python_node_stitches_its_inputs_past_and_a_pulse_forgets_it() {
    // `goofi.Stream` is the node's whole memory: the counter steps by one per FRAME, so a node
    // reaching four samples back reads exactly four — which a node holding no past cannot.
    let _py = require_python();
    let g = Goofi::new();
    install(&g, "stitcher.py", STITCHER);
    let src = g.add("_TestCounter");
    let node = g.add("Stitcher");
    g.link(src, "out", node, "input");
    free_run(&g, src, 20.0);
    let probe = g.probe(node, "out");
    let probe = &probe;
    let all_four = |_: &Goofi| {
        probe.latest().filter(|d| f32s(d).iter().all(|v| (v - 4.0).abs() < 1e-6)).map(|_| ())
    };
    g.until("the stitched past to reach back four samples", all_four);

    g.call("node param request", j!({ "node": hex(node), "param": "stitcher/reset", "request": "pulse" }));
    g.until("the forgotten past to read short", |_| {
        probe.latest().filter(|d| f32s(d).iter().any(|v| *v < 4.0)).map(|_| ())
    });
    g.until("the past to fill again", all_four);
    assert!(g.error(node).is_none(), "a stitching node carries no error");
}

#[test]
fn a_python_node_reads_every_meta_rule_off_the_pymod() {
    // One implementation of each rule lives in the core; a Python node reaches it through the
    // wheel, so the labels and the rate on the wire ARE the core's answer.
    let _py = require_python();
    let g = Goofi::new();
    install(&g, "axes.py", AXES);
    let node = g.add("Axes");
    free_run(&g, node, 50.0);
    let probe = g.probe(node, "out");
    let probe = &probe;
    let under = |g: &Goofi, rule: &str, shape: Vec<usize>| {
        g.set_param(node, "axes", "rule", rule);
        g.until(rule, |_| {
            probe.latest().filter(|d| d.as_array().map(|a| a.shape() == shape).unwrap_or(false))
        })
    };

    let d = under(&g, "drop_last", vec![2]);
    assert_eq!(labels(&d, "dim0"), ["a", "b"], "the kept axis keeps its labels");
    assert_eq!(labels(&d, "dim1"), Vec::<String>::new(), "the dropped axis takes its labels");
    assert_eq!(d.meta().sfreq(), None, "the rate goes with the last axis");

    let d = under(&g, "drop_first", vec![3]);
    assert_eq!(labels(&d, "dim0"), ["1", "2", "3"], "the higher axis moved down with its labels");
    assert_eq!(d.meta().sfreq(), Some(250.0), "the last axis is still time, so the rate stays");

    let d = under(&g, "keep", vec![2, 2]);
    assert_eq!(labels(&d, "dim1"), ["1", "3"], "a subset keeps the selected labels, in order");

    let d = under(&g, "insert_first", vec![1, 2, 3]);
    assert_eq!(labels(&d, "dim0"), ["only"], "the new axis carries the names it was given");
    assert_eq!(labels(&d, "dim2"), ["1", "2", "3"], "the old axes moved up with their labels");
    assert_eq!(d.meta().sfreq(), Some(250.0), "an axis inserted before time leaves the rate");

    let d = under(&g, "insert_last", vec![2, 3, 1]);
    assert_eq!(labels(&d, "dim2"), Vec::<String>::new(), "a new axis is born unlabelled");
    assert_eq!(d.meta().sfreq(), None, "a new LAST axis takes the rate away");

    let d = under(&g, "concat", vec![2, 6]);
    assert_eq!(labels(&d, "dim1"), ["1", "2", "3", "1", "2", "3"], "a join concatenates in order");
    assert!(g.error(node).is_none(), "the meta rules leave no error");
}

#[cfg(feature = "embed")]
mod inproc {
    use std::time::Duration;

    use super::*;
    use goofi_core::{Data, Meta, Param, Value};
    use goofi_node::{ParamGroups, Params};
    use goofi_signal_sdk::{Inputs, MultiFrames, Node, NodeCtx, Outputs};
    use goofi_python::inproc::PyNode;
    use goofi_python::subproc::RemoteNode;
    use indexmap::IndexMap;

    /// Run one node once with `data` on its single slot, one frame per source on its multi slot
    /// `input`, and params, reading back `out`. Below the graph: what is compared is the MARSHALLING.
    fn once(node: &mut dyn Node, input: Option<Data>, sources: &[&str], params: &ParamGroups)
        -> (goofi_signal_sdk::NodeResult, Option<Data>) {
        let singles: IndexMap<&'static str, Option<Data>> = IndexMap::from([("data", input)]);
        let mut multis = MultiFrames::new();
        multis.insert("input", sources.iter().map(|s| (s.to_string(), frame())).collect());
        let inp = Inputs::with_multi(&singles, &multis);
        let mut outmap: IndexMap<&'static str, Option<Data>> = IndexMap::new();
        outmap.insert("out", None);
        let mut ctx = NodeCtx::new();
        let res = {
            let mut out = Outputs::new(&mut outmap);
            node.process(&inp, &mut out, &mut ctx, &Params::new(params))
        };
        (res, outmap.get("out").unwrap().clone())
    }

    fn frame() -> Data {
        let bytes: Vec<u8> = [1.0f32, 2.0, 3.0].iter().flat_map(|x| x.to_le_bytes()).collect();
        Data::array_f32(vec![3], bytes, Meta::new().with_sfreq(Some(250.0))).unwrap()
    }

    /// The interpreter the subprocess half of a parity test spawns.
    fn subproc_python() -> String {
        let ft = goofi_python::inproc::interpreter_path()
            .expect("no FT interpreter (PYO3_PYTHON) — run `cargo run -p goofi-init`");
        std::env::var("GOOFI_SUBPROC_TEST_PYTHON").unwrap_or(ft)
    }

    /// One authored node exercised by both tiers. Its int32 output makes both run the shared
    /// cast-to-f32 and carry-input-meta paths.
    const PARITY: &str = r#"
import goofi
import numpy as np
class Parity(goofi.Node):
    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {"gain": {"factor": goofi.IntParam(1, 0, 100)}}
    def setup(self):
        self._base = 10
    def process(self, data):
        return {"out": (data.data * self.params.gain.factor + self._base).astype(np.int32)}
"#;

    /// The same node, able to tell "called with None" from "never called at all".
    const ABSENT: &str = r#"
import goofi
import numpy as np
class Absent(goofi.Node):
    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    def process(self, data):
        if data is None:
            return {"out": np.array([-1.0], dtype=np.float32)}
        return {"out": data.data * 2.0}
"#;

    #[test]
    fn one_source_run_on_both_tiers_produces_the_same_frame() {
        let py = subproc_python();
        let mut params = ParamGroups::new();
        params.insert("gain".into(), IndexMap::from([("factor".to_string(), Param::int(3, 0, 100))]));

        // In-process seeds params and runs setup; the child runs setup lazily on its first request.
        let mut here = PyNode::from_source(PARITY, vec![("data", false)], vec!["out"]).expect("PyNode");
        here.setup(&mut NodeCtx::new(), &Params::new(&params)).expect("in-process setup");
        let (_, a) = once(&mut here, Some(frame()), &[], &params);
        let mut there = RemoteNode::new(&py, PARITY, vec![("data", false)]);
        let (_, b) = once(&mut there, Some(frame()), &[], &params);

        let (a, b) = (a.expect("in-process frame"), b.expect("subprocess frame"));
        assert_eq!(f32s(&a), f32s(&b), "the two tiers must produce identical values");
        assert_eq!(f32s(&a), vec![13.0, 16.0, 19.0], "shared cast + param + setup base");
        assert_eq!((a.meta().sfreq(), b.meta().sfreq()), (Some(250.0), Some(250.0)),
                   "identical carried meta");
        match (a.value(), b.value()) {
            (Value::Array(sa), Value::Array(sb)) => assert_eq!(sa.shape(), sb.shape()),
            _ => panic!("both tiers must return arrays"),
        }
    }

    #[test]
    fn both_tiers_pass_a_declared_input_with_no_frame_as_none() {
        // The subprocess wire carries only the slots that HOLD a frame, so the child widens it back.
        let py = subproc_python();
        let p = ParamGroups::new();
        let mut here = PyNode::from_source(ABSENT, vec![("data", false)], vec!["out"]).expect("PyNode");
        here.setup(&mut NodeCtx::new(), &Params::new(&p)).expect("in-process setup");
        let (a_res, a) = once(&mut here, None, &[], &p);
        let mut there = RemoteNode::new(&py, ABSENT, vec![("data", false)]);
        let (b_res, b) = once(&mut there, None, &[], &p);

        assert!(a_res.is_ok(), "in-process tier errored on an absent input: {:?}", a_res.err());
        assert!(b_res.is_ok(), "subprocess tier errored on an absent input: {:?}", b_res.err());
        let a = a.expect("in-process emitted nothing — `process` was not called with None");
        let b = b.expect("subprocess emitted nothing — `process` was not called with None");
        assert_eq!(f32s(&a), f32s(&b), "the tiers must answer an absent input identically");
        assert_eq!(f32s(&a), vec![-1.0], "both took the node's own `data is None` branch");

        assert_eq!(f32s(&once(&mut here, Some(frame()), &[], &p).1.unwrap()), vec![2.0, 4.0, 6.0]);
        assert_eq!(f32s(&once(&mut there, Some(frame()), &[], &p).1.unwrap()), vec![2.0, 4.0, 6.0]);
    }

    #[test]
    fn several_python_nodes_run_at_once_inside_the_live_graph() {
        assert!(!PyNode::gil_enabled().unwrap(), "the interpreter must be free-threaded");
        // Each first run waits inside `process` until all four are there, on a barrier every
        // instance's module finds on `sys`: a serialized tier never lets the second one in.
        const MEETING: &str = r#"
import sys, threading
import goofi
class Meeting(goofi.Node):
    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    def process(self, data):
        if not getattr(self, "met", False):
            sys.__dict__.setdefault("_goofi_meeting", threading.Barrier(4)).wait()
            self.met = True
        return {"out": data.data * 2.0}
"#;
        static IN: &[goofi_node::SlotDecl] = &[goofi_node::SlotDecl {
            name: "data", kind: goofi_core::SlotType::Array,
            trigger_process: true, multi: false, required: false }];
        static OUT: &[goofi_node::OutputDecl] =
            &[goofi_node::OutputDecl { name: "out", kind: goofi_core::SlotType::Array }];
        static MEETING_TIER: goofi_node::IsolationCell =
            goofi_node::IsolationCell::new(goofi_node::Isolation::InProcess);
        static MANIFEST: goofi_node::NodeManifest = goofi_node::NodeManifest {
            type_name: "Meeting", tags: &[], doc: "meets three siblings inside its first run",
            inputs: IN, outputs: OUT, params: &[],
            producer: false,
        };

        let g = Goofi::new();
        g.register_dyn(&MANIFEST, Box::new(|_| {
            Box::new(PyNode::from_source(MEETING, vec![("data", false)], vec!["out"]).expect("PyNode"))
        }), &MEETING_TIER);
        let src = g.add("_TestCounter");
        let meeting: Vec<_> = (0..4).map(|_| g.add("Meeting")).collect();
        let probes: Vec<_> = meeting.iter().map(|u| g.probe(*u, "out")).collect();
        for u in &meeting {
            g.link(src, "out", *u, "data");
        }
        for p in &probes {
            g.until("all four inside `process` at once, and each to emit", |_| p.latest());
        }
        assert!(!PyNode::gil_enabled().unwrap(), "the GIL must stay disabled");
    }

    #[test]
    fn a_param_reference_reads_this_node_and_follows_its_edit() {
        let g = Goofi::new();
        g.state.graph.lock().unwrap().set_evaluator(std::sync::Arc::new(
            goofi_python::inproc::PyExprEvaluator::new().expect("the evaluator constructs")));
        let osc = g.add("LFO");
        let probe = g.probe(osc, "out");
        g.ready(osc);
        g.set_param(osc, "output", "mode", "block");
        g.set_param(osc, "lfo", "frequency", 8.0);
        // `me` is this node: the amplitude follows the node's OWN frequency param.
        let r = g.call("node param edit", j!({ "node": hex(osc), "param": "lfo/amplitude",
            "expression": "me.params.lfo.frequency / 4" }));
        assert!(r["error"].is_null(), "{r}");
        g.until("the amplitude to read 2 through `me`", |_| {
            probe.latest().filter(|d| f32s(d).iter().any(|v| v.abs() > 1.5)).map(|_| ())
        });
        // An authored edit of the referenced param re-binds its reader.
        g.set_param(osc, "lfo", "frequency", 2.0);
        g.until("the edit to re-evaluate the reader", |_| {
            probe.latest().filter(|d| f32s(d).iter().all(|v| v.abs() < 0.9)).map(|_| ())
        });
        // The same reference across nodes, by name.
        let osc2 = g.add("LFO");
        let probe2 = g.probe(osc2, "out");
        g.ready(osc2);
        g.set_param(osc2, "output", "mode", "block");
        let name = g.doc()["nodes"][hex(osc)]["name"].as_str().unwrap().to_string();
        let r = g.call("node param edit", j!({ "node": hex(osc2), "param": "lfo/amplitude",
            "expression": format!("nd('{name}').params.lfo.frequency + 1") }));
        assert!(r["error"].is_null(), "{r}");
        g.until("the cross-node read to evaluate", |_| {
            probe2.latest().filter(|d| f32s(d).iter().any(|v| v.abs() > 2.0)).map(|_| ())
        });
    }

    #[test]
    fn the_patch_rate_variable_re_rates_every_producer_at_once() {
        // `common.max_frequency` is BOUND to `variables.system.default_ufreq`, and a binding needs the evaluator.
        let g = Goofi::new();
        g.state.graph.lock().unwrap().set_evaluator(std::sync::Arc::new(
            goofi_python::inproc::PyExprEvaluator::new().expect("the evaluator constructs")));
        g.call("variable entry edit", j!({ "name": "system.default_ufreq", "value": 5.0 }));

        let osc = g.add("LFO");
        let probe = g.probe(osc, "out");
        g.ready(osc);
        let bound = g.doc()["nodes"][hex(osc)]["params"]["common"]["max_frequency"].clone();
        assert_eq!((&bound["expr"], &bound["mode"]), (&j!("variables.system.default_ufreq"), &j!("expression")),
                   "the manifest's declared binding was seeded live, not flattened to a literal");

        // Counting emitted frames is the only way to see a rate: a stated value reads correct anyway.
        let runs = |window: Duration| {
            let (mut seen, mut last, end) = (0, None, std::time::Instant::now() + window);
            while std::time::Instant::now() < end {
                let now = probe.latest().map(|d| d.meta().index());
                if now.is_some() && now != last {
                    seen += 1;
                    last = now;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            seen
        };
        let slow = runs(Duration::from_millis(800));
        assert!(slow <= 8, "5 Hz produced {slow} frames in 0.8 s — the variable is not pacing it");

        g.call("variable entry edit", j!({ "name": "system.default_ufreq", "value": 60.0 }));
        // Half the old period apart, which the 5 Hz rate never allows.
        goofi_tests::until_faster_than(&g, &probe, 0.1);

        // The evaluator's namespace: `math`'s names and `time()` are simply there, beside `np`.
        let r = g.call("node param edit", j!({ "node": hex(osc), "param": "lfo/amplitude",
            "expression": "2 * sin(pi / 2) + exp(0) * (1 if time() > 0 else 0)" }));
        assert!(r["error"].is_null(), "the namespace compiles: {r}");
        // The doc keeps the STORED value; the evaluated one shows in `node state`'s text.
        // Observed in the output, where an amplitude IS observable: a ±1 sine cannot cross 2.
        g.until("math and time to evaluate into the output's swing", |_| {
            probe.latest().filter(|d| f32s(d).iter().any(|v| v.abs() > 2.0)).map(|_| ())
        });
    }
}

#[test]
fn nodes_sharing_a_cyclic_package_all_construct_when_added_at_once() {
    // A package whose `__init__` imports a submodule that imports back through the package. Under
    // the GIL that cycle is legal; without one, two threads entering it at different points can
    // each hold the module lock the other wants, and CPython raises `_DeadlockError`. Four nodes
    // added in a burst is what loading a `.gfi` does, so it is the shape that must survive; only
    // `--features embed` puts them on the in-process tier, where the module bodies share an interpreter.
    let _py = require_python();
    let g = Goofi::new();
    let package = g.state.mount().join("cyc");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("__init__.py"), "from . import rings\nfrom .rings import tone\n").unwrap();
    std::fs::write(package.join("rings.py"), "import cyc\n\ndef tone(n):\n    return n * 2\n").unwrap();
    let root = g.state.mount().to_string_lossy().replace('\\', "/");
    let files: Vec<(String, String)> = [("cyc_one", "CycOne"), ("cyc_two", "CycTwo"), ("cyc_three", "CycThree"), ("cyc_four", "CycFour")]
        .iter().enumerate().map(|(i, (stem, class))| {
            let source = format!(
                "import sys\nsys.path.insert(0, {root:?})\nfrom cyc import tone\nimport goofi\nimport numpy as np\n\
                 class {class}(goofi.Node):\n    OUTPUTS = {{\"out\": goofi.DataType.ARRAY}}\n    PRODUCER = True\n\
                 \x20   def process(self):\n        return {{\"out\": np.array([tone({i})], dtype=np.float32)}}\n",
            );
            (format!("{stem}.py"), source)
        }).collect();
    let names = goofi_tests::install_all(&g, &files.iter().map(|(f, s)| (f.as_str(), s.as_str())).collect::<Vec<_>>());
    let added: Vec<Uid> = names.iter().map(|n| g.add(n)).collect();
    for (name, uid) in names.iter().zip(&added) {
        g.ready(*uid);
        assert!(g.error(*uid).is_none(), "{name} constructed: {:?}", g.error(*uid));
    }
}
