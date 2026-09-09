//! Harmonic geometry through real node sessions, including GPU uploads and saved examples.

use std::path::{Path, PathBuf};

use goofi_core::Data;
use goofi_tests::{f32s, install_all, labels, render, require_python, shape, Goofi, OutputProbe, Uid};
#[cfg(feature = "embed")]
use goofi_tests::{drive, j};

const FILES: &[&str] = &[
    "harmonic_morph.py", "harmonic_geometry.py", "geometry_blend.py", "harmonic_transport.py",
    "geometry_metrics.py", "geometry_view.py", "harmonic_modes.py", "harmonic_voices.py",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn install_bundle(g: &Goofi, extra: &[(&str, &str)]) {
    let mut files: Vec<(String, String)> = FILES.iter().map(|name| {
        (name.to_string(), std::fs::read_to_string(root().join("node-bundles/harmonic-geometry").join(name)).unwrap())
    }).collect();
    files.extend(extra.iter().map(|(name, source)| (name.to_string(), source.to_string())));
    let pairs: Vec<_> = files.iter().map(|(name, source)| (name.as_str(), source.as_str())).collect();
    install_all(g, &pairs);
}

fn frame(g: &Goofi, node: Uid, probe: &OutputProbe, mut want: impl FnMut(&Data) -> bool) -> Data {
    g.until("harmonic geometry frame", |g| {
        if let Some(error) = g.error(node).filter(|e| !e.contains("has no data")) {
            panic!("{}: {error}", g.name(&node.to_string()));
        }
        probe.latest().filter(&mut want)
    })
}

fn geometry_kind(data: &Data) -> String {
    let info = data.as_table().unwrap()["info"].as_str().unwrap();
    let info: serde_json::Value = serde_json::from_str(info).unwrap();
    info["metadata"]["method"].as_str().unwrap_or_default().to_string()
}

fn save_frame(name: &str, data: &Data) {
    let dir = root().join("target/harmonic-geometry/frames");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.f32")), data.as_array().unwrap().as_bytes()).unwrap();
    std::fs::write(dir.join(format!("{name}.json")), serde_json::to_vec(&shape(data)).unwrap()).unwrap();
}

#[test]
fn one_harmonic_frame_runs_every_geometry_family_and_its_dashboard() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[]);
    let source = g.add("HarmonicMorph");
    let geometry = g.add("HarmonicGeometry");
    let view = g.add("GeometryView");
    let metrics = g.add("GeometryMetrics");
    let transport = g.add("HarmonicTransport");
    g.link(source, "harmonic", geometry, "input");
    g.link(source, "harmonic", view, "harmonic");
    g.link(geometry, "geometry", view, "input");
    g.link(geometry, "geometry", metrics, "input");
    g.link(metrics, "metrics", view, "metrics");
    let generated = g.probe(geometry, "geometry");
    let dashboard = g.probe(view, "dashboard");
    let measured = g.probe(metrics, "values");
    let methods = [
        "compound", "lateral", "rotary", "trace_3d", "closed_2d", "closed_3d", "pairwise",
        "rose", "epicycloid", "hypocycloid", "tuning_circle", "times_table", "interval_graph",
        "chord_graph", "consonance_polygon", "stern_brocot", "continued_fraction", "farey",
        "subharmonic_tree", "ifs", "lsystem", "recursive_polygon", "pitch_lattice",
        "tube", "knot", "torus", "sphere", "cylinder", "point_cloud", "tree_3d", "polyhedron",
        "plate", "pairwise_plate", "symmetric_plate", "triple_plate", "circular_plate", "elastic",
        "harmonic_field", "quasicrystal", "wave_lattice", "vortex", "sources", "acoustic",
        "faraday", "spherical_field", "spherical_mesh",
    ];
    for method in methods {
        g.set_param(geometry, "geometry", "method", method);
        let geom = frame(&g, geometry, &generated, |d| geometry_kind(d) == method);
        let coordinates = &geom.as_table().unwrap()["coordinates"];
        if let Ok(parts) = coordinates.as_table() {
            assert!(!parts.is_empty(), "{method} returns its parts");
            for points in parts.values() {
                assert!(f32s(points).iter().all(|v| v.is_finite()), "{method} has finite parts");
            }
        } else {
            assert!(!f32s(coordinates).is_empty(), "{method} returns geometry");
            assert!(!f32s(coordinates).iter().any(|v| v.is_infinite()), "{method} has no infinities");
        }
        let image = frame(&g, view, &dashboard, |d| {
            matches!(d.meta().get("geometry_kind"), Some(goofi_core::MetaValue::Str(v)) if v == method)
        });
        assert_eq!(shape(&image), [660, 960, 3]);
        assert!(f32s(&image).iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        let values = frame(&g, metrics, &measured, |d| {
            matches!(d.meta().get("geometry_kind"), Some(goofi_core::MetaValue::Str(v)) if v == method)
        });
        assert_eq!(labels(&values, "dim0").len(), f32s(&values).len());
        save_frame(method, &image);
    }
    let values = frame(&g, metrics, &measured, |d| !f32s(d).is_empty());
    assert_eq!(labels(&values, "dim0").len(), f32s(&values).len());

    for (method, group, name, choices) in [
        ("faraday", "field", "faraday_pattern", vec!["stripe", "square", "hexagonal", "twelve_fold", "random"]),
        ("pairwise_plate", "field", "symmetry", vec!["none", "d4_max", "d4_sum"]),
        ("plate", "field", "strategy", vec!["stern_brocot", "continued_fraction", "rounded", "best_simple"]),
        ("ifs", "structure", "contraction", vec!["ratio_inverse", "log_ratio", "fixed_half"]),
        ("times_table", "structure", "circle_mode", vec!["ratio", "pitch_class", "integer"]),
    ] {
        g.set_param(geometry, "geometry", "method", method);
        for choice in choices {
            g.set_param(geometry, group, name, choice);
            let count = generated.count();
            frame(&g, geometry, &generated, |d| generated.count() > count+2 && geometry_kind(d) == method);
        }
    }

    g.set_param(geometry, "geometry", "method", "circular_plate");
    g.set_param(geometry, "field", "symmetry", "none");
    frame(&g, geometry, &generated, |d| geometry_kind(d) == "circular_plate");
    g.link(geometry, "geometry", transport, "input");
    let transported = g.probe(transport, "geometry");
    let flow = g.probe(transport, "flow");
    for method in ["sand", "particles", "tracer", "streaming"] {
        g.set_param(transport, "transport", "method", method);
        let geom = frame(&g, transport, &transported, |d| geometry_kind(d) == method);
        let kind = geom.as_table().unwrap()["type"].as_str().unwrap();
        assert_eq!(kind, match method { "sand" => "field_2d", "particles" => "point_cloud_2d", _ => "vector_field_2d" });
        if kind == "vector_field_2d" {
            let pixels = frame(&g, transport, &flow, |d| shape(d) == [96, 96, 4]);
            assert!(f32s(&pixels).iter().all(|v| v.is_finite()));
            assert_eq!(f32s(&pixels)[3], 0.0, "the circular domain stays masked");
        }
    }
}

