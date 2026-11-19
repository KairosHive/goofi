//! The timeline: a curve track and a note track written through ops, read through their groups'
//! variables at the clock's position, renamed, saved, reopened and undone.

use std::sync::Arc;

use goofi_python::inproc::PyExprEvaluator;
use goofi_tests::{hex, j, number, Goofi};
use serde_json::Value;

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
    // A keyframe with no curve is smooth: the value goes through it at the slope the keyframes
    // beside it give it. Down a straight fall it stays on the line all the way, where an easing
    // levels out at every keyframe.
    let level = |g: &Goofi, time: f64, value: f64, what: &str| {
        at(g, time);
        g.until(&format!("{what} at {time}"), |g| ((number(g, "sweep.value") - value).abs() < 1e-3).then_some(()));
    };
    let fall = |curve: &str| j!([{ "at": 0.0, "value": 10.0, "curve": curve }, { "at": 2.0, "value": 5.0, "curve": curve }, { "at": 4.0, "value": 0.0 }]);
    g.call("timeline track edit", j!({ "track": "sweep", "keys": [{ "at": 0.0, "value": 10.0 }, { "at": 2.0, "value": 5.0 }, { "at": 4.0, "value": 0.0 }] }));
    assert_eq!(g.doc()["timeline"][sweep["id"].as_str().unwrap()]["keys"][1]["curve"], j!("smooth"));
    for (time, value) in [(0.5, 8.75), (1.9, 5.25), (2.1, 4.75), (3.5, 1.25)] {
        level(&g, time, value, "the smooth fall");
    }
    g.call("timeline track edit", j!({ "track": "sweep", "keys": fall("easing") }));
    for (time, value) in [(0.5, 9.375), (1.9, 5.025), (2.1, 4.975)] {
        level(&g, time, value, "the eased fall");
    }
    // A keyframe may stand before 0, as one may stand past the timeline's length: the clock does
    // not come there, and the curve from it is the curve at 0.
    g.call("timeline track edit", j!({ "track": "sweep", "keys": [{ "at": -2.0, "value": 0.0, "curve": "linear" }, { "at": 2.0, "value": 10.0 }] }));
    for (time, value) in [(0.0, 5.0), (1.0, 7.5), (3.0, 10.0)] {
        level(&g, time, value, "the line from before 0");
    }
    // At a peak a smooth curve is level, and it comes up to the peak and never past it; where
    // the keyframes go on the same way, it goes through at their slope.
    g.call("timeline track edit", j!({ "track": "sweep", "keys": [{ "at": 0.0, "value": 0.0 }, { "at": 2.0, "value": 10.0 }, { "at": 4.0, "value": 0.0 }] }));
    for (time, value) in [(1.0, 6.25), (1.9, 9.95125), (2.0, 10.0), (2.1, 9.95125), (3.0, 6.25)] {
        level(&g, time, value, "the smooth peak");
    }
    // A slope is no more than twice the lesser of the two lines it stands between, so a short
    // step after a long fall does not swing the value past its keyframes.
    g.call("timeline track edit", j!({ "track": "sweep", "keys": [{ "at": 0.0, "value": 10.0 }, { "at": 1.0, "value": 1.0 }, { "at": 4.0, "value": 0.0 }] }));
    for time in [0.25, 0.5, 0.75, 0.95, 1.5, 2.5, 3.5] {
        at(&g, time);
        let floor = if time < 1.0 { 1.0 } else { 0.0 };
        g.until(&format!("the level at {time} between its keyframes"), |g| (number(g, "sweep.value") > floor && number(g, "sweep.value") < if time < 1.0 { 10.0 } else { 1.0 }).then_some(()));
    }
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
    // Played, the thread wakes for the note's start and for its end.
    g.call("clock set", j!({ "playing": true, "time": 0.9 }));
    g.until("the note to start", |g| (held(g, "keys.notes", 60) == 0.5).then_some(()));
    g.until("the note to end", |g| (held(g, "keys.notes", 60) == 0.0).then_some(()));
    at(&g, 1.0);

    // A track is put before another, or last: the document holds them in the order every panel
    // draws them in.
    let order = |g: &Goofi| g.doc()["timeline"].as_object().unwrap().values().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
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
        (j!({ "track": "keys", "range": [60, 72] }), "a note track takes `notes`"),
        (j!({ "track": "sweep", "notes": [] }), "a note track takes"),
        (j!({ "track": "sweep", "keys": [{ "at": 1.0, "value": 0.0 }, { "at": 1.0, "value": 1.0 }] }), "each later"),
        (j!({ "track": "sweep", "to": "keys" }), "already exists"),
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
    let keys = |g: &Goofi, name: &str| -> Vec<(f64, f64)> { track(g, name)["keys"].as_array().unwrap().iter().map(|k| (k["at"].as_f64().unwrap(), k["value"].as_f64().unwrap())).collect() };
    let record = |g: &Goofi, name: &str, on: bool| {
        g.call("timeline track record", j!({ "track": name, "on": on }));
        g.until("the track to say whether it records", |g| (number(g, &format!("{name}.recording")) == f64::from(u8::from(on))).then_some(()));
    };
    // Play the clock until it has moved on, pause it, and answer the beat it stands at. A seek
    // would begin another pass of a recording; time that goes by does not.
    let play = |g: &Goofi| -> f64 {
        let from = g.call("clock set", j!({ "playing": true }))["time"].as_f64().unwrap();
        g.until("the clock to move on", |g| (g.call("clock set", j!({}))["time"].as_f64().unwrap() > from + 0.2).then_some(()));
        g.call("clock set", j!({ "playing": false }))["time"].as_f64().unwrap()
    };
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;

    // A curve track records its input: an expression as a param's, with no `me`, and none
    // before one is set.
    g.call("variable entry add", j!({ "name": "desk.level", "value": 0.0 }));
    g.call("timeline track add", j!({ "kind": "curve", "name": "take" }));
    assert!(g.refuse("timeline track record", j!({ "track": "take", "on": true })).contains("no `input`"));
    assert!(g.refuse("timeline track edit", j!({ "track": "take", "input": "me.out.gain" })).contains("without `me`"));
    assert!(g.refuse("timeline track edit", j!({ "track": "take", "rate": 0.0 })).contains("1..120"));
    g.call("timeline track edit", j!({ "track": "take", "input": "sm('head').gain" }));
    let before = j!([{ "at": 0.1, "value": 3.0 }, { "at": 500.0, "value": 7.0 }]);
    g.call("timeline track edit", j!({ "track": "take", "input": "vars.desk.level", "rate": 20.0, "keys": before }));
    // The level is 0 while the clock plays from beat 0 to a beat `up`, where it is 10 and the
    // take ends: its keyframes are in place of those of its stretch, and the one past it stays.
    record(&g, "take", true);
    let up = play(&g);
    g.call("variable entry edit", j!({ "name": "desk.level", "value": 10.0 }));
    record(&g, "take", false);
    let first = keys(&g, "take");
    assert!(first.len() == 3 && first[0] == (0.0, 0.0) && near(first[1].0, up) && first[1].1 == 10.0 && first[2] == (500.0, 7.0), "{first:?} for a take that ends at {up}");
    assert_eq!((&track(&g, "take")["range"], &track(&g, "take")["rate"]), (&j!([0.0, 10.0]), &j!(20.0)), "the range holds what came");
    // A second take, from there to a beat `down` where the level is 0 again, goes after it.
    record(&g, "take", true);
    let down = play(&g);
    g.call("variable entry edit", j!({ "name": "desk.level", "value": 0.0 }));
    record(&g, "take", false);
    let second = keys(&g, "take");
    assert!(second.len() == 4 && second[..2] == first[..2] && near(second[2].0, down) && second[2].1 == 0.0, "{second:?} for a take from {up} to {down}");
    // The curve plays what was taken. A level is read a little past its keyframe, not on it: a
    // level that was held is a step there, and a beat and its second differ in their last digit.
    for (time, level) in [(0.0, 0.0), (up + 1e-4, 10.0), (down + 1e-4, 0.0), (500.0, 7.0)] {
        at(&g, time);
        g.until(&format!("the recorded level at {time}"), |g| ((number(g, "take.value") - level).abs() < 1e-2).then_some(()));
    }
    // A take is one step back: the second gone, the track holds the first.
    g.call("undo", j!({}));
    assert_eq!(keys(&g, "take"), first);
    // A rename of what the input reads follows into it, as into any expression.
    g.call("variable entry rename", j!({ "name": "desk.level", "to": "desk.fader" }));
    assert_eq!(track(&g, "take")["input"], j!("vars.desk.fader"));
    // A bezier a recording fits plays as the cubic it is: handles a third and two thirds along.
    let bowed = j!([{ "at": 0.0, "value": 0.0, "curve": { "custom": [[0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 1.0], [1.0, 1.0, -1.0 / 3.0, 0.0, 0.0, 0.0]] } }, { "at": 1.0, "value": 1.0 }]);
    g.call("timeline track edit", j!({ "track": "take", "keys": bowed }));
    at(&g, 0.5);
    g.until("the bezier's value half way", |g| ((number(g, "take.value") - 0.875).abs() < 1e-6).then_some(()));

    // A node's output is an input as it is a param's: what the node emits is what is taken.
    let reader = g.add("_TestScalar");
    let emitted = g.probe(reader, "out");
    g.call("timeline track add", j!({ "kind": "curve", "name": "heard" }));
    g.call("timeline track edit", j!({ "track": "heard", "input": format!("nd('{}')", g.name(&hex(reader))) }));
    record(&g, "heard", true);
    g.set_param(reader, "control", "value", 4.0);
    g.until("the node to emit 4", |_| emitted.latest().filter(|d| goofi_tests::f32s(d) == [4.0]));
    record(&g, "heard", false);
    let heard = track(&g, "heard");
    assert_eq!((&heard["range"], heard["keys"].as_array().and_then(|k| k.last()).map(|k| &k["value"])), (&j!([0.0, 4.0]), Some(&j!(4.0))), "{heard}");

    // A note track records the keys its input holds, a MIDI device's `notes`, on any channel.
    g.graph().variables().grab_midi("pads", "a test port").unwrap();
    let press = |g: &Goofi, key: usize, velocity: f64| {
        let mut keys = vec![0.0; 2048];
        keys[key] = velocity;
        g.graph().variables().drive("pads.notes", goofi_core::Data::numbers(keys));
    };
    g.call("timeline track add", j!({ "kind": "note", "name": "played" }));
    let there = j!({ "at": 0.0, "length": 1.0, "pitch": 50, "velocity": 0.5 });
    g.call("timeline track edit", j!({ "track": "played", "input": "vars.pads.notes", "notes": [there] }));
    // Pitch 80 on channel 2 is down from beat 1 until the clock is paused, and comes up there.
    at(&g, 1.0);
    press(&g, 128 + 80, 0.75);
    record(&g, "played", true);
    let lifted = play(&g);
    press(&g, 0, 0.0);
    record(&g, "played", false);
    let played = track(&g, "played");
    let note = &played["notes"][1];
    assert_eq!((&played["notes"][0], &note["at"], &note["pitch"], &note["velocity"]), (&there, &j!(1.0), &j!(80), &j!(0.75)), "the note is added to those there");
    assert!(near(note["length"].as_f64().unwrap(), lifted - 1.0), "{note} for a key that came up at {lifted}");
    // A track is a group with `notes` too, so another can read it as its input, and a rename
    // of it follows there.
    g.call("timeline track add", j!({ "kind": "note", "name": "copy" }));
    g.call("timeline track edit", j!({ "track": "copy", "input": "vars.played.notes" }));
    g.call("timeline track edit", j!({ "track": "played", "to": "lead" }));
    assert_eq!(track(&g, "copy")["input"], j!("vars.lead.notes"));
    g.call("timeline track edit", j!({ "track": "lead", "to": "played" }));
    // A key still down when the recording ends is a note until there.
    at(&g, 4.0);
    press(&g, 60, 1.0);
    record(&g, "played", true);
    let ended = play(&g);
    record(&g, "played", false);
    let last = track(&g, "played")["notes"][2].clone();
    assert!(last["at"] == j!(4.0) && last["pitch"] == j!(60) && near(last["length"].as_f64().unwrap(), ended - 4.0), "{last} for a recording that ended at {ended}");
}

