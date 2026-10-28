//! Audio render-cost probe (not a test): N oscillator chains into one output, rendered on the
//! external clock, so what one block costs the clock thread is read against its 1.33 ms budget.
//! Then the control-side holds of the runtime lock that a callback can find taken.
//!   cargo run -p goofi-tests --example audio_cost --release
use std::time::{Duration, Instant};

use goofi_tests::{drive, hex, j, Goofi};

const BLOCK: usize = 64;

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn bench(chains: usize, deep: bool) {
    let g = Goofi::new();
    let out = g.add("AudioOut");
    let mix = g.add("Gain");
    g.link(mix, "out", out, "input");
    for _ in 0..chains {
        let osc = g.add("Osc");
        let mut last = osc;
        if deep {
            for ty in ["audio:Filter", "audio:Delay", "audio:Reverb", "audio:Gain"] {
                let n = g.add(ty);
                g.link(last, "out", n, "input");
                last = n;
            }
        }
        g.link(last, "out", mix, "input");
    }
    // One block per drive, so each sample is the cost of one 64-frame block of the whole plan.
    drive(&g, BLOCK * 50);
    let mut per_block: Vec<f64> = Vec::new();
    for _ in 0..2000 {
        let t = Instant::now();
        drive(&g, BLOCK);
        per_block.push(t.elapsed().as_secs_f64() * 1e6);
    }
    per_block.sort_by(f64::total_cmp);
    let status = g.call("session status", j!({}));
    let nodes = status["audio"]["nodes"].as_array().cloned().unwrap_or_default();
    let worst = nodes
        .iter()
        .max_by(|a, b| a["max_us"].as_f64().unwrap_or(0.0).total_cmp(&b["max_us"].as_f64().unwrap_or(0.0)))
        .map(|n| format!("{} mean {:.1} µs max {:.1} µs", n["type_name"], n["mean_us"].as_f64().unwrap_or(0.0), n["max_us"].as_f64().unwrap_or(0.0)))
        .unwrap_or_default();
    println!(
        "{chains:>3} chains{} ({} nodes): block p50 {:>7.1} µs  p99 {:>7.1} µs  max {:>7.1} µs  of a 1333 µs block; pool wait max {} µs; worst node: {worst}",
        if deep { " x5" } else { "" },
        nodes.len(),
        percentile(&per_block, 0.5),
        percentile(&per_block, 0.99),
        percentile(&per_block, 1.0),
        status["audio"]["pool_wait_max_us"],
    );
    // What authoring costs the clock: the ops that take the runtime lock on the control side.
    let victim = g.add("Osc");
    g.link(victim, "out", mix, "input");
    drive(&g, BLOCK * 10);
    let t = Instant::now();
    g.call("node remove", j!({ "node": hex(victim) }));
    let remove = t.elapsed();
    let t = Instant::now();
    g.call("node param edit", j!({ "node": hex(mix), "param": "gain/gain", "value": 0.5 }));
    let edit = t.elapsed();
    println!("     node remove op {:>7.1} µs   param edit op {:>7.1} µs (the op, an upper bound on the lock hold)", remove.as_secs_f64() * 1e6, edit.as_secs_f64() * 1e6);
    // Give the status drain its moment before the instance goes.
    std::thread::sleep(Duration::from_millis(100));
}

fn main() {
    for chains in [1usize, 8, 32] {
        bench(chains, false);
    }
    for chains in [4usize, 16] {
        bench(chains, true);
    }
}
