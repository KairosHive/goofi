//! A state machine built through ops, read through its playheads' variables, and driven by every
//! decision mode and trigger kind, then saved, reopened and undone; and the stepper itself on explicit instants.

use std::sync::Arc;

use goofi_core::indexmap::IndexMap;
use goofi_core::Data;
use goofi_graph::machine::Machines;
use goofi_python::inproc::PyExprEvaluator;
use goofi_tests::{f32s, hex, j, Goofi, Viewer};
use serde_json::Value;

/// A playhead variable as text.
fn text(g: &Goofi, name: &str) -> String {
    g.variable(name).as_str().unwrap_or_default().to_string()
}

fn number(g: &Goofi, name: &str) -> f64 {
    g.variable(name).as_f64().unwrap_or(f64::NAN)
}

/// Poll until the playhead is at rest in `state`: `prev` is cleared by the arrival itself, where
/// `progress` reads 1 the instant the flight ends, before the machine has landed it.
fn at_rest_in(g: &Goofi, playhead: &str, state: &str) {
    g.until(&format!("{playhead} to rest in {state}"), |g| {
        (text(g, &format!("{playhead}.state")) == state && text(g, &format!("{playhead}.prev")).is_empty()).then_some(())
    });
}

#[test]
fn a_machine_moves_its_playheads_through_its_states_and_writes_their_variables() {
    let g = Goofi::new();
    g.graph().set_evaluator(Arc::new(PyExprEvaluator::new().unwrap()));
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
    // A new kind reads every state's value as a param reads a frame; one it cannot hold, such as a
    // text outside the options, falls back to the default.
    write("machine attribute add", j!({ "machine": "seq", "name": "mood", "value": 0.0, "kind": num }));
    write("machine state edit", j!({ "machine": "seq", "name": "A", "values": { "mood": 0.7 } }));
    let mood = |g: &Goofi| g.doc()["machines"]["seq"]["states"]["A"]["values"]["mood"].clone();
    let kinds = [
        (j!({ "type": "string" }), j!(""), j!("0.7")),
        (num.clone(), j!(0.0), j!(0.7)),
        (j!({ "type": "num", "vmin": 0.0, "vmax": 1.0, "color": true }), j!([0.0, 0.0, 0.0, 1.0]), j!([0.7, 0.7, 0.7, 0.7])),
        (j!({ "type": "bool" }), j!(0), j!(1)),
        (j!({ "type": "string", "options": ["calm", "wild"] }), j!("calm"), j!("calm")),
    ];
    for (kind, default, want) in kinds {
        write("machine attribute edit", j!({ "machine": "seq", "name": "mood", "kind": kind, "value": default }));
        assert_eq!(mood(&g), want, "a state's value follows the kind {kind}");
    }
    write("machine attribute remove", j!({ "machine": "seq", "name": "mood" }));
    // These states hold until fired: a new state would draw evenly among its transitions out.
    for name in ["A", "B", "C"] {
        write("machine state edit", j!({ "machine": "seq", "name": name, "mode": "expression", "expression": "-1" }));
    }
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
    assert!(number(&g, "head.arrived") >= 0.0, "born at a patch second");
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
    g.call("machine fire", j!({ "machine": "seq", "playhead": "head", "transition": "t1" }));
    assert!(g.stays(|g| g.call("session status", j!({}))["dirty"] == false), "a fire changes nothing in the patch");

    // An eased transition is sampled: monotonic, ending at the target, `progress` rising to 1.
    let t2 = write("machine transition add", j!({ "machine": "seq", "from": "B", "to": "C", "triggers": [{ "kind": "manual" }],
                                                    "duration": 0.6, "curve": "smooth" }))["id"].clone();
    g.call("machine fire", j!({ "machine": "seq", "playhead": "head", "transition": t2 }));
    let mut samples: Vec<(f64, f64)> = Vec::new();
    let mut heard: Vec<f32> = Vec::new();
    g.until("the eased move to land in C", |g| {
        let (state, progress) = (text(g, "head.state"), number(g, "head.progress"));
        if state == "C" {
            samples.push((progress, number(g, "head.gain")));
            heard.extend(probe.latest().map(|d| f32s(&d)[0]));
        }
        (state == "C" && text(g, "head.prev").is_empty()).then_some(())
    });
    assert!(samples.windows(2).all(|w| w[0].0 <= w[1].0 && w[0].1 >= w[1].1), "progress rises and gain falls: {samples:?}");
    assert!(samples.iter().any(|(p, _)| *p > 0.0 && *p < 1.0), "the move was seen in flight: {samples:?}");
    // The reader's param holds the functional and reads it for each of its own runs.
    assert!(heard.iter().any(|v| *v > 0.0 && *v < 1.0), "the reader heard the move on the way: {heard:?}");
    assert_eq!(number(&g, "head.gain"), 0.0);
    assert_eq!(g.variable("head.label"), j!("c"), "a string switches on arrival");

    // A state decides every interval: a weighted transition out is drawn, a weight of 0 never
    // is. A transition's weight leaves with it.
    let t3 = write("machine transition add", j!({ "machine": "seq", "from": "C", "to": "A" }))["id"].clone();
    let why = g.refuse("machine state edit", j!({ "machine": "seq", "name": "C", "weights": { "t1": 1.0 } }));
    assert!(why.contains("no transition out"), "{why}");
    write("machine state edit", j!({ "machine": "seq", "name": "C", "mode": "random", "interval": 0.2, "noop": 0.0, "weights": { t3.as_str().unwrap(): 1.0 } }));
    at_rest_in(&g, "head", "A");
    write("machine state edit", j!({ "machine": "seq", "name": "C", "interval": 0.05, "noop": 1.0, "weights": { t3.as_str().unwrap(): 0.0 } }));
    g.call("machine jump", j!({ "machine": "seq", "playhead": "head", "state": "C" }));
    at_rest_in(&g, "head", "C");
    assert!(g.stays(|g| text(g, "head.state") == "C"), "a weight of 0 never leaves");
    write("machine transition remove", j!({ "machine": "seq", "id": t3 }));
    assert_eq!(g.doc()["machines"]["seq"]["states"]["C"]["weights"], j!({}), "the weight left with its transition");
    write("machine state edit", j!({ "machine": "seq", "name": "C", "mode": "expression" }));

    // A `when` trigger reads a variable through the one expression language, on the rising edge.
    write("variable entry add", j!({ "name": "desk.gate", "value": 0.0 }));
    let why = g.refuse("machine transition add", j!({ "machine": "seq", "from": "C", "to": "A", "triggers": [{ "kind": "when", "expression": "nd('reader')" }] }));
    assert!(why.contains("reads a node"), "{why}");
    let t4 = write("machine transition add", j!({ "machine": "seq", "from": "C", "to": "A", "triggers": [{ "kind": "when", "expression": "variables.desk.gate > 0" }] }))["id"].clone();
    assert!(g.stays(|g| text(g, "head.state") == "C"), "a closed gate holds");
    g.call("variable entry edit", j!({ "name": "desk.gate", "value": 1.0 }));
    at_rest_in(&g, "head", "A");
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
    // Two computed reads of a playhead variable ride along: one of them spells the very text a
    // transition's `when` compiles, so the evaluator holds one function for both.
    let computed = g.add("_TestScalar");
    g.call("node param edit", j!({ "node": hex(computed), "param": "control/value", "expression": "variables.lead.level * 2 + 1" }));
    let shared = g.add("_TestScalar");
    g.call("node param edit", j!({ "node": hex(shared), "param": "control/value", "expression": "variables.lead.level >= 5.0" }));
    write("machine transition add", j!({ "machine": "seq", "from": "C", "to": "Start", "triggers": [{ "kind": "when", "expression": "variables.lead.level >= 5.0" }] }));
    let bound = |g: &Goofi, step: &str, uid: goofi_tests::Uid, want: f32| {
        let state = g.call("node state", j!({ "node": hex(uid) }))["text"].as_str().unwrap_or_default().to_string();
        assert!(!state.contains("[error:"), "{step}: a computed read of a playhead variable stands: {state}");
        let probe = g.probe(uid, "out");
        g.until("the computed read to follow the playhead", |_| probe.latest().filter(|d| f32s(d) == [want]));
    };
    bound(&g, "before the save", computed, 1.4);
    bound(&g, "before the save", shared, 0.0);
    let path = dir.path().join("machine.gfi");
    g.call("session save", j!({ "path": path.to_string_lossy() }));
    let manifest = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    assert!(manifest.contains("machines:") && !manifest.contains("lead.level:"), "the model rides, the playhead's variables are its to re-derive: {manifest}");
    let other = Goofi::new();
    other.graph().set_evaluator(Arc::new(PyExprEvaluator::new().unwrap()));
    other.call("session load", j!({ "path": path.to_string_lossy() }));
    assert_eq!(other.doc()["machines"], g.doc()["machines"]);
    at_rest_in(&other, "lead", "Start");
    at_rest_in(&other, "second", "C");
    assert_eq!(number(&other, "lead.level"), 0.2);
    assert_eq!(other.call("variable list", j!({}))["groups"]["lead"]["machine"], j!("seq"));
    bound(&other, "loaded elsewhere", computed, 1.4);
    bound(&other, "loaded elsewhere", shared, 0.0);

    // Undo walks every edit back; the groups go with the playheads and the root is empty.
    while g.call("undo", j!({}))["changed"] == true {}
    assert_eq!(g.doc()["machines"], j!({}));
    assert!(g.doc()["variables"]["lead.level"].is_null() && g.doc()["variables"]["second.state"].is_null());
    assert!(g.call("variable list", j!({}))["groups"].get("lead").is_none());
    while g.call("redo", j!({}))["changed"] == true {}
    assert!(g.doc()["machines"]["seq"]["playheads"]["lead"].is_object(), "{}", g.doc()["machines"]);
    at_rest_in(&g, "lead", "Start");
    // And loaded over ITSELF, which is the load that replaces a running patch's bindings.
    g.call("session load", j!({ "path": path.to_string_lossy() }));
    at_rest_in(&g, "lead", "Start");
    bound(&g, "loaded over itself", computed, 1.4);
    bound(&g, "loaded over itself", shared, 0.0);
}

