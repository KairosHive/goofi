//! Upscaling through the public graph, including the CPU/texture boundary.
use goofi_tests::{Goofi, f32s, hex, install_all, j, render, require_python, shape};

const SHADER: &str = include_str!("../../../../node-bundles/graphics/Upscale.wgsl");
const ESRGAN: &str = include_str!("../../../../node-bundles/image/real_esrgan.py");
const SOURCE: &str = r#"
import goofi
import numpy as np
class UpscaleSource(goofi.Node):
    PRODUCER = True
    OUTPUTS = {'out': goofi.DataType.ARRAY}
    PARAMS = {'test': {'step': goofi.IntParam(0, 0, 100), 'size': goofi.IntParam(8, 1, 512)},
              'common': {'autotrigger': goofi.BoolParam(True)}}
    def setup(self):
        self.last = None
    def process(self):
        if self.last == self.params.test.step:
            return None
        self.last = self.params.test.step
        size = self.params.test.size
        y, x = np.mgrid[:size, :size]
        data = np.stack([(x+self.params.test.step)%7/6, y/max(1,size-1), (x+y)%2], axis=-1).astype(np.float32)
        return (data, {'source_step': self.params.test.step})
"#;

#[test]
fn shader_upscaling_modes_resize_and_preserve_constant_colors() {
    let g = Goofi::new();
    let directory = g.state.mount().join("nodes_graphics");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("Upscale.wgsl"), SHADER).unwrap();
    g.call("library refresh", j!({}));
    let source = g.add("graphics:Constant");
    g.set_param(source, "common", "width", 7);
    g.set_param(source, "common", "height", 5);
    g.set_param(source, "colour", "r", 0.25);
    g.set_param(source, "colour", "g", 0.5);
    g.set_param(source, "colour", "b", 0.75);
    let node = g.add("graphics:Upscale");
    g.link(source, "out", node, "input");
    let probe = g.probe(node, "out");
    for method in ["linear", "fsr1", "nis"] {
        g.set_param(node, "upscale", "method", method);
        for sharpness in [0.0, 0.5, 1.0] {
            g.set_param(node, "upscale", "sharpness", sharpness);
            for (width, height) in [(14, 10), (28, 20), (1, 1)] {
                g.set_param(node, "common", "width", width);
                g.set_param(node, "common", "height", height);
                let seen = probe.count();
                let frame = g.until("a resized shader frame", |g| {
                    if let Some(error) = g.error(node) {
                        panic!("{method}: {error}");
                    }
                    render(g, 1);
                    probe.latest().filter(|d| {
                        probe.count() > seen && shape(d) == vec![height as usize, width as usize, 4]
                    })
                });
                for pixel in f32s(&frame).chunks_exact(4) {
                    for (value, expected) in pixel.iter().zip([0.25, 0.5, 0.75, 1.0]) {
                        assert!(
                            (value - expected).abs() < 0.004,
                            "{method}, sharpness {sharpness}: {pixel:?}"
                        );
                    }
                }
            }
        }
    }
    g.set_param(source, "common", "width", 1);
    g.set_param(source, "common", "height", 1);
    for value in [0.0, 1.0] {
        for channel in ["r", "g", "b"] {
            g.set_param(source, "colour", channel, value);
        }
        g.set_param(node, "common", "width", 8);
        g.set_param(node, "common", "height", 8);
        for method in ["fsr1", "nis"] {
            g.set_param(node, "upscale", "method", method);
            g.until("finite black and white borders", |g| {
                render(g, 1);
                probe.latest().filter(|d| {
                    shape(d) == vec![8, 8, 4]
                        && f32s(d).chunks_exact(4).all(|pixel| {
                            pixel[..3]
                                .iter()
                                .all(|x| x.is_finite() && (x - value as f32).abs() < 0.002)
                        })
                })
            });
        }
    }
}

#[test]
fn realesrgan_consumes_frames_once_and_obeys_lifecycle_and_modes() {
    let _py = require_python();
    let fake = r#"
class Model:
    def __init__(self, path):
        pass
    def process(self, image, precision, scale):
        return np.repeat(np.repeat(image,scale,axis=0),scale,axis=1)
    def close(self):
        pass
"#;
    let node_source = format!(
        "{}\n{fake}\n{}",
        ESRGAN[..ESRGAN.find("class Model:").unwrap()].replace("import torch\n", ""),
        &ESRGAN[ESRGAN.find("class RealESRGAN(").unwrap()..]
    );
    let root = tempfile::tempdir().unwrap();
    let weights = root.path().join("fixture.pth");
    std::fs::write(&weights, "fixture").unwrap();
    let g = Goofi::new();
    let types = install_all(
        &g,
        &[
            ("real_esrgan.py", &node_source),
            ("upscale_source.py", SOURCE),
        ],
    );
    let node = g.add(&types[0]);
    let source = g.add(&types[1]);
    g.link(source, "out", node, "image");
    g.call(
        "node param edit",
        j!({"node":hex(source),"param":"test/step","triggers":true}),
    );
    let probe = g.probe(node, "image");
    let status = g.probe(node, "status");
    g.set_param(node, "model", "weights", weights.to_string_lossy().as_ref());
    g.until("stopped by default", |_| {
        status
            .latest()
            .filter(|d| d.as_str().is_ok_and(|s| s.contains("Stopped")))
    });
    assert_eq!(probe.count(), 0);
    g.set_param(node, "runtime", "state", "start");
    let mut step = 0;
    for precision in ["fp16", "fp32"] {
        for scale in [2, 4] {
            step += 1;
            g.set_param(node, "model", "precision", precision);
            g.set_param(node, "upscale", "scale", scale);
            g.set_param(source, "test", "step", step);
            let frame = g.until("a new source frame uses selected settings", |g| {
                if let Some(error) = g.error(node) {
                    panic!("{error}");
                }
                probe.latest().filter(|d| {
                    let meta = serde_json::to_value(goofi_core::MetaJson(d.meta())).unwrap();
                    meta["source_step"] == step
                        && meta["upscale"]["precision"] == precision
                        && meta["upscale"]["scale"] == scale
                })
            });
            assert_eq!(
                shape(&frame),
                vec![8 * scale as usize, 8 * scale as usize, 3]
            );
            let seen = probe.count();
            assert!(
                g.stays(|_| probe.count() == seen),
                "unchanged input must not be processed again"
            );
        }
    }
    g.set_param(node, "runtime", "state", "pause");
    g.until("paused", |_| {
        status
            .latest()
            .filter(|d| d.as_str().is_ok_and(|s| s == "Paused"))
    });
    let seen = probe.count();
    g.set_param(source, "test", "step", 10);
    assert!(g.stays(|_| probe.count() == seen));
    g.set_param(node, "runtime", "state", "start");
    g.until("resume accepts the waiting frame", |_| {
        (probe.count() > seen).then_some(())
    });
    g.set_param(node, "runtime", "state", "stop");
    g.until("stop releases the model", |_| {
        status
            .latest()
            .filter(|d| d.as_str().is_ok_and(|s| s.contains("Stopped")))
    });
    g.set_param(node, "runtime", "state", "start");
    g.set_param(source, "test", "step", 11);
    g.until("restart produces a new frame", |_| {
        probe.latest().filter(|d| {
            serde_json::to_value(goofi_core::MetaJson(d.meta())).unwrap()["source_step"] == 11
        })
    });
    g.call("node remove", j!({"node":hex(node)}));
}
