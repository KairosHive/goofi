//! Harmonic geometry through real node sessions, including GPU uploads and saved examples.

use std::path::{Path, PathBuf};
use std::collections::BTreeMap;

use goofi_core::Data;
use goofi_tests::{f32s, install_all, labels, render, require_python, shape, Goofi, OutputProbe, Uid};
#[cfg(feature = "embed")]
use goofi_tests::{drive, j};

const FILES: &[&str] = &[
    "harmonic_morph.py", "harmonic_geometry.py", "geometry_blend.py", "harmonic_transport.py",
    "geometry_metrics.py", "geometry_view.py", "harmonic_modes.py", "harmonic_voices.py", "ratio_sequence.py",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn array(values: Vec<f32>, dims: Vec<usize>) -> Data {
    Data::array_f32(dims, values.iter().flat_map(|v| v.to_le_bytes()).collect(), Default::default()).unwrap()
}

// Inspect the wire independently of the Python encoder. These are test views,
// not extra node outputs or another application data path.
fn inspect_frame(d: &Data) -> BTreeMap<String, Data> {
    let shape = shape(d);
    let values = f32s(d);
    let mut result = BTreeMap::new();
    if shape.len() == 2 && shape[0] == 4 {
        for (i, name) in ["ratios", "amplitudes", "phases", "damping"].iter().enumerate() {
            result.insert(name.to_string(), array(values[i*shape[1]..(i+1)*shape[1]].to_vec(), vec![shape[1]]));
        }
    } else {
        assert_eq!(&shape[1..], &[256, 4]);
        let h: Vec<_> = values[..32].chunks_exact(2).map(|v| (v[0]*1024.0+v[1]) as usize).collect();
        assert_eq!(h[0], 7319);
        let names = ["curve_2d", "curve_3d", "point_cloud_2d", "point_cloud_3d", "polygon", "curve_set_2d", "curve_set_3d", "polygon_set", "graph", "tree", "mesh_3d", "field_2d", "vector_field_2d"];
        result.insert("type".into(), Data::string(names[h[1]], Default::default()));
        let coords: Vec<_> = values[32..32+h[3]*4].chunks_exact(4).flat_map(|v| v[..h[2]].iter().copied()).collect();
        let dims = if h[1] == 11 { vec![h[4], h[5]] } else if h[1] == 12 { vec![h[4], h[5], h[2]] } else { vec![h[3], h[2]] };
        result.insert("coordinates".into(), array(coords, dims));
        if h[1] >= 11 {
            result.insert("coverage".into(), array(values[32..32+h[3]*4].chunks_exact(4).map(|v| v[3]).collect(), vec![h[4], h[5]]));
        }
        if let Some(goofi_core::MetaValue::Str(info)) = d.meta().get("info") {
            result.insert("info".into(), Data::string(info.as_str(), Default::default()));
        }
    }
    result
}

fn active_row(d: &Data, row: usize) -> Vec<f32> {
    let values = f32s(d);
    let count = shape(d)[1];
    (0..count).filter(|i| values[count+i] > 0.0).map(|i| values[row*count+i]).collect()
}

fn sequence_ratios(d: &Data) -> Vec<f32> {
    let values = f32s(d);
    let count = shape(d)[1];
    let t = match d.meta().get("mix") { Some(goofi_core::MetaValue::Float(v)) => *v as f32, other => panic!("mix: {other:?}") };
    (0..count).map(|i| ((1.0-t)*values[i].ln()+t*values[count+i].ln()).exp()).collect()
}

fn install_bundle(g: &Goofi, extra: &[(&str, &str)]) {
    let mut files: Vec<(String, String)> = FILES.iter().map(|name| {
        let bundle = if matches!(*name, "harmonic_morph.py" | "harmonic_voices.py" | "ratio_sequence.py") { "biotuner" } else { "harmonic-geometry" };
        (name.to_string(), std::fs::read_to_string(root().join("node-bundles").join(bundle).join(name)).unwrap())
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
    let inspected = inspect_frame(data);
    let info = inspected["info"].as_str().unwrap();
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
fn geometry_array_preserves_indices_parts_masks_and_rejects_bad_connectivity() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[("geometry_array_fixture.py", include_str!("fixtures/geometry_array_fixture.py")),
        ("GeometryField.wgsl", include_str!("fixtures/geometry_field.wgsl"))]);
    let source = g.add("GeometryArrayFixture");
    let renderer = g.add("graphics:GeometryRender");
    let view = g.add("GeometryView");
    g.set_param(renderer, "common", "width", 64);
    g.set_param(renderer, "common", "height", 64);
    g.set_param(renderer, "view", "radius", 1.0);
    g.link(source, "geometry", renderer, "geometry");
    g.link(source, "geometry", view, "input");
    let encoded = g.probe(source, "geometry");
    let pixels = g.probe(renderer, "out");
    let capture = |method: &str| {
        g.set_param(source, "fixture", "kind", method);
        frame(&g, source, &encoded, |d| geometry_kind(d) == method);
        let before = pixels.count();
        g.until("new indexed geometry reaches the GPU", |g| {
            render(g, 1);
            assert!(g.error(renderer).is_none(), "{:?}", g.error(renderer));
            pixels.latest().filter(|_| pixels.count() > before+2)
        })
    };
    let mesh = f32s(&capture("mesh"));
    assert!(mesh[(32*64+32)*4+3] > 0.9, "indices 4095, 4096 and 4097 remain distinct after upload");
    let parts = f32s(&capture("parts"));
    assert!(parts.chunks_exact(4).any(|p| p[3] > 0.5));
    assert_eq!(parts[(32*64+32)*4+3], 0.0, "separate curves do not acquire a joining segment");
    capture("invalid");
    g.until("invalid topology is rejected at the CPU boundary", |g| g.error(view).filter(|e| e.contains("geometry integer digits")));
    capture("mesh");
    g.until("valid topology recovers", |g| g.error(view).is_none().then_some(()));
    let field = g.add("graphics:GeometryField");
    g.link(source, "geometry", field, "geometry");
    let output = g.probe(field, "out");
    g.set_param(source, "fixture", "kind", "field");
    frame(&g, source, &encoded, |d| geometry_kind(d) == "field");
    let image = g.until("the field arrives with its mask", |g| {
        render(g, 1);
        output.latest().filter(|d| f32s(d)[(32*64+48)*4+3] > 0.9)
    });
    let values = f32s(&image);
    assert_eq!(values[3], 0.0);
    assert!(values.iter().all(|v| v.is_finite()));
    assert!((values[(32*64+48)*4]-16.0/63.0).abs() < 0.001);
}

#[test]
fn gpu_geometry_renders_curves_graphs_points_and_meshes() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[("GeometryRender.wgsl", include_str!("../../../node-bundles/harmonic-geometry/GeometryRender.wgsl"))]);
    let source = g.add("HarmonicMorph");
    let geometry = g.add("HarmonicGeometry");
    let renderer = g.add("graphics:GeometryRender");
    g.set_param(geometry, "geometry", "points", 64);
    g.set_param(geometry, "geometry", "resolution", 16);
    g.set_param(renderer, "common", "width", 64);
    g.set_param(renderer, "common", "height", 48);
    g.link(source, "harmonic", geometry, "input");
    g.link(geometry, "geometry", renderer, "geometry");
    let data = g.probe(geometry, "geometry");
    let output = g.probe(renderer, "out");
    for method in ["closed_2d", "trace_3d", "interval_graph", "point_cloud", "torus"] {
        g.set_param(geometry, "geometry", "method", method);
        frame(&g, geometry, &data, |d| geometry_kind(d) == method);
        let before = output.count();
        let image = g.until("GPU geometry has visible finite pixels", |g| {
            render(g, 1);
            assert!(g.error(renderer).is_none(), "{:?}", g.error(renderer));
            output.latest().filter(|d| output.count() > before+1 && shape(d) == [48, 64, 4] && f32s(d).chunks_exact(4).any(|px| px[3] > 0.5))
        });
        assert!(f32s(&image).iter().all(|v| v.is_finite()));
        save_frame(&format!("gpu-geometry-{method}"), &image);
        if method == "torus" {
            let surface = f32s(&image);
            g.set_param(renderer, "ink", "style", "wireframe");
            g.until("wireframe changes the mesh rendering", |g| {
                render(g, 1);
                assert!(g.error(renderer).is_none(), "{:?}", g.error(renderer));
                output.latest().filter(|d| {
                    let pixels = f32s(d);
                    pixels.iter().all(|v| v.is_finite()) && pixels.iter().zip(&surface).any(|(a, b)| (a-b).abs() > 0.05)
                })
            });
        }
    }
}

