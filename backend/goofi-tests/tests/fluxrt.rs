//! The optional FluxRT node uses the ordinary graph and graphics interfaces.
use goofi_tests::{f32s, hex, install, install_all, j, render, require_python, shape, Goofi};

const NODE: &str = include_str!("../../../node-bundles/image-generation/flux_rt.py");
const IMPORT: &str = "from fluxrt.stream_processor.model_inference_subprocess import ModelInferenceSubprocess";
const MODEL: &str = r#"
from types import SimpleNamespace
class Inference:
    @staticmethod
    def release():
        pass
    def __init__(self, config):
        self.config = config
        self.interpolation_exp = config['interpolation_exp']
        self.controls = None
    def configure(self, controls):
        if self.controls is None or controls[4:] != self.controls[4:]:
            self.previous_frame = None
        self.controls = controls
    def set_reference(self, enabled, primary):
        self.config['use_reference_image'] = enabled
        self.reference_primary = primary
    def process_init(self):
        time.sleep(0.5)
        self.process_state = {'seed': self.config['default_seed']}
        self.prompt = self.config['default_prompt']
        self.prompt_embeds = np.array([0.0], dtype=np.float32)
        self.previous_frame = None
        self.update_controller = SimpleNamespace(reset_cache=lambda: None)
    def update_prompt_embeds(self, prompt):
        self.prompt = prompt
        self.prompt_embeds = np.array([200.0 if prompt == 'warm' else 20.0], dtype=np.float32)
    def noise_for_seed(self, seed):
        return float(seed % 256)
    @staticmethod
    def blend_noise(source, target, progress):
        return source*(1-progress) + target*progress
    def process_frame_with_pipeline(self, image):
        time.sleep(0.04)
        if self.prompt == 'fail':
            raise ValueError('fixture inference failed')
        frame = image.copy()
        frame[..., 0] = int(self.noise)
        if self.config['use_reference_image']:
            frame[..., 1] = np.clip(np.asarray(self.reference_image)[0, 0, 0]*self.controls.style, 0, 255)
        frame[..., 2] = int(self.prompt_embeds[0])
        return frame
    def convert_np_to_torch(self, frame):
        return frame
    def interpolate_frames(self, frame):
        previous = frame if self.previous_frame is None else self.previous_frame
        frames = np.stack([previous*(1-t)+frame*t for t in np.linspace(0, 1, 2**self.interpolation_exp+1)[1:]])
        self.previous_frame = frame
        return frames[..., ::-1].astype(np.uint8)
"#;

fn fake_node() -> String {
    let start = NODE.find("class Inference(").unwrap();
    let end = NODE.find("class Runtime:").unwrap();
    format!("{}\n{MODEL}\n{}\navailable_commit = lambda: None\nprocess_commit = lambda: 2.5 * 1024**3\n", NODE[..start].replace(IMPORT, ""), &NODE[end..])
}

fn flux_meta(data: &goofi_core::Data) -> serde_json::Value {
    serde_json::to_value(goofi_core::MetaJson(data.meta())).unwrap()["fluxrt"].clone()
}

#[test]
fn fluxrt_paces_blended_output_without_extra_model_passes() {
    let _py = require_python();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("configs")).unwrap();
    std::fs::write(root.path().join("configs/benchmark_config.json"), "{}").unwrap();
    let g = Goofi::new();
    let ty = install(&g, "flux_rt.py", &fake_node().replace("time.sleep(0.04)", "time.sleep(0.25)"));
    let node = g.add(&ty);
    g.set_param(node, "model", "root", root.path().to_string_lossy().as_ref());
    g.set_param(node, "smoothing", "output_fps", 30.0);
    g.set_param(node, "transition", "frames", 4);
    let image = g.add("_TestImage");
    g.link(image, "out", node, "source");
    let output = g.probe(node, "image");
    g.set_param(node, "runtime", "state", "start");
    g.until("paced output is faster than inference with four RIFE anchors", |_| {
        output.latest().filter(|d| {
            let meta = flux_meta(d);
            meta["target_fps"] == 30.0 && meta["playback"] == "rife + blend"
                && meta["output_fps"].as_f64().unwrap() > 20.0
                && meta["generation_fps"].as_f64().unwrap() < 4.5
        })
    });
    g.set_param(node, "runtime", "state", "pause");
    let status = g.probe(node, "status");
    g.until("pause stops paced playback", |_| status.latest().filter(|d| d.as_str().is_ok_and(|s| s.contains("Paused"))));
    let seen = output.count();
    assert!(g.stays(|_| output.count() == seen));
    g.set_param(node, "smoothing", "output_fps", 0.0);
    g.set_param(node, "runtime", "state", "start");
    g.until("zero restores RIFE-only delivery live", |_| output.latest().filter(|d| flux_meta(d)["playback"] == "rife"));
    assert!(g.error(node).is_none());
}

