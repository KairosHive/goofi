//! Local image files enter the same RGB graph interface as live sources.
use goofi_tests::{Goofi, f32s, hex, install, j, require_python, shape};

#[test]
fn image_file_loads_resizes_reloads_and_recovers() {
    let _py = require_python();
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("reference.ppm");
    let write = |rgb: [u8; 3]| {
        let mut bytes = b"P6\n128 64\n255\n".to_vec();
        for _ in 0..128*64 { bytes.extend(rgb); }
        std::fs::write(&path, bytes).unwrap();
    };
    write([255, 128, 0]);
    let g = Goofi::new();
    let ty = install(&g, "image_file.py", include_str!("../../../node-bundles/image/image_file.py"));
    let node = g.add(&ty);
    let probe = g.probe(node, "image");
    g.set_param(node, "file", "path", path.to_string_lossy().as_ref());
    let image = g.until("local image becomes RGB data", |_| probe.latest());
    assert_eq!(shape(&image), vec![64, 128, 3]);
    assert_eq!(&f32s(&image)[..3], &[1.0, 128.0/255.0, 0.0]);
    g.set_param(node, "image", "max_size", 64);
    g.until("resize preserves aspect ratio", |_| probe.latest().filter(|d| shape(d) == vec![32, 64, 3]));
    write([0, 0, 255]);
    g.call("node param pulse", j!({"node":hex(node),"param":"file/reload"}));
    g.until("reload replaces image pixels", |_| probe.latest().filter(|d| f32s(d)[2] == 1.0));
    g.set_param(node, "file", "path", folder.path().join("missing.png").to_string_lossy().as_ref());
    g.until("missing image reports an error", |g| g.error(node));
    g.set_param(node, "file", "path", path.to_string_lossy().as_ref());
    let seen = probe.count();
    g.until("valid path recovers without a restart", |g| (g.error(node).is_none() && probe.count() > seen).then_some(()));
}
