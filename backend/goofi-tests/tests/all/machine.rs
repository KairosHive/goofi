//! A state machine built through ops, read through its playheads' variables, and driven by every
//! trigger kind, then saved, reopened and undone.

use std::sync::Arc;

use goofi_tests::{f32s, hex, j, Goofi};
use serde_json::Value;

/// A playhead variable as text.
fn text(g: &Goofi, name: &str) -> String {
    g.variable(name).as_str().unwrap_or_default().to_string()
}

fn number(g: &Goofi, name: &str) -> f64 {
    g.variable(name).as_f64().unwrap_or(f64::NAN)
}

/// Poll until the playhead is at rest in `state`.
#[track_caller]
fn at_rest_in(g: &Goofi, playhead: &str, state: &str) {
    g.until(&format!("{playhead} to rest in {state}"), |g| {
        (text(g, &format!("{playhead}.state")) == state && number(g, &format!("{playhead}.progress")) == 1.0).then_some(())
    });
}

#[test]
fn a_machine_moves_its_playheads_through_its_states_and_writes_their_variables() {
    let g = Goofi::new();
    let evaluator = Arc::new(goofi_tests::FirstVar::default());
    g.graph().set_evaluator(evaluator.clone());
    let write = |op: &str, payload: Value| -> Value { g.call(op, payload) };

    assert_eq!(write("machine add", j!({ "name": "seq" }))["name"], "seq");
    assert!(g.doc()["machines"]["seq"].is_object(), "the machine is a document root: {}", g.doc()["machines"]);
    let why = g.refuse("machine add", j!({ "name": "seq" }));
    assert!(why.contains("already exists"), "{why}");
    // An attribute is a value every state may set; its kind must hold its default, and the
    // machine's own three element names are not an attribute's to take.
    let num = j!({ "type": "num", "vmin": 0.0, "vmax": 1.0 });
    write("machine attribute add", j!({ "machine": "seq", "name": "gain", "value": 0.0, "kind": num }));
    write("machine attribute add", j!({ "machine": "seq", "name": "label", "value": "a" }));
    assert_eq!(g.doc()["machines"]["seq"]["attributes"]["label"]["kind"]["type"], j!("string"), "a text default is a string attribute");
    let why = g.refuse("machine attribute add", j!({ "machine": "seq", "name": "state", "value": 0.0 }));
    assert!(why.contains("machine's own"), "{why}");
    let why = g.refuse("machine attribute add", j!({ "machine": "seq", "name": "shape", "value": "x", "kind": num }));
    assert!(why.contains("`num`") && why.contains("string"), "{why}");
    let why = g.refuse("machine attribute edit", j!({ "machine": "seq", "name": "label", "kind": { "type": "string", "options": ["b"] } }));
    assert!(why.contains("options"), "the options must hold the default: {why}");
    write("machine state add", j!({ "machine": "seq", "name": "A", "pos": [0.0, 0.0], "values": { "gain": 0.2, "label": "a" } }));
    write("machine state add", j!({ "machine": "seq", "name": "B", "pos": [200.0, 0.0], "values": { "gain": 1.0 } }));
    write("machine state add", j!({ "machine": "seq", "name": "C", "pos": [400.0, 0.0], "values": { "gain": 0.0, "label": "c" } }));
    let why = g.refuse("machine state add", j!({ "machine": "seq", "name": "D", "values": { "nosuch": 1.0 } }));
    assert!(why.contains("no attribute `nosuch`"), "{why}");
    let why = g.refuse("machine transition add", j!({ "machine": "seq", "from": "A", "to": "Z" }));
    assert!(why.contains("enters `Z`"), "{why}");

    // A playhead is a variable group: the machine's runtime elements and one entry per attribute,
    // born at the start state's values, locked whole against every hand but the machine's.
    write("machine playhead add", j!({ "machine": "seq", "name": "head", "color": "#f00", "start": "A" }));
    at_rest_in(&g, "head", "A");
    assert_eq!(number(&g, "head.gain"), 0.2);
    assert_eq!(g.variable("head.label"), j!("a"));
    assert_eq!(g.variable("head.prev"), j!(""));
    let listed = g.call("variable list", j!({}));
    assert_eq!(listed["groups"]["head"]["machine"], j!("seq"), "{}", listed["groups"]);
    assert_eq!(g.doc()["variables"]["head.gain"]["control"]["kind"], j!("knob"), "a number attribute's variable wears a knob");
    assert_eq!(g.doc()["variables"]["head.gain"]["lock"], Value::Null, "the lock is the group's, not the entry's own");
    for (op, payload) in [
        ("variable entry edit", j!({ "name": "head.gain", "value": 0.5 })),
        ("variable entry remove", j!({ "name": "head.gain" })),
        ("variable group rename", j!({ "from": "head", "to": "tail" })),
        ("variable group lock", j!({ "group": "head", "value": false })),
        ("variable entry add", j!({ "name": "head.extra", "value": 1.0 })),
    ] {
        let why = g.refuse(op, payload);
        assert!(why.contains("lock"), "`{op}` is held by the machine: {why}");
    }
    let why = g.refuse("variable group add", j!({ "group": "head" }));
    assert!(why.contains("already exists"), "{why}");
    let why = g.refuse("machine playhead add", j!({ "machine": "seq", "name": "system", "start": "A" }));
    assert!(why.contains("variable group already"), "{why}");

    // A param reads an attribute as it reads any variable, bare or inside a computation.
    let reader = g.add("_TestScalar");
    g.call("node param edit", j!({ "node": hex(reader), "param": "control/value", "expression": "variables.head.gain" }));
    let probe = g.probe(reader, "out");
    g.until("the reader to emit the start value", |_| probe.latest().filter(|d| f32s(d) == [0.2]));

    // A manual transition: instant, the entering state's values land, an unset attribute is kept.
    let t1 = write("machine transition add", j!({ "machine": "seq", "from": "A", "to": "B", "triggers": [{ "kind": "manual" }] }))["id"].clone();
    assert_eq!(t1, "t1", "the manager mints the id");
    let why = g.refuse("machine fire", j!({ "machine": "seq", "playhead": "head", "transition": "t9" }));
    assert!(why.contains("no transition `t9`"), "{why}");
    g.call("machine fire", j!({ "machine": "seq", "playhead": "head", "transition": "t1" }));
    at_rest_in(&g, "head", "B");
    assert_eq!(number(&g, "head.gain"), 1.0);
    assert_eq!(g.variable("head.label"), j!("a"), "B leaves `label` alone, so the playhead keeps it");
    g.until("the reader to follow the move", |_| probe.latest().filter(|d| f32s(d) == [1.0]));
    assert_eq!(g.call("session status", j!({}))["dirty"], true);
    let dir = tempfile::tempdir().unwrap();
    g.call("session save", j!({ "path": dir.path().join("clean.gfi").to_string_lossy() }));
    let why = g.refuse("machine fire", j!({ "machine": "seq", "playhead": "head", "transition": "t1" }));
    assert!(why.contains("playhead `head` is in `B`"), "{why}");
    g.call("machine jump", j!({ "machine": "seq", "playhead": "head", "state": "A" }));
    at_rest_in(&g, "head", "A");
    g.call("machine fire", j!({ "machine": "seq", "playhead": "head", "transition": "t1" }));
    at_rest_in(&g, "head", "B");
    assert!(g.stays(|g| g.call("session status", j!({}))["dirty"] == false), "a fire changes nothing in the patch");

    // The real variable producer delivers the eased move's destination. Intermediate values
    // and exact progress are covered below with the same public runtime on a controlled clock.
    let t2 = write("machine transition add", j!({ "machine": "seq", "from": "B", "to": "C", "triggers": [{ "kind": "manual" }],
                                                    "duration": 0.6, "curve": "smooth" }))["id"].clone();
    g.call("machine fire", j!({ "machine": "seq", "playhead": "head", "transition": t2 }));
    at_rest_in(&g, "head", "C");
    assert_eq!(number(&g, "head.gain"), 0.0);
    assert_eq!(g.variable("head.label"), j!("c"), "a string switches on arrival");

    // A dwell: the playhead leaves after its seconds, reached by polling. A chance of 0 never participates.
    let t3 = write("machine transition add", j!({ "machine": "seq", "from": "C", "to": "A", "triggers": [{ "kind": "after", "seconds": 0.2 }] }))["id"].clone();
    at_rest_in(&g, "head", "A");
    write("machine transition edit", j!({ "machine": "seq", "id": t3, "chance": 0.0, "triggers": [{ "kind": "after", "seconds": 0.05 }] }));
    g.call("machine jump", j!({ "machine": "seq", "playhead": "head", "state": "C" }));
    at_rest_in(&g, "head", "C");
    assert!(g.stays(|g| text(g, "head.state") == "C"), "a transition of chance 0 never leaves");
    write("machine transition remove", j!({ "machine": "seq", "id": t3 }));

    // A `when` trigger reads a variable through the one expression language, on the rising edge.
    write("variable entry add", j!({ "name": "desk.gate", "value": 0.0 }));
    let why = g.refuse("machine transition add", j!({ "machine": "seq", "from": "C", "to": "A", "triggers": [{ "kind": "when", "expression": "nd('reader')" }] }));
    assert!(why.contains("reads a node"), "{why}");
    let t4 = write("machine transition add", j!({ "machine": "seq", "from": "C", "to": "A", "triggers": [{ "kind": "when", "expression": "variables.desk.gate > 0" }] }))["id"].clone();
    assert!(g.stays(|g| text(g, "head.state") == "C"), "a closed gate holds");
    g.call("variable entry edit", j!({ "name": "desk.gate", "value": 1.0 }));
    at_rest_in(&g, "head", "A");
    assert!(evaluator.evals.load(std::sync::atomic::Ordering::Relaxed) > 0, "the gate went through the evaluator");
    g.call("machine jump", j!({ "machine": "seq", "playhead": "head", "state": "C" }));
    at_rest_in(&g, "head", "C");
    assert!(g.stays(|g| text(g, "head.state") == "C"), "a gate already open on arrival is no edge");
    g.call("variable entry edit", j!({ "name": "desk.gate", "value": 0.0 }));
    assert!(g.stays(|g| text(g, "head.state") == "C"), "a gate closing is no edge either");
    g.call("compound", j!({ "ops": [
        { "op": "variable entry edit", "payload": { "name": "desk.gate", "value": 1.0 } },
        { "op": "variable entry edit", "payload": { "name": "desk.gate", "value": 0.0 } }
    ] }));
    assert!(g.stays(|g| text(g, "head.state") == "C"), "a batch exposes only its settled gate value");
    g.call("variable entry lock", j!({ "name": "desk.gate", "config": true, "value": false }));
    let why = g.refuse("variable entry edit", j!({ "name": "desk.gate", "value": 1.0, "expression": "1" }));
    assert!(why.contains("locked"), "{why}");
    assert_eq!(g.variable("desk.gate").as_f64(), Some(0.0));
    assert!(g.stays(|g| text(g, "head.state") == "C"), "a refused edit cannot send a transient rising edge");
    g.call("variable entry lock", j!({ "name": "desk.gate", "config": false, "value": false }));
    g.call("variable entry edit", j!({ "name": "desk.gate", "value": 2.0 }));
    at_rest_in(&g, "head", "A");
    write("machine transition remove", j!({ "machine": "seq", "id": t4 }));

    // Two playheads meet: who takes the transition is the policy's. `alone` is the inverse.
    write("machine playhead add", j!({ "machine": "seq", "name": "second", "color": "#0f0", "start": "C" }));
    at_rest_in(&g, "second", "C");
    let meet = write("machine transition add", j!({ "machine": "seq", "from": "B", "to": "C", "triggers": [{ "kind": "meet", "policy": "fifo" }] }))["id"].clone();
    g.call("machine jump", j!({ "machine": "seq", "playhead": "head", "state": "B" }));
    at_rest_in(&g, "head", "B");
    g.call("machine jump", j!({ "machine": "seq", "playhead": "second", "state": "B" }));
    at_rest_in(&g, "head", "C");
    assert!(g.stays(|g| text(g, "second.state") == "B"), "fifo: the longest resident went, the arrival stays");
    write("machine transition edit", j!({ "machine": "seq", "id": meet, "triggers": [{ "kind": "meet", "policy": "lifo" }] }));
    g.call("machine jump", j!({ "machine": "seq", "playhead": "head", "state": "B" }));
    at_rest_in(&g, "head", "C");
    assert!(g.stays(|g| text(g, "second.state") == "B"), "lifo: the arrival went, the resident stays");
    write("machine transition edit", j!({ "machine": "seq", "id": meet, "triggers": [{ "kind": "meet", "policy": "all" }] }));
    g.call("machine jump", j!({ "machine": "seq", "playhead": "head", "state": "B" }));
    at_rest_in(&g, "head", "C");
    at_rest_in(&g, "second", "C");
    let alone = write("machine transition add", j!({ "machine": "seq", "from": "C", "to": "A", "triggers": [{ "kind": "alone" }] }))["id"].clone();
    g.call("machine jump", j!({ "machine": "seq", "playhead": "second", "state": "B" }));
    at_rest_in(&g, "head", "A");
    assert!(g.stays(|g| text(g, "second.state") == "B"), "the one that left is not the one left behind");
    write("machine transition remove", j!({ "machine": "seq", "id": alone }));
    write("machine transition remove", j!({ "machine": "seq", "id": meet }));

    // Renames reach what reads them: a playhead's group and an attribute's element both follow.
    write("machine playhead rename", j!({ "machine": "seq", "name": "head", "to": "lead" }));
    assert_eq!(g.doc()["nodes"][hex(reader)]["params"]["control"]["value"]["expression"], j!("variables.lead.gain"));
    assert!(g.doc()["variables"]["head.gain"].is_null() && g.doc()["variables"]["lead.gain"].is_object());
    write("machine attribute rename", j!({ "machine": "seq", "name": "gain", "to": "level" }));
    assert_eq!(g.doc()["nodes"][hex(reader)]["params"]["control"]["value"]["expression"], j!("variables.lead.level"));
    assert_eq!(g.doc()["machines"]["seq"]["states"]["A"]["values"]["level"].as_f64(), Some(0.2));
    write("machine state rename", j!({ "machine": "seq", "name": "A", "to": "Start" }));
    assert_eq!(g.doc()["machines"]["seq"]["playheads"]["lead"]["start"], j!("Start"));
    assert_eq!(g.doc()["machines"]["seq"]["transitions"]["t1"]["from"], j!("Start"));
    at_rest_in(&g, "lead", "Start");
    // A removed attribute leaves every state and every playhead.
    write("machine attribute remove", j!({ "machine": "seq", "name": "label" }));
    assert!(g.doc()["variables"]["lead.label"].is_null());
    assert!(g.doc()["machines"]["seq"]["states"]["C"]["values"].get("label").is_none());
    // A state a playhead starts in refuses to go; another takes its transitions with it.
    let why = g.refuse("machine state remove", j!({ "machine": "seq", "name": "Start" }));
    assert!(why.contains("starts in"), "{why}");
    write("machine state remove", j!({ "machine": "seq", "name": "B" }));
    assert!(g.doc()["machines"]["seq"]["transitions"].get("t1").is_none(), "t1 left B with it");

    // Saved and opened elsewhere: the model comes back whole, every playhead in its start state.
    g.call("machine jump", j!({ "machine": "seq", "playhead": "second", "state": "Start" }));
    at_rest_in(&g, "second", "Start");
    let path = dir.path().join("machine.gfi");
    g.call("session save", j!({ "path": path.to_string_lossy() }));
    let manifest = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    assert!(manifest.contains("machines:") && !manifest.contains("lead.level:"), "the model rides, the playhead's variables are its to re-derive: {manifest}");
    let other = Goofi::new();
    other.call("session load", j!({ "path": path.to_string_lossy() }));
    assert_eq!(other.doc()["machines"], g.doc()["machines"]);
    at_rest_in(&other, "lead", "Start");
    at_rest_in(&other, "second", "C");
    assert_eq!(number(&other, "lead.level"), 0.2);
    assert_eq!(other.call("variable list", j!({}))["groups"]["lead"]["machine"], j!("seq"));

    // Reloading the same live session starts a new clock epoch and resets its runtime.
    other.call("machine jump", j!({ "machine": "seq", "playhead": "lead", "state": "C" }));
    at_rest_in(&other, "lead", "C");
    other.call("session load", j!({ "path": path.to_string_lossy() }));
    at_rest_in(&other, "lead", "Start");
    assert_eq!(number(&other, "lead.level"), 0.2);

    // Undo walks every edit back; the groups go with the playheads and the root is empty.
    while g.call("undo", j!({}))["changed"] == true {}
    assert_eq!(g.doc()["machines"], j!({}));
    assert!(g.doc()["variables"]["lead.level"].is_null() && g.doc()["variables"]["second.state"].is_null());
    assert!(g.call("variable list", j!({}))["groups"].get("lead").is_none());
    while g.call("redo", j!({}))["changed"] == true {}
    assert!(g.doc()["machines"]["seq"]["playheads"]["lead"].is_object(), "{}", g.doc()["machines"]);
    at_rest_in(&g, "lead", "Start");
}

