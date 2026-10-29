//! Parameter modulation through the public expression interface.

use goofi_core::{control, Param};
use goofi_node::{EvalCtx, ExprEvaluator};
use goofi_python::inproc::PyExprEvaluator;

#[test]
fn modulation_uses_the_current_target_range_and_coordinate() {
    let evaluator = PyExprEvaluator::new().unwrap();
    let evaluate = |source: &str, t: f64, lo: f64, hi: f64| {
        let code = evaluator.compile(source).unwrap();
        let result = evaluator.eval(code.id, &EvalCtx { locals: &[], t, range: (lo, hi) });
        evaluator.release(code.id);
        result.map(|d| control::number_of(&d))
    };
    for (t, expected) in [(0.0, 4.0), (0.25, 6.0), (0.75, 2.0)] {
        assert!((evaluate("lfo()", t, 2.0, 6.0).unwrap() - expected).abs() < 1e-10);
    }
    assert_eq!(evaluate("lfo(src=0.25, vmin=-3, vmax=9)", 0.75, 2.0, 6.0).unwrap(), 9.0);
    assert_eq!(evaluate("lfo(src=0.25)", 0.0, -10.0, 10.0).unwrap(), 10.0);
    for t in [-2.3, 0.0, 0.25, 1.0, 100.5] {
        let value = evaluate("nse()", t, 2.0, 6.0).unwrap();
        assert!((2.0..=6.0).contains(&value));
        assert_eq!(value, evaluate(&format!("nse(src={t})"), 999.0, 2.0, 6.0).unwrap());
        let unit = evaluate("nse(vmin=0, vmax=1)", t, 2.0, 6.0).unwrap();
        // The result crosses the f32 carrier, so the two readings agree to f32 precision.
        assert!((value - (2.0 + 4.0 * unit)).abs() < 1e-5);
    }
    assert_ne!(evaluate("nse()", 0.0, 0.0, 1.0).unwrap(), evaluate("nse()", 1.0, 0.0, 1.0).unwrap());
    let left = evaluate("nse()", 1.0 - 1e-6, 0.0, 1.0).unwrap();
    let right = evaluate("nse()", 1.0 + 1e-6, 0.0, 1.0).unwrap();
    assert!((left - right).abs() < 1e-9);
    for kind in ["lfo", "nse"] {
        for freq in [0.0, 0.05, 0.5, 2.0] {
            let actual = evaluate(&format!("{kind}(freq={freq})"), 0.37, 2.0, 6.0).unwrap();
            let expected = evaluate(&format!("{kind}()"), 0.37 * freq, 2.0, 6.0).unwrap();
            assert!((actual - expected).abs() < 1e-10);
            let explicit = evaluate(&format!("{kind}(freq={freq}, src=0.37)"), 999.0, 2.0, 6.0).unwrap();
            assert_eq!(actual, explicit);
        }
    }
    // The result is a frame; the reader reads it into its param, an int rounded.
    let code = evaluator.compile("lfo()").unwrap();
    for (lo, hi) in [(2, 6), (-10, 20)] {
        let target = Param::int(0, lo, hi);
        let result = evaluator.eval(code.id, &EvalCtx { locals: &[], t: 0.25, range: (lo as f64, hi as f64) }).unwrap();
        let read = control::read(&result, &target);
        assert!(matches!(&read, Param::Num { value, int: true, .. } if value[0] == hi as f64), "{read:?}");
    }
    evaluator.release(code.id);
    for source in ["lfo(1)", "nse(1)", "lfo(rate=1)", "nse(rate=1)"] {
        assert!(evaluate(source, 0.0, 0.0, 1.0).is_err(), "{source}");
    }

    // One text compiles once however many bindings hold it, and the last release lets it go.
    let (a, b) = (evaluator.compile("t + 1").unwrap(), evaluator.compile("t + 1").unwrap());
    assert_eq!(a.id, b.id, "one source, one compiled function");
    evaluator.release(a.id);
    assert_eq!(control::number_of(&evaluator.eval(b.id, &EvalCtx { locals: &[], t: 2.0, range: (0.0, 1.0) }).unwrap()), 3.0, "held by the other");
    evaluator.release(b.id);
    assert!(evaluator.eval(b.id, &EvalCtx { locals: &[], t: 2.0, range: (0.0, 1.0) }).is_err(), "released");

    // A FUNCTIONAL is a compiled expression's code as a frame: it crosses the wire as bytes, equal
    // by those bytes, and whoever reads it runs it at their own time; a clamp is the code's own.
    let compiled = evaluator.compile("min(2 + 3 * t, 5)").unwrap();
    let frame = goofi_core::Data::functional(compiled.code.clone(), goofi_core::Meta::default());
    evaluator.release(compiled.id);
    let crossed = goofi_codec::decode(&goofi_codec::encode(&frame).unwrap()).unwrap();
    assert_eq!(crossed, frame);
    assert_eq!(crossed.as_functional(), frame.as_functional());
    for (t, want) in [(0.0, 2.0), (0.5, 3.5), (1.0, 5.0), (7.0, 5.0)] {
        let value = goofi_node::at(&evaluator, &crossed, t, (0.0, 1.0)).unwrap();
        assert_eq!(control::number_of(&value), want, "at t={t}");
    }
    let plain = goofi_core::Data::number(4.0);
    assert_eq!(goofi_node::at(&evaluator, &plain, 9.0, (0.0, 1.0)).unwrap(), plain, "a plain frame is itself");
    assert!(goofi_codec::decode(&goofi_codec::encode(&goofi_core::Data::functional(b"junk".to_vec(), goofi_core::Meta::default())).unwrap()).is_ok());
    assert!(goofi_node::at(&evaluator, &goofi_core::Data::functional(b"junk".to_vec(), goofi_core::Meta::default()), 0.0, (0.0, 1.0)).is_err(), "code that is not code is a run error, not a panic");
}

