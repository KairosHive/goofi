//! `graphics:Noise` folding its octaves: the `fractal` modes, and the one property that says the
//! cascade is doing what it is named for.

use goofi_tests::{f32s, hex, j, Goofi, Uid};

/// One rendered frame from `uid`'s output that is NOT `was`. A param edit reaches the plan at the
/// settle after it, so the first frame back still belongs to the old value — waiting for the
/// picture to change is what makes each reading the one that was asked for.
fn drawn(g: &Goofi, uid: Uid, was: &[f32]) -> Vec<f32> {
    let probe = g.probe(uid, "out");
    g.until("a frame drawn since the edit", |g| {
        if let Some(e) = g.error(uid) {
            panic!("Noise failed instead: {e}");
        }
        goofi_tests::render(g, 1);
        let got = probe.latest().map(|d| f32s(&d))?;
        let moved = was.is_empty() || got.iter().zip(was).any(|(a, b)| (a - b).abs() > 1e-6);
        moved.then_some(got)
    })
}

/// Mean, spread and excess kurtosis of the red channel — the field, before colour.
fn moments(v: &[f32]) -> (f32, f32, f32) {
    let red: Vec<f64> = v.chunks_exact(4).map(|px| px[0] as f64).collect();
    let n = red.len() as f64;
    let mean = red.iter().sum::<f64>() / n;
    let var = red.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let sd = var.sqrt().max(1e-12);
    let kurt = red.iter().map(|x| ((x - mean) / sd).powi(4)).sum::<f64>() / n - 3.0;
    (mean as f32, sd as f32, kurt as f32)
}

#[test]
fn the_cascade_concentrates_the_field_without_dimming_or_flattening_it() {
    // A lognormal cascade is what makes a field MULTIFRACTAL, and the visible signature of that is
    // intermittency: the same brightness and spread, carried by fewer, sharper places. Measured
    // out of band against mfractal's wavelet-leaders estimator, this sweep takes c2 from -0.013 to
    // -0.42, monotonically. c2 needs pywavelets, which goofi does not depend on — kurtosis is the
    // half of the story that can be checked here, and it is the half that moved with c2.
    let g = Goofi::new();
    let nz = g.add("graphics:Noise");
    g.set_param(nz, "common", "width", 256);
    g.set_param(nz, "common", "height", 256);
    g.set_param(nz, "noise", "harmonics", 7);
    g.set_param(nz, "noise", "period", 0.5);
    g.set_param(nz, "move", "speed", 0.0);
    g.set_param(nz, "noise", "fractal", "multifractal");
    g.ready(nz);

    let mut seen: Vec<(f32, f32, f32, f32)> = Vec::new();
    let mut last: Vec<f32> = Vec::new();
    for amount in [0.0f32, 1.0, 2.0] {
        g.set_param(nz, "noise", "amount", amount);
        last = drawn(&g, nz, &last);
        let (mean, sd, kurt) = moments(&last);
        seen.push((amount, mean, sd, kurt));
    }

    // Rising intermittency is the claim, and it is what a flat or a saturated field would fail.
    for pair in seen.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        assert!(hi.3 > lo.3 + 0.5, "kurtosis must climb with `amount`: {seen:?}");
    }
    // …and it must not buy that by going dark, or by collapsing to one grey with a few spikes.
    // Holding the exponent's MEAN at one did exactly that: the spread fell twelvefold.
    for (amount, mean, sd, _) in &seen {
        assert!((mean - 0.5).abs() < 0.05, "the field stays centred at amount {amount}: mean {mean}");
        assert!(*sd > 0.04, "the field keeps its contrast at amount {amount}: spread {sd}");
    }

    // Every other fold renders and is its own picture, not a copy of `fbm`.
    g.set_param(nz, "noise", "amount", 1.0);
    let mut pictures = Vec::new();
    for mode in ["fbm", "ridged", "billow", "warped", "multifractal", "multifractional"] {
        g.set_param(nz, "noise", "fractal", mode);
        let frame = drawn(&g, nz, &last);
        last = frame.clone();
        assert_eq!(frame.len(), 256 * 256 * 4, "{mode} draws the size it was told to");
        let (mean, sd, _) = moments(&frame);
        assert!(sd > 0.01, "{mode} draws a picture, not a flat field (spread {sd}, mean {mean})");
        pictures.push((mode, frame));
    }
    for pair in pictures.windows(2) {
        let differing = pair[0].1.iter().zip(&pair[1].1).filter(|(a, b)| (*a - *b).abs() > 0.02).count();
        assert!(differing > 1000, "`{}` and `{}` are different folds, not the same one", pair[0].0, pair[1].0);
    }
    assert!(g.error(nz).is_none(), "Noise stands with an error");
    g.call("node remove", j!({ "node": hex(nz) }));
}
