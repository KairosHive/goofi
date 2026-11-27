//! The patch clock: its position pauses, jumps and restarts at a length, an expression's `t` and
//! the machines stand with it, and the run time that stamps a frame never stops.

use std::sync::Arc;

use goofi_runtime::python::inproc::PyExprEvaluator;
use goofi_tests::{f32s, hex, j, number, Goofi};

fn time(g: &Goofi) -> f64 {
    number(g, "clock.time")
}

#[test]
fn the_position_pauses_jumps_and_restarts_and_the_run_time_goes_on() {
    let g = Goofi::new();
    g.graph().set_evaluator(Arc::new(PyExprEvaluator::new().unwrap()));
    // The position plays from the load, and `t` is the position.
    let began = time(&g);
    g.until("the position to play", |g| (time(g) > began).then_some(()));
    assert_eq!(number(&g, "clock.playing"), 1.0);
    let reader = g.add("_TestScalar");
    g.call("node param edit", j!({ "node": hex(reader), "param": "control/value", "expression": "t" }));
    let probe = g.probe(reader, "out");
    let read = |_: &Goofi| probe.latest().map(|d| (f64::from(f32s(&d)[0]), d.meta().procstart().unwrap_or(f64::NAN)));
    g.until("the node to read the position", |g| read(g).filter(|(t, _)| *t > 0.0));

    // A pause holds the position, and with it `t`; the node runs on, and its stamps go on.
    assert_eq!(g.call("clock set", j!({ "playing": false }))["playing"], j!(false));
    let held = time(&g);
    assert_eq!(number(&g, "clock.playing"), 0.0);
    let (_, stamped) = g.until("the node to read the held position", |g| read(g).filter(|(t, _)| (*t - held).abs() < 1e-3));
    g.until("a later stamp on the held position", |g| read(g).filter(|(t, at)| (*t - held).abs() < 1e-3 && *at > stamped));
    assert!(g.stays(|g| time(g) == held), "a paused position stands");

    // A jump puts it anywhere, paused or not; a resume plays from there; a stop is both at once.
    g.call("clock set", j!({ "time": 100.0 }));
    assert_eq!(time(&g), 100.0);
    g.call("clock set", j!({ "playing": true }));
    g.until("the position to play from the jump", |g| (time(g) > 100.0).then_some(()));
    assert_eq!(g.call("clock set", j!({ "playing": false, "time": 0.0 })), j!({ "playing": false, "time": 0.0, "section": null }));
    assert!(g.refuse("clock set", j!({ "time": -1.0 })).contains("at least 0"));

    // With a length, in beats, the position restarts from 0 when it reaches it. At the tempo of
    // 120 a beat is half a second.
    g.call("variable entry edit", j!({ "name": "clock.length", "value": 0.4 }));
    g.call("clock set", j!({ "playing": true }));
    let mut last = time(&g);
    g.until("the position to restart at the length", |g| {
        let now = time(g);
        (now < std::mem::replace(&mut last, now)).then_some(())
    });
    assert!(g.stays(|g| time(g) < 0.2), "the position stays under its length");
    // A section loops the position inside it, and is the clock's own: no variable holds it.
    g.call("variable entry edit", j!({ "name": "clock.length", "value": 20.0 }));
    g.call("clock set", j!({ "playing": false, "time": 0.0 }));
    assert_eq!(g.call("clock set", j!({ "section": [1.0, 1.2] }))["section"], j!([1.0, 1.2]));
    assert_eq!(time(&g), 0.0, "before the section the position is its own");
    g.call("clock set", j!({ "playing": true, "time": 0.9 }));
    g.until("the position to reach the section", |g| (time(g) >= 1.0).then_some(()));
    assert!(g.stays(|g| (1.0..1.2).contains(&time(g))), "the position stays in the section");
    assert!(g.call("clock set", j!({ "section": [0.0, 0.0] }))["section"].is_null(), "a section of no length is none");
    // A section stands in the length's place: the position plays past the length up to it, and
    // never goes back to 0 on the way.
    g.call("variable entry edit", j!({ "name": "clock.length", "value": 0.4 }));
    g.call("clock set", j!({ "section": [1.0, 1.2], "time": 0.3 }));
    let mut lowest = f64::INFINITY;
    g.until("the position to pass the length for the section", |g| {
        lowest = lowest.min(time(g));
        (time(g) >= 1.0).then_some(())
    });
    assert!(lowest >= 0.3, "the position went back to {lowest} on its way to the section");
    assert!(g.call("clock set", j!({ "section": [0.0, 0.0] }))["section"].is_null());
    g.call("variable entry edit", j!({ "name": "clock.length", "value": 20.0 }));
    assert!(g.refuse("clock set", j!({ "section": [-1.0, 2.0] })).contains("at least 0"));
    assert!(g.refuse("clock set", j!({ "section": [1.0, 1.0001] })).contains("a millisecond"));
    // Endless again, it goes on from where it stands, past the length; it does not jump to the
    // run time, which is a thousand seconds on after the restart below.
    g.call("variable entry edit", j!({ "name": "clock.length", "value": 2000.0 }));
    g.call("clock set", j!({ "time": 999.9 }));
    g.until("the position to restart at a thousand seconds", |g| (time(g) < 999.0).then_some(()));
    g.call("variable entry edit", j!({ "name": "clock.length", "value": 0.0 }));
    assert!(time(&g) < 999.0, "the position went on from where it stood");

    // A paused clock plays again from zero on a load: the clock is the patch's.
    g.call("variable entry edit", j!({ "name": "clock.bpm", "value": 90.0 }));
    g.call("clock set", j!({ "playing": false, "time": 50.0, "section": [40.0, 60.0] }));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clock.gfi");
    g.call("session save", j!({ "path": path.to_string_lossy() }));
    g.call("session load", j!({ "path": path.to_string_lossy() }));
    assert_eq!((number(&g, "clock.playing"), number(&g, "clock.bpm")), (1.0, 90.0), "the clock plays, and its tempo rode the file");
    assert!(time(&g) < 50.0, "the position began again");
    assert!(g.call("clock set", j!({}))["section"].is_null(), "a section is of the patch that was");
}