/// A playhead variable among an advance's writes.
fn written<'a>(writes: &'a [(String, Data)], name: &str) -> &'a Data {
    &writes.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no write of {name}: {writes:?}")).1
}

/// The number a write holds at `t`: a functional run, a plain frame as it is.
fn value_at(ev: &PyExprEvaluator, d: &Data, t: f64) -> f64 {
    goofi_core::control::number_of(&goofi_node::at(ev, d, t, (0.0, 1.0)).unwrap())
}

#[test]
fn the_stepper_settles_every_instant_it_is_owed_however_it_is_advanced() {
    // A two-state ping-pong with sample-length dwells and travel, driven on explicit instants: the
    // same model advanced densely, sparsely, and with one long catch-up lands every deadline at the
    // same instant and writes the same frames, because no wake time enters a stamp.
    let ev = Arc::new(PyExprEvaluator::new().unwrap());
    let period = 1.0 / 48_000.0;
    let model: IndexMap<String, goofi_graph::machine::Machine> = serde_json::from_value(j!({ "pp": {
        "attributes": { "gain": { "default": 0.0, "kind": { "type": "num", "vmin": 0.0, "vmax": 1.0 } } },
        "states": {
            "A": { "values": { "gain": 0.0 }, "interval": period, "mode": "sequence" },
            "B": { "values": { "gain": 1.0 }, "interval": period, "mode": "sequence" }
        },
        "transitions": {
            "ab": { "from": "A", "to": "B", "duration": period, "curve": "linear" },
            "ba": { "from": "B", "to": "A", "duration": period, "curve": "linear" }
        },
        "playheads": { "p": { "start": "A" } }
    } })).unwrap();
    let none = |_: &str| None;
    let run = |instants: &[f64]| -> Vec<Vec<(String, Data)>> {
        let mut m = Machines::default();
        m.configure(model.clone(), Some(ev.clone()));
        instants.iter().map(|t| m.advance(*t, &none)).collect()
    };
    let dense: Vec<f64> = (0..=2002).map(|i| i as f64 * period / 2.0).collect();
    let sparse: Vec<f64> = (0..=2002).filter(|i| i % 73 == 0 || *i == 2002).map(|i| i as f64 * period / 2.0).collect();
    let (d, s) = (run(&dense), run(&sparse));
    assert_eq!(d.last(), s.last(), "the same instant reached by 2002 steps and by 29 says the same");
    assert_eq!(run(&[0.0, dense[2002]]).last(), d.last(), "or by one");
    // Deadlines compose from deadlines: born at 0, each hop dwells a period from its arrival and
    // lands a period after it leaves, so the 500th arrival is that chain of additions and nothing
    // of the grid it was looked at on. The write rides the f32 carrier.
    let chained = (0..1000).fold(0.0f64, |t, _| t + period);
    let last = d.last().unwrap();
    assert_eq!(written(last, "p.arrived"), &Data::number(chained));
    assert_eq!(written(last, "p.state").as_str().unwrap(), "A");
    // Mid-flight the attribute is a functional: the origin at departure, the target at the end,
    // the linear blend between, and clamped past either end. The write is one frame, not samples.
    let mid = &d[3]; // 1.5 periods in: dwelt one, now halfway through the flight to B
    let gain = written(mid, "p.gain");
    assert!(gain.as_functional().is_some(), "{gain:?}");
    assert_eq!(value_at(&ev, gain, period), 0.0);
    assert!((value_at(&ev, gain, 1.5 * period) - 0.5).abs() < 1e-9);
    assert_eq!(value_at(&ev, gain, 2.0 * period), 1.0);
    assert_eq!(value_at(&ev, gain, 7.0), 1.0, "past the end the target holds");
    assert_eq!(value_at(&ev, gain, 0.0), 0.0, "before the start the origin holds");
    let progress = written(mid, "p.progress");
    assert!((value_at(&ev, progress, 1.75 * period) - 0.75).abs() < 1e-9);
    assert_eq!(d[2].iter().filter(|(n, _)| n == "p.gain").count(), 1);
    // A request is stamped where it is made and lands in order with the deadlines: a jump at
    // 0.4 periods, before the first decision is due, puts the head in B then; the one out of B is
    // due a period after THAT instant, not after the advance that carried it.
    let mut m = Machines::default();
    m.configure(model.clone(), Some(ev.clone()));
    m.advance(0.0, &none);
    m.jump(0.4 * period, "pp", "p", "B");
    let w = m.advance(1.3 * period, &none);
    assert_eq!((written(&w, "p.state").as_str().unwrap(), written(&w, "p.arrived")), ("B", &Data::number(0.4 * period)));
    let w = m.advance(1.9 * period, &none);
    assert_eq!((written(&w, "p.state").as_str().unwrap(), written(&w, "p.prev").as_str().unwrap()), ("A", "B"), "left B at 1.4 periods");
    assert!((value_at(&ev, written(&w, "p.progress"), 1.9 * period) - 0.5).abs() < 1e-9, "halfway through a one-period flight begun at 1.4");
}

