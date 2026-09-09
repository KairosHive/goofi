//! Parameter modulation through the public expression interface.
#![cfg(feature = "embed")]

use std::collections::HashMap;
use goofi_core::Param;
use goofi_node::{EvalCtx, ExprEvaluator};
use goofi_python::inproc::PyExprEvaluator;

#[test]
fn modulation_uses_the_current_target_range_and_coordinate() {
    let evaluator = PyExprEvaluator::new().unwrap();
    let locals = HashMap::new();
    let evaluate = |source: &str, t: f64, lo: f64, hi: f64| {
        let target = Param::Float { value: 0.0, vmin: lo, vmax: hi };
        let code = evaluator.compile(source).unwrap();
        let result = evaluator.eval(code.id, &EvalCtx { locals: &locals, t, target: &target });
        evaluator.release(code.id);
        result.map(|p| match p {
            Param::Float { value, .. } => value,
            _ => panic!("expected a float"),
        })
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
        assert!((value - (2.0 + 4.0 * unit)).abs() < 1e-10);
    }
    assert_ne!(evaluate("noi()", 0.0, 0.0, 1.0).unwrap(), evaluate("noi()", 1.0, 0.0, 1.0).unwrap());
    let left = evaluate("noi()", 1.0 - 1e-6, 0.0, 1.0).unwrap();
    let right = evaluate("noi()", 1.0 + 1e-6, 0.0, 1.0).unwrap();
    assert!((left - right).abs() < 1e-9);
    let code = evaluator.compile("lfo()").unwrap();
    for (lo, hi) in [(2, 6), (-10, 20)] {
        let target = Param::Int { value: 0, vmin: lo, vmax: hi };
        let result = evaluator.eval(code.id, &EvalCtx { locals: &locals, t: 0.25, target: &target }).unwrap();
        assert!(matches!(result, Param::Int { value, .. } if value == hi));
    }
    evaluator.release(code.id);
    for source in ["lfo(1)", "noi(1)", "lfo(rate=1)", "noi(rate=1)"] {
        assert!(evaluate(source, 0.0, 0.0, 1.0).is_err(), "{source}");
    }
}