#[test]
fn a_machine_steps_by_the_position() {
    let g = Goofi::new();
    g.graph().set_evaluator(Arc::new(PyExprEvaluator::new().unwrap()));
    for (op, payload) in [
        ("machine add", j!({ "name": "pulse" })),
        ("machine state add", j!({ "machine": "pulse", "name": "a" })),
        ("machine state add", j!({ "machine": "pulse", "name": "b" })),
        ("machine transition add", j!({ "machine": "pulse", "from": "a", "to": "b" })),
        ("machine transition add", j!({ "machine": "pulse", "from": "b", "to": "a" })),
        ("machine state edit", j!({ "machine": "pulse", "name": "a", "mode": "sequence", "interval": 0.1 })),
        ("machine state edit", j!({ "machine": "pulse", "name": "b", "mode": "sequence", "interval": 0.1 })),
        ("machine playhead add", j!({ "machine": "pulse", "name": "p", "start": "a" })),
    ] {
        g.call(op, payload);
    }
    let state = |g: &Goofi| g.variable("p.state").as_str().unwrap_or_default().to_string();
    g.until("the playhead to move", |g| (state(g) == "b").then_some(()));
    // Paused, no beat comes due: the playhead rests where it is for longer than its interval.
    g.call("clock set", j!({ "playing": false }));
    let rests = state(&g);
    assert!(g.stays(|g| state(g) == rests), "a paused clock holds the machine");
    // Played again, the beat comes due.
    g.call("clock set", j!({ "playing": true }));
    g.until("the playhead to move on", |g| (state(g) != rests).then_some(()));
    // A position that went back starts every run over, as a load does.
    g.call("machine state edit", j!({ "machine": "pulse", "name": "b", "interval": -1.0 }));
    g.until("the playhead to rest in b", |g| (state(g) == "b").then_some(()));
    g.call("machine state edit", j!({ "machine": "pulse", "name": "a", "interval": -1.0 }));
    g.call("clock set", j!({ "time": 0.0 }));
    g.until("the run to start over", |g| (state(g) == "a").then_some(()));
    // A jump far ahead settles none of the beats between: the machine steps on from there.
    for name in ["a", "b"] {
        g.call("machine state edit", j!({ "machine": "pulse", "name": name, "interval": 0.1 }));
    }
    g.until("the playhead to step again", |g| (state(g) == "b").then_some(()));
    g.call("clock set", j!({ "time": 1e9 }));
    g.until("the playhead to step to a after the jump", |g| (state(g) == "a").then_some(()));
    g.until("the playhead to step on to b", |g| (state(g) == "b").then_some(()));
    // A section too long for a wait to count down to its end is a section the position never
    // leaves: a machine at rest under it owes nothing, and steps again once it has a beat.
    for name in ["a", "b"] {
        g.call("machine state edit", j!({ "machine": "pulse", "name": name, "interval": -1.0 }));
    }
    assert_eq!(g.call("clock set", j!({ "section": [0.0, 1e20] }))["section"], j!([0.0, 1e20]));
    for name in ["a", "b"] {
        g.call("machine state edit", j!({ "machine": "pulse", "name": name, "interval": 0.1 }));
    }
    g.until("the playhead to step to a under the long section", |g| (state(g) == "a").then_some(()));
    g.until("the playhead to step on to b under it", |g| (state(g) == "b").then_some(()));
}
