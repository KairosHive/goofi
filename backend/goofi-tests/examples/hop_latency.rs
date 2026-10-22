//! Hop-latency probe (not a test): `_TestConst(len)` → N × `_TestEcho` on a booted instance,
//! read off the lineage stamps every frame carries. Per hop: the consumer's run `time` minus the
//! producer's `emit`, which is the transport wake plus the decode. End to end: the last `emit`
//! minus the first. The producer's cap is `signal.default_ufreq`, set through the op, which
//! needs the evaluator the binary installs — a bare graph never paces a producer.
//!   cargo run -p goofi-tests --example hop_latency --release
use std::time::{Duration, Instant};

use goofi_core::MetaValue;
use goofi_graph::Uid;
use goofi_tests::{j, Goofi, OutputProbe};

fn entry(list: &[MetaValue], at: usize) -> Option<&std::collections::BTreeMap<String, MetaValue>> {
    match list.get(at) {
        Some(MetaValue::Map(m)) => Some(m),
        _ => None,
    }
}

fn float(m: &std::collections::BTreeMap<String, MetaValue>, key: &str) -> Option<f64> {
    match m.get(key) {
        Some(MetaValue::Float(f)) => Some(*f),
        _ => None,
    }
}

/// The chain of entries a frame was made from, following each entry's first input.
fn chain(list: &[MetaValue]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(e) = entry(list, at) {
        let (Some(time), Some(emit)) = (float(e, "time"), float(e, "emit")) else { break };
        out.push((time, emit));
        let next = match e.get("inputs") {
            Some(MetaValue::Map(inputs)) => inputs.values().find_map(|v| match v {
                MetaValue::List(l) => match l.first() {
                    Some(MetaValue::Uint(u)) => Some(*u as usize),
                    Some(MetaValue::Int(i)) => Some(*i as usize),
                    _ => None,
                },
                _ => None,
            }),
            _ => None,
        };
        match next {
            Some(n) if n > at => at = n,
            _ => break,
        }
    }
    out
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn build(len: i64, hops: usize, cap: f64) -> (Goofi, Uid, Vec<Uid>, OutputProbe) {
    let g = Goofi::new();
    g.graph().set_evaluator(std::sync::Arc::new(goofi_python::inproc::PyExprEvaluator::new().expect("the evaluator")));
    g.call("variable entry edit", j!({ "name": "signal.default_ufreq", "value": cap }));
    let src = g.add("_TestConst");
    g.set_param(src, "constant", "length", len);
    let mut nodes = Vec::new();
    let mut prev = src;
    for _ in 0..hops {
        let n = g.add("_TestEcho");
        g.link(prev, "out", n, "input");
        nodes.push(n);
        prev = n;
    }
    g.ready(prev);
    let probe = g.probe(prev, "out");
    probe.expect_frame(&mut g.graph(), "the chain to emit");
    std::thread::sleep(Duration::from_secs(1));
    (g, src, nodes, probe)
}

fn measure(label: &str, len: i64, hops: usize, cap: f64, window: Duration) {
    let (_g, _src, _nodes, probe) = build(len, hops, cap);
    let index = |p: &OutputProbe| p.latest().and_then(|d| d.meta().index()).unwrap_or(0);
    let first = index(&probe);
    let t = Instant::now();
    let mut hop_lat: Vec<Vec<f64>> = vec![Vec::new(); hops];
    let mut e2e: Vec<f64> = Vec::new();
    let mut last_seen = None;
    while t.elapsed() < window {
        if let Some(frame) = probe.latest() {
            let idx = frame.meta().index();
            if idx != last_seen {
                last_seen = idx;
                if let Some(list) = frame.meta().source() {
                    let c = chain(list);
                    if c.len() == hops + 1 {
                        for h in 0..hops {
                            hop_lat[h].push((c[h].0 - c[h + 1].1) * 1e6);
                        }
                        e2e.push((c[0].1 - c[hops].1) * 1e6);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_micros(200));
    }
    let emitted = index(&probe).saturating_sub(first);
    let rate = emitted as f64 / t.elapsed().as_secs_f64();
    e2e.sort_by(f64::total_cmp);
    let per_hop: Vec<f64> = hop_lat
        .iter()
        .map(|v| {
            let mut v = v.clone();
            v.sort_by(f64::total_cmp);
            percentile(&v, 0.5)
        })
        .collect();
    println!(
        "{label:<36} {rate:>8.0} frames/s   e2e p50 {:>7.0} µs  p90 {:>7.0} µs  p99 {:>7.0} µs  (n={})  hop p50: {}",
        percentile(&e2e, 0.5),
        percentile(&e2e, 0.9),
        percentile(&e2e, 0.99),
        e2e.len(),
        per_hop.iter().map(|h| format!("{h:.0}")).collect::<Vec<_>>().join(" "),
    );
}

fn main() {
    println!("--- the patch default: signal.default_ufreq = 30 ---");
    measure("len=64, 4 hops, 30 Hz", 64, 4, 30.0, Duration::from_secs(3));
    println!("--- producer capped at 1000 Hz ---");
    for (len, hops) in [(64i64, 1usize), (64, 4), (64, 8), (4096, 8), (65536, 4)] {
        measure(&format!("len={len}, {hops} hops, 1000 Hz"), len, hops, 1000.0, Duration::from_secs(3));
    }
    println!("--- producer uncapped (1e6) ---");
    for (len, hops) in [(64i64, 1usize), (64, 8), (4096, 8), (65536, 1), (65536, 4)] {
        measure(&format!("len={len}, {hops} hops, uncapped"), len, hops, 1e6, Duration::from_secs(3));
    }

    println!("--- settle cost on a quiet graph of N echo nodes, driven directly ---");
    for n in [10usize, 50, 200] {
        let (g, src, nodes, _probe) = build(64, n, 30.0);
        let mut graph = g.graph();
        let t = Instant::now();
        for _ in 0..100 {
            graph.drain_status();
        }
        let quiet = t.elapsed().as_secs_f64() / 100.0 * 1e6;
        let t = Instant::now();
        for i in 0..100 {
            graph.update_param(src, "constant", "value", goofi_core::Param::float(i as f64, -1e9, 1e9)).unwrap();
            graph.settle();
        }
        let edit = t.elapsed().as_secs_f64() / 100.0 * 1e6;
        let t = Instant::now();
        for i in 0..20 {
            graph.restart_node(nodes[i % nodes.len()]).unwrap();
            graph.settle();
        }
        let restart = t.elapsed().as_secs_f64() / 20.0 * 1e6;
        drop(graph);
        println!("{n:>4} nodes: quiet drain+settle {quiet:>8.1} µs   param edit+settle {edit:>8.1} µs   restart+settle {restart:>8.1} µs");
    }
}