/// The value a curve track's keyframes play at the beat `t`: from the keyframe at or before it,
/// eased to the next by its curve.
fn level(keys: &[goofi_graph::timeline::Key], t: f64) -> f64 {
    let next = keys.iter().position(|k| k.at > t).unwrap_or(keys.len() - 1).max(1);
    let (from, to) = (&keys[next - 1], &keys[next]);
    from.value + (to.value - from.value) * goofi_graph::timeline::shape(keys, next - 1).at((t - from.at) / (to.at - from.at))
}

#[test]
fn a_take_becomes_the_keyframes_that_follow_it() {
    use goofi_core::ease::Curve;
    use goofi_graph::timeline::{Lanes, Sample, Take, Track};
    let sample = |at: f64, value: f64| Sample { at, value, held: false };
    let curve = || Track { name: "wave".into(), input: String::new(), lanes: Lanes::Curve { range: [0.0, 1.0], rate: 30.0, keys: Vec::new() } };
    let merged = |passes: Vec<Vec<Sample>>| {
        let mut track = curve();
        track.merge(&Take::Samples(passes)).unwrap();
        track.check().unwrap();
        let Lanes::Curve { range, keys, .. } = track.lanes else { unreachable!() };
        (range, keys)
    };
    // Every beat of a take, a hundredth apart, with what the take, the line through its samples,
    // is there.
    let along = |take: &[Sample]| -> Vec<(f64, f64)> {
        let (from, to) = (take[0].at, take[take.len() - 1].at);
        let line = |t: f64| {
            let i = take.iter().rposition(|s| s.at <= t).unwrap().min(take.len() - 2);
            take[i].value + (take[i + 1].value - take[i].value) * (t - take[i].at) / (take[i + 1].at - take[i].at)
        };
        (0..=((to - from) * 100.0) as usize).map(|i| from + i as f64 / 100.0).map(|t| (t, line(t))).collect()
    };
    let furthest = |keys: &[goofi_graph::timeline::Key], take: &[Sample]| along(take).iter().map(|&(t, v)| (level(keys, t) - v).abs()).fold(0.0, f64::max);

    // Two seconds of a sine at 32 samples a second: few keyframes, each eased by a bezier, and
    // the curve within the tolerance, a hundredth of the range, all along the take.
    let wave: Vec<Sample> = (0..=64).map(|i| f64::from(i) / 32.0).map(|t| sample(t, (t * std::f64::consts::TAU).sin())).collect();
    let (range, keys) = merged(vec![wave.clone()]);
    assert!(keys.len() < 16 && keys.iter().any(|k| matches!(k.curve, Curve::Custom(_))), "a smooth wave takes few keyframes: {}", keys.len());
    assert!((range[0] + 1.0).abs() < 1e-9 && (range[1] - 1.0).abs() < 1e-9, "the range holds the wave: {range:?}");
    assert!(furthest(&keys, &wave) <= 0.02, "the curve follows the wave: {}", furthest(&keys, &wave));
    // A take whose samples lie in pairs far apart, as a level that jumps: a cubic through the
    // four of them would swing far past both levels in between.
    let jump = vec![sample(0.0, 0.0), sample(1.95, 0.0), sample(2.0, 10.0), sample(3.95, 10.0)];
    let (range, keys) = merged(vec![jump.clone()]);
    assert!(furthest(&keys, &jump) <= 0.1 && range == [0.0, 10.0], "the curve stays on the take between its samples: {} off, {keys:?}", furthest(&keys, &jump));
    // A sample the input held is a step: the value stays until the next keyframe.
    let (_, keys) = merged(vec![vec![Sample { held: true, ..sample(0.0, 0.0) }, sample(2.0, 10.0)]]);
    assert_eq!((keys[0].curve.clone(), level(&keys, 1.99), level(&keys, 2.0)), (Curve::Step, 0.0, 10.0));
    // Each pass is in place of the stretch it covers, the later over the earlier.
    let mut track = curve();
    track.merge(&Take::Samples(vec![wave])).unwrap();
    track.merge(&Take::Samples(vec![vec![sample(1.5, 0.0), sample(2.0, 0.0)], vec![sample(0.0, 0.5), sample(1.0, 0.5)]])).unwrap();
    let Lanes::Curve { keys, .. } = &track.lanes else { unreachable!() };
    let held: Vec<(f64, f64)> = keys.iter().map(|k| (k.at, k.value)).collect();
    assert_eq!((&held[..2], &held[held.len() - 2..]), (&[(0.0, 0.5), (1.0, 0.5)][..], &[(1.5, 0.0), (2.0, 0.0)][..]), "{held:?}");
    // A take that leaves the curve more keyframes than it holds is refused, and the track is as
    // it was: one pass too many, or two that are each within it.
    let was = track.clone();
    let zigzag = |from: i32, to: i32| -> Vec<Sample> { (from..to).map(|i| sample(f64::from(i), f64::from(i % 2))).collect() };
    for take in [vec![zigzag(0, 3000)], vec![zigzag(5000, 6200), zigzag(3000, 4200)]] {
        let refusal = track.merge(&Take::Samples(take));
        assert!(refusal.is_err_and(|why| why.contains("more than 2000 keyframes")) && track == was);
    }
}