#[test]
fn a_state_decides_in_sequence_by_weight_or_by_expression_once_per_interval() {
    // A hub with three dead-end spokes, on explicit instants: each second's decision is read off
    // where the playhead went, then a jump puts it back in the hub, its place in a sequence kept.
    let ev = Arc::new(PyExprEvaluator::new().unwrap());
    let walk = |hub: Value, read: &dyn Fn(&str) -> Option<Data>| -> (String, Vec<String>) {
        let model: IndexMap<String, goofi_graph::machine::Machine> = serde_json::from_value(j!({ "m": {
            "states": { "H": hub, "X": {}, "Y": {}, "Z": {} },
            "transitions": { "hx": { "from": "H", "to": "X" }, "hy": { "from": "H", "to": "Y" }, "hz": { "from": "H", "to": "Z" } },
            "playheads": { "p": { "start": "H" } }
        } })).unwrap();
        let mut m = Machines::default();
        m.configure(model, Some(ev.clone()));
        m.advance(0.0, read);
        let mut path = String::new();
        for k in 1..=6 {
            let w = m.advance(k as f64, read);
            match written(&w, "p.state").as_str().unwrap() {
                "H" => path.push('-'),
                spoke => {
                    path.push_str(spoke);
                    m.jump(k as f64, "m", "p", "H");
                    m.advance(k as f64, read);
                }
            }
        }
        (path, m.take_errors())
    };
    let none = |_: &str| None;
    assert_eq!(walk(j!({ "mode": "sequence" }), &none).0, "XYZXYZ", "the next spoke in authored order, round the ring");
    let (path, _) = walk(j!({}), &none);
    assert!(path.len() == 6 && !path.contains('-'), "a new state draws evenly and never waits: {path}");
    let (path, _) = walk(j!({ "noop": 0.0, "weights": { "hx": 1.0, "hy": 2.0 } }), &none);
    assert!(path.len() == 6 && path.chars().all(|c| c == 'X' || c == 'Y'), "a draw among the weighted spokes alone: {path}");
    let (path, _) = walk(j!({ "noop": 1.0, "weights": { "hx": 0.0, "hy": 0.0, "hz": 1.0 } }), &none);
    assert!(path.chars().all(|c| c == '-' || c == 'Z'), "{path}");
    // An expression picks the spoke by index; a negative one, or None, waits; past the last is an error.
    let go = |name: &str| (name == "desk.go").then(|| Data::number(1.0));
    assert_eq!(walk(j!({ "mode": "expression", "expression": "variables.desk.go * 2" }), &go).0, "ZZZZZZ");
    assert_eq!(walk(j!({ "mode": "expression", "expression": "variables.desk.go - 2" }), &go), ("------".to_string(), Vec::new()));
    assert_eq!(walk(j!({ "mode": "expression", "expression": "None if variables.desk.go else 0" }), &go), ("------".to_string(), Vec::new()));
    let (path, errors) = walk(j!({ "mode": "expression", "expression": "variables.desk.go * 5" }), &go);
    assert_eq!(path, "------");
    assert!(errors.iter().any(|e| e.contains("past the last of 3")), "{errors:?}");
}

