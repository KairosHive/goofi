//! Where one op's time goes (not a test): the pieces of a `node param edit` on N echo nodes,
//! each timed alone on the held graph — the command, the settle, the projection, the diff — and
//! what the status drain pays on a quiet graph, which it pays on every wake without an op.
//!   cargo run -p goofi-tests --example op_parts --release
use std::time::Instant;

use goofi_core::Param;
use goofi_tests::{hex, j, Goofi};

fn us(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e6
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    for n in [50usize, 200, 500] {
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
        let whole = median(
            (0..7)
                .map(|i| {
                    let t = Instant::now();
                    g.call("node param edit", j!({ "node": hex(src), "param": "constant/value", "value": 0.1 * i as f64 }));
                    us(t)
                })
                .collect(),
        );
        let mut graph = g.graph();
        let t = Instant::now();
        let before = graph.replica();
        let replica = us(t);
        let t = Instant::now();
        let text = before.to_string();
        let to_string = us(t);
        let t = Instant::now();
        graph.update_param(src, "constant", "value", Param::float(0.25, -1e9, 1e9)).unwrap();
        let command = us(t);
        let t = Instant::now();
        graph.settle();
        let settle = us(t);
        let after = graph.replica();
        let t = Instant::now();
        let ops = goofi_bridge::doc::diff_ops(&before, &after);
        let diff = us(t);
        // Nothing touched: what the drain's settle and the drain itself cost on a quiet graph.
        let t = Instant::now();
        graph.settle();
        let idle_settle = us(t);
        let t = Instant::now();
        graph.drain_status();
        let idle_drain = us(t);
        drop(graph);
        println!(
            "{n:>4} nodes: whole op {whole:>9.0} µs | replica {replica:>8.0} µs ({} KB, to_string {to_string:>6.0} µs) | update_param {command:>8.0} µs | settle {settle:>8.0} µs | diff {diff:>8.0} µs ({} ops) | idle settle {idle_settle:>8.0} µs | idle drain {idle_drain:>8.0} µs",
            text.len() / 1024,
            ops.len(),
        );
    }
}
