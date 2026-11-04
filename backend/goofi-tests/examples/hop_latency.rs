//! Hop-latency probe (not a test): `_TestConst(len)` → N × `_TestEcho` on a booted instance,
//! read off the stamps every frame carries. A frame names the input frame it was made from by
//! node, output, index, `procstart` and `procend`, so a probe on every hop joins the chain by
//! index: per hop, the WAIT is the consumer's `procstart` minus the producer's `procend` (the
//! transport wake plus the decode) and the RUN is the consumer's own `procend` minus `procstart`.
//! End to end: the last `procend` minus the source's. The producer's cap is `signal.default_ufreq`,
//! set through the op, which needs the evaluator the binary installs — a bare graph never paces
//! a producer.
//!   cargo run -p goofi-tests --example hop_latency --release
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use goofi_core::{Data, MetaValue};
use goofi_graph::Uid;
use goofi_tests::{j, Goofi, OutputProbe};

/// The input frame a frame was made from, on its one input: the producer's index and the
/// instant the frame left it.
fn made_from(d: &Data) -> Option<(u64, f64)> {
    let source = d.meta().source()?;
    let MetaValue::List(held) = source.values().next()? else { return None };
    let MetaValue::Map(from) = held.first()? else { return None };
    let index = match from.get("index")? {
        MetaValue::Uint(u) => *u,
        MetaValue::Int(i) if *i >= 0 => *i as u64,
        _ => return None,
    };
    let MetaValue::Float(procend) = from.get("procend")? else { return None };
    Some((index, *procend))
}

/// The newest frames one hop emitted, by index, as many as a chain may run behind.
#[derive(Default)]
struct Recent(VecDeque<(u64, Data)>);

impl Recent {
    fn note(&mut self, d: Data) {
        let Some(index) = d.meta().index() else { return };
        if self.0.back().is_some_and(|(i, _)| *i == index) {
            return;
        }
        self.0.push_back((index, d));
        while self.0.len() > 256 {
            self.0.pop_front();
        }
    }

    fn frame(&self, index: u64) -> Option<&Data> {
        self.0.iter().rev().find(|(i, _)| *i == index).map(|(_, d)| d)
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn build(len: i64, hops: usize, cap: f64) -> (Goofi, Uid, Vec<Uid>, Vec<OutputProbe>) {
    let g = Goofi::new();
    g.graph().set_evaluator(std::sync::Arc::new(goofi_python::inproc::PyExprEvaluator::new().expect("the evaluator")));
    g.call("variable entry edit", j!({ "name": "signal.default_ufreq", "value": cap }));
    let src = g.add("_TestConst");
    g.set_param(src, "constant", "length", len);
    let mut nodes = Vec::new();
    let mut probes = Vec::new();
    let mut prev = src;
    for _ in 0..hops {
        let n = g.add("_TestEcho");
        g.link(prev, "out", n, "input");
        probes.push(g.probe(n, "out"));
        nodes.push(n);
        prev = n;
    }
    g.ready(prev);
    probes.last().expect("one hop at least").expect_frame(&mut g.graph(), "the chain to emit");
    std::thread::sleep(Duration::from_secs(1));
    (g, src, nodes, probes)
}

fn index(probe: &OutputProbe) -> u64 {
    probe.latest().and_then(|d| d.meta().index()).unwrap_or(0)
}

fn measure(label: &str, len: i64, hops: usize, cap: f64, window: Duration) {
    let (_g, _src, _nodes, probes) = build(len, hops, cap);
    let last = probes.last().expect("one hop at least");
    let first = index(last);
    let t = Instant::now();
    let mut recent: Vec<Recent> = (0..hops).map(|_| Recent::default()).collect();
    let mut hop_wait: Vec<Vec<f64>> = vec![Vec::new(); hops];
    let mut hop_run: Vec<Vec<f64>> = vec![Vec::new(); hops];
    let mut e2e: Vec<f64> = Vec::new();
    let mut last_seen = None;
    while t.elapsed() < window {
        for (probe, held) in probes.iter().zip(&mut recent) {
            if let Some(d) = probe.latest() {
                held.note(d);
            }
        }
        if let Some(frame) = last.latest() {
            let idx = frame.meta().index();
            if idx != last_seen {
                last_seen = idx;
                // Walk back from the last hop: each frame names the index of the one it was made
                // from, which the hop before it holds among its recent frames.
                let mut at = Some(frame);
                let mut ends: Vec<f64> = Vec::new();
                for h in (0..hops).rev() {
                    let Some(d) = at else { break };
                    let (Some(began), Some(left)) = (d.meta().procstart(), d.meta().procend()) else { break };
                    let Some((from_index, from_left)) = made_from(&d) else { break };
                    ends.push(left);
                    hop_wait[h].push((began - from_left) * 1e6);
                    hop_run[h].push((left - began) * 1e6);
                    at = if h > 0 { recent[h - 1].frame(from_index).cloned() } else { Some(d) };
                    if h == 0 {
                        ends.push(from_left);
                    }
                }
                if ends.len() == hops + 1 {
                    e2e.push((ends[0] - ends[hops]) * 1e6);
                }
            }
        }
        std::thread::sleep(Duration::from_micros(200));
    }
    let emitted = index(last).saturating_sub(first);
    let rate = emitted as f64 / t.elapsed().as_secs_f64();
    e2e.sort_by(f64::total_cmp);
    let p50 = |v: &Vec<f64>| {
        let mut v = v.clone();
        v.sort_by(f64::total_cmp);
        percentile(&v, 0.5)
    };
    let per_hop: Vec<String> = hop_wait.iter().zip(&hop_run).map(|(w, r)| format!("{:.0}+{:.0}", p50(w), p50(r))).collect();
    println!(
        "{label:<36} {rate:>8.0} frames/s   e2e p50 {:>7.0} µs  p90 {:>7.0} µs  p99 {:>7.0} µs  (n={})  hop p50 wait+run: {}",
        percentile(&e2e, 0.5),
        percentile(&e2e, 0.9),
        percentile(&e2e, 0.99),
        e2e.len(),
        per_hop.join(" "),
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
        let (g, src, nodes, _probes) = build(64, n, 30.0);
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