#[test]
fn fluxrt_waits_for_memory_and_recovers_without_a_node_error() {
    let _py = require_python();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("configs")).unwrap();
    std::fs::write(root.path().join("configs/benchmark_config.json"), "{}").unwrap();
    let memory = root.path().join("memory.txt");
    let private = root.path().join("private.txt");
    std::fs::write(&memory, "1").unwrap();
    std::fs::write(&private, "2.5").unwrap();
    let source = format!("{}\navailable_commit = lambda: float(Path({}).read_text()) * 1024**3\nprocess_commit = lambda: float(Path({}).read_text()) * 1024**3\n",
                         fake_node(), serde_json::to_string(&memory.to_string_lossy()).unwrap(),
                         serde_json::to_string(&private.to_string_lossy()).unwrap());
    let g = Goofi::new();
    let ty = install(&g, "flux_rt.py", &source);
    let node = g.add(&ty);
    g.set_param(node, "model", "root", root.path().to_string_lossy().as_ref());
    let image = g.add("_TestImage");
    let source_probe = g.probe(image, "out");
    g.link(image, "out", node, "source");
    let output = g.probe(node, "image");
    let status = g.probe(node, "status");
    g.set_param(node, "runtime", "state", "start");
    g.until("low memory waits without a node error", |g| {
        assert!(g.error(node).is_none(), "memory fixture: {:?}", g.error(node));
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.contains("Waiting for memory")))
    });
    assert!(g.error(node).is_none());
    assert_eq!(output.count(), 0);
    let seen = source_probe.count();
    g.until("the source still runs after the refused model load", |_| (source_probe.count() > seen).then_some(()));
    g.set_param(node, "runtime", "state", "stop");
    g.until("Stop cancels the memory wait", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.starts_with("Stopped")))
    });
    g.set_param(node, "runtime", "state", "start");
    g.until("Start waits again", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.contains("Waiting for memory")))
    });
    std::fs::write(&memory, "16.6").unwrap();
    g.until("memory recovery automatically loads below the old 20 GiB limit", |_| output.latest());
    assert!(g.error(node).is_none());
    g.set_param(node, "runtime", "state", "stop");
    g.until("loaded model stops", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.starts_with("Stopped")))
    });
    let seen = output.count();
    std::fs::write(&private, "6").unwrap();
    std::fs::write(&memory, "13").unwrap();
    g.set_param(node, "runtime", "state", "start");
    g.until("warm restart accounts for memory already held by the worker", |_| (output.count() > seen).then_some(()));
}