#[test]
fn peak_rows_morph_without_losing_alignment_or_reviving_silence() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[("geometry_peaks.py", include_str!("fixtures/geometry_peaks.py"))]);
    let producer = g.add("GeometryPeaks");
    let morph = g.add("HarmonicMorph");
    let geometry = g.add("HarmonicGeometry");
    let source = g.probe(morph, "harmonic");
    let packed = g.probe(morph, "packed");
    let ratios = g.probe(morph, "tuning");
    let output = g.probe(geometry, "geometry");
    g.set_param(morph, "source", "row", 1);
    g.set_param(morph, "source", "mode_a", "peaks");
    g.set_param(morph, "source", "ratios_b", "1, 3/2");
    g.set_param(morph, "morph", "phase_spread", 0.0);
    g.set_param(morph, "morph", "easing", "linear");
    g.link(producer, "peaks", morph, "a");
    g.link(producer, "amps", morph, "ampsA");
    g.link(producer, "phases", morph, "phasesA");
    g.link(morph, "harmonic", geometry, "input");
    let d = frame(&g, morph, &source, |d| {
        let table = d.as_table().unwrap();
        f32s(&table["ratios"]) == [1.0, 2.5, 4.0]
            && f32s(&table["phases"]).iter().zip([0.1, 0.25, 0.4]).all(|(a, b)| (a-b).abs() < 1e-6)
            && f32s(&table["amplitudes"]).iter().zip([1.0/7.5, 2.5/7.5, 4.0/7.5]).all(|(a, b)| (a-b).abs() < 1e-6)
    });
    let t = d.as_table().unwrap();
    assert!(f32s(&t["phases"]).iter().zip([0.1, 0.25, 0.4]).all(|(a, b)| (a-b).abs() < 1e-6));
    assert!(f32s(&t["amplitudes"]).iter().zip([1.0/7.5, 2.5/7.5, 4.0/7.5]).all(|(a, b)| (a-b).abs() < 1e-6));
    assert!(d.meta().sfreq().is_none(), "a harmonic frame is not sampled at its source waveform's rate");
    let d = frame(&g, morph, &packed, |d| shape(d) == [4, 32] && (f32s(d)[1]-2.5).abs() < 1e-6);
    assert!(f32s(&d).iter().all(|v| v.is_finite()));
    g.set_param(morph, "morph", "mix", 0.5);
    frame(&g, morph, &source, |d| {
        let table = d.as_table().unwrap();
        let r = f32s(&table["ratios"]);
        r.len() == 3 && (r[1]-(2.5f32*1.5).sqrt()).abs() < 1e-5
    });
    g.set_param(morph, "morph", "mix", 1.0);
    frame(&g, morph, &ratios, |d| f32s(d) == [1.0, 1.5]);
    g.set_param(morph, "morph", "mix", 0.0);
    g.set_param(producer, "source", "state", "silence");
    frame(&g, morph, &source, |d| f32s(&d.as_table().unwrap()["amplitudes"]).iter().all(|v| *v == 0.0));
    frame(&g, geometry, &output, |d| f32s(&d.as_table().unwrap()["coordinates"]).is_empty());
    g.set_param(producer, "source", "state", "empty");
    frame(&g, morph, &ratios, |d| f32s(d).is_empty());
    g.set_param(producer, "source", "state", "invalid");
    g.until("invalid peaks to be reported", |g| g.error(morph).filter(|e| e.contains("positive")));
    g.set_param(producer, "source", "state", "short");
    g.until("valid peaks to recover", |g| {
        (g.error(morph).is_none()).then(|| ratios.latest()).flatten().filter(|d| f32s(d) == [1.0, 2.5])
    });
}

