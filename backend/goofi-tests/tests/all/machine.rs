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
    // An attribute is a value every state may set; its widget must draw its default, and the
    // machine's own three element names are not an attribute's to take.
    let knob = j!({ "kind": "knob", "min": 0.0, "max": 1.0, "step": 0.01, "x": 0.0, "y": 0.0, "w": 4.0, "h": 4.0 });
    write("machine attribute add", j!({ "machine": "seq", "name": "gain", "value": 0.0, "control": knob }));
    write("machine attribute add", j!({ "machine": "seq", "name": "label", "value": "a" }));
    let why = g.refuse("machine attribute add", j!({ "machine": "seq", "name": "state", "value": 0.0 }));
    assert!(why.contains("machine's own"), "{why}");
    let why = g.refuse("machine attribute add", j!({ "machine": "seq", "name": "shape", "value": "x", "control": knob }));
    assert!(why.contains("knob") && why.contains("string"), "{why}");
    write("machine state add", j!({ "machine": "seq", "name": "A", "pos": [0.0, 0.0], "values": { "gain": 0.2, "label": "a" } }));
    write("machine state add", j!({ "machine": "seq", "name": "B", "pos": [200.0, 0.0], "values": { "gain": 1.0 } }));
    write("machine state add", j!({ "machine": "seq", "name": "C", "pos": [400.0, 0.0], "values": { "gain": 0.0, "label": "c" } }));
    let why = g.refuse("machine state add", j!({ "machine": "seq", "name": "D", "values": { "nosuch": 1.0 } }));
    assert!(why.contains("no attribute `nosuch`"), "{why}");
    let why = g.refuse("machine transition add", j!({ "machine": "seq", "from": "A", "to": "Z" }));
    assert!(why.contains("enters `Z`"), "{why}");

    // A playhead is a variable group: the machine's three elements and one entry per attribute,
    // born at the start state's values, locked whole against every hand but the machine's.
    write("machine playhead add", j!({ "machine": "seq", "name": "head", "color": "#f00", "start": "A" }));
    at_rest_in(&g, "head", "A");
    assert_eq!(number(&g, "head.gain"), 0.2);
    assert_eq!(g.variable("head.label"), j!("a"));
    assert_eq!(g.variable("head.prev"), j!(""));
    let listed = g.call("variable list", j!({}));
    assert_eq!(listed["groups"]["head"]["machine"], j!("seq"), "{}", listed["groups"]);
    assert_eq!(g.doc()["variables"]["head.gain"]["control"]["kind"], j!("knob"), "the attribute's widget rides the variable");
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
    g.call("machine fire", j!({ "machine": "seq", "playhead": "head", "transition": "t1" }));
    assert!(g.stays(|g| g.call("session status", j!({}))["dirty"] == false), "a fire changes nothing in the patch");

    // An eased transition is sampled: monotonic, ending at the target, `progress` rising to 1.
    let t2 = write("machine transition add", j!({ "machine": "seq", "from": "B", "to": "C", "triggers": [{ "kind": "manual" }],
                                                    "duration": 0.6, "curve": "smooth" }))["id"].clone();
    g.call("machine fire", j!({ "machine": "seq", "playhead": "head", "transition": t2 }));
    let mut samples: Vec<(f64, f64)> = Vec::new();
    g.until("the eased move to land in C", |g| {
        let (state, progress) = (text(g, "head.state"), number(g, "head.progress"));
        if state == "C" {
            samples.push((progress, number(g, "head.gain")));
        }
        (state == "C" && progress == 1.0).then_some(())
    });
    assert!(samples.windows(2).all(|w| w[0].0 <= w[1].0 && w[0].1 >= w[1].1), "progress rises and gain falls: {samples:?}");
    assert!(samples.iter().any(|(p, _)| *p > 0.0 && *p < 1.0), "the move was seen in flight: {samples:?}");
    assert_eq!(number(&g, "head.gain"), 0.0);
    assert_eq!(g.variable("head.label"), j!("c"), "a string switches on arrival");

    // A dwell: the playhead leaves after its seconds, reached by polling. A chance of 0 never rolls.
    let t3 = write("machine transition add", j!({ "machine": "seq", "from": "C", "to": "A", "triggers": [{ "kind": "after", "seconds": 0.2 }] }))["id"].clone();
    at_rest_in(&g, "head", "A");
    write("machine transition edit", j!({ "machine": "seq", "id": t3, "triggers": [{ "kind": "after", "seconds": 0.05, "chance": 0.0 }] }));
    g.call("machine jump", j!({ "machine": "seq", "playhead": "head", "state": "C" }));
    at_rest_in(&g, "head", "C");
    assert!(g.stays(|g| text(g, "head.state") == "C"), "a failed roll re-arms and never leaves");
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

    // Undo walks every edit back; the groups go with the playheads and the root is empty.
    while g.call("undo", j!({}))["changed"] == true {}
    assert_eq!(g.doc()["machines"], j!({}));
    assert!(g.doc()["variables"]["lead.level"].is_null() && g.doc()["variables"]["second.state"].is_null());
    assert!(g.call("variable list", j!({}))["groups"].get("lead").is_none());
    while g.call("redo", j!({}))["changed"] == true {}
    assert!(g.doc()["machines"]["seq"]["playheads"]["lead"].is_object(), "{}", g.doc()["machines"]);
    at_rest_in(&g, "lead", "Start");
}