#[test]
fn fluxrt_mixes_prompts_and_smooths_live_targets_without_rife() {
    let _py = require_python();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("configs")).unwrap();
    std::fs::write(root.path().join("configs/benchmark_config.json"), "{}").unwrap();
    let g = Goofi::new();
    let ty = install(&g, "flux_rt.py", &fake_node());
    let node = g.add(&ty);
    g.set_param(node, "model", "root", root.path().to_string_lossy().as_ref());
    g.set_param(node, "transition", "frames", 1);
    g.set_param(node, "model", "prompt", "warm");
    g.set_param(node, "model", "prompt_b", "cool");
    g.set_param(node, "smoothing", "time", 2.0);
    let src = g.add("_TestImage");
    g.link(src, "out", node, "source");
    let probe = g.probe(node, "image");
    g.set_param(node, "runtime", "state", "start");
    // Encode A explicitly after startup; the fixture's initial embedding is zero.
    g.until("initial generation", |_| probe.latest());
    g.set_param(node, "model", "prompt", "cool");
    g.set_param(node, "transition", "duration", 0.0);
    g.until("A is encoded", |_| probe.latest().filter(|d| (f32s(d)[2]-20.0/255.0).abs() < 0.005));
    g.set_param(node, "model", "prompt", "warm");
    g.until("A endpoint", |_| probe.latest().filter(|d| (f32s(d)[2]-200.0/255.0).abs() < 0.005));
    g.set_param(node, "model", "prompt_mix", 1.0);
    g.until("continuous prompt mix has intermediate pixels", |_| probe.latest().filter(|d| {
        let mix = flux_meta(d)["prompt_mix"].as_f64().unwrap();
        mix > 0.1 && mix < 0.9 && f32s(d)[2] > 0.1 && f32s(d)[2] < 0.75
    }));
    g.set_param(node, "model", "prompt_mix", 0.0);
    g.call("node param pulse", j!({"node":hex(node),"param":"transition/cut"}));
    g.until("cut settles the latest target", |_| probe.latest().filter(|d| {
        flux_meta(d)["cut"] == 1 && flux_meta(d)["prompt_mix"] == 0.0
            && (f32s(d)[2]-200.0/255.0).abs() < 0.005
    }));
    g.set_param(node, "transition", "duration", 2.0);
    g.set_param(node, "model", "prompt", "cool");
    g.until("prompt text still blends with RIFE off", |_| probe.latest().filter(|d| {
        let progress = flux_meta(d)["blend_progress"].as_f64().unwrap();
        progress > 0.1 && progress < 0.9 && f32s(d)[2] > 0.1 && f32s(d)[2] < 0.75
            && flux_meta(d)["rife_frames"] == 1
    }));
    g.set_param(node, "runtime", "state", "stop");
}

#[test]
fn fluxrt_feedback_tracks_motion_and_rejects_unreliable_history() {
    let _py = require_python();
    let g = Goofi::new();
    let helpers = NODE[..NODE.find("class Inference(").unwrap()].replace(IMPORT, "");
    let fixture = format!("{helpers}\n{}", r#"
class FeedbackProbe(goofi.Node):
    OUTPUTS = {'out': goofi.DataType.ARRAY}
    PRODUCER = True
    PARAMS = {'test': {'mode': goofi.IntParam(0, 0, 5)}}
    def process(self):
        rng = np.random.default_rng(72)
        source = cv2.GaussianBlur(rng.integers(0, 256, (128, 128, 3), dtype=np.uint8), (5, 5), 0)
        output = 255-source
        history = Feedback()
        history.remember(source, output)
        mode = self.params.test.mode
        current = np.roll(source, 4, axis=1)
        expected = current*.6 + np.roll(output, 4, axis=1)*.4
        if mode == 1:
            current = rng.integers(0, 256, source.shape, dtype=np.uint8)
        elif mode == 2:
            current = np.full_like(source, 128)
            history.remember(current, output)
        elif mode == 3:
            history.clear()
        mixed, coverage = history.prepare(current, 0 if mode == 4 else .4, mode != 5)
        inner = np.s_[12:-12, 12:-12]
        error = np.abs(mixed[inner]-expected[inner]).mean()
        unaligned = np.abs((current*.6+output*.4)[inner]-expected[inner]).mean()
        unchanged = np.max(np.abs(mixed.astype(float)-current))
        return np.array([mode, coverage, error, unaligned, unchanged], dtype=np.float32)
"#);
    let ty = install(&g, "feedback_probe.py", &fixture);
    let node = g.add(&ty);
    let probe = g.probe(node, "out");
    let aligned = g.until("feedback follows a known source translation", |_| probe.latest());
    let values = f32s(&aligned);
    assert!(values[1] > 0.5, "coverage: {values:?}");
    assert!(values[2] < values[3]*0.5, "alignment: {values:?}");
    for mode in 1..=5 {
        g.set_param(node, "test", "mode", mode);
        let frame = g.until("feedback mode reaches the output", |_| {
            probe.latest().filter(|d| f32s(d)[0] == mode as f32)
        });
        let values = f32s(&frame);
        match mode {
            1 => assert!(values[1] < 0.1, "unrelated noise: {values:?}"),
            2..=4 => assert!(values[1] == 0.0 && values[4] == 0.0, "bypass: {values:?}"),
            5 => assert!((values[1]-1.0).abs() < 0.001 && values[4] > 1.0),
            _ => unreachable!(),
        }
    }
}

#[test]
fn fluxrt_freezes_the_guide_and_toggles_a_reference_live() {
    let _py = require_python();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("configs")).unwrap();
    std::fs::write(root.path().join("configs/benchmark_config.json"), "{}").unwrap();
    let g = Goofi::new();
    let source = r#"
import goofi
import numpy as np
class SolidGuide(goofi.Node):
    PRODUCER = True
    OUTPUTS = {'out': goofi.DataType.ARRAY}
    PARAMS = {'image': {'value': goofi.FloatParam(0.2, 0.0, 1.0)}}
    def process(self):
        return np.full((16, 16, 3), self.params.image.value, dtype=np.float32)
"#;
    let types = install_all(&g, &[("flux_rt.py", &fake_node()), ("solid_guide.py", source)]);
    let node = g.add(&types[0]);
    let guide = g.add(&types[1]);
    let style = g.add(&types[1]);
    g.set_param(style, "image", "value", 0.8);
    g.set_param(node, "model", "root", root.path().to_string_lossy().as_ref());
    g.set_param(node, "transition", "frames", 1);
    g.set_param(node, "transition", "duration", 0.0);
    g.set_param(node, "smoothing", "time", 0.0);
    g.link(guide, "out", node, "source");
    g.link(style, "out", node, "reference");
    let probe = g.probe(node, "image");
    g.set_param(node, "runtime", "state", "start");
    g.until("guide reaches inference", |_| probe.latest().filter(|d| (f32s(d)[1]-0.2).abs() < 0.005));
    g.set_param(node, "source", "freeze", true);
    g.call("node param pulse", j!({"node":hex(node),"param":"transition/cut"}));
    g.until("freeze is applied before the source changes", |_| probe.latest().filter(|d| flux_meta(d)["cut"] == 1));
    g.set_param(guide, "image", "value", 0.5);
    assert!(g.stays(|_| probe.latest().is_some_and(|d| (f32s(&d)[1]-0.2).abs() < 0.005)));
    g.set_param(node, "source", "freeze", false);
    g.until("unfreeze accepts the latest guide", |_| probe.latest().filter(|d| (f32s(d)[1]-0.5).abs() < 0.005));
    g.set_param(node, "reference", "enabled", true);
    g.until("reference is enabled without reloading", |_| probe.latest().filter(|d| flux_meta(d)["reference_enabled"] == true && (f32s(d)[1]-0.8).abs() < 0.005));
    g.set_param(node, "smoothing", "time", 2.0);
    g.set_param(node, "reference", "enabled", false);
    g.until("reference weight fades before omission", |_| probe.latest().filter(|d| {
        let weight = flux_meta(d)["style_strength"].as_f64().unwrap();
        flux_meta(d)["reference_enabled"] == true && weight > 0.1 && weight < 0.9
    }));
    g.until("disabled reference is omitted", |_| probe.latest().filter(|d| flux_meta(d)["reference_enabled"] == false && (f32s(d)[1]-0.5).abs() < 0.005));
    g.set_param(node, "runtime", "state", "stop");
}