#[test]
fn harmonic_shaders_render_the_same_plate_and_keep_their_state() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[]);
    let source = g.add("HarmonicMorph");
    g.set_param(source, "source", "ratios_b", "1, 5/4, 3/2");
    let geometry = g.add("HarmonicGeometry");
    g.set_param(geometry, "geometry", "method", "plate");
    g.set_param(geometry, "geometry", "resolution", 64);
    let modes = g.add("HarmonicModes");
    g.link(source, "harmonic", geometry, "input");
    g.link(source, "harmonic", modes, "input");
    let field = g.probe(geometry, "field");
    let cpu = frame(&g, geometry, &field, |d| shape(d) == [64, 64]);
    let encoded = g.probe(modes, "modes");
    frame(&g, modes, &encoded, |d| shape(d) == [4, 32]);
    let shader = g.add("graphics:HarmonicChladni");
    g.set_param(shader, "common", "width", 64);
    g.set_param(shader, "common", "height", 64);
    g.link(modes, "modes", shader, "modes");
    g.link(source, "packed", shader, "harmonics");
    let gpu = g.probe(shader, "out");
    let image = g.until("GPU plate matches the CPU field", |g| {
        render(g, 1);
        assert!(g.error(shader).is_none(), "{:?}", g.error(shader));
        gpu.latest().filter(|d| shape(d) == [64, 64, 4] && f32s(d)[3] > 0.9)
    });
    let error = f32s(&cpu).iter().zip(f32s(&image).chunks_exact(4)).map(|(a, b)| (a-b[0]).abs()).fold(0.0f32, f32::max);
    assert!(error < 0.003, "GPU cosine modes match Biotuner (max error {error})");
    let ink = g.add("graphics:HarmonicInk");
    g.link(shader, "out", ink, "input");
    let inked = g.probe(ink, "out");
    let knot = g.add("graphics:HarmonicLissajous");
    g.set_param(knot, "common", "width", 128);
    g.set_param(knot, "common", "height", 128);
    g.link(source, "packed", knot, "harmonics");
    let drawn = g.probe(knot, "out");
    for (name, node, probe) in [("gpu-plate", ink, &inked), ("gpu-lissajous", knot, &drawn)] {
        let image = g.until("a nonempty shader image", |g| {
            render(g, 1);
            assert!(g.error(node).is_none(), "{:?}", g.error(node));
            probe.latest().filter(|d| f32s(d).chunks_exact(4).any(|p| p[3] > 0.5 && p[0] > 0.1))
        });
        assert!(f32s(&image).iter().all(|v| v.is_finite()));
        save_frame(name, &image);
    }
    let transport = g.add("HarmonicTransport");
    g.set_param(transport, "transport", "method", "tracer");
    g.link(geometry, "geometry", transport, "input");
    let flow = g.probe(transport, "flow");
    frame(&g, transport, &flow, |d| shape(d) == [64, 64, 4]);
    let dye = g.add("graphics:HarmonicFlow");
    g.set_param(dye, "common", "width", 64);
    g.set_param(dye, "common", "height", 64);
    g.link(transport, "flow", dye, "flow");
    let moving = g.probe(dye, "out");
    let initial = g.until("the seeded ink", |g| {
        render(g, 1);
        assert!(g.error(dye).is_none(), "{:?}", g.error(dye));
        moving.latest().filter(|d| shape(d) == [64, 64, 4] && f32s(d)[3] > 0.9)
    });
    let count = moving.count();
    let changed = g.until("the ink to evolve", |g| {
        render(g, 1);
        moving.latest().filter(|d| moving.count() > count + 10 && f32s(d) != f32s(&initial))
    });
    assert!(f32s(&changed).iter().all(|v| v.is_finite()));
    g.set_param(dye, "motion", "rate", 0.0);
    let count = moving.count();
    let still = g.until("frozen state after pending GPU readbacks", |g| {
        render(g, 1);
        moving.latest().filter(|_| moving.count() > count + 5)
    });
    let count = moving.count();
    let frozen = g.until("frozen frames to arrive", |g| {
        render(g, 1);
        moving.latest().filter(|_| moving.count() > count + 5)
    });
    assert_eq!(f32s(&still), f32s(&frozen), "rate zero freezes the stored state exactly");
    save_frame("gpu-flow", &changed);
}