/// An authored session's model, driven through the public clocked runtime. No wall-clock
/// intervals or worker wake schedule decide the assertions below.
fn clocked(g: &Goofi) -> goofi_graph::machine::Machines {
    let mut runtime = goofi_graph::machine::Machines::default();
    configure(&mut runtime, g, 0, Some(Arc::new(goofi_tests::FirstVar::default())));
    runtime
}

fn configure(runtime: &mut goofi_graph::machine::Machines, g: &Goofi, at: goofi_core::time::Tick, evaluator: Option<Arc<dyn goofi_node::ExprEvaluator>>) {
    let graph = g.graph();
    let store = graph.variables();
    let producers = store.entries().filter_map(|(name, _)| Some((name.to_string(), store.generation(name)?))).collect();
    drop(store);
    runtime.configure(at, graph.machines().clone(), evaluator, producers, graph.variable_projection());
}

fn frames(runtime: &mut goofi_graph::machine::Machines, at: goofi_core::time::Tick) -> indexmap::IndexMap<String, goofi_core::Data> {
    runtime.advance(at, &|_| None).into_iter().collect()
}

fn base(g: &Goofi) {
    g.call("machine add", j!({ "name": "clocked" }));
    g.call("machine attribute add", j!({ "machine": "clocked", "name": "value", "value": 0.0 }));
    g.call("machine attribute add", j!({ "machine": "clocked", "name": "label", "value": "start" }));
    for name in ["A", "B", "C"] {
        g.call("machine state add", j!({ "machine": "clocked", "name": name, "values": { "value": if name == "B" { 1.0 } else { 0.0 } } }));
    }
    g.call("machine playhead add", j!({ "machine": "clocked", "name": "lead", "start": "A" }));
    g.call("machine playhead add", j!({ "machine": "clocked", "name": "twin", "start": "A" }));
}

