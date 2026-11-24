//! Compile-stall probe (not a test): a stage rendering under the external clock while a heavy
//! shader is registered and compiled. What a render tick costs while the compile runs is what a
//! shader edit costs every other node on the screen.
//!   cargo run -p goofi-tests --example compile_stall --release
use std::time::{Duration, Instant};

use goofi_tests::{j, render, Goofi};

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// A shader whose pipeline takes the driver a while: one `shade` of many dependent lines.
fn heavy(lines: usize) -> String {
    let mut body = String::from("/* goofi\n{ \"doc\": \"heavy to compile\" }\n*/\nfn shade(uv: vec2f) -> vec4f {\n    var c = uv.x;\n");
    for i in 0..lines {
        body.push_str(&format!("    c = sin(c * 1.01{} + uv.y * {}.0) + cos(c * uv.x);\n", i % 10, i % 7));
    }
    body.push_str("    return vec4f(c, c, c, 1.0);\n}\n");
    body
}

fn ticks(g: &Goofi, for_: Duration) -> Vec<f64> {
    let mut out = Vec::new();
    let t = Instant::now();
    while t.elapsed() < for_ {
        let tick = Instant::now();
        render(g, 1);
        out.push(tick.elapsed().as_secs_f64() * 1e3);
    }
    out.sort_by(f64::total_cmp);
    out
}

fn main() {
    let g = Goofi::new();
    let stage = g.add("graphics:Constant");
    g.ready(stage);
    g.set_param(stage, "common", "width", 256);
    g.set_param(stage, "common", "height", 256);
    let probe = g.probe(stage, "out");
    g.until("the stage to render", |g| {
        render(g, 1);
        probe.latest().map(|_| ())
    });
    render(&g, 30);
    let quiet = ticks(&g, Duration::from_secs(2));
    println!("| phase | ticks | p50 ms | p99 ms | max ms |");
    println!("| --- | --- | --- | --- | --- |");
    println!("| quiet | {} | {:.2} | {:.2} | {:.2} |", quiet.len(), percentile(&quiet, 0.5), percentile(&quiet, 0.99), percentile(&quiet, 1.0));
    let dir = g.state.mount().join("nodes_graphics");
    std::fs::create_dir_all(&dir).unwrap();
    for (label, lines) in [("compile 400 lines", 400usize), ("compile 1600 lines", 1600)] {
        std::fs::write(dir.join("Heavy.wgsl"), heavy(lines)).unwrap();
        let asked = Instant::now();
        g.call("library refresh", j!({}));
        let refresh_ms = asked.elapsed().as_secs_f64() * 1e3;
        let during = ticks(&g, Duration::from_secs(4));
        println!(
            "| {label} (refresh took {refresh_ms:.1} ms) | {} | {:.2} | {:.2} | {:.2} |",
            during.len(),
            percentile(&during, 0.5),
            percentile(&during, 0.99),
            percentile(&during, 1.0)
        );
    }
}
