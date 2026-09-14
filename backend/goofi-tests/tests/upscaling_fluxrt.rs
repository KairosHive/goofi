//! Real moving graphics -> FluxRT -> upscalers, with normal graph transport.
use goofi_tests::{Goofi, OutputProbe, Uid, f32s, hex, install_all, j, require_python, shape};
use std::time::{Duration, Instant};

fn save_image(path: &std::path::Path, data: &goofi_core::Data) {
    let s = shape(data);
    let mut bytes = format!("P6\n{} {}\n255\n", s[1], s[0]).into_bytes();
    for pixel in f32s(data).chunks_exact(s[2]) {
        bytes.extend(
            pixel[..3]
                .iter()
                .map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8),
        );
    }
    std::fs::write(path, bytes).unwrap();
}

fn measure(
    g: &Goofi,
    flux: Uid,
    flux_probe: &OutputProbe,
    output: Option<(Uid, &OutputProbe)>,
) -> serde_json::Value {
    let start = Instant::now();
    let mut flux_frames = 0;
    let mut output_frames = 0;
    let mut flux_seen = flux_probe.count();
    let mut output_seen = output.map_or(0, |(_, p)| p.count());
    let mut costs = Vec::new();
    while start.elapsed() < Duration::from_secs(10) {
        assert!(g.error(flux).is_none(), "FluxRT: {:?}", g.error(flux));
        let now = flux_probe.count();
        flux_frames += now - flux_seen;
        flux_seen = now;
        if let Some((node, probe)) = output {
            assert!(g.error(node).is_none(), "Upscaler: {:?}", g.error(node));
            let now = probe.count();
            output_frames += now - output_seen;
            if now > output_seen {
                let data = probe.latest().unwrap();
                let meta = serde_json::to_value(goofi_core::MetaJson(data.meta())).unwrap();
                if let Some(seconds) = meta["upscale"]["seconds"].as_f64() {
                    costs.push(seconds * 1000.0);
                }
            }
            output_seen = now;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let seconds = start.elapsed().as_secs_f64();
    assert!(flux_frames > 2, "FluxRT stopped delivering frames");
    if output.is_some() {
        assert!(output_frames > 2, "Upscaler stopped delivering frames");
    }
    j!({"seconds":seconds,"flux_delivered_fps":flux_frames as f64/seconds,
        "output_delivered_fps":output_frames as f64/seconds,"inference_ms":costs})
}

#[test]
#[ignore = "requires FluxRT and RealESRGAN weights, CUDA, and a graphics GPU"]
fn fluxrt_drives_all_upscaling_modes_in_a_live_graph() {
    let _py = require_python();
    let root = std::env::var("GOOFI_FLUXRT_ROOT").expect("set GOOFI_FLUXRT_ROOT");
    let weights = std::env::var("GOOFI_REALESRGAN_WEIGHTS").expect("set GOOFI_REALESRGAN_WEIGHTS");
    let output = std::path::PathBuf::from(
        std::env::var("GOOFI_UPSCALE_RESULTS").expect("set GOOFI_UPSCALE_RESULTS"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let g = Goofi::timed();
    let style_image = std::env::var("GOOFI_UPSCALE_STYLE_IMAGE").ok();
    let files = vec![
        (
            "flux_rt.py",
            include_str!("../../../node-bundles/image-generation/flux_rt.py"),
        ),
        (
            "real_esrgan.py",
            include_str!("../../../node-bundles/image/real_esrgan.py"),
        ),
        ("image_file.py", include_str!("../../../node-bundles/image/image_file.py")),
    ];
    let types = install_all(&g, &files);
    let noise = g.add("graphics:Noise");
    g.set_param(noise, "common", "width", 320);
    g.set_param(noise, "common", "height", 320);
    g.set_param(noise, "move", "speed", 0.1);
    g.set_param(noise, "noise", "mono", true);
    let read = g.add("signal:GraphicsIn");
    g.set_param(read, "graphics", "mode", "pixels");
    g.set_param(read, "graphics", "size", 320);
    g.link(noise, "out", read, "input");
    let select = g.add("signal:Select");
    g.set_param(select, "select", "axis", 2);
    g.set_param(select, "select", "mode", "index");
    g.set_param(select, "select", "include", "0:3");
    g.link(read, "out", select, "input");
    let flux = g.add(&types[0]);
    g.set_param(flux, "model", "root", root);
    g.set_param(flux,"model","prompt","Turn this into a surreal blue and gold watercolor landscape with a stone doorway and floating moons.");
    g.link(select, "out", flux, "source");
    let flux_probe = g.probe(flux, "image");
    if let Some(path) = style_image.as_ref() {
        let style = g.add(&types[2]);
        g.set_param(style, "image", "max_size", 320);
        g.set_param(style, "file", "path", path.as_str());
        let loaded = g.probe(style, "image");
        g.until("ImageFile loads the reference", |g| {
            assert!(g.error(style).is_none(), "ImageFile: {:?}", g.error(style));
            loaded.latest()
        });
        g.link(style, "image", flux, "reference");
        g.set_param(flux, "reference", "enabled", true);
        g.set_param(flux,"model","prompt","Use the moving first image as a loose composition guide. Use the second image for dark green and bronze colors, golden filigree and glowing red details. Create a mysterious celestial garden with sacred geometric forms and fine painterly texture.");
    }
    g.set_param(flux, "runtime", "state", "start");
    let source = g.until("FluxRT loads and generates", |g| {
        for node in [noise, read, select, flux] {
            assert!(
                g.error(node).is_none(),
                "source chain {}: {:?}",
                hex(node),
                g.error(node)
            );
        }
        flux_probe
            .latest()
            .filter(|d| shape(d) == vec![320, 320, 3])
    });
    save_image(&output.join("flux-source.ppm"), &source);
    g.set_param(flux, "model", "prompt_b", "A small friendly robot in a lush garden, modern 3D animation, soft sculpted forms.");
    g.set_param(flux, "smoothing", "time", 0.5);
    for (mix, frames, role, enabled) in [(0.5, 1, "reference", true), (1.0, 4, "source", false), (0.0, 8, "source", true)] {
        g.set_param(flux, "model", "prompt_mix", mix);
        g.set_param(flux, "transition", "frames", frames);
        g.set_param(flux, "reference", "main_image", role);
        g.set_param(flux, "reference", "enabled", enabled && style_image.is_some());
        g.set_param(flux, "source", "freeze", frames == 1);
        g.set_param(flux, "reference", "influence", if frames == 1 { 8.0 } else { 1.0 });
        let frame = g.until("live conditioning controls settle", |g| {
            assert!(g.error(flux).is_none(), "FluxRT: {:?}", g.error(flux));
            flux_probe.latest().filter(|d| {
                let meta = serde_json::to_value(goofi_core::MetaJson(d.meta())).unwrap();
                meta["fluxrt"]["rife_frames"] == frames
                    && meta["fluxrt"]["reference_role"] == role
                    && meta["fluxrt"]["reference_enabled"] == (enabled && style_image.is_some())
                    && meta["fluxrt"]["prompt_mix"].as_f64().is_some_and(|x| (x-mix).abs() < 0.001)
            })
        });
        assert!(f32s(&frame).iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x)));
        save_image(&output.join(format!("controls-{frames}-{role}.ppm")), &frame);
    }
    g.call("node param pulse", j!({"node":hex(flux),"param":"transition/cut"}));
    g.until("cut is applied by the real model", |_| flux_probe.latest().filter(|d| {
        serde_json::to_value(goofi_core::MetaJson(d.meta())).unwrap()["fluxrt"]["cut"] == 1
    }));
    let mut reports =
        vec![j!({"method":"flux-only","measurement":measure(&g,flux,&flux_probe,None)})];
    let upload = g.add("graphics:SignalIn");
    g.set_param(upload, "common", "width", 320);
    g.set_param(upload, "common", "height", 320);
    g.set_param(upload, "signal", "autoscale", false);
    g.set_param(upload, "signal", "min", 0.0);
    g.link(flux, "image", upload, "input");
    for method in ["linear", "fsr1", "nis"] {
        for size in [640, 1280] {
            let shader = g.add("graphics:Upscale");
            g.set_param(shader, "upscale", "method", method);
            g.set_param(shader, "common", "width", size);
            g.set_param(shader, "common", "height", size);
            g.link(upload, "out", shader, "input");
            let texture = g.probe(shader, "out");
            let seen = texture.count();
            let frame = g.until("shader method produces the requested size", |g| {
                assert!(g.error(shader).is_none(), "{method}: {:?}", g.error(shader));
                texture.latest().filter(|d| {
                    texture.count() > seen && shape(d) == vec![size as usize, size as usize, 4]
                })
            });
            assert!(f32s(&frame).iter().all(|x| x.is_finite()));
            save_image(&output.join(format!("{method}-{size}.ppm")), &frame);
            let row = j!({"method":method,"size":size,"measurement":measure(&g,flux,&flux_probe,Some((shader,&texture)))});
            eprintln!("{row}");
            reports.push(row);
            drop(texture);
            g.call("node remove", j!({"node":hex(shader)}));
        }
    }
    g.call("node remove", j!({"node":hex(upload)}));
    let ai = g.add(&types[1]);
    g.set_param(ai, "model", "weights", weights);
    g.link(flux, "image", ai, "image");
    let enhanced = g.probe(ai, "image");
    g.set_param(ai, "runtime", "state", "start");
    for precision in ["fp16", "fp32"] {
        for scale in [2, 4] {
            g.set_param(ai, "model", "precision", precision);
            g.set_param(ai, "upscale", "scale", scale);
            let frame = g.until("RealESRGAN applies the next settings", |g| {
                assert!(g.error(ai).is_none(), "RealESRGAN: {:?}", g.error(ai));
                enhanced.latest().filter(|d| {
                    let meta = serde_json::to_value(goofi_core::MetaJson(d.meta())).unwrap();
                    meta["upscale"]["precision"] == precision && meta["upscale"]["scale"] == scale
                })
            });
            assert_eq!(
                shape(&frame),
                vec![320 * scale as usize, 320 * scale as usize, 3]
            );
            assert!(
                f32s(&frame)
                    .iter()
                    .all(|x| x.is_finite() && (0.0..=1.0).contains(x))
            );
            save_image(
                &output.join(format!("realesrgan-{precision}-{scale}x.ppm")),
                &frame,
            );
            let row = j!({"method":"realesrgan","precision":precision,"scale":scale,
                "measurement":measure(&g,flux,&flux_probe,Some((ai,&enhanced)))});
            eprintln!("{row}");
            reports.push(row);
        }
    }
    for frames in [4, 8] {
        g.set_param(flux, "transition", "frames", frames);
        g.set_param(
            flux,
            "transition",
            "mode",
            if frames == 4 { "cut" } else { "prompt blend" },
        );
        g.set_param(flux, "model", "seed", frames * 100);
        g.until("FluxRT transition metadata survives upscaling", |g| {
            assert!(g.error(ai).is_none(), "RealESRGAN: {:?}", g.error(ai));
            enhanced.latest().filter(|d| {
                let meta = serde_json::to_value(goofi_core::MetaJson(d.meta())).unwrap();
                meta["fluxrt"]["rife_frames"] == frames && meta["fluxrt"]["seed"] == frames * 100
            })
        });
    }
    let status = g.probe(ai, "status");
    g.set_param(ai, "runtime", "state", "pause");
    g.until("real upscaler pauses", |_| {
        status
            .latest()
            .filter(|d| d.as_str().is_ok_and(|s| s == "Paused"))
    });
    let seen = enhanced.count();
    assert!(g.stays(|_| enhanced.count() == seen));
    g.set_param(ai, "runtime", "state", "start");
    g.until("real upscaler resumes", |_| {
        (enhanced.count() > seen).then_some(())
    });
    g.set_param(ai, "runtime", "state", "stop");
    g.until("real upscaler unloads", |_| {
        status
            .latest()
            .filter(|d| d.as_str().is_ok_and(|s| s.contains("Stopped")))
    });
    g.set_param(flux, "runtime", "state", "stop");
    std::fs::write(
        output.join("report.json"),
        serde_json::to_string_pretty(&reports).unwrap(),
    )
    .unwrap();
}