/// The distinct values of `xs` strictly inside `(lo, hi)`.
fn inside(xs: &[f32], lo: f32, hi: f32) -> Vec<f32> {
    let mut found: Vec<f32> = xs.iter().copied().filter(|x| *x > lo && *x < hi).collect();
    found.sort_by(f32::total_cmp);
    found.dedup();
    found
}

#[tokio::test]
async fn a_flight_is_heard_between_its_frames_by_a_viewer_a_computation_and_an_alias() {
    // One functional leaves the machine when a flight departs, and no frame follows until it
    // lands. Every reader runs it at its own instant: a viewer each tick of its pace, a computed
    // param before each run, an alias variable each follower interval — none sees two steps.
    let g = Goofi::new();
    g.graph().set_evaluator(Arc::new(PyExprEvaluator::new().unwrap()));
    let base = g.serve().await;
    for (op, payload) in [
        ("machine add", j!({ "name": "m" })),
        ("machine attribute add", j!({ "machine": "m", "name": "gain", "value": 0.0, "kind": { "type": "num", "vmin": 0.0, "vmax": 1.0 } })),
        ("machine state add", j!({ "machine": "m", "name": "A", "values": { "gain": 0.0 } })),
        ("machine state add", j!({ "machine": "m", "name": "B", "values": { "gain": 1.0 } })),
        ("machine transition add", j!({ "machine": "m", "from": "A", "to": "B", "triggers": [{ "kind": "manual" }], "duration": 1.5, "curve": "linear" })),
        ("machine state edit", j!({ "machine": "m", "name": "A", "noop": 1.0, "weights": { "t1": 0.0 } })),
        ("machine playhead add", j!({ "machine": "m", "name": "head", "start": "A" })),
        ("variable entry add", j!({ "name": "desk.twice", "value": 0.0 })),
        ("variable entry edit", j!({ "name": "desk.twice", "expression": "variables.head.gain * 2" })),
    ] {
        g.call(op, payload);
    }
    let reader = g.add("_TestScalar");
    g.call("node param edit", j!({ "node": hex(reader), "param": "control/value", "expression": "variables.head.gain + 1" }));
    let probe = g.probe(reader, "out");
    g.until("the reader to compute from the start value", |_| probe.latest().filter(|d| f32s(d) == [1.0]));
    g.until("the alias to follow the start value", |g| (number(g, "desk.twice") == 0.0).then_some(()));
    let mut viewer = Viewer::open(&base, "variables", "head.gain").await;
    viewer.until(|d| f32s(d) == [0.0]).await;

    g.call("machine fire", j!({ "machine": "m", "playhead": "head", "transition": "t1" }));
    let (mut seen, mut computed, mut alias) = (Vec::new(), Vec::new(), Vec::new());
    loop {
        let value = f32s(&viewer.decoded().await)[0];
        seen.push(value);
        computed.extend(probe.latest().map(|d| f32s(&d)[0]));
        alias.push(number(&g, "desk.twice") as f32);
        if value >= 1.0 {
            break;
        }
    }
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "the viewer hears the climb in order: {seen:?}");
    assert!(inside(&seen, 0.0, 1.0).len() >= 2, "the viewer hears the move between its frames: {seen:?}");
    assert!(inside(&computed, 1.0, 2.0).len() >= 2, "a computation reads the functional at each run: {computed:?}");
    assert!(inside(&alias, 0.0, 2.0).len() >= 2, "an alias follows the functional in time: {alias:?}");
    at_rest_in(&g, "head", "B");
    g.until("the alias to land", |g| (number(g, "desk.twice") == 2.0).then_some(()));
}