#[test]
fn endpoint_fields_blend_with_masks_and_recover_from_mismatched_domains() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[]);
    let chord = g.add("HarmonicMorph");
    let a = g.add("HarmonicGeometry");
    let b = g.add("HarmonicGeometry");
    let blend = g.add("GeometryBlend");
    g.set_param(a, "geometry", "method", "circular_plate");
    g.set_param(b, "geometry", "method", "quasicrystal");
    for node in [a, b] { g.link(chord, "harmonic", node, "input"); }
    g.link(a, "geometry", blend, "a");
    g.link(b, "geometry", blend, "b");
    let generated = g.probe(blend, "geometry");
    let field_a = g.probe(a, "field");
    let field_b = g.probe(b, "field");
    let av = f32s(&frame(&g, a, &field_a, |d| shape(d) == [96, 96]));
    let bv = f32s(&frame(&g, b, &field_b, |d| shape(d) == [96, 96]));
    g.until("different physical domains are refused", |g| g.error(blend).filter(|e| e.contains("grid mismatch")));
    g.set_param(blend, "blend", "space", "image");
    for mix in [0.0, 0.5, 1.0] {
        g.set_param(blend, "blend", "mix", mix);
        let d = g.until("registered endpoint field", |g| {
            generated.latest().filter(|d| {
                let table = d.as_table().unwrap();
                let info: serde_json::Value = serde_json::from_str(table["info"].as_str().unwrap()).unwrap();
                g.error(blend).is_none() && info["metadata"]["morph"] == mix
            })
        });
        let table = d.as_table().unwrap();
        let values = f32s(&table["coordinates"]);
        let coverage = f32s(&table["coverage"]);
        assert_eq!(coverage[0], mix as f32);
        for ((a, b), (v, alpha)) in av.iter().zip(&bv).zip(values.iter().zip(&coverage)) {
            if *alpha == 0.0 { assert!(v.is_nan()); }
            else {
                let expected = (1.0-mix as f32)*if a.is_finite() { *a } else { 0.0 } + mix as f32*b;
                assert!((v-expected).abs() < 1e-5);
            }
        }
    }
    // The same cable can change to curves. Resampling fixes the count before the morph.
    g.set_param(a, "geometry", "method", "lateral");
    g.set_param(b, "geometry", "method", "trace_3d");
    let curve = g.until("curve correspondence after fields", |g| {
        generated.latest().filter(|d| g.error(blend).is_none() && d.as_table().unwrap()["type"].as_str().unwrap() == "curve_3d")
    });
    assert_eq!(shape(&curve.as_table().unwrap()["coordinates"]), [512, 3]);
    assert!(f32s(&curve.as_table().unwrap()["coordinates"]).iter().all(|v| v.is_finite()));
}