#[test]
fn a_clocked_machine_keeps_sample_phase_through_dense_sparse_and_delayed_advances() {
    use goofi_core::time::ticks;
    let g = Goofi::new();
    base(&g);
    let period = ticks(1.0 / 48_000.0).unwrap();
    for (from, to) in [("A", "B"), ("B", "A")] {
        g.call("machine transition add", j!({ "machine": "clocked", "from": from, "to": to,
            "triggers": [{ "kind": "after", "seconds": 1.0 / 48_000.0 }], "duration": 1.0 / 48_000.0, "curve": "linear" }));
    }
    let mut dense = clocked(&g);
    let mut sparse = clocked(&g);
    assert_eq!(frames(&mut dense, 0), frames(&mut sparse, 0));
    for i in 1..=1000 {
        let at = period * i + period / 2;
        let actual = frames(&mut dense, at);
        // Both playheads have exactly the same logical arrival and travel progress.
        for field in ["state", "prev", "progress", "transition", "arrived", "value"] {
            assert_eq!(actual[&format!("lead.{field}")], actual[&format!("twin.{field}")], "{field} at sample {i}");
        }
        if i % 73 == 0 || i == 1000 {
            assert_eq!(actual, frames(&mut sparse, at), "catch-up at sample {i}");
        }
        if i == 1 {
            assert_eq!(goofi_core::control::number_of(&actual["lead.progress"]), 0.5);
            assert_eq!(goofi_core::control::number_of(&actual["lead.value"]), 0.5);
            assert_eq!(serde_json::to_value(&actual["lead.arrived"]).unwrap(), j!("0"));
        }
    }
    let actual = frames(&mut sparse, period * 1002);
    assert_eq!(serde_json::to_value(&actual["lead.arrived"]).unwrap(), j!((period * 1002).to_string()));
    assert_eq!(goofi_core::control::number_of(&actual["lead.progress"]), 1.0);

    // The data-plane projection uses the same settled trajectories on both audio grids.
    // Sparse catch-up must retain every intrablock edge, not just the final held value.
    for rate in [48_000, 96_000] {
        use goofi_core::samples::{SampleClock, SampleSpan, CONTROL_DELAY};
        let clock = SampleClock { patch_epoch: 0, epoch: 1, origin: 0, first: 0, rate };
        let cutoff = period * 1000 + period / 2;
        let first = clock.index(CONTROL_DELAY);
        let dense_samples: indexmap::IndexMap<_, _> = dense.sampled_writes(clock, first, cutoff).into_iter().collect();
        let sparse_samples: indexmap::IndexMap<_, _> = sparse.sampled_writes(clock, first, cutoff).into_iter().collect();
        assert_eq!(dense_samples, sparse_samples, "the same history at {rate} Hz");
        let packet = &dense_samples["lead.value"];
        let span = SampleSpan::of(packet).expect("audio positions are part of the produced frame");
        assert_eq!(span.clock, clock);
        assert_eq!(span.first, first);
        let mut oracle = clocked(&g);
        for (i, value) in f32s(packet).into_iter().enumerate() {
            let at = clock.at(first + i as u64).saturating_sub(CONTROL_DELAY);
            let logical = frames(&mut oracle, at);
            assert_eq!(value, goofi_core::control::number_of(&logical["lead.value"]) as f32,
                "sample {i} at {rate} Hz is the runtime's value at the same source instant");
        }
    }
    assert!(dense.health().iter().chain(sparse.health().iter()).all(|(_, _, health)| health.expressions.is_empty() && health.playheads.is_empty()));

    // Renames preserve a flight, its start/end, held values and the owner's publication right.
    let mut renamed = clocked(&g);
    let before = frames(&mut renamed, period + period / 2);
    let clock = goofi_core::samples::SampleClock { patch_epoch: 0, epoch: 1, origin: 0, first: 0, rate: 96_000 };
    let first = clock.index(goofi_core::samples::CONTROL_DELAY);
    let sampled_before: indexmap::IndexMap<_, _> = renamed.sampled_writes(clock, first, period + period / 2).into_iter().collect();
    let owner = renamed.owner("lead.value").unwrap().clone();
    g.call("machine rename", j!({ "machine": "clocked", "to": "renamed" }));
    g.call("machine state rename", j!({ "machine": "renamed", "name": "B", "to": "End" }));
    g.call("machine attribute rename", j!({ "machine": "renamed", "name": "value", "to": "gain" }));
    g.call("machine playhead rename", j!({ "machine": "renamed", "name": "lead", "to": "voice" }));
    configure(&mut renamed, &g, period + period / 2, None);
    let after = frames(&mut renamed, period + period / 2);
    for field in ["prev", "progress", "transition", "arrived"] { assert_eq!(before[&format!("lead.{field}")], after[&format!("voice.{field}")]); }
    assert_eq!(before["lead.value"], after["voice.gain"]);
    assert_eq!(after["voice.state"], goofi_core::Data::text("End"));
    assert_eq!(renamed.owner("voice.gain"), Some(&owner));
    let sampled_after: indexmap::IndexMap<_, _> = renamed.sampled_writes(clock, first, period + period / 2).into_iter().collect();
    assert_eq!(sampled_before["lead.value"], sampled_after["voice.gain"], "renames preserve already completed sample positions and trajectories");
    assert_eq!(frames(&mut renamed, period * 2)["voice.gain"], goofi_core::Data::number(1.0));

    g.call("machine remove", j!({ "machine": "renamed" }));
    base(&g);
    let store = g.graph().variable_store();
    assert!(!store.lock().drive("lead.value", &owner, goofi_core::Data::number(9.0), None), "a replaced owner cannot publish into the same names");

    // Alias conditions read the owned trajectory at the decision tick, even when the follower
    // still holds the graph's resting source. Dense and delayed observation reach the same edge.
    let python = Arc::new(goofi_python::inproc::PyExprEvaluator::new().unwrap());
    g.graph().set_evaluator(python.clone());
    for (name, expression) in [("reads.gate", "variables.lead.value >= 0.5"), ("reads.alias", "variables.reads.gate")] {
        g.call("variable entry add", j!({ "name": name, "value": 0.0 }));
        g.call("variable entry edit", j!({ "name": name, "expression": expression }));
    }
    g.call("machine transition add", j!({ "machine": "clocked", "from": "A", "to": "B", "duration": 4.0 / 48_000.0,
        "triggers": [{ "kind": "manual" }] }));
    g.call("machine transition add", j!({ "machine": "clocked", "from": "A", "to": "C",
        "triggers": [{ "kind": "when", "expression": "variables.reads.alias", "edge": "level" }] }));
    let mut dense = goofi_graph::machine::Machines::default();
    let mut sparse = goofi_graph::machine::Machines::default();
    for runtime in [&mut dense, &mut sparse] {
        configure(runtime, &g, 0, Some(python.clone()));
        let _ = frames(runtime, 0);
        runtime.fire(0, "clocked", "lead", "t1", &|_| None).unwrap();
    }
    for i in 1..=3 { let _ = frames(&mut dense, period * i); }
    assert_eq!(frames(&mut dense, period * 3), frames(&mut sparse, period * 3));
    assert_eq!(frames(&mut sparse, period * 3)["twin.state"], goofi_core::Data::text("C"), "the alias sees the live flight, not a worker's cached false value");
    let binding = g.graph().variables().binding("reads.gate").unwrap();
    g.call("machine playhead rename", j!({ "machine": "clocked", "name": "lead", "to": "voice" }));
    assert_eq!(g.graph().variables().binding("reads.gate"), Some(binding), "rewriting a reference preserves its producer identity");
    let binding = g.graph().variables().binding("reads.alias").unwrap();
    g.call("variable group rename", j!({ "from": "reads", "to": "follow" }));
    assert_eq!(g.graph().variables().binding("follow.alias"), Some(binding), "renaming an alias preserves the existing binding owner");
}

