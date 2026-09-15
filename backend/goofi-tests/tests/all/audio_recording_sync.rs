//! Simultaneous tracks share exact boundaries, even when the external clock
//! renders faster than the file writer and stop follows the last block at once.

use goofi_tests::{drive, ep, hex, j, Goofi};

#[test]
fn repeated_audio_takes_keep_every_track_sample_aligned() {
    let g = Goofi::new();
    let root = tempfile::tempdir().unwrap();
    let source = g.add("audio:Osc");
    let gain = g.add("audio:Gain");
    g.set_param(gain, "gain", "gain", 0.5);
    g.link(source, "out", gain, "input");
    g.ready(source);
    g.ready(gain);
    for node in [source, gain] {
        g.call("record arm", j!({ "output": ep(hex(node), "out") }));
    }

    // Enough takes to wrap the audio rings. No warm-up drive, subscriber poll,
    // or sleep is allowed to hide a start/stop race.
    for take in 0..30 {
        let started = g.call("record start", j!({ "root": root.path(), "name": format!("sync-{take}") }));
        let folder = std::path::PathBuf::from(started["folder"].as_str().unwrap());
        drive(&g, 64 * 480);
        g.call("record stop", j!({}));
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(folder.join("manifest.json")).unwrap(),
        ).unwrap();
        let streams = manifest["streams"].as_array().unwrap();
        assert_eq!(streams.len(), 2, "take {take}: {manifest}");
        let entry = |node| streams.iter().find(|s| s["uid"] == hex(node)).unwrap();
        let a = entry(source);
        let b = entry(gain);
        for stream in [a, b] {
            assert_eq!(stream["frames"], 64 * 480, "take {take}: {stream}");
            assert_eq!(stream["dropped"], 0, "take {take}: {stream}");
        }
        assert_eq!(a["t0_patch"], b["t0_patch"], "the first sample is shared in take {take}");
        let read = |entry: &serde_json::Value| {
            let path = folder.join(entry["file"].as_str().unwrap());
            let wav = std::fs::read(&path).unwrap();
            assert_eq!(&wav[..4], b"RIFF");
            let samples = wav[44..].chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect::<Vec<_>>();
            let blocks = std::fs::read_to_string(path.with_extension("jsonl")).unwrap()
                .lines().map(|line| {
                    let row: serde_json::Value = serde_json::from_str(line).unwrap();
                    (row["meta"]["index"].as_u64().unwrap(), row["t"].as_f64().unwrap())
                }).collect::<Vec<_>>();
            (samples, blocks)
        };
        let (a, a_blocks) = read(a);
        let (b, b_blocks) = read(b);
        assert_eq!(a_blocks, b_blocks, "block indices and times match in take {take}");
        assert_eq!(a.len(), 64 * 480);
        assert_eq!(b.len(), a.len());
        assert!(a.iter().any(|sample| sample.abs() > 0.5), "the take contains audio");
        for (i, (a, b)) in a.iter().zip(&b).enumerate() {
            assert_eq!(*b, *a * 0.5, "sample {i} in take {take}");
        }
    }
}