#[test]
fn fluxrt_keeps_graph_controls_live_and_delivers_rgb_to_graphics() {
    let _py = require_python();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("configs")).unwrap();
    std::fs::write(root.path().join("configs/benchmark_config.json"), "{}").unwrap();
    let g = Goofi::new();
    let source = r#"
import goofi
import numpy as np
class StyleImage(goofi.Node):
    OUTPUTS = {'out': goofi.DataType.ARRAY}
    PRODUCER = True
    PARAMS = {'image': {'value': goofi.FloatParam(0.2, 0.0, 1.0)}}
    def process(self):
        return np.full((8, 8, 3), self.params.image.value, dtype=np.float32)
"#;
    let types = install_all(&g, &[("flux_rt.py", &fake_node()), ("style_image.py", source)]);
    let node = g.add(&types[0]);
    g.set_param(node, "model", "root", root.path().to_string_lossy().as_ref());
    g.set_param(node, "reference", "enabled", true);
    let style = g.add(&types[1]);
    g.link(style, "out", node, "reference");
    let src = g.add("_TestImage");
    g.link(src, "out", node, "source");
    let probe = g.probe(node, "image");
    let status = g.probe(node, "status");
    g.until("the node waits for Start", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.contains("Stopped")))
    });
    assert_eq!(probe.count(), 0);
    g.set_param(node, "runtime", "state", "start");
    g.until("the model is loading", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.contains("Loading")))
    });
    g.set_param(node, "source", "width", 448);
    let error = g.until("size is fixed during model loading", |g| g.error(node));
    assert!(error.contains("Restart FluxRT after changing source/width or source/height"), "{error}");
    g.set_param(node, "source", "width", 320);
    g.set_param(node, "runtime", "state", "stop");
    g.until("Stop during loading waits for release", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.starts_with("Stopped")))
    });
    g.set_param(node, "runtime", "state", "start");
    g.until("Start begins a new load", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.contains("Loading")))
    });
    g.set_param(node, "runtime", "state", "pause");
    g.until("Pause is accepted during loading", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.contains("Paused")))
    });
    assert!(g.stays(|_| probe.count() == 0), "a model paused during loading must not deliver images");
    g.set_param(node, "runtime", "state", "start");
    let frame = g.until("the asynchronously generated RGB image", |_| {
        probe.latest().filter(|d| shape(d) == vec![320, 320, 3])
    });
    assert!((f32s(&frame)[0] - 52.0 / 255.0).abs() < 0.001);
    assert!((f32s(&frame)[1] - 0.2).abs() < 0.001);
    g.set_param(node, "consistency", "align_motion", false);
    g.set_param(node, "consistency", "strength", 0.2);
    g.until("feedback is active in the persistent runtime", |_| {
        probe.latest().filter(|d| flux_meta(d)["feedback_coverage"].as_f64().unwrap() > 0.99)
    });
    g.set_param(node, "runtime", "state", "pause");
    g.until("feedback can pause", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.contains("Paused")))
    });
    g.call("node param pulse", j!({"node": hex(node), "param": "consistency/reset"}));
    g.set_param(node, "runtime", "state", "start");
    g.until("Reset while paused clears history on resume", |_| {
        probe.latest().filter(|d| flux_meta(d)["feedback_reset"] == 1
            && flux_meta(d)["feedback_coverage"] == 0.0)
    });
    g.set_param(node, "consistency", "strength", 0.0);
    g.until("zero consistency disables feedback", |_| {
        probe.latest().filter(|d| flux_meta(d)["consistency"] == 0.0
            && flux_meta(d)["feedback_coverage"] == 0.0)
    });
    g.set_param(style, "image", "value", 0.8);
    assert!(g.stays(|_| probe.latest().is_some_and(|d| (f32s(&d)[1] - 0.2).abs() < 0.001)),
        "a changing style input does not reset the model until Apply");
    g.set_param(node, "transition", "duration", 3.0);
    g.call("node param pulse", j!({"node": hex(node), "param": "reference/capture"}));
    g.until("Apply blends the style reference over the selected duration", |_| {
        probe.latest().filter(|d| (0.3..0.5).contains(&f32s(d)[1])
            && (0.1..0.6).contains(&flux_meta(d)["blend_progress"].as_f64().unwrap()))
    });
    g.set_param(style, "image", "value", 0.0);
    assert!(g.stays(|_| probe.latest().is_some_and(|d| f32s(&d)[1] > 0.25)),
        "a replacement style waits for Apply during an active blend");
    g.call("node param pulse", j!({"node": hex(node), "param": "reference/capture"}));
    g.until("a replacement style continues from the current reference", |_| {
        probe.latest().filter(|d| flux_meta(d)["blend_progress"].as_f64().unwrap() < 0.15
            && (0.25..0.8).contains(&f32s(d)[1]))
    });
    g.set_param(node, "transition", "duration", 0.2);
    g.until("duration edits complete the remaining style blend", |_| {
        probe.latest().filter(|d| flux_meta(d)["blend_progress"] == 1.0 && f32s(d)[1] == 0.0)
    });
    g.set_param(style, "image", "value", 0.8);
    assert!(g.stays(|_| probe.latest().is_some_and(|d| f32s(&d)[1] == 0.0)));
    g.call("node param pulse", j!({"node": hex(node), "param": "reference/capture"}));
    g.until("Apply captures the latest style image", |_| {
        probe.latest().filter(|d| (f32s(d)[1] - 0.8).abs() < 0.001)
    });
    g.set_param(node, "transition", "duration", 1.0);
    let seed_start = std::time::Instant::now();
    g.set_param(node, "model", "seed", 170);
    g.until("a seed change blends noise over the selected duration", |_| {
        probe.latest().filter(|d| flux_meta(d)["seed"] == 170
            && (0.25..0.65).contains(&flux_meta(d)["blend_progress"].as_f64().unwrap())
            && (0.25..0.62).contains(&f32s(d)[0]))
    });
    g.set_param(node, "model", "seed", 100);
    g.until("a replacement seed starts from the active noise blend", |_| {
        probe.latest().filter(|d| flux_meta(d)["seed"] == 100
            && flux_meta(d)["blend_progress"].as_f64().unwrap() < 0.2
            && (0.25..0.62).contains(&f32s(d)[0]))
    });
    g.set_param(node, "model", "seed", 170);
    g.until("the seed change reaches the persistent runtime", |_| {
        probe.latest().filter(|d| flux_meta(d)["seed"] == 170
            && flux_meta(d)["blend_progress"] == 1.0
            && (f32s(d)[0] - 170.0 / 255.0).abs() < 0.001)
    });
    assert!(seed_start.elapsed().as_secs_f64() >= 1.0);
    g.set_param(node, "transition", "duration", 1.0);
    g.set_param(node, "model", "prompt", "warm");
    let blended = g.until("the prompt produces intermediate pixels and progress", |_| {
        probe.latest().filter(|d| {
            let meta = flux_meta(d);
            let progress = meta["blend_progress"].as_f64().unwrap();
            meta["prompt"] == "warm" && (0.25..0.45).contains(&progress)
                && (0.1..0.5).contains(&f32s(d)[2])
        })
    });
    assert_eq!(flux_meta(&blended)["rife_frames"], 4);
    g.set_param(node, "model", "prompt", "cool");
    g.until("a new prompt continues from the active blend", |_| {
        probe.latest().filter(|d| {
            let meta = flux_meta(d);
            meta["prompt"] == "cool" && meta["blend_progress"].as_f64().unwrap() < 0.1
                && (0.12..0.55).contains(&f32s(d)[2])
        })
    });
    g.set_param(node, "transition", "duration", 0.2);
    g.until("a shorter remaining duration reaches the replacement endpoint", |_| {
        probe.latest().filter(|d| flux_meta(d)["prompt"] == "cool"
            && flux_meta(d)["blend_progress"] == 1.0
            && (f32s(d)[2] - 20.0/255.0).abs() < 0.001)
    });
    g.set_param(node, "transition", "duration", 1.0);
    g.set_param(node, "transition", "frames", 4);
    g.set_param(node, "model", "prompt", "warm");
    g.until("a second blend starts with four interpolation frames", |_| {
        probe.latest().filter(|d| flux_meta(d)["prompt"] == "warm"
            && flux_meta(d)["rife_frames"] == 4
            && flux_meta(d)["blend_progress"].as_f64().unwrap() < 0.5)
    });
    g.set_param(node, "transition", "mode", "cut");
    g.until("cut completes an active blend", |_| {
        probe.latest().filter(|d| flux_meta(d)["transition"] == "cut"
            && flux_meta(d)["blend_progress"] == 1.0
            && (f32s(d)[2] - 200.0/255.0).abs() < 0.001)
    });
    g.set_param(style, "image", "value", 0.4);
    assert!(g.stays(|_| probe.latest().is_some_and(|d| (f32s(&d)[1]-0.8).abs() < 0.001)));
    g.call("node param pulse", j!({"node": hex(node), "param": "reference/capture"}));
    g.until("cut mode applies a style without intermediate images", |_| {
        let d = probe.latest()?;
        if (f32s(&d)[1]-0.8).abs() < 0.001 { return None; }
        assert!((f32s(&d)[1]-0.4).abs() < 0.001);
        Some(d)
    });
    g.set_param(style, "image", "value", 0.8);
    assert!(g.stays(|_| probe.latest().is_some_and(|d| (f32s(&d)[1]-0.4).abs() < 0.001)));
    g.call("node param pulse", j!({"node": hex(node), "param": "reference/capture"}));
    g.until("restore the style for the remaining controls", |_| {
        probe.latest().filter(|d| (f32s(d)[1]-0.8).abs() < 0.001)
    });
    g.set_param(node, "model", "seed", 52);
    g.until("cut mode switches seeds without intermediate images", |_| {
        let d = probe.latest()?;
        if flux_meta(&d)["seed"] != 52 { return None; }
        assert!((f32s(&d)[0] - 52.0/255.0).abs() < 0.001);
        Some(d)
    });
    g.set_param(node, "model", "seed", 170);
    g.set_param(node, "transition", "duration", 0.0);
    g.set_param(node, "transition", "mode", "prompt blend");
    g.set_param(node, "model", "prompt", "cool");
    g.until("zero duration cuts to the new prompt", |_| {
        probe.latest().filter(|d| flux_meta(d)["prompt"] == "cool"
            && flux_meta(d)["blend_progress"] == 1.0
            && (f32s(d)[2] - 20.0/255.0).abs() < 0.001)
    });
    g.set_param(node, "source", "width", 512);
    g.set_param(node, "source", "height", 256);
    let error = g.until("size edits require a restart", |g| g.error(node));
    assert!(error.contains("Restart FluxRT after changing source/width or source/height"), "{error}");
    g.set_param(node, "model", "steps", 8);
    g.set_param(node, "model", "guidance", 2.0);
    g.set_param(node, "source", "influence", 0.5);
    g.set_param(node, "reference", "influence", 0.5);
    g.call("node restart", j!({"node": hex(node)}));
    let probe = g.probe(node, "image");
    let status = g.probe(node, "status");
    g.until("a restart applies the new size and controls", |_| {
        probe.latest().filter(|d| {
            let meta = flux_meta(d);
            shape(d) == vec![256, 512, 3] && meta["steps"] == 8
                && meta["guidance"] == 2.0 && meta["source_strength"] == 0.5
                && meta["style_strength"] == 0.5 && meta["spatial_cache"] == false
                && (f32s(d)[1]-0.4).abs() < 0.005 && meta["output_fps"].as_f64().unwrap() > 0.0
        })
    });
    g.until("FPS is readable in the node's status output", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text|
            text.contains("FPS output") && text.contains("FPS generated") && text.contains("512 x 256")))
    });
    let upload = g.add("graphics:SignalIn");
    g.set_param(upload, "common", "width", 32);
    g.set_param(upload, "common", "height", 32);
    g.link(node, "image", upload, "input");
    let texture = g.probe(upload, "out");
    g.until("FluxRT output uploads through the standard graphics boundary", |g| {
        render(g, 1);
        texture.latest().filter(|d| shape(d) == vec![32, 32, 4]
            && (f32s(d)[0] - 170.0 / 255.0).abs() < 0.002)
    });
    g.set_param(node, "runtime", "state", "pause");
    g.until("Pause holds the model", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.contains("Paused")))
    });
    let paused = probe.count();
    assert!(g.stays(|_| probe.count() == paused), "Pause must stop image delivery");
    g.set_param(node, "runtime", "state", "start");
    g.until("Start resumes the paused model", |_| (probe.count() > paused).then_some(()));
    g.set_param(node, "runtime", "state", "stop");
    g.until("Stop finishes active work and releases the runtime", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.starts_with("Stopped")))
    });
    let stopped = probe.count();
    assert!(g.stays(|_| probe.count() == stopped), "Stop must stop image delivery");
    g.set_param(node, "runtime", "state", "start");
    g.until("Start reloads after Stop without replacing the node", |_| (probe.count() > stopped).then_some(()));
    g.set_param(node, "model", "prompt", "fail");
    let error = g.until("inference failures use the normal node error", |g| g.error(node));
    assert!(error.contains("fixture inference failed"), "{error}");
    g.call("node remove", j!({"node": hex(node)}));
}