#[test]
fn a_state_selects_ordered_weighted_or_uniform_transitions_and_owns_the_priority_order() {
    use goofi_core::time::ticks;
    let g = Goofi::new();
    base(&g);
    for to in ["B", "C"] {
        g.call("machine transition add", j!({ "machine": "clocked", "from": "A", "to": to,
            "triggers": [{ "kind": "after", "seconds": 1.0 }] }));
    }
    g.call("machine state edit", j!({ "machine": "clocked", "name": "A", "order": ["t2", "t1"] }));
    assert_eq!(g.call("machine list", j!({}))["outgoing"]["clocked"]["A"], j!(["t2", "t1"]));
    assert_eq!(g.doc()["machine_outgoing"]["clocked"]["A"], j!(["t2", "t1"]), "the replica carries order with its authored model");
    let mut runtime = clocked(&g);
    assert_eq!(serde_json::to_value(&frames(&mut runtime, ticks(1.0).unwrap())["lead.state"]).unwrap(), j!("C"));
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t2", "chance": 0.0 }));
    let mut runtime = clocked(&g);
    assert_eq!(serde_json::to_value(&frames(&mut runtime, ticks(1.0).unwrap())["lead.state"]).unwrap(), j!("B"), "failed ordered chance permits the next enabled branch");
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t2", "chance": 1.0, "weight": 0.0 }));
    g.call("machine state edit", j!({ "machine": "clocked", "name": "A", "selection": "weighted" }));
    let mut runtime = clocked(&g);
    assert_eq!(serde_json::to_value(&frames(&mut runtime, ticks(1.0).unwrap())["lead.state"]).unwrap(), j!("B"));
    g.call("machine state edit", j!({ "machine": "clocked", "name": "A", "selection": "uniform" }));
    let mut single = clocked(&g);
    let expected = frames(&mut single, ticks(1.0).unwrap());
    // Duplicating an OR trigger does not give its transition a second vote.
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t2", "triggers": [
        { "kind": "after", "seconds": 1.0 }, { "kind": "after", "seconds": 1.0 } ] }));
    let mut duplicate = clocked(&g);
    assert_eq!(frames(&mut duplicate, ticks(1.0).unwrap()), expected);
    let why = g.refuse("machine state edit", j!({ "machine": "clocked", "name": "A", "order": ["t1", "t1"] }));
    assert!(why.contains("unique outgoing"), "{why}");
    let why = g.refuse("machine transition edit", j!({ "machine": "clocked", "id": "t1", "chance": 1.5 }));
    assert!(why.contains("chance"), "{why}");
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t2", "from": "B" }));
    assert_eq!(g.doc()["machines"]["clocked"]["states"]["A"]["order"], j!(["t1"]));
    assert_eq!(g.doc()["machine_outgoing"]["clocked"]["A"], j!(["t1"]));
}