#[test]
fn phase_wrap_extensions_and_mode_walks_keep_their_endpoint_weights() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[("geometry_peaks.py", include_str!("fixtures/geometry_peaks.py"))]);
    let producer = g.add("GeometryPeaks");
    let morph = g.add("HarmonicMorph");
    g.set_param(producer, "source", "state", "wrap");
    g.set_param(morph, "source", "mode_a", "peaks");
    g.set_param(morph, "source", "mode_b", "peaks");
    g.set_param(morph, "morph", "phase_spread", 0.0);
    g.set_param(morph, "morph", "mix", 0.5);
    for side in ["a", "b"] { g.link(producer, "peaks", morph, side); }
    g.link(producer, "amps", morph, "ampsA");
    g.link(producer, "amps", morph, "ampsB");
    g.link(producer, "phases", morph, "phasesA");
    g.link(producer, "opposite", morph, "phasesB");
    let harmonic = g.probe(morph, "harmonic");
    let base = frame(&g, morph, &harmonic, |d| {
        f32s(&d.as_table().unwrap()["phases"]).iter().all(|v| (v.abs()-std::f32::consts::PI).abs() < 1e-5)
    });
    let tuning = g.probe(morph, "tuning");
    let amplitudes = g.probe(morph, "amps");
    for kind in ["harmonics", "subharmonics"] {
        g.set_param(morph, "extension", "amount", 0.0);
        g.set_param(morph, "extension", "kind", kind);
        let count = harmonic.count();
        g.until("an extended frame at its zero endpoint", |_| (harmonic.count() > count+2).then_some(()));
        let r = f32s(&tuning.latest().unwrap());
        assert_eq!(r, [1.0, 2.0, 3.0], "zero extension retains the original frequencies");
        let weights = f32s(&amplitudes.latest().unwrap());
        assert!(weights.iter().zip(f32s(&base.as_table().unwrap()["amplitudes"])).all(|(a, b)| (a-b).abs() < 1e-6));
        g.set_param(morph, "extension", "amount", 1.0);
        let extended = frame(&g, morph, &tuning, |d| f32s(d).len() > 3);
        let r = f32s(&extended);
        assert!(r.windows(2).all(|p| p[0] < p[1]), "coincident extension tones are consolidated");
        assert!(if kind == "harmonics" { r.last().unwrap() > &3.0 } else { r[0] < 1.0 });
    }
    // A new subharmonic can be near an existing tone without being that tone.
    g.set_param(morph, "extension", "amount", 0.0);
    g.set_param(producer, "source", "state", "near");
    let near = frame(&g, morph, &tuning, |d| {
        let r = f32s(d);
        r.len() == 2 && r[0] == 1.0 && r[1] > 2.0 && r[1] < 2.00001
    });
    assert_eq!(f32s(&near).len(), 2, "zero growth does not activate a nearby subharmonic");
    g.set_param(morph, "extension", "amount", 0.5);
    let growing = frame(&g, morph, &harmonic, |d| {
        let table = d.as_table().unwrap();
        let weights = f32s(&table["amplitudes"]);
        weights.iter().filter(|v| **v > 0.0).count() > 2
            && f32s(&table["ratios"]).iter().any(|v| *v > 2.0 && *v < 2.00001)
    });
    assert!(f32s(&growing.as_table().unwrap()["phases"]).iter()
        .all(|v| (v.abs()-std::f32::consts::PI).abs() < 1e-5), "extension phases stay at the wrap boundary");
    // Shared voice columns keep a silent source silent, including an enabled extension.
    let voices = g.add("HarmonicVoices");
    let gains = g.probe(voices, "gain");
    g.link(morph, "harmonic", voices, "input");
    g.set_param(producer, "source", "state", "silence");
    frame(&g, voices, &gains, |d| shape(d) == [8, 1] && f32s(d).iter().all(|v| *v == 0.0));

    let a = g.add("HarmonicMorph");
    let b = g.add("HarmonicMorph");
    g.set_param(a, "source", "ratios_b", "1, 5/4, 3/2");
    g.set_param(b, "source", "ratios_a", "1, 7/5, 8/5");
    g.set_param(b, "source", "ratios_b", "1, 7/5, 8/5");
    let mapped_a = g.add("HarmonicModes");
    let mapped_b = g.add("HarmonicModes");
    let walk = g.add("HarmonicModes");
    g.link(a, "harmonic", mapped_a, "input");
    g.link(b, "harmonic", mapped_b, "input");
    g.link(a, "harmonic", walk, "input");
    g.link(b, "harmonic", walk, "target");
    for node in [a, b, mapped_a, mapped_b, walk] { g.ready(node); }
    let pa = g.probe(mapped_a, "modes");
    let pb = g.probe(mapped_b, "modes");
    let pm = g.probe(walk, "modes");
    let av = f32s(&frame(&g, mapped_a, &pa, |d| shape(d) == [4, 32]));
    let bv = f32s(&frame(&g, mapped_b, &pb, |d| shape(d) == [4, 32]));
    for mix in [0.0, 0.5, 1.0] {
        g.set_param(walk, "modes", "mix", mix);
        frame(&g, walk, &pm, |d| f32s(d)[..96].iter().zip(av[..96].iter().zip(&bv[..96]))
            .all(|(v, (a, b))| (v-((1.0-mix as f32)*a+mix as f32*b)).abs() < 1e-6));
    }
}