#[test]
#[cfg(feature = "embed")]
fn global_string_selection_reaches_the_shader_as_its_option_index() {
    let _py = require_python();
    let g = Goofi::new();
    g.state.graph.lock().unwrap().set_evaluator(std::sync::Arc::new(
        goofi_python::inproc::PyExprEvaluator::new().expect("the same evaluator as the CLI")));
    install_all(&g, &[("EnumColor.wgsl", include_str!("fixtures/enum_color.wgsl"))]);
    let node = g.add("graphics:EnumColor");
    g.call("global entry add", serde_json::json!({"name": "surface.finish", "type": "string", "value": "green"}));
    let bound = g.call("node param edit", serde_json::json!({"node": node.to_string(), "param": "color/choice", "expression": "globals.surface.finish"}));
    assert!(bound["error"].is_null(), "{bound}");
    let output = g.probe(node, "out");
    for (choice, expected) in [("green", [0.0, 1.0, 0.0, 1.0]), ("blue", [0.0, 0.0, 1.0, 1.0]), ("red", [1.0, 0.0, 0.0, 1.0])] {
        g.call("global entry edit", serde_json::json!({"name": "surface.finish", "value": choice}));
        g.until("the global dropdown selects the actual shader color", |g| {
            render(g, 1);
            assert!(g.error(node).is_none(), "{:?}", g.error(node));
            output.latest().filter(|d| f32s(d)[..4] == expected)
        });
    }
    assert!(g.error(node).is_none());
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
    g.link(metrics, "values", view, "metrics");
    let generated = g.probe(geometry, "geometry");
    let dashboard = g.probe(view, "image");
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
        let coordinates = &inspect_frame(&geom)["coordinates"];
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
    let flow = g.probe(transport, "geometry");
    for method in ["sand", "particles", "tracer", "streaming"] {
        g.set_param(transport, "transport", "method", method);
        let geom = frame(&g, transport, &transported, |d| geometry_kind(d) == method);
        let inspected = inspect_frame(&geom);
        let kind = inspected["type"].as_str().unwrap();
        assert_eq!(kind, match method { "sand" => "field_2d", "particles" => "point_cloud_2d", _ => "vector_field_2d" });
        if kind == "vector_field_2d" {
            let pixels = frame(&g, transport, &flow, |d| shape(&inspect_frame(d)["coordinates"]) == [96, 96, 2]);
            assert!(!f32s(&pixels).iter().any(|v| v.is_infinite()));
            assert_eq!(f32s(&inspect_frame(&pixels)["coverage"])[0], 0.0, "the circular domain stays masked");
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
    let packed = g.probe(morph, "harmonic");
    let ratios = g.probe(morph, "harmonic");
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
        let table = inspect_frame(d);
        f32s(&table["ratios"]) == [1.0, 2.5, 4.0]
            && f32s(&table["phases"]).iter().zip([0.1, 0.25, 0.4]).all(|(a, b)| (a-b).abs() < 1e-6)
            && f32s(&table["amplitudes"]).iter().zip([1.0/7.5, 2.5/7.5, 4.0/7.5]).all(|(a, b)| (a-b).abs() < 1e-6)
    });
    let t = inspect_frame(&d);
    assert!(f32s(&t["phases"]).iter().zip([0.1, 0.25, 0.4]).all(|(a, b)| (a-b).abs() < 1e-6));
    assert!(f32s(&t["amplitudes"]).iter().zip([1.0/7.5, 2.5/7.5, 4.0/7.5]).all(|(a, b)| (a-b).abs() < 1e-6));
    assert!(d.meta().sfreq().is_none(), "a harmonic frame is not sampled at its source waveform's rate");
    let d = frame(&g, morph, &packed, |d| shape(d) == [4, 3] && (f32s(d)[1]-2.5).abs() < 1e-6);
    assert!(f32s(&d).iter().all(|v| v.is_finite()));
    g.set_param(morph, "morph", "mix", 0.5);
    frame(&g, morph, &source, |d| {
        let table = inspect_frame(d);
        let r = f32s(&table["ratios"]);
        r.len() == 3 && (r[1]-(2.5f32*1.5).sqrt()).abs() < 1e-5
    });
    g.set_param(morph, "morph", "mix", 1.0);
    frame(&g, morph, &ratios, |d| active_row(d, 0) == [1.0, 1.5]);
    g.set_param(morph, "morph", "mix", 0.0);
    g.set_param(producer, "source", "state", "silence");
    frame(&g, morph, &source, |d| f32s(&inspect_frame(d)["amplitudes"]).iter().all(|v| *v == 0.0));
    frame(&g, geometry, &output, |d| f32s(&inspect_frame(d)["coordinates"]).is_empty());
    g.set_param(producer, "source", "state", "empty");
    frame(&g, morph, &ratios, |d| active_row(d, 0).is_empty());
    g.set_param(producer, "source", "state", "invalid");
    g.until("invalid peaks to be reported", |g| g.error(morph).filter(|e| e.contains("positive")));
    g.set_param(producer, "source", "state", "short");
    g.until("valid peaks to recover", |g| {
        (g.error(morph).is_none()).then(|| ratios.latest()).flatten().filter(|d| active_row(d, 0) == [1.0, 2.5])
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
    let field = g.probe(geometry, "geometry");
    let cpu = inspect_frame(&frame(&g, geometry, &field, |d| shape(&inspect_frame(d)["coordinates"]) == [64, 64]))["coordinates"].clone();
    let encoded = g.probe(modes, "modes");
    frame(&g, modes, &encoded, |d| shape(d) == [4, 32]);
    let shader = g.add("graphics:HarmonicChladni");
    g.set_param(shader, "common", "width", 64);
    g.set_param(shader, "common", "height", 64);
    g.link(modes, "modes", shader, "modes");
    g.link(source, "harmonic", shader, "harmonics");
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
    g.link(source, "harmonic", knot, "harmonics");
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
    let flow = g.probe(transport, "geometry");
    frame(&g, transport, &flow, |d| shape(&inspect_frame(d)["coordinates"]) == [64, 64, 2]);
    let dye = g.add("graphics:HarmonicFlow");
    g.set_param(dye, "common", "width", 64);
    g.set_param(dye, "common", "height", 64);
    g.link(transport, "geometry", dye, "flow");
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
    let field_a = g.probe(a, "geometry");
    let field_b = g.probe(b, "geometry");
    let av = f32s(&inspect_frame(&frame(&g, a, &field_a, |d| shape(&inspect_frame(d)["coordinates"]) == [96, 96]))["coordinates"]);
    let bv = f32s(&inspect_frame(&frame(&g, b, &field_b, |d| shape(&inspect_frame(d)["coordinates"]) == [96, 96]))["coordinates"]);
    g.until("different physical domains are refused", |g| g.error(blend).filter(|e| e.contains("grid mismatch")));
    g.set_param(blend, "blend", "space", "image");
    for mix in [0.0, 0.5, 1.0] {
        g.set_param(blend, "blend", "mix", mix);
        let d = g.until("registered endpoint field", |g| {
            generated.latest().filter(|d| {
                let table = inspect_frame(d);
                let info: serde_json::Value = serde_json::from_str(table["info"].as_str().unwrap()).unwrap();
                g.error(blend).is_none() && info["metadata"]["morph"] == mix
            })
        });
        let table = inspect_frame(&d);
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
        generated.latest().filter(|d| g.error(blend).is_none() && inspect_frame(d)["type"].as_str().unwrap() == "curve_3d")
    });
    assert_eq!(shape(&inspect_frame(&curve)["coordinates"]), [512, 3]);
    assert!(f32s(&inspect_frame(&curve)["coordinates"]).iter().all(|v| v.is_finite()));
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
        f32s(&inspect_frame(d)["phases"]).iter().all(|v| (v.abs()-std::f32::consts::PI).abs() < 1e-5)
    });
    let tuning = g.probe(morph, "harmonic");
    let amplitudes = g.probe(morph, "harmonic");
    for kind in ["harmonics", "subharmonics"] {
        g.set_param(morph, "extension", "amount", 0.0);
        g.set_param(morph, "extension", "kind", kind);
        let count = harmonic.count();
        g.until("an extended frame at its zero endpoint", |_| (harmonic.count() > count+2).then_some(()));
        let r = active_row(&tuning.latest().unwrap(), 0);
        assert_eq!(r, [1.0, 2.0, 3.0], "zero extension retains the original frequencies");
        let weights = active_row(&amplitudes.latest().unwrap(), 1);
        assert!(weights.iter().zip(f32s(&inspect_frame(&base)["amplitudes"])).all(|(a, b)| (a-b).abs() < 1e-6));
        g.set_param(morph, "extension", "amount", 1.0);
        let extended = frame(&g, morph, &tuning, |d| active_row(d, 0).len() > 3);
        let r = active_row(&extended, 0);
        assert!(r.windows(2).all(|p| p[0] < p[1]), "coincident extension tones are consolidated");
        assert!(if kind == "harmonics" { r.last().unwrap() > &3.0 } else { r[0] < 1.0 });
    }
    // A new subharmonic can be near an existing tone without being that tone.
    g.set_param(morph, "extension", "amount", 0.0);
    g.set_param(producer, "source", "state", "near");
    let near = frame(&g, morph, &tuning, |d| {
        let r = active_row(d, 0);
        r.len() == 2 && r[0] == 1.0 && r[1] > 2.0 && r[1] < 2.00001
    });
    assert_eq!(active_row(&near, 0).len(), 2, "zero growth does not activate a nearby subharmonic");
    g.set_param(morph, "extension", "amount", 0.5);
    let growing = frame(&g, morph, &harmonic, |d| {
        let table = inspect_frame(d);
        let weights = f32s(&table["amplitudes"]);
        weights.iter().filter(|v| **v > 0.0).count() > 2
            && f32s(&table["ratios"]).iter().any(|v| *v > 2.0 && *v < 2.00001)
    });
    assert!(f32s(&inspect_frame(&growing)["phases"]).iter()
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
    check_cookbook_archives(None);
}