#[test]
fn random_timers_travel_and_failed_chances_are_seeded_and_cadence_independent() {
    use goofi_core::time::ticks;
    let g = Goofi::new();
    base(&g);
    g.call("machine edit", j!({ "machine": "clocked", "seed": 91 }));
    for (from, to) in [("A", "B"), ("B", "A")] {
        g.call("machine transition add", j!({ "machine": "clocked", "from": from, "to": to,
            "triggers": [{ "kind": "after", "seconds": { "min": 0.1, "max": 0.2 } }],
            "chance": 0.35, "duration": { "min": 0.2, "max": 0.4 }, "curve": "linear" }));
    }
    let mut dense = clocked(&g);
    let mut delayed = clocked(&g);
    for i in 0..=1000 { let _ = frames(&mut dense, ticks(f64::from(i) / 100.0).unwrap()); }
    assert_eq!(frames(&mut dense, ticks(10.0).unwrap()), frames(&mut delayed, ticks(10.0).unwrap()));
    dense.reset(ticks(10.0).unwrap(), Some("clocked")).unwrap();
    let reset = frames(&mut dense, ticks(10.0).unwrap());
    assert_eq!(serde_json::to_value(&reset["lead.arrived"]).unwrap(), j!(ticks(10.0).unwrap().to_string()));
    let mut fresh = clocked(&g);
    let fresh = frames(&mut fresh, ticks(0.15).unwrap());
    let reset = frames(&mut dense, ticks(10.15).unwrap());
    for field in ["state", "progress", "value"] { assert_eq!(reset[&format!("lead.{field}")], fresh[&format!("lead.{field}")], "reset {field}"); }
    let why = g.refuse("machine transition edit", j!({ "machine": "clocked", "id": "t1", "duration": { "min": 2, "max": 1 } }));
    assert!(why.contains("random range"), "{why}");

    // The seeded self-chain takes two moves, then fails its chance and remains usable.
    g.call("machine edit", j!({ "machine": "clocked", "seed": 0 }));
    g.call("machine playhead edit", j!({ "machine": "clocked", "name": "twin", "start": "C" }));
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "to": "A", "internal": true,
        "triggers": [{ "kind": "always" }], "chance": 0.5, "duration": 0 }));
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t2", "from": "A", "to": "B",
        "triggers": [{ "kind": "event", "name": "done" }], "chance": 1.0, "duration": 0 }));
    g.call("machine state edit", j!({ "machine": "clocked", "name": "A", "order": ["t2", "t1"] }));
    let mut runtime = clocked(&g);
    assert_eq!(serde_json::to_value(&frames(&mut runtime, 0)["lead.state"]).unwrap(), j!("A"));
    assert!(runtime.health().iter().all(|(_, _, health)| health.playheads.is_empty()), "a terminating random chain is not a proven cycle");
    runtime.event(0, "clocked", "done", Some("lead")).unwrap();
    assert_eq!(serde_json::to_value(&frames(&mut runtime, 0)["lead.state"]).unwrap(), j!("B"), "the head remains available after the terminating chance draw");
}

#[test]
fn events_guards_and_local_actions_follow_one_settled_lifecycle() {
    use goofi_core::time::ticks;
    let g = Goofi::new();
    base(&g);
    g.call("machine state edit", j!({ "machine": "clocked", "name": "A", "exit_values": { "label": "exit" } }));
    g.call("machine state edit", j!({ "machine": "clocked", "name": "B", "values": { "value": null } }));
    g.call("machine transition add", j!({ "machine": "clocked", "from": "A", "to": "B",
        "triggers": [{ "kind": "event", "name": "next" }], "guard": "variables.desk.gate", "chance": 0.0,
        "values": { "value": 0.75 } }));
    let mut runtime = clocked(&g);
    let closed = |_: &str| Some(goofi_core::Data::number(0.0));
    let open = |_: &str| Some(goofi_core::Data::number(1.0));
    let _ = runtime.advance(0, &closed);
    assert!(runtime.fire(ticks(1.0).unwrap(), "clocked", "lead", "t1", &closed).unwrap_err().contains("guard is false"));
    runtime.event(ticks(1.0).unwrap(), "clocked", "next", None).unwrap();
    let data: indexmap::IndexMap<_, _> = runtime.advance(ticks(1.0).unwrap(), &open).into_iter().collect();
    assert_eq!(serde_json::to_value(&data["lead.state"]).unwrap(), j!("A"), "an event cannot bypass chance");
    // Queued effects occur before later deadlines, even when one call catches up past both.
    runtime.jump(ticks(1.25).unwrap(), "clocked", "twin", "B").unwrap();
    let data: indexmap::IndexMap<_, _> = runtime.advance(ticks(1.5).unwrap(), &open).into_iter().collect();
    assert_eq!(serde_json::to_value(&data["twin.arrived"]).unwrap(), j!(ticks(1.25).unwrap().to_string()));
    assert!(runtime.event(ticks(1.0).unwrap(), "clocked", "next", None).unwrap_err().contains("already processed"));
    runtime.fire(ticks(2.0).unwrap(), "clocked", "lead", "t1", &open).unwrap();
    let data = frames(&mut runtime, ticks(2.0).unwrap());
    assert_eq!(serde_json::to_value(&data["lead.state"]).unwrap(), j!("B"));
    assert_eq!(serde_json::to_value(&data["lead.label"]).unwrap(), j!("exit"));
    assert_eq!(goofi_core::control::number_of(&data["lead.value"]), 0.75);
    assert_eq!(serde_json::to_value(&data["twin.state"]).unwrap(), j!("B"));
    runtime.reset(ticks(3.0).unwrap(), Some("clocked")).unwrap();
    let data = frames(&mut runtime, ticks(3.0).unwrap());
    assert_eq!(serde_json::to_value(&data["lead.label"]).unwrap(), j!("start"), "reset initializes defaults rather than keeping prior values");
    // The same runtime validation is reached through the effect vocabulary.
    let why = g.refuse("machine fire", j!({ "machine": "clocked", "playhead": "lead", "transition": "t1" }));
    assert!(why.contains("guard"), "{why}");
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "guard": "", "chance": 1.0 }));
    g.call("machine event", j!({ "machine": "clocked", "event": "next" }));
    at_rest_in(&g, "lead", "B");
    at_rest_in(&g, "twin", "B");
}

