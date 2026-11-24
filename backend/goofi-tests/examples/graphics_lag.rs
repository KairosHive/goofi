//! Graphics tap-lag probe (not a test): the engine on its own 60 Hz clock, a `/data` viewer on a
//! constant-colour stage, and the time from a colour edit to the frame that shows it — the whole
//! path a browser sees: render, readback, half tick, reducer pace, socket. Then the delivered rate.
//!   cargo run -p goofi-tests --example graphics_lag --release
use std::time::{Duration, Instant};

use goofi_tests::{hex, j, shape, Goofi, Viewer};

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// The red of the top-left texel on a 0..1 scale, whatever width it arrived in.
fn red(d: &goofi_core::Data) -> f32 {
    let goofi_core::Value::Array(a) = d.value() else { return f32::NAN };
    let b = a.as_bytes();
    let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    if v > 1.5 { v / 255.0 } else { v }
}

async fn probe(label: &str, size: i64, spec: serde_json::Value, box_: usize, viewer_fps: f64) {
    let g = Goofi::timed();
    g.call("variable entry edit", j!({ "name": "system.viewer_fps", "value": viewer_fps }));
    let base = g.serve().await;
    let stage = g.add("graphics:Constant");
    g.ready(stage);
    g.set_param(stage, "common", "width", size);
    g.set_param(stage, "common", "height", size);
    g.set_param(stage, "constant", "colour", [0.1, 0.5, 1.0, 1.0]);
    let mut viewer = Viewer::open(&base, &hex(stage), "out").await;
    viewer.view_at(spec, 60.0).await;
    // The declared box, and a warm engine: lavapipe compiles on the first tick.
    let first = viewer.until(|d| shape(d).len() == 3 && shape(d)[0] == box_).await;
    let got = shape(&first);
    for _ in 0..30 {
        viewer.frame().await;
    }
    // Rate and spacing of what the socket delivers.
    let mut arrivals: Vec<f64> = Vec::new();
    let t = Instant::now();
    let mut last = Instant::now();
    while t.elapsed() < Duration::from_secs(3) {
        let _ = viewer.frame().await;
        arrivals.push(last.elapsed().as_secs_f64() * 1e3);
        last = Instant::now();
    }
    arrivals.sort_by(f64::total_cmp);
    let rate = arrivals.len() as f64 / t.elapsed().as_secs_f64();
    // Edit-to-pixel latency: a new red, and the first frame that carries it.
    let mut lat: Vec<f64> = Vec::new();
    for i in 1..=20u32 {
        let want = 0.1 + 0.04 * i as f32;
        let t = Instant::now();
        g.set_param(stage, "constant", "colour", [want, 0.5, 1.0, 1.0]);
        viewer.until(|d| (red(d) - want).abs() < 0.01).await;
        lat.push(t.elapsed().as_secs_f64() * 1e3);
    }
    lat.sort_by(f64::total_cmp);
    let status = g.call("session status", j!({}));
    println!(
        "{label:<22} {size}² -> {:?} cap {viewer_fps:>2}: {rate:>5.1} frames/s  gap p50 {:>5.1} ms p99 {:>6.1} ms  edit->pixel p50 {:>6.1} ms p90 {:>6.1} ms max {:>6.1} ms  tick max {} µs",
        got,
        percentile(&arrivals, 0.5),
        percentile(&arrivals, 0.99),
        percentile(&lat, 0.5),
        percentile(&lat, 0.9),
        percentile(&lat, 1.0),
        status["graphics"]["tick_max_us"],
    );
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let u8_box = |n: usize| j!([{ "dtype": "array", "ndim": [["ge", 2], ["le", 3]], "dims": [],
        "reduce": [{ "dim": 0, "max": n }, { "dim": 1, "max": n }], "aspect": true, "depth": "u8" }]);
    let f32_box = |n: usize| j!([{ "dtype": "array", "ndim": [["ge", 2], ["le", 3]], "dims": [],
        "reduce": [{ "dim": 0, "max": n }, { "dim": 1, "max": n }], "aspect": true }]);
    probe("u8 box 128", 256, u8_box(128), 128, 30.0).await;
    probe("u8 box 128", 256, u8_box(128), 128, 60.0).await;
    probe("u8 box 256 (image)", 512, u8_box(256), 256, 30.0).await;
    probe("u8 box 256 (image)", 512, u8_box(256), 256, 60.0).await;
    probe("f32 box 256", 512, f32_box(256), 256, 60.0).await;
}
