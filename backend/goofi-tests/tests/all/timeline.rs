//! The timeline: a curve track and a note track written through ops, read through their groups'
//! variables at the clock's position, renamed, saved, reopened and undone.

use std::sync::Arc;

use goofi_python::inproc::PyExprEvaluator;
use goofi_tests::{hex, j, Goofi};
use serde_json::Value;

fn number(g: &Goofi, name: &str) -> f64 {
    g.variable(name).as_f64().unwrap_or(f64::NAN)
}

/// The velocity a note track holds for `pitch` now.
fn held(g: &Goofi, name: &str, pitch: usize) -> f64 {
    g.variable(name)[pitch].as_f64().unwrap_or(f64::NAN)
}

/// Put the paused clock at `time`.
fn at(g: &Goofi, time: f64) {
    g.call("clock set", j!({ "playing": false, "time": time }));
}

#[test]
fn a_track_writes_its_group_at_the_clocks_position() {
    let g = Goofi::new();
    g.graph().set_evaluator(Arc::new(PyExprEvaluator::new().unwrap()));
    at(&g, 0.0);
    // A track's times are beats: at a tempo of 60 a beat is a second.
    g.call("variable entry edit", j!({ "name": "clock.bpm", "value": 60.0 }));

    // A curve track: one value, eased between its keyframes and held outside them.
    let sweep = g.call("timeline track add", j!({ "kind": "curve", "name": "sweep" }));
    assert_eq!(sweep["name"], j!("sweep"));
    let ramp = j!([{ "at": 3.0, "value": 10.0 }, { "at": 1.0, "value": 0.0 }]);
    g.call("timeline track edit", j!({ "track": "sweep", "range": [0.0, 10.0], "keys": ramp }));
    let listed = g.call("variable list", j!({}));
    assert_eq!(listed["groups"]["sweep"]["owner"]["track"], sweep["id"], "the track's group is the track's: {}", listed["groups"]);
    let held_as = g.doc()["timeline"][sweep["id"].as_str().unwrap()].clone();
    assert_eq!((&held_as["keys"][0]["at"], &held_as["range"]), (&j!(1.0), &j!([0.0, 10.0])), "the keyframes are held in time order: {held_as}");
    for (time, value) in [(0.0, 0.0), (1.0, 0.0), (2.0, 5.0), (3.0, 10.0), (9.0, 10.0)] {
        at(&g, time);
        g.until(&format!("the level at {time}"), |g| ((number(g, "sweep.value") - value).abs() < 1e-3).then_some(()));
    }
    // The curve from a keyframe to the next is that keyframe's: a step holds until the next one.
    let stepped = j!([{ "at": 1.0, "value": 0.0, "curve": "step" }, { "at": 3.0, "value": 10.0 }]);
    g.call("timeline track edit", j!({ "track": "sweep", "keys": stepped }));
    at(&g, 2.0);
    g.until("the stepped level before its next keyframe", |g| (number(g, "sweep.value") == 0.0).then_some(()));
    // A time follows the tempo: beat 2 is one second at 120 and two at 60.
    g.call("timeline track edit", j!({ "track": "sweep", "keys": ramp }));
    at(&g, 1.0);
    g.call("variable entry edit", j!({ "name": "clock.bpm", "value": 120.0 }));
    g.until("the level at beat 2 of a tempo of 120", |g| ((number(g, "sweep.value") - 5.0).abs() < 1e-3).then_some(()));
    g.call("variable entry edit", j!({ "name": "clock.bpm", "value": 60.0 }));
    g.until("the level at beat 1 of a tempo of 60", |g| (number(g, "sweep.value") == 0.0).then_some(()));
    // At a tempo of nothing no beat comes: the value before the first keyframe holds.
    g.call("variable entry edit", j!({ "name": "clock.bpm", "value": 0.0 }));
    at(&g, 2.0);
    g.until("the level at a tempo of nothing", |g| (number(g, "sweep.value") == 0.0).then_some(()));
    at(&g, 1.0);
    g.call("variable entry edit", j!({ "name": "clock.bpm", "value": 60.0 }));

    // A note track: the velocity of each note the position is in, as a device's `notes` has it.
    g.call("timeline track add", j!({ "kind": "note", "name": "keys" }));
    g.call("timeline track edit", j!({ "track": "keys", "notes": [{ "at": 1.0, "length": 1.0, "pitch": 60, "velocity": 0.5 }] }));
    for (time, velocity) in [(0.5, 0.0), (1.5, 0.5), (2.5, 0.0)] {
        at(&g, time);
        g.until(&format!("the note at {time}"), |g| (held(g, "keys.notes", 60) == velocity).then_some(()));
    }
    // Its keyboard shows two octaves from C3 until it is told another range.
    let keyboard = |g: &Goofi| g.call("timeline list", j!({}))["tracks"].as_object().unwrap().values().find(|t| t["name"] == "keys").unwrap()["range"].clone();
    assert_eq!(keyboard(&g), j!([48, 72]));
    g.call("timeline track edit", j!({ "track": "keys", "range": [36, 84], "scale": "minor", "root": "D" }));
    assert_eq!(keyboard(&g), j!([36, 84]));
    let doc = g.doc();
    let held_as = doc["timeline"].as_object().unwrap().values().find(|t| t["name"] == "keys").unwrap();
    assert_eq!((&held_as["scale"], &held_as["root"], held_as["notes"].as_array().map(Vec::len)), (&j!("minor"), &j!("D"), Some(1)), "the keyboard's scale, and the notes as they were");
    // Played, the thread wakes for the note's start and for its end.
    g.call("clock set", j!({ "playing": true, "time": 0.9 }));
    g.until("the note to start", |g| (held(g, "keys.notes", 60) == 0.5).then_some(()));
    g.until("the note to end", |g| (held(g, "keys.notes", 60) == 0.0).then_some(()));
    at(&g, 1.0);

    // What does not hold together is refused whole.
    for (payload, why) in [
        (j!({ "track": "keys", "notes": [{ "at": 0.0, "length": 0.0, "pitch": 60, "velocity": 0.5 }] }), "lasts more than 0"),
        (j!({ "track": "keys", "keys": [] }), "a note track takes `notes`"),
        (j!({ "track": "keys", "range": [60, 60] }), "a lower pitch to a higher one"),
        (j!({ "track": "keys", "range": [60.5, 72] }), "two pitches"),
        (j!({ "track": "keys", "scale": "wrong" }), "a scale and a root"),
        (j!({ "track": "sweep", "root": "D" }), "a note track takes"),
        (j!({ "track": "sweep", "keys": [{ "at": 1.0, "value": 0.0 }, { "at": 1.0, "value": 1.0 }] }), "each later"),
        (j!({ "track": "sweep", "to": "keys" }), "a variable group already"),
    ] {
        let refusal = g.refuse("timeline track edit", payload);
        assert!(refusal.contains(why), "{refusal}");
    }
    assert!(g.refuse("timeline track add", j!({ "kind": "audio" })).contains("`note` or `curve`"));

    // A rename moves the group with what it holds, and every expression that reads it follows.
    let reader = g.add("_TestScalar");
    g.call("node param edit", j!({ "node": hex(reader), "param": "control/value", "expression": "vars.sweep.value" }));
    at(&g, 2.0);
    g.call("timeline track edit", j!({ "track": "sweep", "to": "rise" }));
    assert_eq!(g.doc()["nodes"][hex(reader)]["params"]["control"]["value"]["expression"], j!("vars.rise.value"));
    g.until("the renamed track's variable", |g| ((number(g, "rise.value") - 5.0).abs() < 1e-3).then_some(()));

    // Saved and opened elsewhere, the tracks come back and play; their variables are theirs to derive.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeline.gfi");
    g.call("session save", j!({ "path": path.to_string_lossy() }));
    let manifest = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    assert!(manifest.contains("timeline:") && !manifest.contains("keys.notes"), "{manifest}");
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let files: Vec<String> = (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().to_string()).collect();
    assert!(!files.iter().any(|f| f.contains("keys.notes")), "the file carries no value its owner derives: {files:?}");
    let other = Goofi::new();
    other.graph().set_evaluator(Arc::new(PyExprEvaluator::new().unwrap()));
    other.call("session load", j!({ "path": path.to_string_lossy() }));
    assert_eq!(other.doc()["timeline"], g.doc()["timeline"]);
    at(&other, 1.5);
    other.until("the loaded note", |g| (held(g, "keys.notes", 60) == 0.5).then_some(()));

    // A removed track takes its group; undo walks every edit back.
    g.call("timeline track remove", j!({ "track": "keys" }));
    assert_eq!(g.variable("keys.notes"), Value::Null);
    while g.call("undo", j!({}))["changed"] == true {}
    assert_eq!(g.doc()["timeline"], j!({}));
    assert!(g.call("variable list", j!({}))["groups"].get("rise").is_none());
}