#[test]
fn immediate_and_internal_moves_settle_local_values_without_resetting_residence() {
    let g = Goofi::new();
    base(&g);
    g.call("machine state edit", j!({ "machine": "clocked", "name": "A", "values": { "value": 1.0 } }));
    g.call("machine transition add", j!({ "machine": "clocked", "from": "A", "to": "A", "internal": true,
        "triggers": [{ "kind": "when", "expression": "variables.lead.value", "edge": "level" }],
        "values": { "value": 0.0, "label": "internal" } }));
    let mut runtime = clocked(&g);
    let data = frames(&mut runtime, 0);
    assert_eq!(serde_json::to_value(&data["lead.label"]).unwrap(), j!("internal"));
    assert_eq!(serde_json::to_value(&data["twin.label"]).unwrap(), j!("internal"), "both moves are selected before either changes the shared guard input");
    assert_eq!(goofi_core::control::number_of(&data["lead.value"]), 0.0);
    assert_eq!(serde_json::to_value(&data["lead.arrived"]).unwrap(), j!("0"));
    assert!(runtime.health().iter().all(|(_, _, health)| health.expressions.is_empty() && health.playheads.is_empty()), "a changing internal assignment is not a zero-time cycle");
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "internal": false,
        "triggers": [{ "kind": "always" }], "values": { "value": null, "label": null } }));
    let evaluator: Arc<dyn goofi_node::ExprEvaluator> = Arc::new(goofi_tests::FirstVar::default());
    let mut runtime = goofi_graph::machine::Machines::default();
    configure(&mut runtime, &g, 0, Some(evaluator.clone()));
    let _ = frames(&mut runtime, 0);
    assert!(runtime.health().iter().any(|(_, _, health)| health.playheads.get("lead").is_some_and(|why| why.contains("zero-time transition cycle"))));
    // The cycle is stopped rather than retried by every viewer sample.
    let stopped = frames(&mut runtime, 1);
    assert!(runtime.health().iter().any(|(_, _, health)| health.playheads.contains_key("lead")), "a stopped cycle remains a standing fault");
    g.call("variable entry add", j!({ "name": "unrelated.value", "value": 1.0 }));
    configure(&mut runtime, &g, 2, Some(evaluator.clone()));
    let still_stopped = frames(&mut runtime, 2);
    assert_eq!(still_stopped["lead.arrived"], stopped["lead.arrived"], "unrelated edits cannot restart a stopped cycle");
    assert!(runtime.health().iter().any(|(_, _, health)| health.playheads.contains_key("lead")), "a stopped cycle remains a standing fault");
    g.call("machine state edit", j!({ "machine": "clocked", "name": "A", "pos": [100.0, 200.0] }));
    g.call("machine playhead edit", j!({ "machine": "clocked", "name": "lead", "color": "#123456" }));
    configure(&mut runtime, &g, 3, Some(evaluator.clone()));
    assert_eq!(frames(&mut runtime, 3)["lead.arrived"], stopped["lead.arrived"], "presentation edits leave the stopped runtime in place");
    assert!(runtime.health().iter().any(|(_, _, health)| health.playheads.contains_key("lead")));
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "triggers": [{ "kind": "manual" }] }));
    configure(&mut runtime, &g, 4, Some(evaluator));
    let _ = frames(&mut runtime, 4);
    assert!(runtime.health().iter().all(|(_, _, health)| health.playheads.is_empty()), "a corrected behavior can run again");
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "to": "B",
        "triggers": [{ "kind": "when", "expression": "variables.desk.gate", "edge": "falling" }] }));
    let mut runtime = clocked(&g);
    let _ = runtime.advance(0, &|_| Some(goofi_core::Data::number(1.0)));
    let failed: indexmap::IndexMap<_, _> = runtime.advance(1, &|_| None).into_iter().collect();
    assert_eq!(serde_json::to_value(&failed["lead.state"]).unwrap(), j!("A"), "an evaluation failure is not a falling edge");
    assert!(runtime.health().iter().any(|(_, _, health)| !health.expressions.is_empty()));
    let closed: indexmap::IndexMap<_, _> = runtime.advance(2, &|_| Some(goofi_core::Data::number(0.0))).into_iter().collect();
    assert_eq!(serde_json::to_value(&closed["lead.state"]).unwrap(), j!("B"));
    assert!(runtime.health().iter().all(|(_, _, health)| health.expressions.is_empty()), "successful reevaluation clears the standing failure");

    // The worker reports the same standing health through the public document and op.
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "from": "A", "to": "A",
        "triggers": [{ "kind": "always" }] }));
    g.call("machine reset", j!({ "machine": "clocked" }));
    g.until("cycle health in the replica", |g| {
        g.doc()["machine_health"]["clocked"]["playheads"]["lead"].as_str().filter(|why| why.contains("zero-time transition cycle")).map(str::to_string)
    });
    let arrived = text(&g, "lead.arrived");
    g.call("machine state edit", j!({ "machine": "clocked", "name": "A", "pos": [300.0, 400.0] }));
    g.call("machine event", j!({ "machine": "clocked", "event": "a_barrier" }));
    assert_eq!(text(&g, "lead.arrived"), arrived, "a cosmetic public edit does not restart the cycle");
    assert!(g.call("machine list", j!({}))["health"]["clocked"]["playheads"]["lead"].is_string());
    assert!(g.call("session status", j!({}))["errors"].as_array().unwrap().iter().any(|issue|
        issue["machine"] == "clocked" && issue["playhead"] == "lead" && issue["error"].as_str().is_some_and(|why| why.contains("zero-time transition cycle"))),
        "session inspection includes the same standing cycle fault");
    let manifest = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    assert!(!manifest.contains("machine_health"), "health must not dirty or persist in the patch");
    assert!(!manifest.contains("machine_outgoing"), "resolved order is a projection, not authored state");
    g.call("machine jump", j!({ "machine": "clocked", "playhead": "lead", "state": "B" }));
    g.until("jump recovers cycle health", |g| g.doc()["machine_health"]["clocked"]["playheads"].get("lead").is_none().then_some(()));
    assert!(!g.call("session status", j!({}))["errors"].as_array().unwrap().iter().any(|issue|
        issue["machine"] == "clocked" && issue["playhead"] == "lead" && issue.get("transition").is_none()), "session inspection drops a recovered cycle fault");
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "from": "B", "to": "C",
        "triggers": [{ "kind": "when", "expression": "variables.recovery.gate", "edge": "level" }] }));
    g.until("expression health in the replica", |g| g.doc()["machine_health"]["clocked"]["expressions"].as_array().filter(|issues| issues.iter().any(|issue| issue["expression"] == "variables.recovery.gate")).map(|_| ()));
    assert!(g.call("session status", j!({}))["errors"].as_array().unwrap().iter().any(|issue|
        issue["machine"] == "clocked" && issue["transition"] == "t1" && issue["surface"]["kind"] == "when" && issue["expression"] == "variables.recovery.gate"),
        "session inspection preserves the expression's typed location");
    g.call("variable entry add", j!({ "name": "recovery.gate", "value": 0.0 }));
    g.until("settled input recovers expression health", |g| g.doc()["machine_health"]["clocked"]["expressions"].as_array().filter(|errors| errors.is_empty()).map(|_| ()));
    assert!(!g.call("session status", j!({}))["errors"].as_array().unwrap().iter().any(|issue|
        issue["machine"] == "clocked"), "session inspection drops recovered expression faults");

    // Equal source text can have different contracts and different owners. A successful false
    // guard must not clear a failed timer, and recovering one playhead must not clear its peer.
    use goofi_graph::machine::{ExpressionSurface, Machines};
    let python = Arc::new(goofi_python::inproc::PyExprEvaluator::new().unwrap());
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "from": "A", "to": "B", "guard": "",
        "triggers": [{ "kind": "after", "seconds": "-1" }] }));
    let guarded = g.call("machine transition add", j!({ "machine": "clocked", "from": "A", "to": "C", "guard": "-1", "triggers": [{ "kind": "always" }] }))["id"].as_str().unwrap().to_string();
    let mut runtime = Machines::default();
    configure(&mut runtime, &g, 0, Some(python.clone()));
    let _ = frames(&mut runtime, 0);
    let health = runtime.health();
    let issues = &health[0].2.expressions;
    assert_eq!(issues.len(), 2, "both failed timers remain visible after the equal-text false guard");
    assert!(issues.iter().all(|issue| matches!(issue.surface, ExpressionSurface::After { index: 0 }) && issue.expression == "-1"));
    runtime.jump(1, "clocked", "lead", "B").unwrap();
    let _ = frames(&mut runtime, 1);
    let health = runtime.health();
    assert_eq!(health[0].2.expressions.len(), 1);
    assert_eq!(health[0].2.expressions[0].playhead.as_deref(), Some("twin"));
    g.call("machine playhead rename", j!({ "machine": "clocked", "name": "twin", "to": "peer" }));
    configure(&mut runtime, &g, 2, Some(python.clone()));
    let _ = frames(&mut runtime, 2);
    assert_eq!(runtime.health()[0].2.expressions[0].playhead.as_deref(), Some("peer"), "health follows its owner identity");
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "triggers": [{ "kind": "manual" }] }));
    g.call("machine transition edit", j!({ "machine": "clocked", "id": guarded, "guard": "nan" }));
    configure(&mut runtime, &g, 3, Some(python));
    let _ = frames(&mut runtime, 3);
    let health = runtime.health();
    assert_eq!(health[0].2.expressions.len(), 1, "a removed duration surface leaves no stale issue");
    assert_eq!(health[0].2.expressions[0].surface, ExpressionSurface::Guard);
    assert!(health[0].2.expressions[0].error.contains("finite"), "a non-finite guard is a fault, not a quiet false");
}