#[test]
fn a_recording_takes_its_input_at_the_inputs_pace_and_no_faster_than_its_rate() {
    use goofi_core::{Data, Meta};
    use goofi_graph::expr_rewrite::Target;
    use goofi_graph::machine::Machines;
    use goofi_graph::timeline::{Lanes, Sample, Take, Track};
    use goofi_node::Local;
    // What the inputs hold: a node's newest frame, its value stamped with the instant it left
    // the node; a playhead's element, which is a variable; and a device's keys.
    let frame = std::cell::Cell::new((0.0f32, 0.0));
    let (gain, keys) = (std::cell::Cell::new(0.0), std::cell::RefCell::new(vec![0.0; 256]));
    let read = |target: &Target| match target {
        Target::Node { .. } => {
            let (value, left) = frame.get();
            let mut meta = Meta::default();
            meta.set_procend(Some(left));
            Ok(Local::Frame(Data::array_f32(vec![1], value.to_le_bytes().to_vec(), meta).unwrap()))
        }
        Target::Variable { key } if key == "head.gain" => Ok(Local::Frame(Data::number(gain.get()))),
        Target::Variable { key } if key == "pads.notes" => Ok(Local::Frame(Data::numbers(keys.borrow().clone()))),
        other => Err(format!("{other:?} is not readable here")),
    };
    let recorder = |input: &str, lanes: Lanes| {
        let track = Track { name: "wave".into(), input: input.into(), lanes };
        let mut m = Machines::default();
        // At a tempo of 60 a beat is a second.
        m.configure(Default::default(), ([("k1".to_string(), track)].into_iter().collect(), 60.0), None);
        m.record("k1");
        m
    };
    let curve = |input: &str| recorder(input, Lanes::Curve { range: [0.0, 1.0], rate: 30.0, keys: Vec::new() });
    let samples = |take: Option<Take>| match take {
        Some(Take::Samples(passes)) => passes,
        other => panic!("a curve takes samples: {other:?}"),
    };
    let sample = |at: f64, value: f64| Sample { at, value, held: false };
    let says = |said: &[(String, Data)], recording: f64| said.iter().any(|(name, value)| name == "wave.recording" && goofi_core::control::number_of(value) == recording);

    // A node of two frames a second, looked at thirty times a second, as the stepper asks to:
    // one sample a frame, a frame that says the same again too, each joined to the next.
    let mut m = curve("nd('lfo')");
    let levels = [0.0, 1.0, 1.0, 3.0, 4.0];
    for i in 0..=60u32 {
        let now = f64::from(i) / 30.0;
        frame.set((levels[(i / 15) as usize], f64::from(i / 15) * 0.5));
        let said = m.advance(now, &read);
        assert!(says(&said, 1.0), "the track says it records");
        // What it has taken so far is told at every pass, for a panel to draw: the first sample
        // and the newest one are in it.
        let so_far: Vec<f64> = said.iter().find(|(name, _)| name == "wave.take").map(|(_, take)| goofi_core::control::numbers(take).collect()).unwrap();
        let newest = f64::from(i / 15);
        assert_eq!((&so_far[..2], &so_far[so_far.len() - 2..]), (&[0.0, 0.0][..], &[newest * 0.5, f64::from(levels[newest as usize])][..]));
        assert_eq!(m.next_deadline(), Some(now + 1.0 / 30.0), "a recording curve is looked at again at its rate");
    }
    assert_eq!(samples(m.recorded("k1", 2.0, &read)), [[sample(0.0, 0.0), sample(0.5, 1.0), sample(1.0, 1.0), sample(1.5, 3.0), sample(2.0, 4.0)]]);
    let after = m.advance(2.0, &read);
    assert!(says(&after, 0.0) && after.iter().any(|(name, take)| name == "wave.take" && goofi_core::control::numbers(take).count() == 0), "and that it stopped, with nothing taken");

    // A node of a hundred frames a second gives no more than the track's rate.
    let mut m = curve("nd('lfo')");
    for i in 0..=100u32 {
        let now = f64::from(i) / 100.0;
        frame.set((i as f32, now));
        m.advance(now, &read);
    }
    let taken = samples(m.recorded("k1", 1.0, &read)).remove(0);
    assert!(taken.len() > 20 && taken.windows(2).all(|s| s[1].at - s[0].at >= 1.0 / 30.0), "{taken:?}");

    // A position that went back, as a loop does, or was put elsewhere, as a seek does, begins
    // another pass from where it is.
    let mut m = curve("nd('lfo')");
    for (now, level, seek) in [(3.0, 1.0, false), (3.5, 2.0, false), (1.0, 3.0, false), (1.5, 4.0, false), (5.0, 5.0, true)] {
        if seek {
            m.restart();
        }
        frame.set((level, f64::from(level)));
        m.advance(now, &read);
    }
    assert_eq!(samples(m.recorded("k1", 5.0, &read)), [vec![sample(3.0, 1.0), sample(3.5, 2.0)], vec![sample(1.0, 3.0), sample(1.5, 4.0)], vec![sample(5.0, 5.0)]]);

    // An input that reads no node, here a playhead's element as a param reads it, holds its
    // value between two changes: a look that found it the same makes the change a step.
    let mut m = curve("sm('head').gain");
    for (now, level) in [(0.0, 0.0), (0.5, 0.0), (1.0, 5.0), (1.05, 6.0)] {
        gain.set(level);
        m.advance(now, &read);
    }
    assert_eq!(samples(m.recorded("k1", 2.0, &read)), [[Sample { held: true, ..sample(0.0, 0.0) }, sample(1.0, 5.0), sample(1.05, 6.0), sample(2.0, 6.0)]]);

    // A note is a key of the device from when it goes down to when it comes up; a seek ends it
    // where the recording last looked, and a key still down begins another where it lands.
    let mut m = recorder("vars.pads.notes", Lanes::Note { notes: Vec::new() });
    for (now, velocity, seek) in [(1.0, 0.5, false), (1.5, 0.5, false), (3.0, 0.5, true), (3.5, 0.0, false)] {
        if seek {
            m.restart();
        }
        keys.borrow_mut()[128 + 60] = velocity;
        m.advance(now, &read);
    }
    // The notes so far are told while the recording runs, a key still down among them.
    keys.borrow_mut()[128 + 60] = 0.5;
    let said = m.advance(3.75, &read);
    let so_far: Vec<f64> = said.iter().find(|(name, _)| name == "wave.take").map(|(_, take)| goofi_core::control::numbers(take).collect()).unwrap();
    assert_eq!(so_far, [1.0, 0.5, 60.0, 0.5, 3.0, 0.5, 60.0, 0.5, 3.75, 0.01, 60.0, 0.5]);
    keys.borrow_mut()[128 + 60] = 0.0;
    m.advance(3.8, &read);
    // A track the stepper does not hold records nothing.
    m.record("gone");
    assert!(m.recorded("gone", 4.0, &read).is_none());
    let Some(Take::Notes(notes)) = m.recorded("k1", 4.0, &read) else { panic!("a note track takes notes") };
    let taken: Vec<_> = notes.iter().map(|n| (n.at, n.pitch, n.velocity)).collect();
    assert_eq!((taken, notes[0].length, notes[1].length), (vec![(1.0, 60, 0.5), (3.0, 60, 0.5), (3.75, 60, 0.5)], 0.5, 0.5));
}