#[test]
#[ignore = "requires the downloaded FluxRT models and an NVIDIA GPU"]
fn fluxrt_real_model_runs_in_a_goofi_session() {
    let _py = require_python();
    let root = std::env::var("GOOFI_FLUXRT_ROOT").expect("set GOOFI_FLUXRT_ROOT to the prepared clone");
    let g = Goofi::new();
    let ty = install(&g, "flux_rt.py", NODE);
    let node = g.add(&ty);
    g.set_param(node, "model", "root", root);
    g.set_param(node, "runtime", "state", "start");
    g.set_param(node, "reference", "enabled", true);
    let src = g.add("_TestImage");
    g.link(src, "out", node, "reference");
    g.link(src, "out", node, "source");
    let probe = g.probe(node, "image");
    let status = g.probe(node, "status");
    let frame = g.until("a real FluxRT image in goofi", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| shape(d) == vec![320, 320, 3])
    });
    assert!(f32s(&frame).iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    g.set_param(node, "consistency", "align_motion", false);
    g.set_param(node, "consistency", "strength", 0.2);
    g.until("the real model accepts previous-output feedback", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| flux_meta(d)["feedback_coverage"].as_f64().unwrap() > 0.99
            && f32s(d).iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
    });
    g.call("node param pulse", j!({"node": hex(node), "param": "consistency/reset"}));
    g.until("real feedback resets at an inference boundary", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| flux_meta(d)["feedback_reset"] == 1
            && flux_meta(d)["feedback_coverage"] == 0.0)
    });
    g.set_param(node, "consistency", "strength", 0.0);
    g.set_param(node, "source", "width", 448);
    let error = g.until("a real model rejects live size changes", |g| g.error(node));
    assert!(error.contains("Restart FluxRT after changing source/width or source/height"), "{error}");
    g.set_param(node, "source", "width", 320);
    let seen = probe.count();
    g.until("restoring the size resumes the existing model", |g| {
        (g.error(node).is_none() && probe.count() > seen).then_some(())
    });
    g.set_param(node, "source", "width", 448);
    g.set_param(node, "runtime", "state", "stop");
    g.until("Stop unloads the real model", |_| {
        status.latest().filter(|d| d.as_str().is_ok_and(|text| text.starts_with("Stopped")))
    });
    g.set_param(node, "runtime", "state", "start");
    g.until("the model reloads at 448 by 320", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| shape(d) == vec![320, 448, 3])
    });
    g.set_param(node, "transition", "duration", 2.0);
    g.set_param(node, "model", "seed", 170);
    g.until("real seed noise produces an intermediate blend", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| flux_meta(d)["seed"] == 170
            && (0.1..0.9).contains(&flux_meta(d)["blend_progress"].as_f64().unwrap())
            && f32s(d).iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
    });
    g.until("real seed noise reaches the target", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| flux_meta(d)["seed"] == 170
            && flux_meta(d)["blend_progress"] == 1.0)
    });
    let prompt = "Turn this into blue and gold watercolor shapes.";
    g.set_param(node, "model", "prompt", prompt);
    g.until("the real model emits a prompt blend", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| {
            let meta = flux_meta(d);
            meta["prompt"] == prompt && meta["blend_progress"].as_f64().unwrap() < 1.0
                && meta["rife_frames"] == 4
        })
    });
    let next = "Turn this into a surreal midnight garden with luminous flowers.";
    g.set_param(node, "model", "prompt", next);
    g.until("the real model completes a replacement prompt during blending", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| flux_meta(d)["prompt"] == next
            && flux_meta(d)["blend_progress"] == 1.0)
    });
    g.set_param(node, "source", "width", 512);
    g.set_param(node, "source", "height", 256);
    g.set_param(node, "model", "steps", 4);
    g.set_param(node, "model", "guidance", 1.5);
    g.set_param(node, "source", "influence", 0.5);
    g.set_param(node, "reference", "influence", 0.0);
    g.call("node restart", j!({"node": hex(node)}));
    let probe = g.probe(node, "image");
    g.until("real source, style, CFG and steps controls produce an image at the restarted size", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| {
            let meta = flux_meta(d);
            shape(d) == vec![256, 512, 3] && meta["steps"] == 4 && meta["guidance"] == 1.5
                && meta["source_strength"] == 0.5 && meta["style_strength"] == 0.0
                && f32s(d).iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        })
    });
    g.set_param(node, "model", "steps", 8);
    g.until("eight steps run with spatial caching disabled", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| flux_meta(d)["steps"] == 8 && flux_meta(d)["spatial_cache"] == false)
    });
    g.set_param(node, "source", "width", 320);
    g.set_param(node, "source", "height", 320);
    g.set_param(node, "model", "steps", 2);
    g.set_param(node, "model", "guidance", 1.0);
    g.call("node restart", j!({"node": hex(node)}));
    let probe = g.probe(node, "image");
    g.until("the fast preset returns after larger uncached generation", |g| {
        if let Some(error) = g.error(node) { panic!("FluxRT: {error}"); }
        probe.latest().filter(|d| shape(d) == vec![320, 320, 3]
            && flux_meta(d)["steps"] == 2 && flux_meta(d)["spatial_cache"] == true)
    });
    let upload = g.add("graphics:SignalIn");
    g.set_param(upload, "common", "width", 320);
    g.set_param(upload, "common", "height", 320);
    g.link(node, "image", upload, "input");
    let texture = g.probe(upload, "out");
    g.until("real generation reaches the graphics engine", |g| {
        render(g, 1);
        texture.latest().filter(|d| shape(d) == vec![320, 320, 4])
    });
}
