//! Control-plane cost probe (not a test): what one `node param edit` costs the manager on a
//! patch of N nodes — the settle, the document projection and the broadcast every op pays —
//! which is what a knob drag at 60 ops a second is held to.
//!   cargo run -p goofi-tests --example op_cost --release
use std::time::Instant;

use goofi_tests::{hex, j, Goofi};

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    for n in [10usize, 50, 200, 500] {
        let g = Goofi::new();
        let src = g.add("_TestConst");
        let mut prev = src;
        for i in 0..n {
            let e = g.add("_TestEcho");
            if i % 10 == 0 {
                prev = src;
            }
            g.link(prev, "out", e, "input");
            prev = e;
        }
        g.ready(prev);
        let mut edits: Vec<f64> = Vec::new();
        for i in 0..200u32 {
            let t = Instant::now();
            g.call("node param edit", j!({ "node": hex(src), "param": "constant/value", "value": i as f64 * 0.01 }));
            edits.push(t.elapsed().as_secs_f64() * 1e6);
        }
        edits.sort_by(f64::total_cmp);
        let mut moves: Vec<f64> = Vec::new();
        for i in 0..100u32 {
            let t = Instant::now();
            g.call("node edit", j!({ "node": hex(src), "pos": [i as f64 * 2.0, 0.0] }));
            moves.push(t.elapsed().as_secs_f64() * 1e6);
        }
        moves.sort_by(f64::total_cmp);
        let t = Instant::now();
        let doc = g.doc();
        let doc_us = t.elapsed().as_secs_f64() * 1e6;
        let bytes = doc.to_string().len();
        println!(
            "{n:>4} nodes: param edit p50 {:>8.1} µs p99 {:>8.1} µs   node move p50 {:>8.1} µs p99 {:>8.1} µs   doc {} KB read in {:.0} µs",
            percentile(&edits, 0.5),
            percentile(&edits, 0.99),
            percentile(&moves, 0.5),
            percentile(&moves, 0.99),
            bytes / 1024,
            doc_us,
        );
    }
}
