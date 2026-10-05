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
        let value = evaluate("noi()", t, 2.0, 6.0).unwrap();
        assert!((2.0..=6.0).contains(&value));
        assert_eq!(value, evaluate(&format!("noi(src={t})"), 999.0, 2.0, 6.0).unwrap());
        let unit = evaluate("noi(vmin=0, vmax=1)", t, 2.0, 6.0).unwrap();
        // The result crosses the f32 carrier, so the two readings agree to f32 precision.
        assert!((value - (2.0 + 4.0 * unit)).abs() < 1e-5);
    }
    assert_ne!(evaluate("noi()", 0.0, 0.0, 1.0).unwrap(), evaluate("noi()", 1.0, 0.0, 1.0).unwrap());
    let left = evaluate("noi()", 1.0 - 1e-6, 0.0, 1.0).unwrap();
    let right = evaluate("noi()", 1.0 + 1e-6, 0.0, 1.0).unwrap();
    assert!((left - right).abs() < 1e-9);
    for kind in ["lfo", "noi"] {
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
    for source in ["lfo(1)", "noi(1)", "lfo(rate=1)", "noi(rate=1)"] {
        assert!(evaluate(source, 0.0, 0.0, 1.0).is_err(), "{source}");
    }

    // Every caller's time() reads the supplied patch time, including a restarted origin.
    for t in [0.0, 0.25, 10.0, 0.0] {
        assert_eq!(evaluate("time()", t, 0.0, 1.0).unwrap(), t);
        assert_eq!(evaluate("time() - t", t, 0.0, 1.0).unwrap(), 0.0);
    }
    // Machines compile through the same evaluator with pure capabilities. Observation metadata
    // comes from that compile, so explicit helper coordinates do not add a hidden clock.
    for (source, observes_time) in [
        ("time() + sin(t)", true),
        ("lfo() + noi()", true),
        ("lfo(src=None)", true),
        ("lfo(src=(None if __v0[0] else 1))", true),
        ("lfo(src=0.25) + noi(src=2)", false),
        ("np.mean(np.array([2, 4]))", false),
        ("np.array([2, 4]).mean()", false),
        ("1 if 'time()' == 'time()' else 0", false),
        ("  sin(pi / 2) # a trailing comment", false),
    ] {
        let code = evaluator.compile_deterministic(source).unwrap();
        assert_eq!(code.observes_time, observes_time, "{source}");
        let locals = [("__v0".into(), Local::Frame(goofi_core::Data::number(1.0)))];
        let context = EvalCtx { locals: &locals, t: 0.25, range: (0.0, 1.0) };
        let first = evaluator.eval(code.compiled.id, &context).unwrap();
        assert_eq!(first, evaluator.eval(code.compiled.id, &context).unwrap(), "{source} repeats at the same logical instant");
        evaluator.release(code.compiled.id);
    }
    for source in [
        "np.random.random()",
        "np.random.default_rng(0).random()",
        "__import__('time').time()",
        "np.empty(4)",
        "[time() for _ in [1]]",
        "str(sin)",
        "np.sin([1], [0])",
        "np.clip([1], 0, 1, out=np.array([0]))",
        "np.add([1], [2], where=False)",
    ] {
        let error = evaluator.compile_deterministic(source).err().expect(source);
        assert!(error.0.contains("deterministic expression"), "{source}: {error}");
    }
    let shared = std::sync::Arc::new(PyExprEvaluator::new().unwrap());
    let code = shared.compile("time() + 1").unwrap();
    let retained = goofi_node::RetainedExpression::new(shared.clone(), code.id).unwrap();
    assert!(retained.observes_time);
    shared.release(code.id);
    assert_eq!(control::number_of(&retained.eval(&EvalCtx { locals: &[], t: 0.25, range: (0.0, 1.0) }).unwrap()), 1.25, "the read plan retains the exact existing function after its authored owner releases it");
    drop(retained);
    assert!(shared.eval(code.id, &EvalCtx { locals: &[], t: 0.25, range: (0.0, 1.0) }).is_err(), "the last owner releases the function");

    // One sampled input remains a vector at each instant. A reduction sums channels, never
    // neighbouring times, and a conditional or an index has the ordinary scalar-call meaning.
    use goofi_core::samples::{SampleClock, SampleSpan, CONTROL_DELAY};
    use goofi_node::{samples::eval_frame, Local};
    let clock = SampleClock { patch_epoch: 7, epoch: 3, origin: 0, first: 0, rate: 1000 };
    let span = SampleSpan { clock, first: 20, length: 3 };
    let sampled = |span: SampleSpan, channels: usize, values: &[f32]| {
        goofi_core::Data::array_f32(vec![channels, span.length],
            values.iter().flat_map(|v| v.to_le_bytes()).collect(), span.meta()).unwrap()
    };
    let input = sampled(span, 2, &[1.0, 2.0, 3.0, 10.0, 20.0, 30.0]);
    let source = clock.at(span.first).saturating_sub(CONTROL_DELAY);
    assert_eq!(goofi_node::samples::at(&input, source).unwrap(), Some(goofi_core::Data::numbers([1.0, 10.0])));
    assert_eq!(goofi_node::samples::at(&input, source + goofi_core::time::ticks(0.0005).unwrap()).unwrap(), Some(goofi_core::Data::numbers([1.0, 10.0])), "a sub-sample instant takes the effective column, not the next one");
    assert_eq!(goofi_node::samples::at(&input, source - 1).unwrap(), None);
    let future = sampled(SampleSpan { clock: SampleClock { origin: goofi_core::time::ticks(1.0).unwrap(), ..clock }, first: 0, length: 1 }, 1, &[9.0]);
    assert_eq!(goofi_node::samples::at(&future, 0).unwrap(), None, "sample zero cannot hide a future clock origin");
    let locals = vec![("__v0".into(), Local::Frame(input.clone()))];
    let context = EvalCtx { locals: &locals, t: 999.0, range: (0.0, 1.0) };
    for (source, expected) in [
        ("np.sum(__v0)", vec![11.0, 22.0, 33.0]),
        ("__v0[1]", vec![10.0, 20.0, 30.0]),
        ("__v0 * 2", vec![2.0, 4.0, 6.0, 20.0, 40.0, 60.0]),
        ("1 if __v0[0] >= 2 else 0", vec![0.0, 1.0, 1.0]),
        ("time() - t", vec![0.0, 0.0, 0.0]),
    ] {
        let code = evaluator.compile_deterministic(source).unwrap();
        let result = eval_frame(&evaluator, code.compiled.id, &context).unwrap();
        assert_eq!(SampleSpan::of(&result), Some(span), "{source} retains its grid");
        let goofi_core::Value::Array(array) = result.value() else { panic!("numeric sampled result") };
        assert_eq!(array.values().collect::<Vec<_>>(), expected, "{source}");
        evaluator.release(code.compiled.id);
    }
    let code = evaluator.compile("t").unwrap();
    let result = eval_frame(&evaluator, code.id, &context).unwrap();
    let expected = sampled(span, 1, &(0..span.length).map(|i|
        goofi_core::time::seconds(clock.at(span.first + i as u64) - CONTROL_DELAY) as f32).collect::<Vec<_>>());
    assert_eq!(result, expected, "the supplied wall wake time does not move the sample coordinates");
    evaluator.release(code.id);
    let fixture = goofi_tests::FirstVar::default();
    let fixture_code = fixture.compile("__v0").unwrap();
    assert_eq!(eval_frame(&fixture, fixture_code.id, &context).unwrap(), input,
        "the default pointwise implementation carries every channel and sample");
    fixture.release(fixture_code.id);

    let other_span = SampleSpan { first: 21, ..span };
    let other = sampled(other_span, 1, &[100.0, 200.0, 300.0]);
    let intersected = vec![("__v0".into(), Local::Frame(input.clone())), ("__v1".into(), Local::Frame(other))];
    let code = evaluator.compile("__v0[1] + __v1[0]").unwrap();
    let result = eval_frame(&evaluator, code.id, &EvalCtx { locals: &intersected, t: 999.0, range: (0.0, 1.0) }).unwrap();
    assert_eq!(result, sampled(SampleSpan { first: 21, length: 2, ..span }, 1, &[120.0, 230.0]),
        "overlapping sources combine at their common absolute samples");
    evaluator.release(code.id);
    let mixed = vec![("__v0".into(), Local::Frame(input)), ("__v1".into(), Local::Frame(goofi_core::Data::numbers([5.0, 7.0])))];
    let code = evaluator.compile("np.sum(__v0) + np.sum(__v1)").unwrap();
    assert_eq!(eval_frame(&evaluator, code.id, &EvalCtx { locals: &mixed, t: 999.0, range: (0.0, 1.0) }).unwrap(),
        sampled(span, 1, &[23.0, 34.0, 45.0]), "untimed vectors keep their existing meaning");
    evaluator.release(code.id);
    for other_span in [
        SampleSpan { first: 50, ..span },
        SampleSpan { clock: SampleClock { epoch: 4, ..clock }, ..span },
    ] {
        let incompatible = vec![locals[0].clone(), ("__v1".into(), Local::Frame(sampled(other_span, 1, &[1.0, 2.0, 3.0])))];
        let code = evaluator.compile("__v0 + __v1").unwrap();
        assert!(eval_frame(&evaluator, code.id, &EvalCtx { locals: &incompatible, t: 0.0, range: (0.0, 1.0) }).is_err());
        evaluator.release(code.id);
    }
}
