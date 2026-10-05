//! Editing a patch, with two people in it: what a write answers, what a refusal teaches, and how
//! every change comes back.

use serde_json::{Map, Value};

use goofi_tests::{f32s, fixtures::LibraryEngine, Client, Goofi, ep, hex, j};

/// The arrangement flattened to an id-keyed map with a `parent` on each node.
fn entries(g: &Goofi) -> Map<String, Value> {
    fn down(n: &Value, parent: &str, out: &mut Map<String, Value>) {
        let mut e = n.as_object().cloned().unwrap_or_default();
        let id = e["id"].as_str().unwrap().to_string();
        e.insert("parent".into(), Value::from(parent));
        if let Some(kids) = n["children"].as_array() {
            for k in kids {
                down(k, &id, out);
            }
        }
        out.insert(id, Value::Object(e));
    }
    let mut out = Map::new();
    let arrangement = g.doc()["arrangement"].clone();
    for (i, t) in arrangement["tabs"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        let id = t["id"].as_str().unwrap().to_string();
        out.insert(
            id.clone(),
            j!({ "kind": "tab", "name": t["name"].clone(), "order": i }),
        );
        down(&t["root"], &id, &mut out);
    }
    out
}

fn panels(g: &Goofi) -> Vec<String> {
    let mut v: Vec<String> =
        entries(g).iter().filter(|(_, e)| e["kind"] == "panel").map(|(id, _)| id.clone()).collect();
    v.sort();
    v
}

/// The id of the tab LABELLED `name`, resolved as the UI does.
fn tab_id(g: &Goofi, name: &str) -> String {
    entries(g).iter().find(|(_, e)| e["name"] == name).map(|(id, _)| id.clone())
        .unwrap_or_else(|| panic!("no tab labelled `{name}`"))
}

