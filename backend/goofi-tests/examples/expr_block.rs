//! Can a Python param expression run once per audio block? Latency of the embedded interpreter
//! over a `[2, 64]` block, alone and under background Python load, against the block budget.
//!   cargo run -p goofi-tests --features embed --example expr_block --release
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use goofi_core::{Data, Meta, Param};
use goofi_node::{EvalCtx, ExprEvaluator, Local};
use goofi_python::inproc::PyExprEvaluator;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

const BLOCK: usize = 64;
const CHANNELS: usize = 2;
const RATE: f64 = 48_000.0;
const ITERS: usize = 20_000;

fn block() -> Data {
    let n = CHANNELS * BLOCK;
    let samples: Vec<f32> = (0..n).map(|i| (i as f32 * 0.01).sin()).collect();
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    Data::array_f32(vec![CHANNELS, BLOCK], bytes, Meta::default()).unwrap()
}

fn flat_block() -> Data {
    let n = CHANNELS * BLOCK;
    let samples: Vec<f32> = (0..n).map(|i| (i as f32 * 0.01).sin()).collect();
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    Data::array_f32(vec![n], bytes, Meta::default()).unwrap()
}

fn report(label: &str, mut us: Vec<f64>) {
    us.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| us[((us.len() as f64 - 1.0) * p) as usize];
    let budget = BLOCK as f64 / RATE * 1e6;
    let over = us.iter().filter(|v| **v > budget).count();
    println!(
        "{label:<46} med {:>7.1} us  p99 {:>7.1}  p99.9 {:>7.1}  max {:>8.1}   over budget {over}/{}",
        q(0.5),
        q(0.99),
        q(0.999),
        us[us.len() - 1],
        us.len()
    );
}

/// The evaluator as it is: a numpy array in, a 128-vector target, a Python list out.
fn through_evaluator(label: &str, ev: &PyExprEvaluator, source: &str, input: &Data, dims: usize) {
    let code = ev.compile(source).unwrap();
    let target = Param::vec(vec![0.0; dims], -1e9, 1e9);
    let mut locals = HashMap::new();
    locals.insert("x".to_string(), Some(Local::Frame(input.clone())));
    let mut us = Vec::with_capacity(ITERS);
    for i in 0..ITERS {
        let t = Instant::now();
        let r = ev.eval(code.id, &EvalCtx { locals: &locals, t: i as f64 / 750.0, target: &target });
        us.push(t.elapsed().as_secs_f64() * 1e6);
        r.unwrap();
    }
    ev.release(code.id);
    report(label, us);
}

/// A direct path: bytes to a numpy view, eval, result `astype(f32).tobytes()` back — the floor a
/// block-rate evaluator would sit on, with no list conversion and no dict rebuild per call.
fn direct(label: &str, source: &str, input: &Data) {
    let bytes: Vec<u8> = match input.value() {
        goofi_core::Value::Array(s) => s.as_bytes().to_vec(),
        _ => unreachable!(),
    };
    let (code, ns, np) = Python::attach(|py| {
        let np = py.import("numpy").unwrap();
        let ns = PyDict::new(py);
        ns.set_item("np", &np).unwrap();
        let builtins = py.import("builtins").unwrap();
        let code = builtins
            .getattr("compile")
            .unwrap()
            .call1((source, "<expr>", "eval"))
            .unwrap();
        (code.unbind(), ns.unbind(), np.unbind())
    });
    let mut us = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let t = Instant::now();
        Python::attach(|py| {
            let np = np.bind(py);
            let arr = np
                .getattr("frombuffer")
                .unwrap()
                .call1((PyBytes::new(py, &bytes), "<f4"))
                .unwrap()
                .call_method1("reshape", ((CHANNELS, BLOCK),))
                .unwrap();
            let ns = ns.bind(py);
            ns.set_item("x", arr).unwrap();
            let builtins = py.import("builtins").unwrap();
            let r = builtins.getattr("eval").unwrap().call1((code.bind(py), ns)).unwrap();
            let out = r.call_method1("astype", ("<f4",)).unwrap().call_method0("tobytes").unwrap();
            let got: &[u8] = out.extract::<&[u8]>().unwrap();
            assert_eq!(got.len(), CHANNELS * BLOCK * 4);
        });
        us.push(t.elapsed().as_secs_f64() * 1e6);
    }
    report(label, us);
}

