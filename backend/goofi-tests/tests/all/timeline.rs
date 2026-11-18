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
    g.call("timeline track edit", j!({ "track": "keys", "range": [36, 84], "scale": "minor", "root": "D", "grid": "1 s" }));
    assert_eq!(keyboard(&g), j!([36, 84]));
    let doc = g.doc();
    let held_as = doc["timeline"].as_object().unwrap().values().find(|t| t["name"] == "keys").unwrap();
    assert_eq!((&held_as["scale"], &held_as["root"], &held_as["grid"], held_as["notes"].as_array().map(Vec::len)), (&j!("minor"), &j!("D"), &j!("1 s"), Some(1)), "the keyboard's scale and grid, and the notes as they were");
    // Played, the thread wakes for the note's start and for its end.
    g.call("clock set", j!({ "playing": true, "time": 0.9 }));
    g.until("the note to start", |g| (held(g, "keys.notes", 60) == 0.5).then_some(()));
    g.until("the note to end", |g| (held(g, "keys.notes", 60) == 0.0).then_some(()));
    at(&g, 1.0);

    // A track is put before another, or last: the order every panel draws them in.
    let order = |g: &Goofi| {
        let doc = g.doc();
        let mut tracks: Vec<_> = doc["timeline"].as_object().unwrap().values().map(|t| (t["order"].as_u64(), t["name"].as_str().unwrap().to_string())).collect();
        tracks.sort();
        tracks.into_iter().map(|(_, name)| name).collect::<Vec<_>>()
    };
    assert_eq!(order(&g), ["sweep", "keys"]);
    g.call("timeline track move", j!({ "track": "keys", "before": "sweep" }));
    assert_eq!(order(&g), ["keys", "sweep"]);
    g.call("timeline track move", j!({ "track": "keys" }));
    assert_eq!(order(&g), ["sweep", "keys"]);
    g.call("timeline track move", j!({ "track": "sweep", "before": "sweep" }));
    assert_eq!(order(&g), ["sweep", "keys"], "a track put before itself stays");
    assert!(g.refuse("timeline track move", j!({ "track": "keys", "before": "nowhere" })).contains("nowhere"));

    // What does not hold together is refused whole.
    for (payload, why) in [
        (j!({ "track": "keys", "notes": [{ "at": 0.0, "length": 0.0, "pitch": 60, "velocity": 0.5 }] }), "lasts more than 0"),
        (j!({ "track": "keys", "keys": [] }), "a note track takes `notes`"),
        (j!({ "track": "keys", "range": [60, 60] }), "a lower pitch to a higher one"),
        (j!({ "track": "keys", "range": [60.5, 72] }), "two pitches"),
        (j!({ "track": "keys", "scale": "wrong" }), "a scale and a root"),
        (j!({ "track": "keys", "grid": "1/7 beat" }), "its grid is one"),
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

#[test]
fn a_recording_goes_into_its_track() {
    let g = Goofi::new();
    g.graph().set_evaluator(Arc::new(PyExprEvaluator::new().unwrap()));
    at(&g, 0.0);
    g.call("variable entry edit", j!({ "name": "clock.bpm", "value": 60.0 }));
    let track = |g: &Goofi, name: &str| g.call("timeline list", j!({}))["tracks"].as_object().unwrap().values().find(|t| t["name"] == name).unwrap().clone();
    let record = |g: &Goofi, name: &str, on: bool| {
        g.call("timeline track record", j!({ "track": name, "on": on }));
        g.until("the track to say whether it records", |g| (number(g, &format!("{name}.recording")) == f64::from(u8::from(on))).then_some(()));
    };

    // A curve track records its input: an expression, as a param's, and none before one is set.
    g.call("variable entry add", j!({ "name": "desk.level", "value": 0.0 }));
    g.call("timeline track add", j!({ "kind": "curve", "name": "take" }));
    assert!(g.refuse("timeline track record", j!({ "track": "take", "on": true })).contains("no `input`"));
    assert!(g.refuse("timeline track edit", j!({ "track": "take", "input": "me.gain" })).contains("reads no playhead"));
    assert!(g.refuse("timeline track edit", j!({ "track": "take", "rate": 0.0 })).contains("1..120"));
    let before = j!([{ "at": 0.5, "value": 3.0 }, { "at": 5.0, "value": 7.0 }]);
    g.call("timeline track edit", j!({ "track": "take", "input": "vars.desk.level", "rate": 20.0, "keys": before }));
    // The level is 0 from beat 0, 10 from beat 2 and 0 again from beat 4. A recording takes what
    // its input holds when the thread looks, so each level is one take, ended while it holds:
    // the second goes in place of the stretch it covers, which is the end of the first.
    let mut takes = Vec::new();
    for (time, level) in [(2.0, 10.0), (4.0, 0.0)] {
        record(&g, "take", true);
        at(&g, time);
        g.call("variable entry edit", j!({ "name": "desk.level", "value": level }));
        record(&g, "take", false);
        takes.push(track(&g, "take")["keys"].clone());
    }
    let taken = track(&g, "take");
    let times: Vec<f64> = taken["keys"].as_array().unwrap().iter().map(|k| k["at"].as_f64().unwrap()).collect();
    assert_eq!((times.first(), times.last(), times.contains(&0.5)), (Some(&0.0), Some(&5.0), false), "the take is in place of the keyframes of its stretch, and the one past it stays: {taken}");
    assert_eq!((&taken["range"], &taken["rate"]), (&j!([0.0, 10.0]), &j!(20.0)), "the range holds what came");
    // The input held each level until the next: so does the curve.
    for (time, level) in [(1.0, 0.0), (3.0, 10.0), (4.0, 0.0), (5.0, 7.0)] {
        at(&g, time);
        g.until(&format!("the recorded level at {time}"), |g| ((number(g, "take.value") - level).abs() < 0.1).then_some(()));
    }
    // A take is one step back: the second gone, the track holds the first.
    g.call("undo", j!({}));
    assert_eq!(track(&g, "take")["keys"], takes[0]);
    // A rename of what the input reads follows into it, as into any expression.
    g.call("variable entry rename", j!({ "name": "desk.level", "to": "desk.fader" }));
    assert_eq!(track(&g, "take")["input"], j!("vars.desk.fader"));

    // A note track records the keys of a MIDI device, by the device's group, on any channel.
    g.graph().variables().grab_midi("pads", "a test port").unwrap();
    let press = |g: &Goofi, key: usize, velocity: f64| {
        let mut keys = vec![0.0; 2048];
        keys[key] = velocity;
        g.graph().variables().drive("pads.notes", goofi_core::Data::numbers(keys));
    };
    g.call("timeline track add", j!({ "kind": "note", "name": "played" }));
    let there = j!({ "at": 0.0, "length": 1.0, "pitch": 50, "velocity": 0.5 });
    g.call("timeline track edit", j!({ "track": "played", "input": "pads", "notes": [there] }));
    // Pitch 80 on channel 2 is down from beat 1 to beat 3.
    at(&g, 1.0);
    press(&g, 128 + 80, 0.75);
    record(&g, "played", true);
    at(&g, 3.0);
    press(&g, 0, 0.0);
    record(&g, "played", false);
    let played = track(&g, "played");
    assert_eq!(played["notes"], j!([there, { "at": 1.0, "length": 2.0, "pitch": 80, "velocity": 0.75 }]), "the note is added to those there");
    assert_eq!(played["range"], j!([48, 80]), "the keyboard shows the note");
    // A key still down when the recording ends is a note until there.
    at(&g, 4.0);
    press(&g, 60, 1.0);
    record(&g, "played", true);
    at(&g, 4.5);
    record(&g, "played", false);
    assert_eq!(track(&g, "played")["notes"][2], j!({ "at": 4.0, "length": 0.5, "pitch": 60, "velocity": 1.0 }));
}

#[test]
fn a_take_becomes_the_keyframes_that_follow_it() {
    use goofi_graph::timeline::{Lanes, Take, Track};
    // Two seconds of a sine at 32 samples a second, then a second pass over its first half.
    let wave = |t: f64| (t * std::f64::consts::TAU).sin();
    let samples: Vec<(f64, f64)> = (0..=64).map(|i| f64::from(i) / 32.0).map(|t| (t, wave(t))).collect();
    let mut track = Track { name: "wave".into(), order: 0, input: String::new(), lanes: Lanes::Curve { range: [0.0, 1.0], rate: 30.0, keys: Vec::new() } };
    track.merge(Take::Samples(samples.clone()));
    let Lanes::Curve { range, keys, .. } = &track.lanes else { unreachable!() };
    assert!(track.check().is_ok() && keys.len() < 16, "a smooth wave takes few keyframes: {}", keys.len());
    assert!((range[0] + 1.0).abs() < 1e-9 && (range[1] - 1.0).abs() < 1e-9, "the range holds the wave: {range:?}");
    // The curve as it plays: from the keyframe at or before `t`, eased to the next by its curve.
    let level = |t: f64| {
        let next = keys.iter().position(|k| k.at > t).unwrap_or(keys.len() - 1).max(1);
        let (from, to) = (&keys[next - 1], &keys[next]);
        from.value + (to.value - from.value) * from.curve.at((t - from.at) / (to.at - from.at))
    };
    let off = samples.iter().map(|&(t, v)| (level(t) - v).abs()).fold(0.0, f64::max);
    assert!(off <= 0.021, "every sample is within a hundredth of the span: {off}");
    // A clock that looped records pass after pass: each replaces the stretch it covers.
    track.merge(Take::Samples(vec![(1.5, 0.0), (2.0, 0.0), (0.0, 0.5), (1.0, 0.5)]));
    let Lanes::Curve { keys, .. } = &track.lanes else { unreachable!() };
    let held: Vec<(f64, f64)> = keys.iter().map(|k| (k.at, k.value)).collect();
    assert_eq!((&held[..2], &held[held.len() - 2..]), (&[(0.0, 0.5), (1.0, 0.5)][..], &[(1.5, 0.0), (2.0, 0.0)][..]), "{held:?}");
}

#[test]
fn a_curve_records_a_node_at_the_nodes_pace_and_no_faster_than_its_rate() {
    use goofi_core::{Data, Meta};
    use goofi_graph::expr_rewrite::Target;
    use goofi_graph::machine::Machines;
    use goofi_graph::timeline::{Lanes, Take, Track};
    use goofi_node::Local;
    // The node's newest frame: its value, stamped with the instant it left the node.
    let frame = std::cell::Cell::new((0.0f32, 0.0));
    let read = |target: &Target| match target {
        Target::Node { .. } => {
            let (value, left) = frame.get();
            let mut meta = Meta::default();
            meta.set_procend(Some(left));
            Ok(Local::Frame(Data::array_f32(vec![1], value.to_le_bytes().to_vec(), meta).unwrap()))
        }
        other => Err(format!("{other:?} is not readable here")),
    };
    let recorder = || {
        let track = Track { name: "wave".into(), order: 0, input: "nd('lfo')".into(), lanes: Lanes::Curve { range: [0.0, 1.0], rate: 30.0, keys: Vec::new() } };
        let mut m = Machines::default();
        // At a tempo of 60 a beat is a second.
        m.configure(Default::default(), ([("k1".to_string(), track)].into_iter().collect(), 60.0), None);
        m.record("k1");
        m
    };
    let samples = |take: Option<Take>| match take {
        Some(Take::Samples(samples)) => samples,
        other => panic!("a curve takes samples: {other:?}"),
    };

    // A node of two frames a second, looked at thirty times a second, as the stepper asks to:
    // one sample a frame, a frame that says the same again too.
    let mut m = recorder();
    let levels = [0.0, 1.0, 1.0, 3.0, 4.0];
    for i in 0..=60u32 {
        let now = f64::from(i) / 30.0;
        frame.set((levels[(i / 15) as usize], f64::from(i / 15) * 0.5));
        let said = m.advance(now, &read);
        assert!(said.iter().any(|(name, value)| name == "wave.recording" && goofi_core::control::number_of(value) == 1.0), "the track says it records");
        assert_eq!(m.next_deadline(), Some(now + 1.0 / 30.0), "a recording curve is looked at again at its rate");
    }
    assert_eq!(samples(m.recorded("k1", 2.0, &read)), [(0.0, 0.0), (0.5, 1.0), (1.0, 1.0), (1.5, 3.0), (2.0, 4.0)]);
    assert!(m.advance(2.0, &read).iter().any(|(name, value)| name == "wave.recording" && goofi_core::control::number_of(value) == 0.0), "and that it stopped");

    // A node of a hundred frames a second gives no more than the track's rate.
    let mut m = recorder();
    for i in 0..=100u32 {
        let now = f64::from(i) / 100.0;
        frame.set((i as f32, now));
        m.advance(now, &read);
    }
    let taken = samples(m.recorded("k1", 1.0, &read));
    assert!(taken.len() > 20 && taken.windows(2).all(|s| s[1].0 - s[0].0 >= 1.0 / 30.0), "{taken:?}");

    // A position that went back is another pass, from where it is.
    let mut m = recorder();
    for (now, level) in [(3.0, 1.0), (3.5, 2.0), (1.0, 3.0), (1.5, 4.0)] {
        frame.set((level, f64::from(level)));
        m.advance(now, &read);
    }
    assert_eq!(samples(m.recorded("k1", 1.5, &read)), [(3.0, 1.0), (3.5, 2.0), (1.0, 3.0), (1.5, 4.0)]);
}