#[test]
fn a_state_without_a_place_lands_clear_of_the_others_and_arrange_rings_them() {
    let g = Goofi::new();
    const GRID: f64 = goofi_graph::canvas::GRID;
    let pos = |name: &str| -> [f64; 2] {
        let p = &g.doc()["machines"]["m"]["states"][name]["pos"];
        [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()]
    };
    let on_grid = |p: [f64; 2]| p[0] % GRID == 0.0 && p[1] % GRID == 0.0;
    g.call("machine add", j!({ "name": "m" }));

    // No `pos`: each card goes to the clear cells nearest the others, on the grid.
    for _ in 0..4 {
        g.call("machine state add", j!({ "machine": "m" }));
    }
    let cards: Vec<[f64; 2]> = (0..4).map(|i| pos(&format!("state{i}"))).collect();
    assert!(cards.iter().all(|p| on_grid(*p)), "{cards:?}");
    for i in 0..4 {
        for k in 0..i {
            let (a, b) = (cards[i], cards[k]);
            assert!((a[0] - b[0]).abs() >= 200.0 || (a[1] - b[1]).abs() >= 44.0, "cards clear each other: {a:?} {b:?}");
        }
    }
    // A named place is the caller's.
    g.call("machine state add", j!({ "machine": "m", "name": "free", "pos": [3.0, 3.0] }));
    assert_eq!(pos("free"), [3.0, 3.0]);

    // `arrange` is a ring in the order a playhead walks them, on the grid, as one undo step.
    for (a, b) in [("state0", "state2"), ("state2", "state1"), ("state1", "free"), ("free", "state0")] {
        g.call("machine transition add", j!({ "machine": "m", "from": a, "to": b, "triggers": [{ "kind": "manual" }] }));
    }
    g.call("machine playhead add", j!({ "machine": "m", "name": "p", "start": "state2" }));
    let moved = g.call("machine arrange", j!({ "machine": "m" }))["moved"].as_u64().unwrap();
    assert!(moved >= 4, "{moved}");
    let names = ["state0", "state1", "state2", "state3", "free"];
    let ring: Vec<[f64; 2]> = names.iter().map(|n| pos(n)).collect();
    assert!(ring.iter().all(|p| on_grid(*p)), "{ring:?}");
    let cx = ring.iter().map(|p| p[0]).sum::<f64>() / 5.0;
    let cy = ring.iter().map(|p| p[1]).sum::<f64>() / 5.0;
    let radii: Vec<f64> = ring.iter().map(|p| ((p[0] - cx) / 1.0).hypot((p[1] - cy) / 0.72)).collect();
    let (lo, hi) = radii.iter().fold((f64::MAX, 0.0f64), |(lo, hi), r| (lo.min(*r), hi.max(*r)));
    assert!(hi - lo < 2.0 * GRID, "every card sits on one ring: {radii:?}");
    assert_eq!(pos("state2")[1], ring.iter().map(|p| p[1]).fold(f64::MAX, f64::min), "the start state sits at the top");
    assert_eq!(g.call("machine arrange", j!({ "machine": "m" }))["moved"], 0, "arrange is idempotent");
    g.call("undo", j!({}));
    assert_eq!(pos("free"), [3.0, 3.0], "one step back restores every card");
}