#[test]
#[cfg(feature = "embed")]
fn common_chord_archive_drives_native_voices_without_devices() {
    check_cookbook_archives(Some("08-common-chord.gfi"));
}

#[test]
#[cfg(feature = "embed")]
fn jade_archive_opens_with_independent_texture_controls() {
    check_cookbook_archives(Some("09-jade-resonance.gfi"));
}

#[test]
#[cfg(feature = "embed")]
fn living_ratios_archive_opens_with_a_live_trace() {
    check_cookbook_archives(Some("10-living-ratios.gfi"));
}

#[test]
#[cfg(feature = "embed")]
fn gpu_geometry_archive_opens_with_live_controls() {
    check_cookbook_archives(Some("01-breathing-lines.gfi"));
}

#[test]
#[cfg(feature = "embed")]
fn peak_geometry_archive_uses_the_biotuner_analysis_chain() {
    check_cookbook_archives(Some("07-peaks-to-worlds.gfi"));
}

#[cfg(feature = "embed")]
fn check_cookbook_archives(selected: Option<&str>) {
    let _py = require_python();
    let g = Goofi::new();
    g.state.graph.lock().unwrap().set_evaluator(std::sync::Arc::new(
        goofi_python::inproc::PyExprEvaluator::new().expect("the same evaluator as the CLI")));
    let recipes: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root().join("node-bundles/harmonic-geometry/examples/recipes.json")).unwrap()).unwrap();
    let mut identities = std::collections::HashSet::new();
    for recipe in recipes.as_array().unwrap() {
        let file = recipe["file"].as_str().unwrap();
        if selected.is_some_and(|selected| file != selected) { continue; }
        g.call("session load", j!({"path": root().join("node-bundles/harmonic-geometry/examples").join(file).to_string_lossy()}));
        let doc = g.doc();
        for uid in doc["nodes"].as_object().unwrap().keys() {
            assert!(identities.insert(uid.clone()), "{file} reuses a node identity from another recipe");
        }
        let lookup = |name: &str| {
            let (uid, _) = doc["nodes"].as_object().unwrap().iter().find(|(_, n)| n["name"] == name).unwrap();
            Uid::from_hex(uid).unwrap()
        };
        assert_eq!(doc["nodes"].as_object().unwrap().len(), recipe["nodes"].as_u64().unwrap() as usize);
        if file.starts_with("09-") {
            let modes = lookup("modeWalk");
            let source = g.probe(modes, "modes");
            frame(&g, modes, &source, |d| shape(d) == [4, 32] && f32s(d)[64..96].iter().any(|v| *v > 0.01));
        }
        for (i, view) in recipe["views"].as_array().unwrap().iter().enumerate() {
            let node = lookup(view[0].as_str().unwrap());
            let probe = g.probe(node, view[1].as_str().unwrap());
            let image = g.until(&format!("{file} viewer {}", view[0]), |g| {
                render(g, 1);
                if let Some(error) = g.error(node).filter(|e| !e.contains("has no data")) {
                    panic!("{file} {}: {error}", view[0]);
                }
                probe.latest().filter(|d| {
                    if let Ok(text) = d.as_str() { return !text.is_empty(); }
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
                        let ready = !(file.starts_with("09-") || file.starts_with("10-")) || {
                            let top: Vec<_> = values[..values.len()/4].chunks_exact(channels).map(|p| p[0]).collect();
                            probe.count() > 5 && high > 0.4 &&
                                top.iter().copied().fold(0.0f32, f32::max)-top.iter().copied().fold(1.0f32, f32::min) > 0.06
                        };
                        visible && ready && high-low > 0.03 && values.iter().all(|v| v.is_finite())
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
        if file.starts_with("07-") {
            let reduced = g.probe(lookup("reduced"), "reduced");
            let expected = f32s(&frame(&g, lookup("reduced"), &reduced, |d| f32s(d).iter().any(|v| v.is_finite())));
            let mut expected: Vec<_> = expected.into_iter().filter(|v| v.is_finite()).collect();
            expected.sort_by(f32::total_cmp);
            expected.dedup();
            g.call("global entry edit", j!({"name": "geometry.mix", "value": 1.0}));
            let tuning = g.probe(lookup("chordTuning"), "out");
            frame(&g, lookup("chordTuning"), &tuning, |d| {
                let values = f32s(d);
                values.len() == expected.len() && values.iter().zip(&expected).all(|(a, b)| (a-b).abs() < 1e-5)
            });
            let matrix = g.probe(lookup("intervals"), "matrix");
            frame(&g, lookup("intervals"), &matrix, |d| shape(d) == [expected.len(), expected.len()]);
            assert!(!doc["nodes"].as_object().unwrap().values().any(|n| n["type"] == "signal:TimbreControls"),
                "the unused timbre branch is replaced by an analysis route that drives geometry");
        }
        if file.starts_with("09-") {
            g.call("global entry edit", j!({"name": "geometry.textureAuto", "value": false}));
            g.call("global entry edit", j!({"name": "geometry.textureMix", "value": 0.35}));
            g.call("global entry edit", j!({"name": "geometry.textureA", "value": "woven silk"}));
            g.call("global entry edit", j!({"name": "geometry.textureB", "value": "porous stone"}));
            let motion = g.probe(lookup("materialMotion"), "out");
            frame(&g, lookup("materialMotion"), &motion, |d| (f32s(d)[0]-0.35).abs() < 1e-6);
            assert!((f32s(&slow.latest().unwrap())[0]-0.75).abs() < 1e-6, "texture motion leaves the mode walk alone");
        }
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

#[test]
fn chladni_relief_follows_the_field_and_relights_without_changing_it() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[]);
    let chord = g.add("HarmonicMorph");
    g.set_param(chord, "source", "ratios_a", "1, 5/4, 3/2, 7/4");
    g.set_param(chord, "source", "ratios_b", "1, 6/5, 7/5, 9/5");
    let modes = g.add("HarmonicModes");
    g.link(chord, "harmonic", modes, "input");
    let mode_data = g.probe(modes, "modes");
    frame(&g, modes, &mode_data, |d| shape(d) == [4, 32]);
    let plate = g.add("graphics:HarmonicChladni");
    g.set_param(plate, "common", "width", 256);
    g.set_param(plate, "common", "height", 256);
    g.set_param(plate, "field", "symmetry", 0.82);
    g.link(modes, "modes", plate, "modes");
    let source = g.probe(plate, "out");
    let material = g.add("graphics:HarmonicRelief");
    g.set_param(material, "common", "width", 384);
    g.set_param(material, "common", "height", 384);
    g.set_param(material, "material", "texture_a", "jade");
    g.set_param(material, "material", "texture_b", "brushed metal");
    g.link(plate, "out", material, "input");
    let output = g.probe(material, "out");
    let capture = |label: &str| {
        let count = output.count();
        g.until(label, |g| {
            render(g, 1);
            assert!(g.error(material).is_none(), "{:?}", g.error(material));
            output.latest().filter(|_| output.count() > count+3)
        })
    };
    let image = capture("the relief after its Chladni input arrives");
    let values = f32s(&image);
    assert!(values.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    let red: Vec<_> = values.chunks_exact(4).map(|p| p[0]).collect();
    assert!(red.iter().copied().fold(0.0, f32::max) > 0.7, "lit metal is visible");
    assert!(red.iter().copied().fold(1.0, f32::min) < 0.25, "the relief retains deep basin contrast");
    save_frame("gpu-relief", &image);
    let field_before = f32s(&source.latest().expect("the upstream plate rendered"));
    let difference = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(a, b)| (a-b).abs()).sum::<f32>()/a.len() as f32;
    let mut finishes: Vec<Vec<f32>> = Vec::new();
    let textures = ["jade", "brushed metal", "woven silk", "porous stone", "sand", "dunes", "lichen", "coral", "cells", "spores", "pollen", "plankton"];
    for finish in textures {
        g.set_param(material, "material", "texture_b", finish);
        g.set_param(material, "material", "texture_mix", 1.0);
        let image = capture("each texture endpoint on the held harmonic field");
        let pixels = f32s(&image);
        assert!(pixels.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        for previous in &finishes { assert!(difference(&pixels, previous) > 0.01, "distinct finish: {finish}"); }
        save_frame(&format!("relief-{}", finish.replace(' ', "-")), &image);
        finishes.push(pixels);
    }
    g.set_param(material, "material", "texture_b", "brushed metal");
    g.set_param(material, "material", "texture_mix", 0.0);
    assert_eq!(f32s(&capture("exact jade endpoint")), finishes[0]);
    g.set_param(material, "material", "texture_mix", 0.5);
    let middle = f32s(&capture("halfway texture has blended surface detail"));
    g.set_param(material, "material", "texture_mix", 0.501);
    let adjacent = f32s(&capture("a small texture step stays smooth"));
    assert!(difference(&middle, &adjacent) > 0.0);
    assert!(difference(&middle, &adjacent) < difference(&finishes[0], &finishes[1])*0.1);
    assert!(difference(&middle, &finishes[0]) > 0.01 && difference(&middle, &finishes[1]) > 0.01);
    for (a, b) in [(4, 6), (7, 8), (9, 11), (5, 10)] {
        g.set_param(material, "material", "texture_a", textures[a]);
        g.set_param(material, "material", "texture_b", textures[b]);
        g.set_param(material, "material", "texture_mix", 0.0);
        assert_eq!(f32s(&capture("organic A endpoint")), finishes[a]);
        g.set_param(material, "material", "texture_mix", 1.0);
        assert_eq!(f32s(&capture("organic B endpoint")), finishes[b]);
        g.set_param(material, "material", "texture_mix", 0.5);
        let middle = f32s(&capture("organic texture blend"));
        g.set_param(material, "material", "texture_mix", 0.501);
        let adjacent = f32s(&capture("small organic texture step"));
        assert!(difference(&middle, &adjacent) < difference(&finishes[a], &finishes[b])*0.1);
    }
    g.set_param(material, "material", "texture_a", "sand");
    g.set_param(material, "material", "texture_mix", 0.0);
    g.set_param(material, "material", "density", 0.0);
    let sparse = f32s(&capture("sparse nodal grains"));
    g.set_param(material, "material", "density", 1.0);
    assert!(difference(&sparse, &f32s(&capture("dense nodal grains"))) > 0.01);
    g.set_param(material, "material", "density", 0.65);
    g.set_param(material, "material", "texture_a", "jade");
    assert_eq!(f32s(&source.latest().unwrap()), field_before, "texture morphing leaves the signed field unchanged");
    g.set_param(material, "material", "texture_mix", 0.0);
    for (width, height) in [(384, 216), (216, 384)] {
        g.set_param(material, "common", "width", width);
        g.set_param(material, "common", "height", height);
        let image = capture("a filled surface in landscape and portrait");
        assert_eq!(shape(&image), [height, width, 4]);
        let pixels = f32s(&image);
        for edge in 0..4 {
            let mut low = 1.0_f32; let mut high = 0.0_f32;
            for i in 0..if edge < 2 { width } else { height } {
                let (x, y) = match edge { 0 => (i, 0), 1 => (i, height-1), 2 => (0, i), _ => (width-1, i) };
                let red = pixels[(y*width+x)*4]; low = low.min(red); high = high.max(red);
            }
            assert!(high-low > 0.15, "surface detail reaches edge {edge} at {width}x{height}");
        }
    }
    g.set_param(material, "common", "width", 384);
    g.set_param(material, "common", "height", 384);
    g.set_param(material, "light", "azimuth", 1.8);
    let relit = capture("a different light on the same relief");
    assert_ne!(f32s(&relit), values, "the light changes the surface picture");
    assert_eq!(f32s(&source.latest().unwrap()), field_before, "lighting leaves the signed field unchanged");
    let held = capture("a held material with no hidden clock animation");
    assert_eq!(f32s(&held), f32s(&relit));
    let before = mode_data.count();
    g.set_param(chord, "morph", "mix", 1.0);
    frame(&g, modes, &mode_data, |_| mode_data.count() > before+2);
    let retuned = capture("a new harmonic structure in the same material");
    assert_ne!(f32s(&retuned), f32s(&held));
    assert_ne!(f32s(&source.latest().unwrap()), field_before);
    for (depth, tilt, roughness, seam) in [(0.0, 0.0, 0.8, 0.15), (0.45, 0.85, 0.12, 0.008)] {
        g.set_param(material, "form", "depth", depth);
        g.set_param(material, "camera", "tilt", tilt);
        g.set_param(material, "material", "roughness", roughness);
        g.set_param(material, "form", "seam", seam);
        let image = capture("valid pictures at the material control limits");
        assert!(f32s(&image).iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    }
    // Expressions can pass through zero; editor ranges do not clamp the public API.
    for (group, name) in [("form", "seam"), ("form", "range"), ("material", "roughness"), ("camera", "zoom")] {
        g.set_param(material, group, name, 0.0);
    }
    let zero = capture("finite output when external controls pass through zero");
    assert!(f32s(&zero).iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
}

#[test]
#[cfg(feature = "embed")]
fn ratio_sequence_holds_glides_loops_and_drives_a_harmonic_field() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[("ratio_clock.py", include_str!("fixtures/ratio_clock.py"))]);
    let clock = g.add("RatioClock");
    let sequence = g.add("RatioSequence");
    g.set_param(sequence, "sequence", "ratios", "1, 2, 4");
    g.set_param(sequence, "sequence", "seconds", 1.0);
    g.set_param(sequence, "sequence", "glide", 0.5);
    g.link(clock, "out", sequence, "clock");
    g.set_param(sequence, "chord", "state", "anchor chord");
    let ratio = g.probe(sequence, "transition");
    let tuning = g.probe(sequence, "transition");
    let seek = |time: f64, expected: f32| {
        let before = ratio.count();
        g.set_param(clock, "clock", "time", time);
        g.until(&format!("ratio {expected} at clock {time}"), |g| {
            assert!(g.error(sequence).is_none(), "{:?}", g.error(sequence));
            ratio.latest().filter(|d| ratio.count() > before+1 && (sequence_ratios(d)[1]-expected).abs() < 1e-5)
        })
    };
    seek(0.0, 1.0);
    seek(0.25, 1.0);
    seek(0.75, 2.0_f32.sqrt());
    seek(1.0, 2.0);
    let chord = frame(&g, sequence, &tuning, |d| sequence_ratios(d) == [1.0, 2.0, 2.0]);
    assert_eq!(shape(&chord), [2, 3]);
    g.set_param(sequence, "sequence", "running", false);
    seek(1.0, 2.0); // Let the pause reach this node before advancing the separate clock node.
    seek(5.0, 2.0);
    g.set_param(sequence, "sequence", "running", true);
    seek(5.0, 2.0);
    seek(5.75, 8.0_f32.sqrt());
    seek(6.0, 4.0);
    seek(6.75, 2.0); // The loop glides from four back to one in pitch.
    seek(7.0, 1.0);
    g.set_param(sequence, "sequence", "direction", "ping-pong");
    seek(7.0, 1.0);
    seek(8.0, 2.0);
    seek(9.0, 4.0);
    seek(10.0, 2.0);
    seek(11.0, 1.0);
    seek(12.0, 2.0);
    g.call("node param pulse", j!({"node": sequence.to_string(), "param": "sequence/reset"}));
    seek(12.0, 1.0);
    g.set_param(sequence, "sequence", "ratios", "9/8, 3/2");
    seek(12.0, 1.125);
    let morph = g.add("HarmonicMorph");
    g.link(sequence, "transition", morph, "transition");
    let packed = g.probe(morph, "harmonic");
    frame(&g, morph, &packed, |d| (f32s(d)[1]-1.125).abs() < 1e-5);
    let plate = g.add("graphics:HarmonicChladni");
    g.set_param(plate, "common", "width", 128);
    g.set_param(plate, "common", "height", 128);
    g.set_param(plate, "field", "approach", 1.0);
    g.link(morph, "harmonic", plate, "harmonics");
    let output = g.probe(plate, "out");
    let capture = || {
        let before = output.count();
        g.until("sequenced ratios reach the shader", |g| {
            render(g, 1);
            output.latest().filter(|_| output.count() > before+3)
        })
    };
    let before = f32s(&capture());
    seek(13.0, 1.5);
    frame(&g, morph, &packed, |d| (f32s(d)[1]-1.5).abs() < 1e-5);
    let after = f32s(&capture());
    assert_ne!(before, after, "the ratio sequence changes the actual harmonic field");
    g.set_param(sequence, "sequence", "ratios", "1, 0");
    g.until("invalid ratio list is reported", |g| g.error(sequence).filter(|e| e.contains("ratios must")));
    g.set_param(sequence, "sequence", "ratios", "5/4");
    g.until("valid ratios clear the earlier error", |g| g.error(sequence).is_none().then_some(()));
    seek(13.0, 1.25);
    seek(18.0, 1.25);
    seek(0.0, 1.25); // A backward clock re-anchors safely.
    assert!(g.error(sequence).is_none());
}

#[test]
fn sequenced_chladni_states_move_continuously_across_step_boundaries() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[("ratio_clock.py", include_str!("fixtures/ratio_clock.py"))]);
    let clock = g.add("RatioClock");
    let sequence = g.add("RatioSequence");
    g.set_param(sequence, "sequence", "ratios", "3/2, 5/4, 4/3");
    g.set_param(sequence, "sequence", "seconds", 1.0);
    g.set_param(sequence, "sequence", "glide", 1.0);
    g.set_param(sequence, "sequence", "direction", "ping-pong");
    g.link(clock, "out", sequence, "clock");
    let modes = g.add("HarmonicModes");
    g.link(sequence, "transition", modes, "transition");
    let mode_data = g.probe(modes, "modes");
    let plate = g.add("graphics:HarmonicChladni");
    g.set_param(plate, "common", "width", 128);
    g.set_param(plate, "common", "height", 128);
    g.set_param(plate, "field", "symmetry", 1.0);
    g.link(modes, "modes", plate, "modes");
    let output = g.probe(plate, "out");
    let capture = |time: f64, m: f32, n: f32| {
        g.set_param(clock, "clock", "time", time);
        g.until(&format!("plate mode ({m}, {n}) at clock {time}"), |_| {
            mode_data.latest().filter(|d| (f32s(d)[0]-m).abs() < 0.0001 && (f32s(d)[32]-n).abs() < 0.0001)
        });
        let count = output.count();
        let image = g.until("the interpolated plate reaches the GPU", |g| {
            render(g, 1);
            output.latest().filter(|_| output.count() > count+3)
        });
        assert!(g.error(modes).is_none());
        f32s(&image)
    };
    let first = capture(0.0, 3.0, 2.0);
    let quarter = capture(0.25, 3.3125, 2.3125);
    let middle = capture(0.5, 4.0, 3.0);
    let before = capture(0.999, 4.999994, 3.999994);
    let boundary = capture(1.0, 5.0, 4.0);
    let after = capture(1.001, 4.999997, 3.999997);
    let second_middle = capture(1.5, 4.5, 3.5);
    let turn = capture(2.0, 4.0, 3.0);
    let reversed = capture(2.001, 4.000003, 3.000003);
    let reverse_middle = capture(2.5, 4.5, 3.5);
    let difference = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(a, b)| (a-b).abs()).sum::<f32>()/a.len() as f32;
    assert!(difference(&first, &quarter) > 0.1, "the geometry moves during the first quarter of a step");
    assert!(difference(&quarter, &middle) > 0.1, "the mode trajectory keeps moving");
    assert!(difference(&middle, &boundary) > 0.1, "the endpoint is a distinct plate state");
    for (a, b) in [(&before, &boundary), (&boundary, &after), (&turn, &reversed)] {
        assert!(difference(a, b) < 0.002, "step changes and turnarounds do not jump to the old source state");
    }
    assert_eq!(second_middle, reverse_middle, "ping-pong retraces the same field");
}

#[test]
fn every_ratio_drives_a_full_chord_and_fixed_basis_fields_match_biotuner() {
    let _py = require_python();
    let g = Goofi::new();
    install_bundle(&g, &[("ratio_clock.py", include_str!("fixtures/ratio_clock.py")),
        ("chladni_reference.py", include_str!("fixtures/chladni_reference.py"))]);
    let reference = g.add("ChladniReference");
    let fields = g.probe(reference, "fields");
    let pairs = g.probe(reference, "pairs");
    let expected = f32s(&frame(&g, reference, &fields, |d| shape(d) == [6, 64, 64]));
    let expected_pairs = f32s(&frame(&g, reference, &pairs, |d| shape(d) == [6, 3, 2]));
    let density = g.probe(reference, "density");
    let expected_density = f32s(&frame(&g, reference, &density, |d| shape(d) == [6, 5, 64, 64]));
    let clock = g.add("RatioClock");
    let sequence = g.add("RatioSequence");
    g.set_param(sequence, "sequence", "ratios", "3/2, 5/4, 4/3, 7/4, 5/3, 9/8");
    g.set_param(sequence, "sequence", "seconds", 1.0);
    g.set_param(sequence, "sequence", "glide", 1.0);
    g.set_param(sequence, "chord", "state", "anchor chord");
    g.link(clock, "out", sequence, "clock");
    let modes = g.add("HarmonicModes");
    g.set_param(modes, "modes", "mapping", "chord pairs");
    g.set_param(modes, "modes", "interpolation", "fields");
    g.set_param(modes, "modes", "max_mode", 24);
    g.link(sequence, "transition", modes, "transition");
    let packed = g.probe(modes, "modes");
    let report = g.probe(modes, "mapping");
    let plate = g.add("graphics:HarmonicChladni");
    g.set_param(plate, "common", "width", 64);
    g.set_param(plate, "common", "height", 64);
    g.set_param(plate, "field", "symmetry", 1.0);
    g.link(modes, "modes", plate, "modes");
    let output = g.probe(plate, "out");
    for step in 0..6 {
        for (phase_index, phase) in [0.0f32, 0.25, 0.5, 0.75, 0.999].into_iter().enumerate() {
            g.set_param(plate, "field", "output", "signed");
            let t = phase*phase*(3.0-2.0*phase);
            let next = (step+1)%6;
            g.set_param(clock, "clock", "time", step as f64+phase as f64);
            frame(&g, modes, &packed, |d| {
                if shape(d) != [4, 64] { return false; }
                let p = f32s(d);
                (0..3).all(|i| (p[i]-expected_pairs[step*6+i*2]).abs() < 1e-6
                    && (p[64+i]-expected_pairs[step*6+i*2+1]).abs() < 1e-6
                    && (p[32+i]-expected_pairs[next*6+i*2]).abs() < 1e-6
                    && (p[96+i]-expected_pairs[next*6+i*2+1]).abs() < 1e-6
                    && (p[128+i]-(1.0-t)/3.0).abs() < 1e-5 && (p[160+i]-t/3.0).abs() < 1e-5)
            });
            let count = output.count();
            let gpu = g.until("a fixed-basis chord blend reaches the GPU", |g| {
                render(g, 1);
                output.latest().filter(|_| output.count() > count+3)
            });
            let pixels = f32s(&gpu);
            let error = pixels.chunks_exact(4).enumerate().map(|(i, p)|
                (p[0]-((1.0-t)*expected[step*4096+i]+t*expected[next*4096+i])).abs()).fold(0.0f32, f32::max);
            assert!(error < 0.003, "step {step}, phase {phase}: CPU/GPU error {error}; fixed basis must not zoom");
            g.set_param(plate, "field", "output", "nodal");
            let count = output.count();
            let gpu = g.until("notebook density reaches the GPU", |g| {
                render(g, 1);
                assert!(g.error(plate).is_none(), "{:?}", g.error(plate));
                output.latest().filter(|_| output.count() > count+3)
            });
            let pixels = f32s(&gpu);
            let error = pixels.chunks_exact(4).enumerate().map(|(i, p)|
                (p[0]-expected_density[(step*5+phase_index)*4096+i]).abs()).fold(0.0f32, f32::max);
            assert!(error < 0.015, "step {step}, phase {phase}: Biotuner D4 nodal density error {error}");
        }
    }
    let mapping = frame(&g, modes, &report, |d| d.as_str().is_ok_and(|s| s.contains("integer chord [8, 9, 16]")));
    assert!(mapping.as_str().unwrap().contains("pairs (8, 9), (8, 16), (9, 16)"));
    for a in 0..6 {
        for b in a+1..6 {
            let difference = expected[a*4096..(a+1)*4096].iter().zip(&expected[b*4096..(b+1)*4096])
                .map(|(a, b)| (a-b).abs()).sum::<f32>()/4096.0;
            assert!(difference > 0.1, "each of the six source ratios produces a distinct chord field");
        }
    }
}