/// The tab strip, in the order it draws.
fn strip(g: &Goofi) -> Vec<String> {
    g.doc()["arrangement"]["tabs"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

/// The manager's own loader, asked to open what the manager just saved.
fn reload_warning(g: &Goofi) -> Value {
    let yaml = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    g.call("session load", j!({ "content": yaml }))["warnings"][0].clone()
}

fn split(g: &Goofi, panel: &str) -> String {
    g.call("layout panel add", j!({ "beside": panel, "side": "right" }))["id"]
        .as_str().expect("a placement answers what it placed").to_string()
}

fn first_panel(g: &Goofi) -> String {
    panels(g).first().cloned().expect("the default tab's one panel")
}

/// A uid that names nothing, which every refusal path is asked about.
const GHOST: &str = "ffffffffffff";

#[test]
fn a_session_of_edits_walks_all_the_way_back_and_forward_again() {
    let g = Goofi::new();
    let osc = g.add("LFO");
    let buf = g.add("Buffer");
    g.link(osc, "out", buf, "input");
    g.set_param(buf, "buffer", "size", 512);
    // ONE step, whatever it carries: a rename, a move and a viewer in a single node edit.
    g.call("node edit", j!({ "node": hex(osc), "name": "carrier", "pos": [40.0, 60.0],
                             "viewer": [{ "slot": "out", "kind": "line" }] }));
    g.set_param(osc, "output", "sfreq", 128.0);
    g.call("variable entry add", j!({ "name": "patch.subj", "value": "P01" }));
    // Every variable is in a group, so a bare name names nothing and is refused.
    let why = g.refuse("variable entry add", j!({ "name": "loose", "value": 1.0 }));
    assert!(why.contains("group"), "an ungrouped variable names the rule: {why}");
    // A rename is a compound: set the new name, delete the old, ONE undo step.
    g.call("compound", j!({ "ops": [
        { "op": "variable entry add", "payload": { "name": "patch.participant", "value": "P01" } },
        { "op": "variable entry remove", "payload": { "name": "patch.subj" } },
    ] }));
    // A rename of an element or of its group follows into every expression that reads it, and
    // each is ONE command with an exact inverse.
    g.call("node param edit", j!({ "node": hex(osc), "param": "lfo/amplitude",
                                   "expression": "variables.patch.participant * 2" }));
    let expr = |g: &Goofi| g.doc()["nodes"][hex(osc)]["params"]["lfo"]["amplitude"]["expression"].clone();
    g.call("variable entry rename", j!({ "name": "patch.participant", "to": "patch.handle" }));
    assert_eq!(expr(&g), j!("variables.patch.handle * 2"), "the element rename followed");
    g.call("variable group rename", j!({ "from": "patch", "to": "desk" }));
    assert_eq!(expr(&g), j!("variables.desk.handle * 2"), "the group rename followed");
    assert!(g.doc()["variables"]["desk.handle"].is_object(), "the member moved with its group");
    let why = g.refuse("variable entry rename", j!({ "name": "desk.handle", "to": "loose" }));
    assert!(why.contains("group"), "a rename out of every group is refused: {why}");

    // A control panel names its group the way an expression does, so the ONE rename moves both.
    g.call("layout panel edit", j!({ "panel": first_panel(&g), "type": "control",
                                     "state": { "group": "desk" } }));
    let group_of = |g: &Goofi| entries(g)[&first_panel(g)]["state"]["group"].clone();
    g.call("variable group rename", j!({ "from": "desk", "to": "board" }));
    assert_eq!(group_of(&g), j!("board"), "the panel followed its group");
    g.call("variable group rename", j!({ "from": "board", "to": "desk" }));
    // A group a panel names is a group with no member yet, and it renames like any other; one
    // that nothing names is not a group at all.
    g.call("layout panel edit", j!({ "panel": first_panel(&g), "state": { "group": "solo" } }));
    g.call("variable group rename", j!({ "from": "solo", "to": "duo" }));
    assert_eq!(group_of(&g), j!("duo"), "the memberless group renamed through its panel");
    let why = g.refuse("variable group rename", j!({ "from": "nobody", "to": "somebody" }));
    assert!(why.contains("no variable group"), "{why}");
    // A variable can be COMPUTED by an expression: a bare one, the manager copies each frame as it
    // comes, and nobody else may set it until the expression is cleared. It follows a node rename.
    g.call("variable entry add", j!({ "name": "desk.level", "value": 0.0 }));
    g.call("variable entry edit", j!({ "name": "desk.level", "expression": "nd('carrier')" }));
    g.until("the computed variable to take the LFO's value", |g| g.variable("desk.level").as_f64().filter(|v| *v != 0.0));
    let why = g.refuse("variable entry edit", j!({ "name": "desk.level", "value": 0.5 }));
    assert!(why.contains("computed") && why.contains("nd('carrier')"), "{why}");
    // An expression READING the followed variable is handed each pick through the expression it
    // compiled once: a moved value refreshes the binding, it does not rebuild it.
    let evaluator = std::sync::Arc::new(goofi_tests::FirstVar::default());
    g.graph().set_evaluator(evaluator.clone());
    let reader = g.add("LFO");
    g.call("node param edit", j!({ "node": hex(reader), "param": "lfo/amplitude", "expression": "variables.desk.level" }));
    let mut ev = g.events();
    let mut amplitude = |_: &Goofi| {
        ev.next("param_values")["nodes"][hex(reader)]["values"]["lfo"]["amplitude"].as_f64()
    };
    let first = g.until("the reader to take the followed value", |g| amplitude(g).filter(|v| *v != 0.0));
    let compiled = evaluator.compiles.load(std::sync::atomic::Ordering::Relaxed);
    g.until("the reader to take the next pick", |g| amplitude(g).filter(|v| *v != first));
    assert_eq!(evaluator.compiles.load(std::sync::atomic::Ordering::Relaxed), compiled,
               "a moved value recompiled the reader's expression");
    g.call("node remove", j!({ "node": hex(reader) }));
    g.call("node edit", j!({ "node": hex(osc), "name": "lfo" }));
    assert_eq!(g.doc()["variables"]["desk.level"]["expression"], j!("nd('lfo')"), "the expression followed the rename");
    g.call("node edit", j!({ "node": hex(osc), "name": "carrier" }));
    // An expression naming what is not there is kept, and says so where the widget reads it.
    for (expression, why) in [("nd('nosuch')", "no node named `nosuch`"), ("nd('carrier').out.nope", "no output `nope`")] {
        g.call("variable entry edit", j!({ "name": "desk.level", "expression": expression }));
        let error = g.doc()["variables"]["desk.level"]["error"].clone();
        assert!(error.as_str().is_some_and(|e| e.contains(why)), "`{expression}` carries its error: {error}");
    }
    g.call("variable entry edit", j!({ "name": "desk.level", "expression": "nd('carrier')" }));
    assert!(g.doc()["variables"]["desk.level"].get("error").is_none(), "a resolved expression carries none");
    g.call("variable entry edit", j!({ "name": "desk.level", "expression": "" }));
    assert!(g.doc()["variables"]["desk.level"].get("expression").is_none(), "an empty expression clears it");
    assert_eq!(g.call("variable entry edit", j!({ "name": "desk.level", "value": 0.5 }))["value"], 0.5);
    // …and it STAYS: the producer still runs, and neither a pick in flight nor one it makes after
    // the clear lands on what the author typed. A probe opened now sees only frames newer than it.
    let after = g.probe(osc, "out");
    g.until("the carrier to emit after the clear", |_| after.latest());
    assert!(g.stays(|g| g.variable("desk.level") == j!(0.5)), "the cleared source writes no more");

    // Hold the replacement too: an old completion cannot hide behind its next valid value.
    g.call("variable entry add", j!({ "name": "desk.source", "value": 0.25 }));
    g.call("variable entry add", j!({ "name": "desk.other", "value": 0.75 }));
    g.call("variable entry add", j!({ "name": "desk.stalled", "value": 0.0 }));
    let old = evaluator.hold_value(0.25);
    let replacement = evaluator.hold_value(0.75);
    g.call("variable entry edit", j!({ "name": "desk.stalled", "expression": "variables.desk.source * 1" }));
    g.until("the old variable evaluation to enter", |_| old.entered().then_some(()));
    g.call("variable entry edit", j!({ "name": "desk.stalled", "expression": "variables.desk.other * 1" }));
    drop(old);
    g.until("the replacement variable evaluation to enter", |_| replacement.entered().then_some(()));
    assert!(g.stays(|g| g.variable("desk.stalled").as_f64() == Some(0.0)), "the active replacement rejects the retired completion: {}", g.variable("desk.stalled"));
    drop(replacement);
    g.until("the replacement variable result", |g| (g.variable("desk.stalled") == j!(0.75)).then_some(()));

    let old = evaluator.hold_value(0.25);
    let replacement = evaluator.hold_value(0.5);
    g.call("variable entry edit", j!({ "name": "desk.stalled", "expression": "variables.desk.source * 1" }));
    g.until("the first A evaluation to enter", |_| old.entered().then_some(()));
    g.call("variable entry edit", j!({ "name": "desk.stalled", "expression": "variables.desk.other * 1" }));
    g.call("variable entry edit", j!({ "name": "desk.source", "value": 0.5 }));
    g.call("variable entry edit", j!({ "name": "desk.stalled", "expression": "variables.desk.source * 1" }));
    drop(old);
    g.until("the new A evaluation to enter", |_| replacement.entered().then_some(()));
    assert!(g.stays(|g| g.variable("desk.stalled") == j!(0.75)), "returning to A does not authorize its retired result");
    drop(replacement);
    g.until("the new A result", |g| (g.variable("desk.stalled") == j!(0.5)).then_some(()));

    g.call("variable entry edit", j!({ "name": "desk.source", "value": 0.25 }));
    g.until("the dependent to follow its source", |g| (g.variable("desk.stalled") == j!(0.25)).then_some(()));
    let old = evaluator.hold_value(0.5);
    let replacement = evaluator.hold_value(0.75);
    g.call("variable entry edit", j!({ "name": "desk.source", "value": 0.5 }));
    g.until("the retired producer evaluation to enter", |_| old.entered().then_some(()));
    g.call("compound", j!({ "ops": [
        { "op": "variable entry remove", "payload": { "name": "desk.source" } },
        { "op": "variable entry add", "payload": { "name": "desk.source", "value": 0.75 } }
    ] }));
    drop(old);
    g.until("the new producer evaluation to enter", |_| replacement.entered().then_some(()));
    assert!(g.stays(|g| g.variable("desk.stalled") == j!(0.25)), "recreating a source retires its dependent evaluation without changing the expression");
    drop(replacement);
    g.until("the new producer result", |g| (g.variable("desk.stalled") == j!(0.75)).then_some(()));
    g.call("variable entry edit", j!({ "name": "desk.stalled", "expression": "'bad'" }));
    g.until("an evaluation error to reach the document", |g| g.doc()["variables"]["desk.stalled"]["error"].as_str().map(str::to_string));
    g.call("variable entry edit", j!({ "name": "desk.stalled", "expression": "variables.desk.source * 1" }));
    g.until("the replacement to clear its error", |g| (g.variable("desk.stalled") == j!(0.75) && g.doc()["variables"]["desk.stalled"].get("error").is_none()).then_some(()));
    for name in ["desk.source", "desk.other", "desk.stalled"] { g.call("variable entry remove", j!({ "name": name })); }

    // A node's computed binding rejects the old completion while its replacement is held.
    g.call("variable entry add", j!({ "name": "desk.before", "value": 0.25 }));
    g.call("variable entry add", j!({ "name": "desk.after", "value": 0.75 }));
    let bound_reader = g.add("_TestConst");
    g.set_param(bound_reader, "constant", "value", 0.0);
    let values = g.probe(bound_reader, "out");
    let old = evaluator.hold_value(0.25);
    let replacement = evaluator.hold_value(0.75);
    g.call("node param edit", j!({ "node": hex(bound_reader), "param": "constant/value", "expression": "variables.desk.before * 1" }));
    g.until("the old node binding to enter", |_| old.entered().then_some(()));
    g.call("node param edit", j!({ "node": hex(bound_reader), "param": "constant/value", "expression": "variables.desk.after * 1" }));
    drop(old);
    g.until("the replacement node evaluation to enter", |_| replacement.entered().then_some(()));
    g.until("the reader to emit while its replacement is held", |_| values.latest());
    assert!(g.stays(|_| values.latest().is_some_and(|frame| f32s(&frame) == [0.0])), "the old result cannot land in the new binding");
    drop(replacement);
    g.until("the replacement result to publish", |_| values.latest().filter(|frame| f32s(frame) == [0.75]));
    let owns_notification = |g: &Goofi, uid| {
        g.call("session status", j!({}))["resources"].as_array().unwrap().iter().any(|entry|
            entry["kind"] == "port" && entry["name"].as_str().is_some_and(|name|
                name.contains(" expression notification ") && name.contains(&format!("_{}_", hex(uid)))))
    };
    g.until("the expression notifier owner to be listed", |g| owns_notification(g, bound_reader).then_some(()));
    let stalled = evaluator.hold_value(0.25);
    g.call("variable entry edit", j!({ "name": "desk.after", "value": 0.25 }));
    g.until("the retired node evaluation to enter", |_| stalled.entered().then_some(()));
    assert!(owns_notification(&g, bound_reader), "the unchanged live binding still owns its notifier while evaluating");
    g.call("node param edit", j!({ "node": hex(bound_reader), "param": "constant/value", "expression": "" }));
    g.until("clearing the held binding to release its notifier", |g| (!owns_notification(g, bound_reader)).then_some(()));
    // Replacement jobs remain owned by their cells, even when the evaluator cannot drain them.
    g.call("variable entry edit", j!({ "name": "desk.after", "value": 0.75 }));
    g.call("node param edit", j!({ "node": hex(bound_reader), "param": "constant/value", "expression": "variables.desk.after * 1" }));
    g.call("node param edit", j!({ "node": hex(bound_reader), "param": "constant/value", "expression": "variables.desk.before * 1" }));
    g.call("node remove", j!({ "node": hex(bound_reader) }));
    drop(stalled);
    let witness = g.add("_TestConst");
    let completed = g.probe(witness, "out");
    g.call("node param edit", j!({ "node": hex(witness), "param": "constant/value", "expression": "variables.desk.after * 1" }));
    g.until("live work after retired replacements to complete", |_| completed.latest().filter(|frame| f32s(frame) == [0.75]));
    assert!(!owns_notification(&g, bound_reader), "a retired result cannot reopen the removed node's notifier");
    g.call("node remove", j!({ "node": hex(witness) }));
    g.until("removing the live reader to release its notifier", |g| (!owns_notification(g, witness)).then_some(()));
    for name in ["desk.before", "desk.after"] { g.call("variable entry remove", j!({ "name": name })); }

    // A panel made a control panel with no group of its own is born naming a fresh one.
    g.call("layout panel edit", j!({ "panel": first_panel(&g), "type": "viewer" }));
    g.call("layout panel edit", j!({ "panel": first_panel(&g), "type": "control" }));
    assert_eq!(group_of(&g), j!("control0"), "the first free `controlN` was minted for it");
    // The `control` door is ONE undo step each; the manager mints the element and the cell where
    // none is given.
    let born = g.call("control add", j!({ "group": "control0", "kind": "knob" }));
    assert_eq!(born["name"], "control0.knob0", "{born}");
    let born = g.call("control add", j!({ "group": "control0", "kind": "slider" }));
    assert_eq!((&born["name"], &born["control"]["x"], &born["control"]["y"]), (&j!("control0.slider0"), &j!(4.0), &j!(0.0)),
               "placed in the first free cell beside the knob: {born}");
    g.call("control edit", j!({ "group": "control0", "element": "slider0", "name": "level", "max": 10.0 }));
    assert_eq!(g.doc()["variables"]["control0.level"]["control"]["max"], 10.0);
    g.call("control edit", j!({ "group": "control0", "element": "level", "expression": "nd('carrier')" }));
    let listed = g.call("control list", j!({}));
    assert_eq!(listed["groups"]["control0"]["elements"][1]["expression"], "nd('carrier')", "{listed}");
    assert_eq!(listed["panels"][0]["group"], "control0", "{listed}");
    // A widget that FOLLOWS a producer is still a widget: a move edits the record BESIDE the value
    // and never the value, so neither a source nor a value lock refuses the drag that made it.
    g.call("variable entry lock", j!({ "name": "control0.level", "value": true }));
    g.call("control edit", j!({ "group": "control0", "element": "level", "x": 0.0, "y": 3.0 }));
    assert_eq!(g.doc()["variables"]["control0.level"]["control"]["y"], 3.0, "the followed widget moved");
    g.call("variable entry lock", j!({ "name": "control0.level", "value": false }));
    // A `paint` widget holds an `[h, w, 4]` RGBA sheet in 0..1. `control paint` draws a script of
    // ops onto it as ONE undoable edit, and a raw snapshot of the variable reads the sheet.
    g.call("control add", j!({ "group": "control0", "kind": "paint", "element": "pad", "resolution": 100 }));
    let texel = |d: &goofi_core::Data, row: usize, col: usize| -> [f32; 4] {
        let at = (row * 100 + col) * 4;
        f32s(d)[at..at + 4].try_into().expect("four channels")
    };
    let blank = g.snapshot_array("variables/control0.pad");
    assert_eq!(goofi_tests::shape(&blank), vec![100, 100, 4]);
    assert!(f32s(&blank).iter().all(|v| *v == 0.0), "a pad is born clear");
    let script = "stroke #ff0000 width 60 : M 100 100 L 900 100\n\
                  stroke #0000ff width 60 soft 20 : M 100 300 L 100 900\n\
                  fill #00ff00 : M 500 500 L 950 500 L 950 950 L 500 950 Z\n\
                  stroke erase width 100 cap butt : M 500 700 L 950 700";
    let drew = g.call("control paint", j!({ "group": "control0", "element": "pad", "ops": script }));
    assert_eq!((&drew["ops"], &drew["shape"]), (&j!(4), &j!([100, 100, 4])), "{drew}");
    let frame = g.until("the drawn sheet", |g| Some(g.snapshot_array("variables/control0.pad")).filter(|d| texel(d, 10, 50)[3] > 0.0));
    let close = |got: [f32; 4], want: [f32; 4]| got.iter().zip(want).all(|(a, b)| (a - b).abs() < 0.02);
    assert!(close(texel(&frame, 10, 50), [1.0, 0.0, 0.0, 1.0]), "{:?}", texel(&frame, 10, 50));
    assert_eq!(texel(&frame, 40, 50)[3], 0.0, "the sheet is clear where nothing was drawn");
    assert!(close(texel(&frame, 60, 80), [0.0, 1.0, 0.0, 1.0]), "{:?}", texel(&frame, 60, 80));
    assert_eq!(texel(&frame, 70, 80)[3], 0.0, "the eraser cut through the fill");
    let soft = texel(&frame, 60, 10);
    assert!(soft[2] > 0.99 && soft[3] > 0.5, "the soft stroke's middle: {soft:?}");
    let edge = texel(&frame, 60, 14);
    assert!(edge[3] > 0.02 && edge[3] < 0.9, "the soft stroke fades at its edge: {edge:?}");
    // A second call draws over the sheet, and undo takes back that one stroke only: the inverse
    // holds the previous sheet and publishes it again.
    g.call("control paint", j!({ "group": "control0", "element": "pad", "ops": "stroke #000 width 100 : M 0 0 L 1000 1000" }));
    assert_eq!(texel(&frame, 25, 25)[3], 0.0, "clear before the diagonal");
    g.until("the diagonal", |g| Some(g.snapshot_array("variables/control0.pad")).filter(|d| texel(d, 25, 25)[3] > 0.0));
    g.call("undo", j!({}));
    g.until("the sheet before the stroke", |g| Some(g.snapshot_array("variables/control0.pad")).filter(|d| f32s(d) == f32s(&frame)));
    // A clear starts the sheet over.
    g.call("control paint", j!({ "group": "control0", "element": "pad", "ops": "clear; stroke width 100 : M 0 0 L 100 100" }));
    g.until("a cleared sheet", |g| Some(g.snapshot_array("variables/control0.pad")).filter(|d| texel(d, 10, 50)[3] == 0.0 && texel(d, 5, 5)[3] > 0.0));
    let why = g.refuse("control paint", j!({ "group": "control0", "element": "pad",
                                            "ops": "stroke : M 1 1\nstrok : M 2 2" }));
    assert!(why.contains("line 2") && why.contains("strok"), "a refusal names the line: {why}");
    let why = g.refuse("control paint", j!({ "group": "control0", "element": "pad", "ops": "stroke : L 1 1" }));
    assert!(why.contains("starts with `M"), "{why}");
    let why = g.refuse("control paint", j!({ "group": "control0", "element": "knob0", "ops": "clear" }));
    assert!(why.contains("knob"), "only a `paint` widget holds a sheet: {why}");
    // A re-sized pad starts clear at its new shape; a text is not a sheet.
    g.call("control edit", j!({ "group": "control0", "element": "pad", "resolution": 8 }));
    assert_eq!(goofi_tests::shape(&g.until("the re-sized sheet", |g| Some(g.snapshot_array("variables/control0.pad")).filter(|d| goofi_tests::shape(d)[0] == 8))), vec![8, 8, 4]);
    let why = g.refuse("variable entry edit", j!({ "name": "control0.pad", "value": "!!" }));
    assert!(why.contains("paint") && why.contains("string"), "{why}");
    g.call("control remove", j!({ "group": "control0", "element": "pad" }));

    for kind in goofi_core::variables::ControlKind::ALL {
        let born = g.call("control add", j!({ "group": "kinds", "kind": kind.as_str() }));
        assert_eq!(born["name"], format!("kinds.{}0", kind.as_str()));
        let record = &born["control"];
        assert!(record["w"].as_f64().is_some_and(|w| (1.0..=goofi_core::variables::CONTROL_COLUMNS).contains(&w)));
        assert!(record["h"].as_f64().is_some_and(|h| h >= 1.0));
        assert_eq!(record["kind"], kind.as_str());
        g.call("control remove", j!({ "group": "kinds", "element": born["name"].as_str().unwrap().split_once('.').unwrap().1 }));
    }

    // A panel's edit mode is the panel's own view and no state of the manager's, so the widget door
    // is held by a config lock exactly as every other variables door is.
    g.call("variable group lock", j!({ "group": "control0", "config": true }));
    for (op, payload) in [
        ("control add", j!({ "group": "control0", "kind": "toggle" })),
        ("control edit", j!({ "group": "control0", "element": "level", "max": 4.0 })),
        ("control edit", j!({ "group": "control0", "element": "level", "expression": "" })),
        ("control remove", j!({ "group": "control0", "element": "level" })),
    ] {
        let why = g.refuse(op, payload);
        assert!(why.contains("config-locked"), "`{op}` under a config lock: {why}");
    }
    g.call("variable group lock", j!({ "group": "control0", "config": false }));
    g.call("control remove", j!({ "group": "control0", "element": "level" }));
    assert!(g.doc()["variables"]["control0.level"].is_null());
    // A lock holds what it names: a group's `config` freezes every name and the membership, an
    // entry's `value` freezes its value — and each lock is ONE undoable command.
    g.call("variable group lock", j!({ "group": "desk", "config": true }));
    for (op, payload) in [
        ("variable entry add", j!({ "name": "desk.more", "value": 1.0 })),
        ("variable entry rename", j!({ "name": "desk.handle", "to": "desk.grip" })),
        ("variable entry remove", j!({ "name": "desk.handle" })),
        ("variable group rename", j!({ "from": "desk", "to": "bench" })),
    ] {
        let why = g.refuse(op, payload);
        assert!(why.contains("config-locked"), "`{op}` under a config lock: {why}");
    }
    assert_eq!(g.call("variable entry edit", j!({ "name": "desk.handle", "value": "P02" }))["value"], "P02",
               "a config lock leaves the value free");
    g.call("variable entry lock", j!({ "name": "desk.handle", "value": true }));
    let why = g.refuse("variable entry edit", j!({ "name": "desk.handle", "value": "P03" }));
    assert!(why.contains("value-locked"), "{why}");
    let held = g.call("variable list", j!({}))["variables"].as_array().unwrap().iter()
        .find(|e| e["name"] == "desk.handle").cloned().unwrap()["lock"].clone();
    assert_eq!(held, j!({ "config": true, "value": true }), "the list answers what holds it, both locks together");
    g.call("variable entry lock", j!({ "name": "desk.handle", "value": false }));
    g.call("variable group lock", j!({ "group": "desk", "config": false }));
    let why = g.refuse("variable group lock", j!({ "group": "system", "config": false }));
    assert!(why.contains("goofi's own"), "the system group's lock is nobody's to set: {why}");
    // A group can be EMPTY, minted at the first free `groupN`, and an entry born with no value
    // holds 0 until edited; each is ONE step, and an edit may change its form.
    assert_eq!(g.call("variable group add", j!({}))["group"], "group0");
    assert!(g.doc()["variable_groups"]["group0"].is_object());
    g.call("undo", j!({}));
    assert!(g.doc()["variable_groups"].get("group0").is_none());
    g.call("redo", j!({}));
    assert_eq!(g.call("variable group add", j!({}))["group"], "group1");
    g.call("variable group rename", j!({ "from": "group0", "to": "bench" }));
    assert_eq!(g.call("variable group add", j!({}))["group"], "group0", "the freed name is minted again");
    g.refuse("variable group add", j!({ "group": "bench" }));
    g.refuse("variable group add", j!({ "group": "bad name" }));
    assert_eq!(g.call("variable entry add", j!({ "group": "bench" }))["name"], "bench.entry0");
    assert_eq!(g.call("variable entry add", j!({ "group": "bench" }))["name"], "bench.entry1");
    // A value is an array or a string, and an edit may switch between the two forms: a number, a
    // bool and a list are arrays, nested for a wider one; text is a string. Undo walks it back.
    g.call("variable entry edit", j!({ "name": "bench.entry0", "value": "hello" }));
    assert_eq!(g.variable("bench.entry0"), "hello");
    g.call("undo", j!({}));
    assert_eq!(g.variable("bench.entry0"), 0);
    g.call("redo", j!({}));
    assert_eq!(g.call("variable entry edit", j!({ "name": "bench.entry1", "value": true }))["value"], 1,
               "a bool is stored as the number it reads as");
    assert_eq!(g.call("variable entry edit", j!({ "name": "bench.entry1", "value": [[1, 2], [3, 4.5]] }))["value"],
               j!([[1, 2], [3, 4.5]]), "a nested list is an array of that shape, printed back as it was typed");
    assert_eq!(g.variable("bench.entry1"), j!([[1, 2], [3, 4.5]]));
    g.refuse("variable entry edit", j!({ "name": "bench.entry1", "value": [[1, 2], [3]] }));
    g.refuse("variable entry edit", j!({ "name": "bench.entry1", "value": { "no": "object" } }));
    g.call("undo", j!({}));
    assert_eq!(g.variable("bench.entry1"), 1);
    g.refuse("variable entry edit", j!({ "name": "system.default_ufreq", "value": "no" }));
    g.call("variable entry add", j!({ "name": "bench.knob", "value": 1.0,
        "control": { "kind": "knob", "x": 0, "y": 0, "w": 3, "h": 3 } }));
    let before = g.doc()["variables"]["bench.knob"].clone();
    let why = g.refuse("variable entry edit", j!({ "name": "bench.knob", "value": "no" }));
    assert!(why.contains("knob") && why.contains("string"), "a widget draws what it can: {why}");
    assert_eq!(g.doc()["variables"]["bench.knob"], before, "a widget's entry keeps its form");

    // …and a compound is a UNIT: a refused step takes back the one that landed, and records nothing,
    // which is what the step count below would catch.
    let why = g.refuse("compound", j!({ "ops": [
        { "op": "variable entry add", "payload": { "name": "patch.tmp", "value": 1.0 } },
        { "op": "node edit", "payload": { "node": GHOST, "name": "renamed" } },
    ] }));
    assert!(why.contains("step 1"), "the refusal names the step that failed: {why}");
    assert!(g.doc()["variables"]["patch.tmp"].is_null(), "the step that landed was taken back: {why}");
    // …and a node added in a refused batch is never born: births happen at settle, from the state
    // the whole batch left, so the add plus its rollback cost the engine nothing.
    let twin = LibraryEngine::named("twin", &["Stillborn"]);
    let births = twin.births();
    g.graph().register_engine(Box::new(twin));
    g.refuse("compound", j!({ "ops": [
        { "op": "node add", "payload": { "type": "twin:Stillborn" } },
        { "op": "node edit", "payload": { "node": GHOST, "name": "renamed" } },
    ] }));
    assert!(births.lock().is_empty(), "a rolled-back add reached the engine: {:?}", births.lock());
    let kept = g.add("twin:Stillborn");
    assert_eq!(*births.lock(), vec![kept], "a committed add is born once");
    // A READ rides a batch — its result in the bare list the batch answers — while an EFFECT is
    // refused: its consequences are not the history's to take back, so it runs alone.
    let ridden = g.call("compound", j!({ "ops": [
        { "op": "nodes inspect" },
        { "op": "node edit", "payload": { "node": hex(osc), "pos": [1.0, 2.0] } },
    ] }));
    assert!(ridden[0]["text"].as_str().is_some_and(|t| t.contains("carrier")),
            "the read's own reply rides the list: {ridden}");
    for bad in ["undo", "compound", "session load", "session new", "session save",
                "node restart", "library refresh"] {
        let why = g.refuse("compound", j!({ "ops": [{ "op": bad }] }));
        assert!(why.contains("not a step"), "`{bad}`: {why}");
    }
    // A batch's deliveries settle ONCE: a compound editing one param twice lands on the node as a
    // single write, carrying the final value. The meter counts `on_param_changed` calls.
    let meter = g.add("_TestParamWrites");
    let mprobe = g.probe(meter, "out");
    g.link(osc, "out", meter, "input");
    let base = g.until("the meter to settle after init", |_| {
        mprobe.latest().map(|d| f32s(&d)[0])
    });
    g.call("compound", j!({ "ops": [
        { "op": "node param edit", "payload": { "node": hex(meter), "param": "control/value", "value": 1.0 } },
        { "op": "node param edit", "payload": { "node": hex(meter), "param": "control/value", "value": 2.0 } },
    ] }));
    g.until("exactly one more delivery, carrying the final value", |g| {
        let n = f32s(&mprobe.latest()?)[0];
        (n >= base + 1.0).then(|| {
            assert_eq!(n, base + 1.0, "two edits in one batch settled as one write");
            assert_eq!(g.doc()["nodes"][hex(meter)]["params"]["control"]["value"]["value"], j!(2));
        })
    });
    let scope = g.call("nodes group", j!({ "nodes": [hex(osc), hex(buf)], "pos": [0.0, 0.0] }))["inst_id"]
        .as_str().unwrap().to_string();
    let built = g.doc();

    // A compound is ONE step though it is an add plus a remove composed.
    let expected_steps = 99 + 2 * goofi_core::variables::ControlKind::ALL.len();
    let mut steps = 0;
    while g.call("undo", j!({}))["changed"] == true {
        steps += 1;
        assert!(steps <= expected_steps, "the stack never emptied");
    }
    assert!(g.nodes().is_empty() && g.instances().is_empty(), "back to an empty patch");
    assert!(g.doc()["variables"]["desk.handle"].is_null() && g.doc()["variables"]["patch.subj"].is_null());
    assert_eq!(steps, expected_steps, "one step per command — a compound (the rename, the two-edit batch) and a three-field node edit are each ONE");

    while g.call("redo", j!({}))["changed"] == true {}
    assert_eq!(g.doc(), built, "redo rebuilt the patch it undid, uid for uid");
    let _ = scope;

    // The history is per session: s1's undo takes back s1's newest step and leaves a peer's node
    // standing, and a fresh command discards the redo run.
    let two = g.client("s2");
    let peer = two.add("Buffer");
    assert_eq!(g.call("undo", j!({}))["changed"], true);
    assert!(g.instances().is_empty() && g.nodes().contains(&hex(peer)), "s1's undo left s2's node standing");
    assert_eq!(g.call("redo", j!({}))["changed"], true);
    g.call("undo", j!({}));
    let before = g.doc();
    let labels = g.state.history_labels("test");
    g.refuse("node add", j!({ "type": "MissingNodeType" }));
    assert_eq!(g.state.history_labels("test"), labels, "a refused write keeps the redo run");
    g.refuse("compound", j!({ "ops": [
        { "op": "variable entry add", "payload": { "name": "patch.tmp", "value": 1.0 } },
        { "op": "node edit", "payload": { "node": GHOST, "name": "renamed" } },
    ] }));
    assert_eq!(g.doc(), before, "the refused batch leaves the graph unchanged");
    assert_eq!(g.state.history_labels("test"), labels, "the refused batch keeps the redo run");
    assert_eq!(g.call("redo", j!({}))["changed"], true);
    g.call("undo", j!({}));
    g.add("Buffer");
    let r = g.call("redo", j!({}));
    assert_eq!(r["changed"], false, "the redo run went with the new command");
    assert!(r["redo"].is_null(), "{r}");
    // Empty groups and the edited entries reach the file and come back.
    let saved = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    g.call("session load", j!({ "content": saved }));
    assert!(g.doc()["variable_groups"]["group0"].is_object() && g.doc()["variable_groups"]["group1"].is_object());
    assert_eq!(g.variable("bench.entry0"), "hello");
    assert_eq!(g.variable("bench.entry1"), 1);

    // The NAME is the arrangement's to mint: a caller that asks for none gets the first free
    // `Tab n`, so nobody has to reserve one against a strip they cannot see settle.
    let g = Goofi::new();
    g.call("layout panel add", j!({}));
    assert_eq!(strip(&g), ["Tab 1", "Tab 2"], "minted, not asked for");
    // …and a label is NOT unique. It addresses nothing — every op names an id — and uniqueness was
    // enforceable only on the way in: a rename's inverse must not refuse, so a peer taking the
    // freed name left an arrangement the loader would not open.
    g.call("layout tab edit", j!({ "tab": tab_id(&g, "Tab 2"), "name": "Tab 1" }));
    assert_eq!(strip(&g), ["Tab 1", "Tab 1"]);
    assert_eq!(reload_warning(&g), Value::Null, "and it still opens");
    while g.call("undo", j!({}))["changed"] == true {}

    // A REORDER can silently invert to a no-op: its content IS a position.
    let g = Goofi::new();
    g.call("layout panel add", j!({ "name": "Two" }));
    g.call("layout panel add", j!({ "name": "Three" }));
    let settled = strip(&g);
    assert_eq!(settled, ["Tab 1", "Two", "Three"]);

    let three = tab_id(&g, "Three");
    g.call("layout move", j!({ "entry": three, "index": 0 }));
    assert_eq!(strip(&g), ["Three", "Tab 1", "Two"], "the tab moved to the head of the strip");
    assert_eq!(g.call("undo", j!({}))["changed"], true);
    assert_eq!(strip(&g), settled, "a reorder's undo puts the tab back where it came from");
    assert_eq!(g.call("redo", j!({}))["changed"], true);
    assert_eq!(strip(&g), ["Three", "Tab 1", "Two"], "and the redo moves it again");

    while g.call("undo", j!({}))["changed"] == true {}
    assert_eq!(strip(&g), ["Tab 1"], "back to the arrangement a fresh patch opens with");
}

/// A knob turn is many previews and one commit: the op each time, but the history keeps one entry
/// whose way back is the state before the drag, the patch stays clean until the commit, a peer
/// sees every step, and a socket cut mid-drag leaves the last committed value.
#[tokio::test]
async fn a_drag_previews_and_commits_as_one_step_and_a_cut_socket_takes_it_back() {
    let g = Goofi::new();
    let osc = g.add("LFO");
    g.set_param(osc, "lfo", "amplitude", 1.0);
    let dir = tempfile::tempdir().unwrap();
    g.call("session save", j!({ "path": dir.path().join("drag.gfi").to_string_lossy() }));
    let amp = |g: &Goofi| g.doc()["nodes"][hex(osc)]["params"]["lfo"]["amplitude"]["value"].clone();
    let dirty = |g: &Goofi| g.call("session status", j!({}))["dirty"] == true;
    let edit = |v: f64| j!({ "node": hex(osc), "param": "lfo/amplitude", "value": v });
    assert!(!dirty(&g));

    for v in [0.2, 0.4, 0.6] {
        g.preview("node param edit", edit(v));
        assert_eq!(amp(&g), j!(v), "a preview moves the graph for real");
    }
    assert!(!dirty(&g), "a preview is not an edit of the patch");
    g.call("node param edit", edit(0.7));
    assert!(dirty(&g), "the commit is");
    assert_eq!(g.call("undo", j!({}))["changed"], true);
    assert_eq!(amp(&g), j!(1), "ONE undo returns to before the drag, not to its last preview");
    assert_eq!(g.call("redo", j!({}))["changed"], true);
    assert_eq!(amp(&g), j!(0.7));

    // An undo with a drag in flight takes the drag back first, then flips.
    g.preview("node param edit", edit(0.3));
    assert_eq!(g.call("undo", j!({}))["changed"], true);
    assert_eq!(amp(&g), j!(1));
    assert_eq!(g.call("redo", j!({}))["changed"], true);
    assert_eq!(amp(&g), j!(0.7));

    // Only an undoable write has a way back to keep; a read and a birth cannot be previewed.
    for (op, payload) in [("session status", j!({})), ("node add", j!({ "type": "LFO" })),
                          ("compound", j!({ "ops": [] }))] {
        let why = g.state.preview(op, payload, "s1").expect_err(op);
        assert!(why.contains("preview"), "{op}: {why}");
    }

    // Over the socket: a peer's replica follows each step, and the socket's close ends the drag.
    let base = g.serve().await;
    let (mut peer, _) = Client::connect_as(&base, "peer").await;
    let (mut tab, _) = Client::connect_as(&base, "tab").await;
    tab.preview("node param edit", edit(0.1), "lfo/amplitude").await;
    peer.until_doc(|d| d.to_json()["nodes"][hex(osc)]["params"]["lfo"]["amplitude"]["value"] == j!(0.1)).await;
    drop(tab);
    peer.until_doc(|d| d.to_json()["nodes"][hex(osc)]["params"]["lfo"]["amplitude"]["value"] == j!(0.7)).await;
    assert!(dirty(&g), "the commit before the drag is still an edit");
}

#[test]
fn a_stale_toggle_converges_instead_of_wedging_the_stack() {
    let one = Goofi::new();
    let two = one.client("s2");
    let osc = one.add("LFO");
    let buf = one.add("Buffer");
    let link = j!({ "from": ep(hex(osc), "out"), "to": ep(hex(buf), "input") });
    one.call("link add", link.clone());
    one.call("link remove", link);
    two.call("node remove", j!({ "node": hex(buf) })); // s1's newest toggle now names a dead uid

    assert_eq!(one.call("undo", j!({}))["changed"], true);
    assert_eq!(one.call("redo", j!({}))["changed"], true);
    for _ in 0..4 {
        assert_eq!(one.call("undo", j!({}))["changed"], true, "the stack stays walkable to empty");
    }

    // The same rule for a boundary port: a peer removed the port under s1's newest rename.
    let osc = one.add("LFO");
    let inst = one.call("nodes group", j!({ "nodes": [hex(osc)], "pos": [0.0, 0.0] }))["inst_id"]
        .as_str().unwrap().to_string();
    let port = one.call("node add", j!({ "type": "InArray", "inst_id": inst, "pos": [0.0, 0.0] }))
        ["uid"].as_str().expect("a port uid").to_string();
    one.call("node edit", j!({ "node": port, "name": "left" }));
    two.call("node remove", j!({ "node": port }));
    assert_eq!(one.call("undo", j!({}))["changed"], true, "the stale port rename still flips");
    assert_eq!(one.call("redo", j!({}))["changed"], true);

    let before = one.doc();
    let bad_control = j!({ "kind": "text", "x": 0, "y": 0, "w": 2, "h": 2 });
    one.refuse("variable entry add", j!({ "name": "audit.bad", "value": 1.0, "control": bad_control }));
    assert_eq!(one.doc(), before);
    one.call("variable entry add", j!({ "name": "audit.bad", "value": 1.0 }));
    one.refuse("variable entry edit", j!({ "name": "audit.bad", "value": 2.0, "control": bad_control }));
    one.call("variable entry add", j!({ "name": "audit.after", "value": 3.0 }));
    assert_eq!(one.variable("audit.bad"), 1.0);
    one.refuse("compound", j!({ "ops": [
        { "op": "variable entry edit", "payload": { "name": "audit.after", "value": 4.0 } },
        { "op": "variable entry edit", "payload": { "name": "audit.bad", "value": 5.0, "control": bad_control } }
    ] }));
    assert_eq!(one.variable("audit.after"), 3.0);
    one.call("undo", j!({}));
    assert!(one.doc()["variables"].get("audit.after").is_none());
    two.call("variable entry remove", j!({ "name": "audit.bad" }));
    one.call("undo", j!({}));
    one.call("redo", j!({}));
    assert!(one.doc()["variables"].get("audit.bad").is_none());

    one.call("variable entry add", j!({ "name": "first.one", "value": 1.0 }));
    two.call("variable entry add", j!({ "name": "second.two", "value": 2.0 }));
    let before = one.doc();
    one.refuse("variable group rename", j!({ "from": "first", "to": "second" }));
    assert_eq!(one.doc(), before);
    two.call("variable group lock", j!({ "group": "locked", "value": true }));
    one.refuse("variable group rename", j!({ "from": "first", "to": "locked" }));
    two.call("layout panel edit", j!({ "panel": first_panel(&one), "type": "control", "state": { "group": "panelonly" } }));
    one.refuse("variable group rename", j!({ "from": "first", "to": "panelonly" }));
    one.call("variable group rename", j!({ "from": "first", "to": "renamed" }));
    one.call("undo", j!({}));
    assert_eq!(one.variable("first.one"), 1.0);
    one.call("redo", j!({}));
    two.call("variable entry add", j!({ "name": "renamed.peer", "value": 2.0 }));
    one.call("undo", j!({}));
    assert_eq!(one.variable("renamed.peer"), 2.0);
    assert_eq!(one.variable("renamed.one"), 1.0);
    assert!(one.doc()["variables"].get("first.peer").is_none());

    one.call("variable entry edit", j!({ "name": "renamed.one", "value": 4.0 }));
    two.call("variable entry lock", j!({ "name": "renamed.one", "value": true }));
    one.call("undo", j!({}));
    assert_eq!(one.variable("renamed.one"), 4.0);
    one.refuse("variable entry edit", j!({ "name": "renamed.one", "value": 5.0 }));
    one.call("variable entry rename", j!({ "name": "second.two", "to": "second.moved" }));
    two.call("variable entry rename", j!({ "name": "second.moved", "to": "second.peer" }));
    one.call("undo", j!({}));
    assert_eq!(one.variable("second.peer"), 2.0);
}

#[test]
fn a_deleted_sub_patch_comes_back_whole_with_the_panels_that_named_it() {
    let g = Goofi::new();
    let a = g.add("LFO");
    let b = g.add("Buffer");
    g.link(a, "out", b, "input");
    let inst = g.call("nodes group", j!({ "nodes": [hex(a), hex(b)], "pos": [0.0, 0.0] }))["inst_id"]
        .as_str().unwrap().to_string();
    let panel = first_panel(&g);
    g.call("layout panel edit", j!({ "panel": panel, "type": "viewer",
                                     "state": { "node": hex(a) } }));

    // A panel can name a boundary PORT — it exposes a real stream — so a removed port has to take
    // its binding with it exactly as a removed node does, or the panel renders empty for good and
    // refuses even a change of viewer kind, because the dead uid has no slots to check against.
    let port = g.call("node add", j!({ "type": "InArray", "inst_id": inst, "pos": [0.0, 0.0] }))
        ["uid"].as_str().expect("a port uid").to_string();
    let second = split(&g, &panel);
    g.call("layout panel edit", j!({ "panel": second, "type": "viewer",
                                     "state": { "node": port, "slot": "value" } }));
    g.call("node remove", j!({ "node": port }));
    let unbound = |p: &str| entries(&g)[p]["state"]["node"].is_null();
    assert!(unbound(&second), "the port took its panel binding: {}", entries(&g)[&second]["state"]);
    g.call("undo", j!({}));
    assert_eq!(entries(&g)[&second]["state"]["node"], port,
               "and one undo gives the port and the binding back together");

    g.call("node remove", j!({ "node": inst }));
    assert!(g.nodes().is_empty() && g.instances().is_empty(), "the subtree went with the scope");
    assert_eq!(entries(&g)[&panel]["state"], j!({ "node": null }), "and the binding with it");
    assert!(unbound(&second),
            "…including a panel that named one of its PORTS, which the subtree sweep must reach");

    g.call("undo", j!({}));
    assert_eq!(g.instances(), vec![inst], "the scope is back at the same uid");
    assert_eq!(g.nodes().len(), 2, "with both members");
    assert_eq!(entries(&g)[&panel]["state"], j!({ "node": hex(a) }),
               "and the panel names its node again");
}

/// Two people undo layout: every SHAPE a layout write comes in, driven through the one
/// interleaving that shows a raw-state restore, and the op list from the REGISTRY proves no op
/// slipped past without one. Then a peer's panel through a foreign undo, and a drag as one step.
#[test]
fn two_people_undo_layout_and_no_slot_is_put_back_no_panel_is_lost_and_a_drag_is_one_step() {
    let ops: Vec<&str> = goofi_bridge::ops::registry().iter()
        .filter(|o| o.handler.is_write() && o.name.starts_with("layout "))
        .map(|o| o.name)
        .collect();
    assert!(ops.contains(&"layout remove") && ops.contains(&"layout move"),
            "the registry filter still finds the layout write ops: {ops:?}");

    // (shape, the op it goes out as). One row per way a caller can spell a layout write — the
    // SHAPES are the truth here and the op names are not, which is how they survived the merged
    // ops splitting into one op per record kind.
    const SHAPES: &[(&str, &str)] = &[
        ("a fresh tab", "layout panel add"),
        ("a tab built around a subtree", "layout move"),
        ("a split", "layout panel add"),
        ("a tab's name", "layout tab edit"),
        ("a panel's type", "layout panel edit"),
        ("a split's shares", "layout split edit"),
        ("a move into a split", "layout move"),
        ("a move beside a panel", "layout move"),
        ("a move within the strip", "layout move"),
        ("a closed panel", "layout remove"),
        ("a closed tab", "layout remove"),
    ];
    for op in &ops {
        assert!(SHAPES.iter().any(|(_, o)| o == op),
                "`{op}` is a layout write op with no shape here — drive it through this guard, and \
                 say why if its inverse may restore a slot");
    }

    let mut stranded = Vec::new();
    for (shape, op) in SHAPES {
        let one = Goofi::new();
        let two = one.client("s2");
        let a = first_panel(&one);
        let b = split(&one, &a);
        one.call("layout panel add", j!({}));
        let c = panels(&one).into_iter().find(|p| *p != a && *p != b).expect("the tab's panel");
        let e = split(&one, &c);
        let far = entries(&one)[&e]["parent"].as_str().unwrap().to_string();
        let near = entries(&one)[&b]["parent"].as_str().unwrap().to_string();
        let two_id = tab_id(&one, "Tab 2");

        one.call(op, match *shape {
            "a fresh tab" => j!({}),
            "a tab built around a subtree" => j!({ "entry": b }),
            "a split" => j!({ "beside": a }),
            "a tab's name" => j!({ "tab": two_id, "name": "Deux" }),
            "a panel's type" => j!({ "panel": b, "type": "console" }),
            "a split's shares" => j!({ "split": near, "fraction": [0.3, 0.7] }),
            "a move into a split" => j!({ "entry": b, "in": far, "index": 0 }),
            "a move beside a panel" => j!({ "entry": b, "beside": c, "side": "bottom" }),
            "a move within the strip" => j!({ "entry": two_id, "index": 0 }),
            "a closed panel" => j!({ "entry": b }),
            "a closed tab" => j!({ "entry": two_id }),
            new => panic!("`{new}` is a shape with no payload here"),
        });
        // The peer builds exactly where a slot-restore inverse would want to write — both places,
        // because a merged op's shapes do not all reach for the same one.
        two.call("layout panel add", j!({}));
        two.call("layout panel add", j!({ "beside": a }));
        assert_eq!(one.call("undo", j!({}))["changed"], true, "{shape}: the undo flipped nothing");

        if reload_warning(&one) != Value::Null {
            stranded.push(*shape);
        }
    }
    let empty: [&str; 0] = [];
    assert_eq!(stranded, empty, "an undo left an arrangement the manager cannot itself open");

    // A peer's panel survives every shape of foreign undo and redo.
    let one = Goofi::new();
    let two = one.client("s2");
    let a = first_panel(&one);

    let mine = split(&one, &a);
    let theirs = split(&two, &mine);
    assert_eq!(one.call("undo", j!({}))["changed"], true);
    let up = entries(&one)[&theirs]["parent"].as_str().unwrap().to_string();
    assert!(entries(&one).contains_key(&up), "the peer's panel still hangs off something");

    let peer2 = split(&two, &a);
    assert_eq!(one.call("redo", j!({}))["changed"], true);
    assert!(panels(&one).contains(&peer2), "the peer's panel survived a foreign redo");

    one.call("layout panel add", j!({ "name": "Signals" }));
    let over = panels(&one).into_iter()
        .find(|p| ![&a, &mine, &theirs, &peer2].contains(&p)).expect("the new tab's panel");
    let far = split(&one, &over);
    let dest = entries(&one)[&far]["parent"].as_str().unwrap().to_string();
    one.call("layout move", j!({ "entry": mine, "in": dest, "index": 0 }));
    let peer3 = split(&two, &a);
    assert_eq!(one.call("undo", j!({}))["changed"], true);
    assert!(panels(&one).contains(&peer3), "the peer's panel survived a foreign undo");
    assert_eq!(reload_warning(&one), Value::Null);

    // The drag feel is FROZEN UX; as primitive ops one drop would cost three to five commands.
    let mover = split(&one, &a);
    one.call("layout panel add", j!({ "name": "Drops", "index": 0 }));
    let target = panels(&one).into_iter()
        .find(|p| ![&a, &mine, &theirs, &peer2, &peer3, &over, &far, &mover].contains(&p)).expect("its panel");
    let before = entries(&one);

    one.call("layout move", j!({ "entry": mover, "beside": target,
                                 "side": "top", "ratio": 0.3 }));
    assert_ne!(entries(&one), before, "the drop moved something");
    // The SIDE really landed: `top` means a column split with the mover FIRST. This is the pin
    // that caught `--side` being read off a dead key and every drop silently going right.
    let parent = entries(&one)[&mover]["parent"].as_str().unwrap().to_string();
    let split = &entries(&one)[&parent];
    assert_eq!(split["axis"], "column", "a `top` drop splits vertically: {split}");
    assert_eq!(split["children"][0]["id"], j!(mover.as_str()),
               "…with the mover on the side it was dropped on: {split}");
    one.refuse("layout move", j!({ "entry": mover, "beside": target, "side": "sideways" }));
    assert_eq!(one.call("undo", j!({}))["changed"], true);
    assert_eq!(entries(&one), before, "ONE ctrl-Z put the whole drag back");

    one.call("layout move", j!({ "entry": mover, "name": "Torn off", "index": 0 }));
    assert_eq!(one.doc()["arrangement"]["tabs"][0]["root"]["id"], mover.as_str(),
               "the dragged panel is the new tab's whole root");
    one.call("undo", j!({}));
    assert_eq!(entries(&one), before, "and one ctrl-Z put that back too");
}

#[test]
fn a_restart_is_recovery_and_touches_neither_the_stack_nor_the_file() {
    // `restart_node` is the one op where "could have mutated the graph" does not imply dirty.
    let g = Goofi::new();
    let osc = g.add("LFO");
    let buf = g.add("Buffer");
    g.link(osc, "out", buf, "input");
    let yaml = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    g.call("session load", j!({ "content": yaml })); // the patch now matches "disk"
    assert_eq!(g.call("session status", j!({}))["dirty"], false);

    let uid = g.nodes()[0].clone();
    let before = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    g.call("node restart", j!({ "node": uid }));

    assert_eq!(g.call("session manifest", j!({}))["yaml"].as_str().unwrap(), before,
               "a restart changes nothing that reaches the .gfi");
    assert_eq!(g.call("session status", j!({}))["dirty"], false, "so it must not dirty the patch");
    assert_eq!(g.call("undo", j!({}))["changed"], false, "and records no history entry");

    // The batch decides dirtiness ONCE, from settled state: a batch that is fully taken back
    // leaves the patch exactly as clean as it found it, though a step landed on the way.
    g.refuse("compound", j!({ "ops": [
        { "op": "node edit", "payload": { "node": uid, "pos": [5.0, 5.0] } },
        { "op": "node edit", "payload": { "node": "ffffffffffff", "name": "ghost" } },
    ] }));
    assert_eq!(g.call("session status", j!({}))["dirty"], false,
               "a rolled-back batch must not leave the unsaved dot raised");
}

/// A canonical 12-hex uid that names nothing.

#[test]
fn a_reply_says_what_the_write_actually_did() {
    let g = Goofi::new();

    let born = g.call("node add", j!({ "type": "LFO" }));
    let osc = born["uid"].as_str().unwrap().to_string();
    assert!(born["name"].as_str().is_some_and(|n| !n.is_empty()), "{born}");
    assert_eq!(born["output_slots"]["out"], "ARRAY", "{born}");
    assert_eq!(born["params"]["lfo"]["frequency"], 1.0, "{born}");

    // A literal is COERCED to the param's declared type, so the value stored may differ.
    let buf = g.add("Buffer");
    let coerced = g.set_param(buf, "buffer", "axis", 1.6);
    assert_eq!(coerced["value"], 2, "an int param rounds: {coerced}");

    // Addressed by uid, answered by NAME: what comes back is what the next op takes.
    let wired = g.call("link add", j!({ "from": ep(&osc, "out"), "to": ep(hex(buf), "input") }));
    let name = born["name"].as_str().unwrap();
    assert_eq!((&wired["from"], &wired["dtype"]), (&j!(ep(name, "out")), &j!("ARRAY")), "{wired}");

    assert_eq!(g.call("node remove", j!({ "node": GHOST }))["removed"], false);
    assert_eq!(g.call("node remove", j!({ "node": osc }))["removed"], true);
    assert_eq!(g.call("link remove", j!({ "from": ep(&osc, "out"), "to": ep(hex(buf), "input") }))["removed"],
               false);

    // A NAME is the front door, and a batch is where that pays: the second line addresses what
    // the first built, with no uid to carry between them.
    let made = g.call("compound", j!({ "ops": [
        { "op": "node add", "payload": { "type": "LFO", "name": "src" } },
        { "op": "node add", "payload": { "type": "Buffer", "name": "win" } },
        { "op": "link add", "payload": { "from": "src/out", "to": "win/input" } },
    ] }));
    assert_eq!(made[0]["name"], "src", "the name asked for is the name minted: {made}");
    assert_eq!(made[2]["from"], "src/out", "{made}");
    assert_eq!(made[2]["to"], "win/input", "{made}");

    // Every op takes it, and every read gives it back — the uid is a second door, never the first.
    assert_eq!(g.call("node param edit",
                      j!({ "node": "win", "param": "buffer/size", "value": 32 }))["value"], 32.0);
    let uid = made[1]["uid"].as_str().unwrap().to_string();
    assert_eq!(g.call("node param edit",
                      j!({ "node": &uid, "param": "buffer/size", "value": 64 }))["value"], 64.0);
    let drawn = g.call("nodes inspect", j!({}))["text"].as_str().unwrap().to_string();
    assert!(drawn.contains("src -- out→input --> win"), "the diagram wires names: {drawn}");
    assert!(!drawn.contains(&uid), "…and shows no uid at all: {drawn}");

    // A fault is reported against the name too, so the read that finds it hands back the handle
    // that fixes it.
    g.call("node param edit", j!({ "node": "src", "param": "common/max_frequency",
                                   "expression": "@@@ not an expression @@@" }));
    let errs = g.call("session status", j!({}))["errors"].as_array().cloned().unwrap_or_default();
    assert!(errs.iter().any(|e| e["node"] == "src"), "the fault names the node: {errs:?}");

    // And a rename moves the handle with it: the old name is nobody's, the new one is the node's.
    g.call("node edit", j!({ "node": "win", "name": "kept" }));
    assert!(g.refuse("node state", j!({ "node": "win" })).contains("win"));
    assert_eq!(g.call("node state", j!({ "node": "kept" }))["text"].as_str()
                   .map(|t| t.starts_with("kept:")), Some(true));
}

#[test]
fn a_refusal_names_what_the_caller_could_try_instead() {
    let g = Goofi::new();
    let osc = g.add("LFO");

    // A variable's FORM is what every expression reading it depends on, so under a config lock it
    // is immutable: a string offered to a config-locked array is refused by naming both.
    let why = g.refuse("variable entry edit", j!({ "name": "system.default_ufreq", "value": "fast" }));
    assert!(why.contains("array") && why.contains("fast"), "{why}");
    let why = g.refuse("variable entry add", j!({ "name": "system.default_ufreq", "value": 9.0 }));
    assert!(why.contains("already exists") && why.contains("variable entry edit"), "{why}");
    assert_eq!(g.call("variable entry edit", j!({ "name": "system.default_ufreq", "value": 12.5 }))["value"], 12.5);

    // An EPHEMERAL variable is goofi's own: the value refuses the edit the way the name refuses the
    // remove, and what it holds is this machine's .goofi folder, not anything a patch said.
    let home = g.call("variable list", j!({}))["variables"].as_array().unwrap().iter()
        .find(|e| e["name"] == "system.goofi_home").cloned().expect("goofi_home is seeded");
    assert_eq!(home["lock"]["value"], j!(true), "{home}");
    assert_eq!(home["value"], j!(goofi_core::path::to_slash(&goofi_supervisor::layout::home())));
    let why = g.refuse("variable entry edit", j!({ "name": "system.goofi_home", "value": "/tmp/elsewhere" }));
    assert!(why.contains("read-only"), "{why}");
    let why = g.refuse("variable entry remove", j!({ "name": "system.goofi_home" }));
    assert!(why.contains("system"), "{why}");

    let why = g.refuse("agent start", j!({ "name": "claude-code" }));
    assert!(why.contains("claude") && why.contains("codex"), "{why}");

    let why = g.refuse("link add", j!({ "from": ep(hex(osc), "out"), "to": ep(GHOST, "input") }));
    assert!(why.contains("`to`") && why.contains(GHOST), "{why}");

    for (op, payload) in [
        ("nodes ungroup", j!({ "subpatch": GHOST })),
        ("node edit", j!({ "node": GHOST, "pos": [1.0, 2.0] })),
        ("node edit", j!({ "node": GHOST, "name": "renamed" })),
        ("node param edit", j!({ "node": GHOST, "param": "buffer/size", "expression": "1" })),
    ] {
        g.refuse(op, payload);
    }

    // An expression reads a name as an ATTRIBUTE — `variables.gain`, `nd('sub').out.slot` — so a name
    // Python cannot parse as one is refused, whatever makes it unparseable. The refusal says the
    // rule rather than the one character it caught.
    for bad in ["a'b", "a\\b", "a\"b", "a b-2", "a_b", "nd()", "1st", "class", ""] {
        let why = g.refuse("node edit", j!({ "node": hex(osc), "name": bad }));
        assert!(why.contains("letters or digits"), "the refusal states the rule: {why}");
        // The same rule at birth — refused, never silently swapped for a minted name.
        if !bad.is_empty() {
            let why = g.refuse("node add", j!({ "type": "LFO", "name": bad }));
            assert!(why.contains("letters or digits"), "{why}");
        }
    }
    g.call("node edit", j!({ "node": hex(osc), "name": "ab2" }));
    // The command tolerates a collision so replay converges; the RPC boundary raises the user error.
    g.add("Buffer");
    g.refuse("node edit", j!({ "node": hex(osc), "name": "buffer0" }));
    // Nothing at all is a caller error: an op that means "edit" must be told what to edit.
    g.refuse("node edit", j!({ "node": hex(osc) }));
}

#[test]
fn an_expression_binds_carries_its_error_and_follows_the_rename_of_what_it_names() {
    let g = Goofi::new();
    let producer = g.add("LFO");
    // Block mode at a stated rate, so a frame carries several samples: the shape error below
    // needs one. There is no evaluator here, so the rate cap's binding cannot stand in for it.
    g.set_param(producer, "output", "mode", "block");
    g.set_param(producer, "common", "max_frequency", 20.0);
    let consumer = g.add("LFO");
    g.call("node edit", j!({ "node": hex(producer), "name": "src" }));

    // A binding that cannot compile is STORED, so the refusal has to travel in the reply.
    // An expression given with no `mode` binds: that is what writing one means.
    let set = |expr: &str| g.call("node param edit", j!({ "node": hex(consumer),
                                                          "param": "common/max_frequency",
                                                          "expression": expr }));
    assert!(set("@@ not an expression @@")["error"].as_str().is_some_and(|e| !e.is_empty()),
            "the compile error must ride the reply");
    assert!(set("")["error"].is_null(), "an empty expression clears the binding");

    let mut ev = g.events();
    set("nd('src') * 1");
    // The record rides `state_update`; `error` is runtime-derived and rides the LIVE plane, never
    // the doc and never an op's echo — an echo is taken before the node has re-evaluated the source
    // the op just moved, so it would carry the error of the source that is gone.
    let d = g.until("the descriptor echo", |_| {
        let p = ev.next("state_update");
        (p["node"] == hex(consumer)).then(|| p["params"]["common"]["max_frequency"].clone())
    });
    assert_eq!((&d["expression"], &d["mode"], &d["triggers"]),
               (&j!("nd('src') * 1"), &j!("expression"), &j!(false)));
    assert!(d["error"].is_string(), "got {:?}", d["error"]);
    assert!(d.get("expression_autoeval").is_none(), "auto-eval is always on, so it is not on the wire");

    g.call("node edit", j!({ "node": hex(producer), "name": "signal" }));
    let expr = g.until("the referrer's echo", |_| {
        let p = ev.next("state_update");
        (p["node"] == hex(consumer))
            .then(|| p["params"]["common"]["max_frequency"]["expression"].clone())
    });
    assert_eq!(expr, "nd('signal') * 1", "the referrer's nd() term followed the rename");
    // A term in a comment or a string is text: one scan decides what is text for every reader.
    set("nd('signal') + len(\"nd('signal') me\")  # me nd('signal') variables.a.b");
    g.call("node edit", j!({ "node": hex(producer), "name": "src" }));
    let expr = g.until("the referrer's echo", |_| {
        let p = ev.next("state_update");
        let d = &p["params"]["common"]["max_frequency"];
        (p["node"] == hex(consumer) && d["expression"].as_str().is_some_and(|e| e.starts_with("nd('src')")))
            .then(|| d["expression"].clone())
    });
    assert_eq!(expr, "nd('src') + len(\"nd('signal') me\")  # me nd('signal') variables.a.b");
    g.call("node edit", j!({ "node": hex(producer), "name": "signal" }));
    set("nd('signal')");

    // A BARE expression — one target, an optional index — never enters Python: the one frame is
    // copied on arrival, in Rust. The text is RETAINED across a mode switch, which is never destructive.
    let level = g.add("_TestScalar");
    g.call("node edit", j!({ "node": hex(level), "name": "level" }));
    g.call("node param edit", j!({ "node": hex(level), "param": "control/value", "value": 0.25 }));
    let param = |g: &Goofi, spec: serde_json::Value| {
        let mut payload = j!({ "node": hex(consumer), "param": "common/max_frequency" });
        payload.as_object_mut().unwrap().extend(spec.as_object().unwrap().clone());
        g.call("node param edit", payload)
    };
    // An op's echo is a `state_update`; a RUNTIME error is read back through `node state`.
    let line = |g: &Goofi| {
        let text = g.call("node state", j!({ "node": hex(consumer) }))["text"].as_str().unwrap().to_string();
        text.lines().find(|l| l.contains("common.max_frequency")).unwrap_or("").to_string()
    };
    let field = |ev: &mut goofi_tests::Events, what: &str, want: &dyn Fn(&serde_json::Value) -> bool| {
        g.until(what, |_| {
            let p = ev.next("state_update");
            let d = &p["params"]["common"]["max_frequency"];
            (p["node"] == hex(consumer) && want(d)).then(|| d.clone())
        })
    };
    for bad in ["nd('nosuch')", "nd('level').out.nope", "nd('level').nope", "nd('a b')"] {
        let why = param(&g, j!({ "expression": bad }))["error"].clone();
        assert!(why.as_str().is_some_and(|e| !e.is_empty()), "`{bad}` is refused or unresolvable: {why}");
    }
    let bound = param(&g, j!({ "expression": "nd('level')" }));
    assert!(bound["error"].is_null(), "a bare read of a running scalar producer binds: {bound}");
    let d = field(&mut ev, "the descriptor shows the expression", &|d| d["expression"] == j!("nd('level')"));
    assert_eq!((&d["mode"], &d["error"]), (&j!("expression"), &j!(null)), "{d}");
    // The value lands with NO evaluator in this process: the runtime copied the frame's one element.
    g.until("the bare value lands", |_| {
        (ev.next("param_values")["nodes"][hex(consumer)]["values"]["common"]["max_frequency"] == j!(0.25)).then_some(())
    });
    // …on the live plane alone: a streamed value never enters the document.
    assert_ne!(g.doc()["nodes"][hex(consumer)]["params"]["common"]["max_frequency"]["value"], j!(0.25));
    // A rename follows into the text.
    g.call("node edit", j!({ "node": hex(level), "name": "gain" }));
    field(&mut ev, "the expression followed the rename", &|d| d["expression"] == j!("nd('gain')"));
    let wide = g.add("_TestConst");
    g.call("node edit", j!({ "node": hex(wide), "name": "wide" }));
    g.set_param(wide, "constant", "value", 0.75);
    g.set_param(wide, "constant", "length", 8);
    param(&g, j!({ "expression": "nd('wide')[7]" }));
    g.until("an indexed bare read takes one element of a wide frame without Python", |_| {
        (ev.next("param_values")["nodes"][hex(consumer)]["values"]["common"]["max_frequency"] == j!(0.75)).then_some(())
    });
    g.call("node edit", j!({ "node": hex(wide), "name": "bank" }));
    field(&mut ev, "the indexed read follows a rename", &|d| d["expression"] == j!("nd('bank')[7]"));
    param(&g, j!({ "expression": "nd('bank')[8]" }));
    g.until("an out-of-range index reports an error", |g| line(g).contains("outside frame").then_some(()));
    // Anything else around the target is Python's, and there is no interpreter in this process.
    for computed in ["nd('bank')[-1]", "nd('bank')[1.5]", "nd('bank')[1][0]", "nd('bank') + 1"] {
        let why = param(&g, j!({ "expression": computed }))["error"].clone();
        assert!(why.as_str().is_some_and(|e| e.contains("no expression evaluator")), "`{computed}`: {why}");
    }
    // A frame with more than one element reads whole: its first element, no error.
    param(&g, j!({ "expression": "nd('signal')" }));
    g.until("the wide frame to read", |g| {
        let l = line(g);
        (l.contains("expr: nd('signal')") && !l.contains("[error")).then_some(())
    });
    // A literal on a driven param switches it to constant; the text stays retained, and a mode
    // alone brings it back. A value edit echoes no descriptor: the document carries the mode.
    param(&g, j!({ "value": 7 }));
    let d = g.doc()["nodes"][hex(consumer)]["params"]["common"]["max_frequency"].clone();
    assert_eq!((&d["value"], &d["mode"], &d["expression"]), (&j!(7), &j!("constant"), &j!("nd('signal')")), "{d}");
    // The echoes of the edits above are still queued, so the subscription is taken afresh.
    ev = g.events();
    param(&g, j!({ "mode": "expression" }));
    let d = field(&mut ev, "the retained expression is live", &|d| d["mode"] == j!("expression"));
    assert_eq!(d["expression"], j!("nd('signal')"), "{d}");
    g.until("the expression reads again", |g| {
        let l = line(g);
        (l.contains("expr: nd('signal')") && !l.contains("[error")).then_some(())
    });
    param(&g, j!({ "expression": "" }));
    let d = field(&mut ev, "an empty expression clears it", &|d| d["mode"] == j!("constant"));
    assert_eq!(d["expression"], j!(null), "{d}");
    param(&g, j!({ "expression": "nd('gain')" }));
    // The runtime clears the shape error on its own clock, so the echo is not the oracle.
    g.until("the expression is live again", |g| {
        let l = line(g);
        (l.contains("expr: nd('gain')") && !l.contains("[error")).then_some(())
    });
    /* …and a client is TOLD, on the same plane that carries the value. Nothing was written for this
       clear — no op, no doc delta — so with the error riding op echoes alone a replica kept showing
       the previous source's failure under an expression that was working. */
    let mut live = g.events();
    let errors_of = |p: &serde_json::Value| p["errors"]["common"].get("max_frequency").cloned();
    param(&g, j!({ "expression": "nd('signal')[999]" }));
    g.until("the index error reaches the live plane", |_| {
        let p = live.next("param_values")["nodes"][hex(consumer)].take();
        errors_of(&p).is_some_and(|e| e.as_str().is_some_and(|m| m.contains("outside frame"))).then_some(())
    });
    param(&g, j!({ "expression": "nd('gain')" }));
    g.until("…and the clear does too", |_| {
        let p = live.next("param_values")["nodes"][hex(consumer)].take();
        (!p.is_null() && errors_of(&p).is_none()).then_some(())
    });
    // A deleted producer leaves the expression standing with its error; undo clears it.
    g.call("node remove", j!({ "node": hex(level) }));
    let l = g.until("the producer is gone", |g| Some(line(g)).filter(|l| l.contains("no node named `gain`")));
    assert!(l.contains("expr: nd('gain')"), "the expression stands while its producer is gone: {l}");
    let mut fresh = g.events();
    g.call("undo", j!({}));
    g.until("undo brought the producer back", |g| {
        let l = line(g);
        (l.contains("expr: nd('gain')") && !l.contains("[error")).then_some(())
    });
    let value_of = |p: &serde_json::Value| p["values"]["common"].get("max_frequency").cloned();
    g.until("…and its value lands again", |_| {
        (value_of(&fresh.next("param_values")["nodes"][hex(consumer)]) == Some(j!(0.25))).then_some(())
    });
    // Re-pointing a bare read at a SILENT producer must not hand it the old producer's last frame:
    // the mailbox starts empty, the literal stands, and the preview is withdrawn.
    let quiet = g.add("_TestEcho");
    g.call("node edit", j!({ "node": hex(quiet), "name": "quiet" }));
    param(&g, j!({ "expression": "nd('quiet')" }));
    g.until("the value is withdrawn", |_| {
        let p = fresh.next("param_values")["nodes"][hex(consumer)].take();
        (!p.is_null() && value_of(&p).is_none()).then_some(())
    });
}

#[test]
fn a_node_can_be_born_configured_at_a_chosen_uid_and_name() {
    // Params are applied under the graph lock, before `node_added`, so the node is born configured.
    let g = Goofi::new();
    let mut ev = g.events();
    let born = g.call("node add", j!({ "type": "LFO",
                                       "param": [{ "name": "common/max_frequency", "value": 42.0 }] }));
    let uid = born["uid"].as_str().unwrap().to_string();
    assert_eq!(ev.next("node_added")["uid"], uid);
    assert_eq!(g.doc()["nodes"][&uid]["params"]["common"]["max_frequency"]["value"], 42.0);

    // Undo/redo do NOT come through here — they restore via the command history.
    g.call("node remove", j!({ "node": uid.clone() }));
    let again = g.call("node add", j!({ "type": "LFO", "member_uid": uid.clone(),
                                        "name": "restoredOsc" }));
    assert_eq!((&again["uid"], &g.doc()["nodes"][&uid]["name"]), (&j!(uid), &j!("restoredOsc")));
}

#[test]
fn a_viewer_bag_persists_and_refuses_a_word_outside_its_vocabulary() {
    // `viewers(uid)` answers `Some({})` for every node, so an unconditional insert would stamp them all.
    let g = Goofi::new();
    let osc = g.add("LFO");
    assert!(g.doc()["nodes"][hex(osc)].get("viewers").is_none(), "no viewers leaf when empty");

    let why = g.refuse("node edit", j!({ "node": hex(osc),
                                         "viewer": [{ "slot": "out", "kind": "waveform" }] }));
    assert!(why.contains("waveform") && why.contains("line") && why.contains("brain"), "{why}");
    let why = g.refuse("node edit", j!({ "node": hex(osc),
                                         "viewer": [{ "slot": "psd", "kind": "line" }] }));
    assert!(why.contains("psd") && why.contains("out"), "an unknown slot names the real ones: {why}");
    let why = g.refuse("node edit", j!({ "node": GHOST,
                                         "viewer": [{ "slot": "out", "kind": "line" }] }));
    assert!(why.contains("no such node"), "{why}");
    let why = g.refuse("node edit", j!({ "node": hex(osc), "viewer": 7 }));
    assert!(why.contains("sequence"), "an arg that is not a list says what one looks like: {why}");
    let why = g.refuse("node edit", j!({ "node": hex(osc), "viewer": [{ "kind": "line" }] }));
    assert!(why.contains("slot"), "an entry without a slot is refused by naming it: {why}");

    g.call("node edit", j!({ "node": hex(osc),
                             "viewer": [{ "slot": "out", "collapsed": false, "kind": "line",
                                          "settings": { "yScale": 2 } }] }));
    assert!(!g.doc()["nodes"][hex(osc)]["viewers"].is_null(), "…and the leaf appears once set");
    // Entries MERGE, slot by slot: naming one setting leaves the kind and the others where they were.
    g.call("node edit", j!({ "node": hex(osc),
                             "viewer": [{ "slot": "out", "settings": { "xScale": 3 } }] }));
    let view = |g: &Goofi| g.doc()["nodes"][hex(osc)]["viewers"].to_string();
    let merged = view(&g);
    for kept in ["\"kind\":\"line\"", "\"yScale\":2", "\"xScale\":3"] {
        assert!(merged.contains(kept), "the patch merged rather than replaced: {merged}");
    }
    // …and it is UNDOABLE, which is what makes it an op rather than a side write.
    g.call("undo", j!({}));
    assert!(!view(&g).contains("xScale"), "the undo took the merge back off: {}", view(&g));
    g.call("redo", j!({}));
    assert!(view(&g).contains("xScale"), "…and the redo put it back: {}", view(&g));
    let yaml = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    assert!(yaml.contains("yScale"), "the view state persists: {yaml}");

    // `clear` removes the slot's stored view — the entry form's spelling of the old null.
    let why = g.refuse("node edit", j!({ "node": hex(osc),
                                         "viewer": [{ "slot": "out", "clear": false }] }));
    assert!(why.contains("clear"), "a false `clear` is refused rather than read as a set: {why}");
    g.call("node edit", j!({ "node": hex(osc), "viewer": [{ "slot": "out", "clear": true }] }));
    assert!(!view(&g).contains("yScale"), "the clear dropped the stored view: {}", view(&g));

    g.call("variable entry add", j!({ "name": "patch.subject", "value": "P01" }));
    // The doc carries a lock only where one is held: the system group's, the machine value's.
    assert_eq!(g.doc()["variable_groups"]["system"]["lock"]["config"], true);
    assert_eq!(g.doc()["variables"]["system.goofi_home"]["lock"]["value"], true);
    assert!(g.doc()["variables"]["patch.subject"].get("lock").is_none());
}

#[test]
fn eight_writers_all_land_and_none_deadlock() {
    // Both the param path and the position path, because they are separate mirror writers.
    const N: usize = 8;
    const ROUNDS: usize = 5;
    let g = Goofi::new();
    let uids: Vec<_> = (0..N).map(|_| g.add("LFO")).collect();
    std::thread::scope(|s| {
        for (i, u) in uids.iter().enumerate() {
            let client = g.client(&format!("s{i}"));
            s.spawn(move || {
                for r in 1..=ROUNDS {
                    client.set_param(*u, "common", "max_frequency", r as f64);
                    client.call("node edit", j!({ "node": hex(*u), "pos": [r as f64, r as f64] }));
                }
            });
        }
    });
    let doc = g.doc();
    for u in &uids {
        let n = &doc["nodes"][hex(*u)];
        assert_eq!(n["params"]["common"]["max_frequency"]["value"].as_f64(), Some(ROUNDS as f64),
                   "a param write was lost on {u}");
        assert_eq!(n["pos"][0].as_f64(), Some(ROUNDS as f64), "a drag was lost on {u}");
    }
}

/// The touched filter's zero point, which the inspector's Clear button moves.
///
/// A plugin's params read as touched when they differ from the plugin's FACTORY default, so
/// loading a preset moves hundreds at once and the filter that exists to show the few in play
/// fills with everything the preset moved. Clearing records what the node holds NOW as the new
/// zero. It must edit no param and break no binding — an expression keeps driving, and the
/// Expression filter still finds it — and it must be undoable like any other document edit.
#[test]
fn clearing_the_touched_baseline_moves_the_zero_point_and_breaks_no_binding() {
    let g = Goofi::new();
    let osc = g.add("LFO");

    // Nothing cleared yet: no baseline key at all, so the zero is each type's declared default.
    let baseline = |g: &Goofi| -> Value { g.doc()["nodes"][hex(osc)]["baseline"].clone() };
    assert!(baseline(&g).is_null(), "a node nobody has cleared carries no baseline: {}", baseline(&g));

    // Move one param off its default and bind another — the two kinds of change the filter counts.
    g.call("node param edit", j!({ "node": hex(osc), "param": "lfo/frequency", "value": "3.5" }));
    g.call("node param edit", j!({ "node": hex(osc), "param": "lfo/amplitude", "expression": "1 + 1" }));

    let cleared = g.call("node baseline", j!({ "node": hex(osc) }));
    assert_eq!(cleared["ok"], j!(true));
    let n = cleared["cleared"].as_u64().expect("a count of the params the zero point covers");
    assert!(n > 0, "the zero point covers the node's params: {n}");

    // The zero point records the VALUE and the SOURCE: a param moving from a constant to an
    // expression is a change even when the number it evaluates to is the same.
    let base = baseline(&g);
    assert_eq!(base["lfo/frequency"]["value"], j!(3.5), "the moved value is the new zero: {base}");
    assert_eq!(base["lfo/amplitude"]["mode"], j!("expression"), "the source is recorded too: {base}");
    assert_eq!(base["lfo/amplitude"]["expression"], j!("1 + 1"), "…and the expression text with it");

    // Nothing was edited: the expression still drives, so the Expression filter still finds it.
    let params = g.doc()["nodes"][hex(osc)]["params"].clone();
    assert_eq!(params["lfo"]["amplitude"]["expression"], j!("1 + 1"), "the binding survived the clear");
    assert_eq!(params["lfo"]["frequency"]["value"], j!(3.5), "and the value is untouched");

    // Undoable like any other document edit.
    assert_eq!(g.call("undo", j!({}))["changed"], true);
    assert!(baseline(&g).is_null(), "undo took the zero point back: {}", baseline(&g));
    assert_eq!(g.call("redo", j!({}))["changed"], true);
    assert_eq!(baseline(&g)["lfo/frequency"]["value"], j!(3.5), "redo put it back");
}

#[test]
fn a_deleted_node_hands_its_wires_through() {
    // A chain stays a chain: what fed the deleted node feeds whatever it fed, by the first wired
    // input whose kind fits, and one undo brings the node and its own wires back.
    let g = Goofi::new();
    let (src, mid, sink) = (g.add("signal:Constant"), g.add("Smooth"), g.add("Smooth"));
    g.link(src, "out", mid, "input");
    g.link(mid, "out", sink, "input");
    let wires = |g: &Goofi| -> Vec<(String, String)> {
        let links = g.doc()["links"].as_object().map(|l| l.values().cloned().collect::<Vec<_>>()).unwrap_or_default();
        links.iter().map(|l| (l["node_out"].as_str().unwrap().into(), l["node_in"].as_str().unwrap().into())).collect()
    };
    g.call("node remove", j!({ "node": hex(mid) }));
    assert_eq!(wires(&g), vec![(hex(src), hex(sink))], "the source now feeds the sink directly");
    g.call("undo", j!({}));
    assert_eq!(wires(&g), vec![(hex(src), hex(mid)), (hex(mid), hex(sink))], "one undo restores the chain");
    g.call("redo", j!({}));
    assert_eq!(wires(&g), vec![(hex(src), hex(sink))], "and redo bridges again");
    g.call("undo", j!({}));

    // The first WIRED input is the one handed through, and only where its kind fits: a pair
    // fed on `b` alone bridges from there, and a FromJson between a String and a Table
    // consumer bridges nothing.
    g.call("node remove", j!({ "node": hex(src) }));
    assert_eq!(wires(&g), vec![(hex(mid), hex(sink))], "a node with no wired input leaves its consumer unfed");
    let pair = g.add("_TestPair");
    g.link(mid, "out", pair, "b");
    g.link(pair, "out", sink, "input");
    assert_eq!(wires(&g), vec![(hex(mid), hex(pair)), (hex(pair), hex(sink))]);
    g.call("node remove", j!({ "node": hex(pair) }));
    assert_eq!(wires(&g), vec![(hex(mid), hex(sink))], "the first wired input, `b`, is handed through");
    let (text, json, table) = (g.add("Text"), g.add("FromJson"), g.add("TableSelect"));
    g.link(text, "out", json, "input");
    g.link(json, "out", table, "input");
    g.call("node remove", j!({ "node": hex(json) }));
    assert_eq!(wires(&g), vec![(hex(mid), hex(sink))], "a String cannot feed a Table consumer, so nothing bridges");
}