fn background(stop: Arc<AtomicBool>, kind: &'static str) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        Python::attach(|py| {
            let src = match kind {
                "numpy" => "import numpy as np\nwhile not stop():\n    a = np.random.rand(4096); b = np.fft.rfft(a); c = float(b[3].real)\n",
                "alloc-nogc" => "import gc\ngc.disable()\nwhile not stop():\n    xs = [[i] for i in range(2000)]\n    for x in xs: x.append(x)\n",
                "alloc-high" => "import gc\ngc.set_threshold(200000, 50, 50)\nwhile not stop():\n    xs = [[i] for i in range(2000)]\n    for x in xs: x.append(x)\n",
                _ => "while not stop():\n    xs = [[i] for i in range(2000)]\n    for x in xs: x.append(x)\n",
            };
            let ns = PyDict::new(py);
            let stop2 = stop.clone();
            let f = pyo3::types::PyCFunction::new_closure(py, None, None, move |_args, _kw| stop2.load(Ordering::Relaxed)).unwrap();
            ns.set_item("stop", f).unwrap();
            py.run(&std::ffi::CString::new(src).unwrap(), Some(&ns), None).unwrap();
        });
    })
}

fn main() {
    goofi_python::inproc::configure_embedded();
    let ev = PyExprEvaluator::new().unwrap();
    println!("GIL enabled: {}", goofi_python::inproc::PyNode::gil_enabled().unwrap());
    println!("block budget at {RATE} Hz, {BLOCK} samples: {:.1} us\n", BLOCK as f64 / RATE * 1e6);

    let scalar = Data::array_f32(vec![1], 0.5f32.to_le_bytes().to_vec(), Meta::default()).unwrap();
    let flat = flat_block();
    let blk = block();

    let suite = |tag: &str| {
        println!("-- {tag}");
        through_evaluator("evaluator: scalar `x * 2` -> 1", &ev, "x * 2", &scalar, 1);
        through_evaluator("evaluator: `x * 2` [128] -> list", &ev, "x * 2", &flat, CHANNELS * BLOCK);
        through_evaluator("evaluator: `np.tanh(x) * lfo()` [128]", &ev, "np.tanh(x) * lfo()", &flat, CHANNELS * BLOCK);
        direct("direct: `x * 2` [2,64] bytes->bytes", "x * 2", &blk);
        direct("direct: `np.tanh(x) * 0.5` [2,64]", "np.tanh(x) * 0.5", &blk);
        direct("direct: `x * x[::-1] + 0.1` [2,64]", "x * x[::-1] + 0.1", &blk);
        println!();
    };

    suite("alone");

    let stop = Arc::new(AtomicBool::new(false));
    let bg: Vec<_> = (0..4).map(|_| background(stop.clone(), "numpy")).collect();
    std::thread::sleep(Duration::from_millis(300));
    suite("4 background threads of numpy work");
    stop.store(true, Ordering::Relaxed);
    for h in bg {
        h.join().unwrap();
    }

    for (kind, tag) in [
        ("alloc", "4 background threads of cyclic allocation (GC)"),
        ("alloc-high", "same, gc threshold0 raised to 200000"),
        ("alloc-nogc", "same, gc.disable()"),
    ] {
        let stop = Arc::new(AtomicBool::new(false));
        let bg: Vec<_> = (0..4).map(|_| background(stop.clone(), kind)).collect();
        std::thread::sleep(Duration::from_millis(300));
        suite(tag);
        stop.store(true, Ordering::Relaxed);
        for h in bg {
            h.join().unwrap();
        }
        Python::attach(|py| py.run(c"import gc; gc.enable(); gc.set_threshold(2000, 10, 10)", None, None).unwrap());
    }
}