#[test]
fn edits_and_effects_share_the_boundary_with_inputs_and_arrivals() {
    use goofi_core::time::{ticks, TICKS_PER_SECOND};
    use goofi_graph::machine::Machines;
    let g = Goofi::new();
    base(&g);
    g.call("machine attribute add", j!({ "machine": "clocked", "name": "typed", "value": 0.25 }));
    g.call("variable entry add", j!({ "name": "desk.gate", "value": 1.0 }));
    let open = |_: &str| Some(goofi_core::Data::number(1.0));
    let evaluator: Arc<dyn goofi_node::ExprEvaluator> = Arc::new(goofi_tests::FirstVar::default());
    let mut runtime = clocked(&g);
    let _ = runtime.advance(0, &open);
    g.call("machine attribute edit", j!({ "machine": "clocked", "name": "typed", "value": "changed", "kind": { "type": "string" } }));
    configure(&mut runtime, &g, 0, Some(evaluator.clone()));
    assert_eq!(frames(&mut runtime, 0)["lead.typed"], goofi_core::Data::text("changed"), "a new kind replaces an incompatible held value");
    g.call("machine attribute edit", j!({ "machine": "clocked", "name": "typed", "value": "next", "kind": { "type": "string", "options": ["next"] } }));
    configure(&mut runtime, &g, 0, Some(evaluator.clone()));
    assert_eq!(frames(&mut runtime, 0)["lead.typed"], goofi_core::Data::text("next"), "new options replace an incompatible held string");
    g.call("machine transition add", j!({ "machine": "clocked", "from": "A", "to": "B",
        "triggers": [{ "kind": "when", "expression": "variables.desk.gate" }] }));
    configure(&mut runtime, &g, ticks(1.0).unwrap(), Some(evaluator.clone()));
    let data: indexmap::IndexMap<_, _> = runtime.advance(ticks(1.0).unwrap(), &open).into_iter().collect();
    assert_eq!(serde_json::to_value(&data["lead.state"]).unwrap(), j!("A"), "a new condition starts with its already-open input as baseline");
    assert!(runtime.health().iter().all(|(_, _, health)| health.expressions.is_empty() && health.playheads.is_empty()));

    // Models at one instant settle before the runtime can take their automatic moves.
    let edited = ticks(1.0).unwrap() + 1;
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "triggers": [{ "kind": "always" }] }));
    configure(&mut runtime, &g, edited, Some(evaluator.clone()));
    let before = frames(&mut runtime, edited - 1);
    assert_eq!(serde_json::to_value(&before["lead.state"]).unwrap(), j!("A"), "a pending edit cannot run before its timestamp");
    assert_eq!(serde_json::to_value(&before["lead.arrived"]).unwrap(), j!("0"));
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "triggers": [{ "kind": "manual" }] }));
    configure(&mut runtime, &g, edited, Some(evaluator.clone()));
    assert_eq!(serde_json::to_value(&frames(&mut runtime, edited)["lead.state"]).unwrap(), j!("A"), "the intermediate immediate transition never runs");

    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "chance": 0.0,
        "triggers": [{ "kind": "after", "seconds": 10.0 }, { "kind": "after", "seconds": 20.0 }] }));
    configure(&mut runtime, &g, ticks(2.0).unwrap(), Some(evaluator.clone()));
    let _ = frames(&mut runtime, ticks(2.0).unwrap());
    assert_eq!(runtime.next_deadline(), ticks(12.0));
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1",
        "triggers": [{ "kind": "after", "seconds": 20.0 }, { "kind": "after", "seconds": 10.0 }] }));
    configure(&mut runtime, &g, ticks(5.0).unwrap(), Some(evaluator.clone()));
    let _ = frames(&mut runtime, ticks(5.0).unwrap());
    assert_eq!(runtime.next_deadline(), ticks(12.0), "moving a trigger preserves its deadline");
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "chance": 1.0,
        "triggers": [{ "kind": "after", "seconds": 0.5 }], "duration": 1.0 }));
    g.call("machine state edit", j!({ "machine": "clocked", "name": "B", "exit_values": { "label": "B-exit" } }));
    configure(&mut runtime, &g, ticks(10.0).unwrap(), Some(evaluator.clone()));
    let _ = frames(&mut runtime, ticks(10.0).unwrap());
    runtime.jump(ticks(11.5).unwrap(), "clocked", "lead", "C").unwrap();
    let data = frames(&mut runtime, ticks(12.0).unwrap());
    assert_eq!(serde_json::to_value(&data["lead.label"]).unwrap(), j!("B-exit"), "a jump at the flight end first lands, then exits B");
    assert_eq!(serde_json::to_value(&data["lead.arrived"]).unwrap(), j!(ticks(11.5).unwrap().to_string()));

    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "duration": 0.0,
        "triggers": [{ "kind": "after", "seconds": 1.0 }] }));
    let mut runtime = clocked(&g);
    let _ = frames(&mut runtime, 0);
    g.call("machine state edit", j!({ "machine": "clocked", "name": "A", "pos": [100, 200] }));
    configure(&mut runtime, &g, ticks(2.0).unwrap(), Some(evaluator.clone()));
    let data = frames(&mut runtime, ticks(2.0).unwrap());
    assert_eq!(serde_json::to_value(&data["lead.arrived"]).unwrap(), j!(ticks(1.0).unwrap().to_string()), "an edit catches up under the old model through the previous deadlines");

    g.call("machine transition add", j!({ "machine": "clocked", "from": "A", "to": "C", "triggers": [{ "kind": "manual" }] }));
    let mut runtime = clocked(&g);
    let _ = frames(&mut runtime, 0);
    runtime.fire(ticks(1.0).unwrap(), "clocked", "lead", "t2", &|_| None).unwrap();
    assert_eq!(serde_json::to_value(&frames(&mut runtime, ticks(1.0).unwrap())["lead.state"]).unwrap(), j!("C"), "the explicit effect precedes automatic selection at its timestamp");

    let python = Arc::new(goofi_python::inproc::PyExprEvaluator::new().unwrap());
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1",
        "triggers": [{ "kind": "after", "seconds": "t + 1" }] }));
    let mut timer = Machines::default();
    configure(&mut timer, &g, 0, Some(python.clone()));
    let _ = frames(&mut timer, 0);
    assert_eq!(timer.next_deadline(), ticks(1.0), "an armed duration no longer observes the clock expression");
    let _ = frames(&mut timer, ticks(0.5).unwrap());
    assert_eq!(timer.next_deadline(), ticks(1.0), "the entry sample stays fixed");

    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "guard": "t >= 0.001",
        "triggers": [{ "kind": "when", "expression": "1", "edge": "level" }] }));
    let mut dense = Machines::default();
    configure(&mut dense, &g, 0, Some(python.clone()));
    let mut delayed = Machines::default();
    configure(&mut delayed, &g, 0, Some(python));
    for i in 0..=150 { let _ = frames(&mut dense, i * TICKS_PER_SECOND / 48_000); }
    let data = frames(&mut delayed, 150 * TICKS_PER_SECOND / 48_000);
    assert_eq!(data, frames(&mut dense, 150 * TICKS_PER_SECOND / 48_000));
    assert_eq!(serde_json::to_value(&data["lead.state"]).unwrap(), j!("B"));
    assert_eq!(serde_json::to_value(&data["lead.arrived"]).unwrap(), j!((48 * TICKS_PER_SECOND / 48_000).to_string()), "the guard observes the shared grid, without a wake-dependent edge");

    // Producer names can change without a new timer sample or condition baseline. A new birth
    // at the same name is different: its timer samples the replacement at the edit boundary.
    g.call("variable entry add", j!({ "name": "desk.seconds", "value": 10.0 }));
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "guard": "",
        "triggers": [{ "kind": "after", "seconds": "variables.desk.seconds" },
            { "kind": "when", "expression": "variables.desk.gate", "edge": "rising" }] }));
    let mut runtime = clocked(&g);
    let read = |key: &str| g.graph().variables().get(key).cloned();
    let _ = runtime.advance(0, &read);
    assert_eq!(runtime.next_deadline(), ticks(10.0));
    g.call("variable entry edit", j!({ "name": "desk.seconds", "value": 20.0 }));
    for (op, payload, at) in [
        ("variable entry rename", j!({ "name": "desk.seconds", "to": "desk.period" }), 2.0),
        ("variable entry rename", j!({ "name": "desk.gate", "to": "desk.enabled" }), 3.0),
        ("variable group rename", j!({ "from": "desk", "to": "controls" }), 4.0),
    ] {
        g.call(op, payload);
        configure(&mut runtime, &g, ticks(at).unwrap(), Some(evaluator.clone()));
        let data: indexmap::IndexMap<_, _> = runtime.advance(ticks(at).unwrap(), &read).into_iter().collect();
        assert_eq!(runtime.next_deadline(), ticks(10.0), "{op} keeps the interval sampled on entry");
        assert_eq!(serde_json::to_value(&data["lead.state"]).unwrap(), j!("A"), "{op} keeps the open condition baseline");
    }
    let data: indexmap::IndexMap<_, _> = runtime.advance(ticks(10.0).unwrap(), &read).into_iter().collect();
    assert_eq!(serde_json::to_value(&data["lead.arrived"]).unwrap(), j!(ticks(10.0).unwrap().to_string()));

    let mut runtime = clocked(&g);
    let _ = runtime.advance(0, &read);
    assert_eq!(runtime.next_deadline(), ticks(20.0));
    g.call("variable entry remove", j!({ "name": "controls.period" }));
    g.call("variable entry add", j!({ "name": "controls.period", "value": 3.0 }));
    configure(&mut runtime, &g, ticks(5.0).unwrap(), Some(evaluator));
    let _ = runtime.advance(ticks(5.0).unwrap(), &read);
    assert_eq!(runtime.next_deadline(), ticks(8.0), "a replacement producer rearms from its edit timestamp");
    let data: indexmap::IndexMap<_, _> = runtime.advance(ticks(8.0).unwrap(), &read).into_iter().collect();
    assert_eq!(serde_json::to_value(&data["lead.arrived"]).unwrap(), j!(ticks(8.0).unwrap().to_string()));

    // A manual fire waits for its exact sample column, then uses its original clock instant.
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "guard": "variables.controls.enabled",
        "triggers": [{ "kind": "manual" }], "duration": { "min": 0.1, "max": 0.2 } }));
    let mut ready = clocked(&g);
    let mut waiting = clocked(&g);
    for runtime in [&mut ready, &mut waiting] { let _ = runtime.advance(0, &read); }
    let at = ticks(1.0).unwrap();
    let clock = goofi_core::samples::SampleClock { patch_epoch: 0, epoch: 0, origin: 0, first: 0, rate: 48_000 };
    let first = clock.index(at + goofi_core::samples::CONTROL_DELAY);
    let window = |first| goofi_core::Data::array_f32(vec![1, 2], [1.0f32, 1.0].into_iter().flat_map(f32::to_le_bytes).collect(),
        goofi_core::samples::SampleSpan { clock, first, length: 2 }.meta()).unwrap();
    let fire = || std::collections::VecDeque::from([goofi_graph::machine::Effect::Fire {
        machine: "clocked".into(), playhead: "lead".into(), transition: "t1".into(),
    }]);
    let mut deferred = fire();
    for _ in 0..3 {
        let (answers, pending) = waiting.apply_effects(at, &mut deferred, &|_| Some(window(first + 1)));
        assert!(answers.is_empty());
        assert!(matches!(pending, Some(goofi_graph::variable_projection::ReadFault::Pending { .. })));
        assert_eq!(deferred.len(), 1);
    }
    assert!(waiting.health().iter().all(|(_, _, health)| health.expressions.is_empty()), "pending coverage is not an expression failure");
    let mut immediate = fire();
    for (runtime, effects) in [(&mut ready, &mut immediate), (&mut waiting, &mut deferred)] {
        let (answers, pending) = runtime.apply_effects(at, effects, &|_| Some(window(first)));
        assert!(pending.is_none());
        assert_eq!(answers, vec![Ok(())]);
        assert!(effects.is_empty());
    }
    assert_eq!(waiting.next_deadline(), ready.next_deadline(), "waiting does not move the fire time or random duration");
    assert_eq!(frames(&mut waiting, ticks(1.05).unwrap()), frames(&mut ready, ticks(1.05).unwrap()));

    // A sampled condition observes every source column, including an odd sample at 96 kHz.
    g.call("machine transition edit", j!({ "machine": "clocked", "id": "t1", "guard": "", "duration": 0,
        "triggers": [{ "kind": "when", "expression": "variables.controls.enabled" }] }));
    let clock = goofi_core::samples::SampleClock { rate: 96_000, ..clock };
    let first = clock.index(goofi_core::samples::CONTROL_DELAY);
    let pulse = goofi_core::Data::array_f32(vec![1, 3], [0.0f32, 1.0, 0.0].into_iter().flat_map(f32::to_le_bytes).collect(),
        goofi_core::samples::SampleSpan { clock, first, length: 3 }.meta()).unwrap();
    let read = |_: &str| Some(pulse.clone());
    let mut dense = clocked(&g);
    let mut delayed = clocked(&g);
    for runtime in [&mut dense, &mut delayed] { let _ = runtime.advance(0, &read); }
    let edge = clock.at(first + 1) - goofi_core::samples::CONTROL_DELAY;
    assert_eq!(dense.next_deadline(), Some(edge));
    let _ = dense.advance(edge, &read);
    let end = clock.at(first + 2) - goofi_core::samples::CONTROL_DELAY;
    let expected: indexmap::IndexMap<_, _> = dense.advance(end, &read).into_iter().collect();
    assert_eq!(expected["lead.arrived"], goofi_core::Data::text(edge.to_string()));
    assert_eq!(expected["lead.state"], goofi_core::Data::text("B"));
    assert_eq!(expected, delayed.advance(end, &read).into_iter().collect::<indexmap::IndexMap<_, _>>());
}