/// Compiles, evaluations and a free-threaded collection run on their own threads at once, as a
/// patch's reducers, node threads and machines do. A collection stops the world until every
/// attached thread parks; an evaluator that blocks attached on a lock whose holder waits to
/// attach never parks, and the manager, the machines and every node freeze with it.
#[test]
fn concurrent_compile_eval_and_collection_all_finish() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    const ROUNDS: u64 = 300;
    let evaluator = Arc::new(PyExprEvaluator::new().unwrap());
    let ctx = || EvalCtx { locals: &[], t: 0.5, range: (0.0, 1.0) };
    let done: Arc<Vec<AtomicU64>> = Arc::new((0..9).map(|_| AtomicU64::new(0)).collect());
    for (i, counter) in (0..9).zip(0..) {
        let (ev, done) = (evaluator.clone(), done.clone());
        std::thread::spawn(move || {
            for round in 0..ROUNDS {
                match i {
                    // The collector: every call is a stop-the-world collection.
                    0 => {
                        let code = ev.compile("__import__('gc').collect() * 0 + t").unwrap();
                        assert_eq!(control::number_of(&ev.eval(code.id, &ctx()).unwrap()), 0.5);
                        ev.release(code.id);
                    }
                    // Compilers: a fresh text each round is a compile under the table lock.
                    1..=4 => {
                        let code = ev.compile(&format!("t + {i} * {round}")).unwrap();
                        ev.release(code.id);
                    }
                    // Evaluators: a held function read attached, as a node thread reads it.
                    _ => {
                        let code = ev.compile("t * 2").unwrap();
                        assert_eq!(control::number_of(&ev.eval(code.id, &ctx()).unwrap()), 1.0);
                        ev.release(code.id);
                    }
                }
                done[counter].fetch_add(1, Ordering::Relaxed);
            }
        });
    }
    let deadline = std::time::Instant::now() + goofi_tests::WAIT;
    while done.iter().any(|d| d.load(Ordering::Relaxed) < ROUNDS) {
        let rounds: Vec<u64> = done.iter().map(|d| d.load(Ordering::Relaxed)).collect();
        assert!(std::time::Instant::now() < deadline, "threads stalled at {rounds:?} of {ROUNDS} rounds each");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn qtz_snaps_volts_per_octave_onto_a_scale() {
    let evaluator = PyExprEvaluator::new().unwrap();
    let evaluate = |source: &str| {
        let code = evaluator.compile(source).unwrap();
        let result = evaluator.eval(code.id, &EvalCtx { locals: &[], t: 0.0, range: (-5.0, 5.0) });
        evaluator.release(code.id);
        result
    };
    let semis = |source: &str| control::number_of(&evaluate(source).unwrap()) * 12.0;
    let near = |source: &str, want: f64| {
        let got = semis(source);
        assert!((got - want).abs() < 1e-4, "{source}: {got} semitones, not {want}");
    };
    // C major by default, 0 V the C: a pitch goes to its nearest degree, in its own octave, and
    // one just under the next C climbs to it.
    near("qtz(0)", 0.0);
    near("qtz(1.4 / 12)", 2.0);
    near("qtz(0.23)", 2.0);
    near("qtz(1 + 11.4 / 12)", 23.0);
    near("qtz(11.6 / 12)", 12.0);
    near("qtz(-0.4 / 12)", 0.0);
    near("qtz(-1.6 / 12)", -1.0);
    // A scale by name and a root by note or by semitones: D major pentatonic is D E F# A B.
    near("qtz(5.4 / 12, 'pentatonic_major', 'D')", 6.0);
    near("qtz(5.4 / 12, scale='pentatonic_major', root=2)", 6.0);
    near("qtz(1.4 / 12, 'minor', 'a')", 2.0);
    near("qtz(3.4 / 12, 'chromatic')", 3.0);
    // An array snaps element by element.
    let frame = evaluate("qtz(np.array([0.0, 1.4 / 12, 4.6 / 12]))").unwrap();
    let goofi_core::Value::Array(a) = frame.value() else { panic!("an array: {frame:?}") };
    let got: Vec<f64> = a.values().map(|v| (f64::from(v) * 12.0 * 1e4).round() / 1e4).collect();
    assert_eq!(got, [0.0, 2.0, 5.0]);
    // A scale or root that is none is the expression's error, naming what there is.
    let why = format!("{:?}", evaluate("qtz(0, 'nope')").unwrap_err());
    assert!(why.contains("major") && why.contains("dorian"), "{why}");
    let why = format!("{:?}", evaluate("qtz(0, root='H')").unwrap_err());
    assert!(why.contains("F#"), "{why}");
}