#[test]
#[cfg(feature = "embed")]
fn cookbook_archives_open_with_live_controls_and_sound_without_devices() {
    let _py = require_python();
    let g = Goofi::new();
    g.state.graph.lock().unwrap().set_evaluator(std::sync::Arc::new(
        goofi_python::inproc::PyExprEvaluator::new().expect("the same evaluator as the CLI")));
    let recipes: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root().join("examples/harmonic-geometry/recipes.json")).unwrap()).unwrap();
    let mut identities = std::collections::HashSet::new();
    for recipe in recipes.as_array().unwrap() {
        let file = recipe["file"].as_str().unwrap();
        g.call("session load", j!({"path": root().join("examples/harmonic-geometry").join(file).to_string_lossy()}));
        let doc = g.doc();
        for uid in doc["nodes"].as_object().unwrap().keys() {
            assert!(identities.insert(uid.clone()), "{file} reuses a node identity from another recipe");
        }
        let lookup = |name: &str| {
            let (uid, _) = doc["nodes"].as_object().unwrap().iter().find(|(_, n)| n["name"] == name).unwrap();
            Uid::from_hex(uid).unwrap()
        };
        assert_eq!(doc["nodes"].as_object().unwrap().len(), recipe["nodes"].as_u64().unwrap() as usize);
        for (i, view) in recipe["views"].as_array().unwrap().iter().enumerate() {
            let node = lookup(view[0].as_str().unwrap());
            let probe = g.probe(node, view[1].as_str().unwrap());
            let image = g.until(&format!("{file} viewer {}", view[0]), |g| {
                render(g, 1);
                if let Some(error) = g.error(node).filter(|e| !e.contains("has no data")) {
                    panic!("{file} {}: {error}", view[0]);
                }
                probe.latest().filter(|d| {
                    let values = f32s(d);
                    let dimensions = shape(d);
                    if dimensions.len() == 3 && matches!(dimensions[2], 3 | 4) {
                        let channels = dimensions[2];
                        let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
                        let mut visible = channels == 3;
                        for pixel in values.chunks_exact(channels) {
                            low = low.min(pixel[0]);
                            high = high.max(pixel[0]);
                            visible |= channels == 4 && pixel[3] > 0.1;
                        }
                        visible && high-low > 0.03 && values.iter().all(|v| v.is_finite())
                    } else {
                        values.iter().any(|v| v.is_finite() && *v > 0.01)
                    }
                })
            });
            if i == 0 { save_frame(file.trim_end_matches(".gfi"), &image); }
        }
        g.call("global entry edit", j!({"name": "geometry.auto", "value": false}));
        g.call("global entry edit", j!({"name": "geometry.mix", "value": 0.75}));
        let slow = g.probe(lookup("slow"), "out");
        g.until("manual mix after control bindings settle", |g| {
            slow.latest().filter(|d| g.error(lookup("slow")).is_none() && (f32s(d)[0]-0.75).abs() < 1e-6)
        });
        if file.starts_with("08-") {
            let voices = lookup("voices");
            let gain = g.probe(voices, "gain");
            let gains = frame(&g, voices, &gain, |d| shape(d) == [8, 1]);
            assert!(f32s(&gains).iter().sum::<f32>() <= 0.15001);
            let mix = g.probe(lookup("mixdown"), "out");
            g.until("native harmonic voices reach the mix", |g| {
                drive(g, 256);
                mix.latest().filter(|d| f32s(d).iter().any(|v| v.abs() > 0.001))
            });
            g.call("global entry edit", j!({"name": "geometry.level", "value": 0.0}));
            frame(&g, voices, &gain, |d| f32s(d).iter().all(|v| *v == 0.0));
            g.until("muted native harmonic voices", |g| {
                drive(g, 256);
                mix.latest().filter(|d| f32s(d).iter().all(|v| *v == 0.0))
            });
        }
        let info = g.call("session status", j!({}));
        assert_eq!(info["errors"], j!([]), "{file}: {info}");
        let copy = root().join("target/harmonic-geometry").join(file);
        g.call("session save", j!({"path": copy.to_string_lossy()}));
        g.call("session load", j!({"path": copy.to_string_lossy()}));
        assert_eq!(g.doc()["nodes"].as_object().unwrap().len(), recipe["nodes"].as_u64().unwrap() as usize);
    }
}