#[test]
fn machines_choose_from_one_shared_pre_decision_snapshot() {
    let g = Goofi::new();
    base(&g);
    g.call("machine state edit", j!({ "machine": "clocked", "name": "A", "values": { "value": 1.0 } }));
    g.call("machine state edit", j!({ "machine": "clocked", "name": "B", "values": { "value": 0.0 } }));
    g.call("machine transition add", j!({ "machine": "clocked", "from": "A", "to": "B",
        "triggers": [{ "kind": "when", "expression": "variables.remote.value", "edge": "level" }] }));
    g.call("machine add", j!({ "name": "other" }));
    g.call("machine attribute add", j!({ "machine": "other", "name": "value", "value": 0.0 }));
    g.call("machine state add", j!({ "machine": "other", "name": "A", "values": { "value": 1.0 } }));
    g.call("machine state add", j!({ "machine": "other", "name": "B", "values": { "value": 0.0 } }));
    g.call("machine playhead add", j!({ "machine": "other", "name": "remote", "start": "A" }));
    g.call("machine transition add", j!({ "machine": "other", "from": "A", "to": "B",
        "triggers": [{ "kind": "when", "expression": "variables.lead.value", "edge": "level" }] }));
    let data = frames(&mut clocked(&g), 0);
    for ph in ["lead", "twin", "remote"] {
        assert_eq!(serde_json::to_value(&data[&format!("{ph}.state")]).unwrap(), j!("B"), "{ph} reads the same settled snapshot");
    }
}
