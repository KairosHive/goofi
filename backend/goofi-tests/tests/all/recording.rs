//! Recording: every engine's frames, on one timeline.

/// A machine with no video encoder, which is what a machine without ffmpeg is. The recorder holds
/// its encoders behind one trait, so this needs no `PATH` and no second door through the product.
struct NoEncoder;

impl NoEncoder {
    const WHY: &'static str = "install the `ffmpeg` package";
}

/// An encoder whose `finish` blocks until it is let go — what a slow ffmpeg finalize is. It is
/// the same seam a second codec would arrive through, driven from the other end.
struct SlowEncoders {
    inside: std::sync::Arc<std::sync::atomic::AtomicBool>,
    release: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

struct Slow {
    inside: std::sync::Arc<std::sync::atomic::AtomicBool>,
    release: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl goofi_record::video::Encoders for SlowEncoders {
    fn probe(&self) -> Result<(), String> {
        Ok(())
    }
    fn extension(&self) -> &'static str {
        "mkv"
    }
    fn open(
        &self,
        _file: &std::path::Path,
        _size: (u32, u32),
        _fps: f64,
        _quality: goofi_core::record::VideoQuality,
    ) -> Result<Box<dyn goofi_record::video::Encoder>, String> {
        Ok(Box::new(Slow { inside: self.inside.clone(), release: self.release.clone() }))
    }
}

impl goofi_record::video::Encoder for Slow {
    fn write(&mut self, _rows: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn finish(&mut self) -> Result<(), String> {
        self.inside.store(true, std::sync::atomic::Ordering::Relaxed);
        while !self.release.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(())
    }
}

impl goofi_record::video::Encoders for NoEncoder {
    fn probe(&self) -> Result<(), String> {
        Err(NoEncoder::WHY.into())
    }
    fn extension(&self) -> &'static str {
        "mkv"
    }
    fn open(
        &self,
        _file: &std::path::Path,
        _size: (u32, u32),
        _fps: f64,
        _quality: goofi_core::record::VideoQuality,
    ) -> Result<Box<dyn goofi_record::video::Encoder>, String> {
        Err(NoEncoder::WHY.into())
    }
}

use goofi_core::time::{stamp, Time};
use goofi_tests::j;

#[test]
fn utc_is_anchored_once_and_advances_monotonically() {
    let t = Time::new();
    let a = t.utc();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let b = t.utc();
    let step = b.duration_since(a).expect("utc never goes backwards");
    assert!(step.as_millis() >= 15, "utc advances with the monotonic clock: {step:?}");
    // The anchor is the origin, so an instant read at a patch second is that second past it.
    let at = t.utc_at(1.5);
    let d = at.duration_since(a).expect("later than the first read");
    assert!(d.as_secs_f64() > 1.4 && d.as_secs_f64() < 1.6, "utc_at follows patch seconds: {d:?}");
    assert_eq!(stamp(a).len(), "YYYYMMDD-HHMMSS.mmm".len());
}

#[test]
fn a_recording_is_a_folder_of_files_their_own_tools_open() {
    let dir = tempfile::tempdir().expect("a temp root");
    let time = std::sync::Arc::new(Time::new());
    let rec = std::sync::Arc::new(goofi_record::Recorder::new(time.clone()));
    rec.start(dir.path(), "probe", None, None).expect("started");
    let id = goofi_record::StreamId {
        uid: goofi_node::Uid(1),
        node: "src".into(),
        slot: "out".into(),
        engine: "signal",
    };
    let array = |shape: Vec<usize>, fill: f32, i: u64| {
        let mut meta = goofi_core::Meta::empty();
        meta.set_time(Some(i as f64 / 256.0));
        meta.set_index(Some(i));
        let n: usize = shape.iter().product();
        let body: Vec<u8> = (0..n).flat_map(|_| fill.to_le_bytes()).collect();
        goofi_codec::encode(&goofi_core::Data::array_f32(shape, body, meta).expect("a frame"))
    };
    let open = |bytes: &[u8]| {
        let kind = goofi_record::frame::read(bytes, None).expect("a frame the recorder reads").kind;
        rec.open(&id, kind, 0.0, goofi_record::StreamMeta::measured(Some(256.0))).expect("opened");
    };
    let hand = |bytes: &[u8]| {
        assert!(
            rec.take_frame(&id, bytes, None, goofi_record::Timeline::Measured, 0.0, false),
            "the writer took the frame"
        );
    };
    open(&array(vec![4], 0.0, 0));
    for i in 0..8u64 {
        hand(&array(vec![4], i as f32, i));
    }
    // Step: a frame that RESHAPES stays in the same file. The `.npy` is flat and the sidecar's
    // shape line is what splits it, so a node whose shape moves — a filling `Buffer`, a peak
    // count — no longer mints a file per tick.
    hand(&array(vec![6], 8.0, 8));
    hand(&array(vec![6], 9.0, 9));
    // Step: a re-arm at the very same patch instant is a NEW file, never the last one truncated.
    open(&array(vec![4], 0.0, 0));
    hand(&array(vec![4], 99.0, 0));
    let folder = rec.stop().expect("the manifest written").expect("a folder");

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("manifest.json")).expect("a manifest"))
            .expect("json");
    assert_eq!(manifest["streams"][0]["frames"], 10, "eight of one shape, then two of another");
    assert_eq!(manifest["streams"][1]["frames"], 1);
    assert_ne!(
        manifest["streams"][0]["file"], manifest["streams"][1]["file"],
        "two entries never name one file"
    );
    assert!(manifest["origin_utc"].is_string(), "the one anchor every file adds to");
    assert_eq!(
        manifest["streams"][0].get("frame"),
        None,
        "a file whose shape MOVED states no one shape: the sidecar is its index"
    );
    assert_eq!(
        manifest["streams"][1]["frame"],
        j!([4]),
        "a file every frame agreed on states the shape that folds it back"
    );

    let file = folder.join(manifest["streams"][0]["file"].as_str().expect("a name"));
    assert_eq!(file.extension().and_then(|e| e.to_str()), Some("npy"), "an array is a .npy");
    assert!(
        file.file_name().unwrap().to_string_lossy().contains("src-out__"),
        "the name carries node, slot and the first sample's UTC"
    );

    // …and the file numpy would open: a FLAT header counting every value the stream carried, and
    // the samples that follow are the ones written, in order, whatever shape each came in.
    let bytes = std::fs::read(&file).expect("the stream");
    let (head, body) = npy(&bytes);
    assert!(head.contains("'descr': '<f4'") && head.contains("(44,)"), "{head}");
    let values: Vec<f32> =
        body.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four"))).collect();
    assert_eq!(values.len(), 44, "eight frames of four, then two of six");
    assert_eq!(values[0], 0.0, "{values:?}");
    assert_eq!(values[28], 7.0, "the last frame of the first shape: {values:?}");
    assert_eq!(values[32], 8.0, "the reshaped frame follows it in the same file: {values:?}");
    assert_eq!(values[43], 9.0, "the last frame is the last one written: {values:?}");

    // …and the sidecar, one line per frame, carrying the instant, the shape where it MOVED, and
    // the meta a .npy cannot hold. The shape supersedes a row count: `n` would say the same
    // thing twice, so an array line does not carry one.
    let beside = std::fs::read_to_string(file.with_extension("jsonl")).expect("the sidecar");
    let lines: Vec<serde_json::Value> =
        beside.lines().map(|l| serde_json::from_str(l).expect("a json line")).collect();
    assert_eq!(lines.len(), 10, "one line per frame");
    assert_eq!(lines[0]["t"].as_f64(), Some(0.0), "the line carries the frame's own instant");
    assert_eq!(lines[0]["shape"], j!([4]), "…and the shape the first frame came in");
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(line.get("n"), None, "an array line carries no row count: line {i}");
    }
    for (i, line) in lines.iter().enumerate().take(8).skip(1) {
        assert_eq!(line.get("shape"), None, "a shape that HELD is not said again: line {i}");
    }
    assert_eq!(lines[8]["shape"], j!([6]), "…and the one that moved is said once");
    assert_eq!(lines[9].get("shape"), None, "…and then held again");
    assert_eq!(lines[9]["meta"]["index"], j!(9), "the meta rides beside the samples");

    // Step: a KILLED writer loses the tail and nothing else — the property every other format
    // this replaced was rejected over. Truncated mid-frame, the whole values are still there.
    let cut = folder.join("cut.npy");
    std::fs::write(&cut, &bytes[..bytes.len() - 6]).expect("a truncated copy");
    let (_, body) = npy(&std::fs::read(&cut).expect("the truncated stream"));
    assert_eq!(body.len() / 4, 42, "forty-two whole values survive a kill inside the last frame");
    rec.start(dir.path(), "audio-formats", None, None).expect("audio recording started");
    for (i, (channels, rate, samples)) in
        [(1, 48_000.0, 4), (1, 48_000.0, 6), (2, 48_000.0, 5), (2, 96_000.0, 3)].into_iter().enumerate()
    {
        let mut meta = goofi_core::Meta::empty();
        meta.set_sfreq(Some(rate));
        meta.set_channels(goofi_core::Axes::new().with(
            0,
            goofi_core::Axis::coords(
                (0..channels).map(|c| goofi_core::Coord::Str(c.to_string().into())).collect::<Vec<_>>(),
            ),
        ));
        let values: Vec<u8> = (0..channels * samples).flat_map(|n| (n as f32).to_le_bytes()).collect();
        let data = goofi_core::Data::array_f32(vec![channels, samples], values, meta).expect("audio block");
        assert!(rec.take_frame(
            &id, &goofi_codec::encode(&data), Some(rate), goofi_record::Timeline::Derived, i as f64, true,
        ));
    }
    let folder = rec.stop().expect("audio stopped").expect("audio folder");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(folder.join("manifest.json")).expect("audio manifest"),
    ).expect("json");
    let entries = manifest["streams"].as_array().expect("streams");
    assert_eq!(entries.len(), 3, "block length stays legal; each format change opens a file");
    for (entry, (channels, rate, counts)) in entries.iter().zip([
        (1u16, 48_000u32, vec![4u64, 6]), (2, 48_000, vec![5]), (2, 96_000, vec![3]),
    ]) {
        let path = folder.join(entry["file"].as_str().expect("file name"));
        let wav = std::fs::read(&path).expect("WAV file");
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), channels);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), rate);
        let samples = counts.iter().sum::<u64>();
        assert_eq!(wav.len() as u64 - 44, samples * u64::from(channels) * 4);
        assert_eq!(entry["frames"], samples);
        assert_eq!(entry["channels"], channels);
        assert_eq!(entry["sfreq"].as_f64(), Some(f64::from(rate)));
        let beside = std::fs::read_to_string(path.with_extension("jsonl")).expect("sidecar");
        let written: Vec<u64> = beside.lines().map(|line| {
            serde_json::from_str::<serde_json::Value>(line).expect("sidecar row")["n"].as_u64().unwrap()
        }).collect();
        assert_eq!(written, counts);
    }

    use std::sync::atomic::{AtomicBool, Ordering};
    for restart in [false, true] {
        let inside = std::sync::Arc::new(AtomicBool::new(false));
        let release = std::sync::Arc::new(AtomicBool::new(false));
        rec.set_encoders(std::sync::Arc::new(SlowEncoders { inside: inside.clone(), release: release.clone() }));
        let old = rec.start(dir.path(), &format!("slow-{restart}"), None, None).expect("started");
        rec.open(&id, goofi_record::Kind::Video { size: (1, 1), fps: 60.0, quality: Default::default() }, time.now(),
            goofi_record::StreamMeta::measured(None)).expect("video opened");
        let closing = rec.clone();
        let stream = id.clone();
        let close = std::thread::spawn(move || closing.close(&stream, "disarmed"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !inside.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let entered = inside.load(Ordering::Relaxed);
        let stopped = if entered { rec.stop() } else { Ok(None) };
        let next = if entered && restart {
            rec.start(dir.path(), "next", None, None).map(Some)
        } else { Ok(None) };
        release.store(true, Ordering::Relaxed);
        close.join().expect("close joined");
        assert!(entered, "the close reached the slow encoder");
        assert!(stopped.expect("stopped").is_some());
        let next = next.expect("next recording started");
        let old_manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(old.join("manifest.json")).expect("old manifest"),
        ).expect("json");
        assert_eq!(old_manifest["streams"].as_array().unwrap().len(), 1);
        assert_eq!(old_manifest["streams"][0]["closed_because"], "disarmed");
        assert!(old_manifest["stopped_utc"].is_string());
        if let Some(next) = next {
            rec.stop().expect("next recording stopped");
            let next_manifest: serde_json::Value = serde_json::from_slice(
                &std::fs::read(next.join("manifest.json")).expect("next manifest"),
            ).expect("json");
            assert_eq!(next_manifest["streams"], j!([]));
        }
    }

}

/// A `.npy` split into its header text and its samples, the way `np.load` reads one.
fn npy(bytes: &[u8]) -> (String, Vec<u8>) {
    assert_eq!(&bytes[..6], b"\x93NUMPY", "the numpy magic");
    let len = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    let head = String::from_utf8(bytes[10..10 + len].to_vec()).expect("ascii");
    ((head), bytes[10 + len..].to_vec())
}

#[test]
fn arming_survives_a_rewire_and_rides_the_document() {
    let g = goofi_tests::Goofi::new();
    let src_uid = g.add("_TestConst");
    let src = goofi_tests::hex(src_uid);
    let dst = goofi_tests::hex(g.add("_TestSink"));

    g.call("record arm", j!({ "output": goofi_tests::ep(&src, "out") }));
    assert_eq!(g.doc()["nodes"][&src]["record"], j!([{ "slot": "out", "quality": "high" }]), "arming is the node's own record");

    let status = g.call("record status", j!({}));
    assert_eq!(status["running"], j!(false), "arming a slot does not begin a recording");

    g.call("link add", j!({ "from": goofi_tests::ep(&src, "out"), "to": goofi_tests::ep(&dst, "input") }));
    assert_eq!(g.doc()["nodes"][&src]["record"], j!([{ "slot": "out", "quality": "high" }]), "a re-wire cannot disarm a recording");

    g.call("undo", j!({}));
    g.call("undo", j!({}));
    assert_eq!(g.doc()["nodes"][&src]["record"], j!([]), "arming has an exact inverse");

    g.call("redo", j!({}));
    assert_eq!(g.doc()["nodes"][&src]["record"], j!([{ "slot": "out", "quality": "high" }]), "and the arm comes back");

    let root = tempfile::tempdir().expect("a temp root");
    let folder = g.call("record start", j!({ "name": "walk", "root": root.path() }))["folder"]
        .as_str()
        .expect("a folder")
        .to_string();
    let status = g.call("record status", j!({}));
    assert_eq!(status["running"], j!(true));
    assert_eq!(status["folder"], j!(folder));
    assert_eq!(g.refuse("record start", j!({ "root": root.path() })), "record start: a recording already runs");

    // The drain is what makes an armed slot reach the disk: nothing here writes a frame by hand.
    let name_of = |g: &goofi_tests::Goofi, hex: &str| -> String {
        g.doc()["nodes"][hex]["name"].as_str().expect("a name").to_string()
    };
    let frames = |g: &goofi_tests::Goofi, node: &str| -> u64 {
        g.call("record status", j!({}))["streams"]
            .as_array()
            .map(|s| s.iter().filter(|e| e["node"] == j!(node)).filter_map(|e| e["frames"].as_u64()).sum())
            .unwrap_or(0)
    };
    let manifest = |folder: &str| -> serde_json::Value {
        let path = std::path::Path::new(folder).join("manifest.json");
        serde_json::from_slice(&std::fs::read(path).expect("a manifest")).expect("json")
    };
    let src_name = name_of(&g, &src);
    g.until("the armed slot to reach the disk", |g| (frames(g, &src_name) > 0).then_some(()));

    // Step: a slot armed while the recording ALREADY runs opens a file of its own.
    let late = g.add("_TestConst");
    let late_hex = goofi_tests::hex(late);
    g.ready(late);
    g.call("record arm", j!({ "output": goofi_tests::ep(&late_hex, "out") }));
    let late_name = name_of(&g, &late_hex);
    g.until("the late arm to reach the disk", |g| (frames(g, &late_name) > 0).then_some(()));

    // Step: a rebirth is a real discontinuity, so it is a NEW file and the manifest says why.
    let mine = |folder: &str, node: &str| -> Vec<serde_json::Value> {
        manifest(folder)["streams"]
            .as_array()
            .expect("streams")
            .iter()
            .filter(|e| e["node"] == j!(node))
            .cloned()
            .collect()
    };
    g.call("node restart", j!({ "node": &src }));
    g.ready(src_uid);
    g.until("the reborn slot to write a second file", |g| {
        let held = mine(&folder, &src_name);
        (held.len() >= 2 && frames(g, &src_name) > 0).then_some(())
    });
    let held = mine(&folder, &src_name);
    assert_eq!(held[0]["closed_because"], j!("reborn"), "the manifest names why the second file began");
    assert_ne!(held[0]["file"], held[1]["file"], "a rebirth never appends to the file before it");
    assert!(held[0]["frames"].as_u64().unwrap_or(0) > 0, "the file before the rebirth holds its frames");

    // Step: a graphics slot records VIDEO. Arming is what puts the stage in demand, and every
    // encoded frame's instant goes in a sidecar, which a dropped frame makes the only timing.
    let shader = g.add("graphics:Constant");
    let shader_hex = goofi_tests::hex(shader);
    g.ready(shader);
    g.set_param(shader, "common", "width", 32);
    g.set_param(shader, "common", "height", 16);
    g.set_param(shader, "colour", "r", 0.5);
    let shader_name = g.doc()["nodes"][&shader_hex]["name"].as_str().expect("a name").to_string();
    let stages = |g: &goofi_tests::Goofi| {
        g.call("session status", j!({}))["graphics"]["stages"].as_u64().expect("a stage count")
    };
    let idle = stages(&g);
    goofi_tests::render(&g, 4);
    assert_eq!(stages(&g), idle, "a node nobody watches renders nothing");
    g.call("record arm", j!({ "output": goofi_tests::ep(&shader_hex, "out") }));
    g.until("the armed shader to reach the encoder", |g| {
        goofi_tests::render(g, 1);
        (frames(g, &shader_name) >= 4).then_some(())
    });
    assert!(stages(&g) > idle, "arming is what puts the stage in demand");

    // Step: a resize is a NEW file — a container holds one size — and a readback still in flight
    // from the size before it never lands in the file the new size opened.
    let videos = |_g: &goofi_tests::Goofi| -> Vec<serde_json::Value> {
        manifest(&folder)["streams"]
            .as_array()
            .expect("streams")
            .iter()
            .filter(|e| e["node"] == j!(shader_name) && e["engine"] == j!("graphics"))
            .cloned()
            .collect()
    };
    g.set_param(shader, "common", "width", 48);
    g.until("the resized shader to open a second video", |g| {
        goofi_tests::render(g, 1);
        (videos(g).len() >= 2).then_some(())
    });
    assert_eq!(videos(&g)[0]["closed_because"], j!("resized"), "{:?}", videos(&g)[0]);
    let so_far = frames(&g, &shader_name);
    g.until("the second video to hold frames of its own", |g| {
        goofi_tests::render(g, 1);
        (frames(g, &shader_name) >= so_far + 4).then_some(())
    });
    g.call("record disarm", j!({ "output": goofi_tests::ep(&shader_hex, "out") }));
    // The ENGINE closes what a disarm departed, so the re-arm below is a new file rather than a
    // pair that cancels inside one tick and leaves the old encoder open.
    g.until("the disarmed video to be closed", |g| {
        goofi_tests::render(g, 1);
        videos(g).iter().any(|e| e["closed_because"] == j!("disarmed")).then_some(())
    });

    // Step: a machine that cannot encode costs the GRAPHICS stream and nothing else. The node
    // wears the standing error every other node failure is worn as, and the recording carries on.
    g.state.recorder.set_encoders(std::sync::Arc::new(NoEncoder));
    let before = frames(&g, &src_name);
    g.call("record arm", j!({ "output": goofi_tests::ep(&shader_hex, "out") }));
    let said = g.until("the shader to wear what it could not record", |g| {
        goofi_tests::render(g, 1);
        g.error(shader)
    });
    assert!(said.contains(NoEncoder::WHY), "the error names the package to install: {said}");
    assert_eq!(g.call("record status", j!({}))["running"], j!(true), "one stream's failure ends nothing");
    g.until("the signal streams to keep recording through it", |g| (frames(g, &src_name) > before).then_some(()));
    g.call("record disarm", j!({ "output": goofi_tests::ep(&shader_hex, "out") }));
    g.until("the error to clear with the arming that caused it", |g| {
        goofi_tests::render(g, 1);
        g.error(shader).is_none().then_some(())
    });

    g.call("record disarm", j!({ "output": goofi_tests::ep(&src, "out") }));
    assert_eq!(g.doc()["nodes"][&src]["record"], j!([]), "disarming empties the node's record");

    g.call("record stop", j!({}));
    assert_eq!(g.call("record status", j!({}))["running"], j!(false));
    assert!(std::path::Path::new(&folder).join("manifest.json").exists(), "the folder holds a manifest");

    // …and the video the graphics slot left is a file, an instant per encoded frame beside it,
    // and a manifest that says IN WORDS what the encoding cost. Never a bare "lossless".
    let m = manifest(&folder);
    let video = m["streams"]
        .as_array()
        .expect("streams")
        .iter()
        .find(|e| e["node"] == j!(shader_name))
        .expect("the graphics stream is in the one manifest");
    assert_eq!(video["engine"], j!("graphics"), "{video}");
    assert_eq!(video["size"], j!([32, 16]), "{video}");
    let says = video["encoding"].as_str().expect("what the encoding cost, in words");
    assert!(!says.contains("lossless") && says.contains("clipped"), "a video never claims it: {says}");
    let file = std::path::Path::new(&folder).join(video["file"].as_str().expect("a name"));
    assert_eq!(file.extension().and_then(|e| e.to_str()), Some("mkv"), "{file:?}");
    let written = std::fs::read(&file).expect("the video");
    assert_eq!(&written[..4], &[0x1a, 0x45, 0xdf, 0xa3], "a Matroska file, and the encoder finished it");
    assert!(written.len() > 512, "the encoder wrote frames, not a header: {} bytes", written.len());
    // The sidecar is the ONE shape every stream's is, video included: a line per frame.
    let counted = video["frames"].as_u64().expect("a count");
    assert!(counted >= 4, "every armed tick reached the encoder: {counted}");
    let instants = beside_instants(&file);
    assert_eq!(instants.len() as u64, counted, "one line per encoded frame");
    for pair in instants.windows(2) {
        assert!(pair[1] > pair[0], "the instants are the patch's own seconds, in order: {instants:?}");
    }

    // Both videos, each with its own size and its own instants: a resize costs no correspondence.
    for entry in m["streams"].as_array().expect("streams").iter().filter(|e| {
        e["node"] == j!(shader_name) && e["engine"] == j!("graphics") && e["frames"] != j!(0)
    }) {
        let made = std::path::Path::new(&folder).join(entry["file"].as_str().expect("a name"));
        let held = entry["frames"].as_u64().expect("a count");
        assert_eq!(beside_instants(&made).len() as u64, held, "one line per encoded frame: {entry}");
        assert!(held >= 4, "each file holds its own frames: {entry}");
        // Decoded, because only the container itself says whether the frames in it line up: a
        // readback of the wrong size makes a file that still counts but no longer decodes to it.
        let size = entry["size"].as_array().expect("a size");
        let texels = size[0].as_u64().expect("width") * size[1].as_u64().expect("height") * 8;
        let read = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&made)
            .args(["-f", "rawvideo", "-pix_fmt", "rgba64le", "-"])
            .output()
            .expect("ffmpeg reads back what it wrote");
        assert_eq!(read.stdout.len() as u64, held * texels, "every frame in the file is one frame: {entry}");
        assert!(read.status.success(), "{}", String::from_utf8_lossy(&read.stderr));
        let red = u16::from_le_bytes(read.stdout[..2].try_into().unwrap()) as f32 / 65535.0;
        assert!((red - 0.5).abs() < 0.04, "the GPU readback keeps the red channel: {red}");
        // …and what a PLAYER finds in it: three components at the rate the manifest states. A
        // 16-bit alpha plane is one VLC cannot allocate, and it plays such a file as nothing.
        let probed = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "v:0"])
            .args(["-show_entries", "stream=codec_name,pix_fmt,r_frame_rate", "-of", "csv=p=0"])
            .arg(&made)
            .output()
            .expect("ffprobe reads what the encoder wrote");
        let says = String::from_utf8_lossy(&probed.stdout).trim().to_string();
        let (codec, format) = says.split_once(',').expect("ffprobe names a codec");
        assert_eq!(codec, "h264", "the recording uses the delivery codec: {says}");
        let (pix, rate) = format.split_once(',').expect("ffprobe names a pixel format and a rate");
        assert_eq!(pix, "yuv420p", "the recorded stream carries no alpha plane: {says}");
        let fps = entry["fps"].as_f64().expect("the rate the manifest states");
        assert_eq!(rate, format!("{fps}/1"), "the container is timed at the rate the manifest states");
    }
    let sizes: Vec<&serde_json::Value> = m["streams"]
        .as_array()
        .expect("streams")
        .iter()
        .filter(|e| e["node"] == j!(shader_name) && e["frames"] != j!(0))
        .map(|e| &e["size"])
        .collect();
    assert_eq!(sizes, vec![&j!([32, 16]), &j!([48, 16])], "each file says the size it holds");

    // …and the stream that could not be encoded is an ENTRY, never a gap the folder leaves
    // unexplained.
    let refused = m["streams"]
        .as_array()
        .expect("streams")
        .iter()
        .find(|e| e["node"] == j!(shader_name) && e["frames"] == j!(0))
        .expect("the stream that never opened is in the manifest too");
    assert_eq!(refused["closed_because"], j!("never opened"), "{refused}");
    assert!(refused["error"].as_str().is_some_and(|e| e.contains(NoEncoder::WHY)), "{refused}");

    // Step: a recording of NOTHING but video, on a machine that cannot encode, is the one case
    // where refusing is the honest answer.
    g.call("record disarm", j!({ "output": goofi_tests::ep(&late_hex, "out") }));
    g.call("record arm", j!({ "output": goofi_tests::ep(&shader_hex, "out") }));
    let refusal = g.refuse("record start", j!({ "root": root.path() }));
    assert!(refusal.contains(NoEncoder::WHY), "the refusal names the package: {refusal}");
    g.call("record disarm", j!({ "output": goofi_tests::ep(&shader_hex, "out") }));

    // Step: finalizing a video holds NO lock anyone else needs. With a `finish` that blocks, an op
    // that takes the graph and a signal stream's own drain both go through while it does.
    use std::sync::atomic::{AtomicBool, Ordering};
    let inside = std::sync::Arc::new(AtomicBool::new(false));
    let release = std::sync::Arc::new(AtomicBool::new(false));
    g.state
        .recorder
        .set_encoders(std::sync::Arc::new(SlowEncoders { inside: inside.clone(), release: release.clone() }));
    g.call("record arm", j!({ "output": goofi_tests::ep(&src, "out") }));
    g.call("record arm", j!({ "output": goofi_tests::ep(&shader_hex, "out") }));
    let closing_folder = g.call("record start", j!({ "root": root.path() }))["folder"]
        .as_str().expect("a folder").to_string();
    g.until("both streams to reach the disk", |g| {
        goofi_tests::render(g, 1);
        (frames(g, &src_name) > 0 && frames(g, &shader_name) > 0).then_some(())
    });
    let went = std::sync::Arc::new(AtomicBool::new(false));
    let waited = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let mut next_folder = None;
    std::thread::scope(|scope| {
        scope.spawn(|| {
            g.call("record disarm", j!({ "output": goofi_tests::ep(&shader_hex, "out") }));
        });
        let (inside, flag, probe, node) = (inside.clone(), went.clone(), &g, src.clone());
        let took = waited.clone();
        scope.spawn(move || {
            // BOUNDED: the close is the render thread's, so a finalize that never begins is an
            // assertion below rather than a scope that never joins.
            let entered = std::time::Instant::now() + std::time::Duration::from_secs(8);
            while !inside.load(Ordering::Relaxed) && std::time::Instant::now() < entered {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            if !inside.load(Ordering::Relaxed) {
                return;
            }
            // The graph mutex, then the recorder's own session — how long they take IS the
            // finding: 8 s while a finalize held them, against microseconds when it holds none.
            let began = std::time::Instant::now();
            probe.call("node state", j!({ "node": node }));
            let seen = frames(probe, &src_name);
            took.store(began.elapsed().as_micros() as u64, Ordering::Relaxed);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while frames(probe, &src_name) <= seen && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let moved = frames(probe, &src_name) > seen;
            flag.store(moved, Ordering::Relaxed);
        });
        // The ENGINE closes what a disarm departed, so the finalize begins on a tick: nothing in
        // this scope reaches `finish` unless something keeps the graphics engine running.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while !went.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
            goofi_tests::render(&g, 1);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if went.load(Ordering::Relaxed) {
            g.call("record stop", j!({}));
            next_folder = Some(g.call("record start", j!({ "root": root.path(), "name": "after-close" }))["folder"]
                .as_str().expect("a folder").to_string());
        }
        release.store(true, Ordering::Relaxed);
    });
    assert!(went.load(Ordering::Relaxed), "a drain keeps writing while a video finalizes");
    let took = waited.load(Ordering::Relaxed);
    assert!(took < 1_000_000, "an op waited {took} us on a finalize that holds no lock it needs");
    g.call("record stop", j!({}));
    g.until("the old recording to retain the closed video", |_g| {
        (mine(&closing_folder, &shader_name).len() == 1).then_some(())
    });
    assert!(mine(&next_folder.expect("the next recording"), &shader_name).is_empty());

    g.call("record disarm", j!({ "output": goofi_tests::ep(&src, "out") }));
    g.state.recorder.set_encoders(std::sync::Arc::new(goofi_record::video::FfmpegEncoders));

    // A recording whose folder went out from under it: the load still opens its patch, because the
    // patch the caller asked for is not the recording's disk.
    g.call("record arm", j!({ "output": goofi_tests::ep(&src, "out") }));
    let second = g.call("record start", j!({ "root": root.path() }))["folder"]
        .as_str()
        .expect("a folder")
        .to_string();
    std::fs::remove_dir_all(&second).expect("the folder goes away");
    g.call("session new", j!({}));
    assert_eq!(g.call("record status", j!({}))["running"], j!(false), "a new patch ends the recording");

    // Step: the record service overflows the OLDEST frame at the subscriber, which nothing on the
    // publishing side can see. Every lost frame is still counted, from the indices themselves.
    let fast = g.add("_TestConst");
    let fast_hex = goofi_tests::hex(fast);
    g.set_param(fast, "constant", "length", 1);
    g.set_param(fast, "common", "max_frequency", 100000.0);
    g.ready(fast);
    g.call("record arm", j!({ "output": goofi_tests::ep(&fast_hex, "out") }));
    // ONE drain thread serves every armed stream, and the load these put on it is what the audio
    // ring's own overflow step later needs to outrun a drain that is keeping up.
    for _ in 0..5 {
        let other = g.add("_TestConst");
        g.set_param(other, "constant", "length", 1);
        g.set_param(other, "common", "max_frequency", 100000.0);
        g.ready(other);
        g.call("record arm", j!({ "output": goofi_tests::ep(goofi_tests::hex(other), "out") }));
    }
    let third = g.call("record start", j!({ "root": root.path() }))["folder"]
        .as_str()
        .expect("a folder")
        .to_string();
    let fast_name = name_of(&g, &fast_hex);
    g.until("the third recording to be writing", |g| (frames(g, &fast_name) > 0).then_some(()));
    let dropped = |g: &goofi_tests::Goofi| -> u64 {
        g.state.recorder.status().streams.iter().filter(|s| s.node == fast_name).map(|s| s.dropped).sum()
    };
    // The drain resolves under the GRAPH lock, so holding it stalls every sweep while the producer
    // publishes far past the 64 the service holds. The overflow is arithmetic, never a rate race.
    {
        let _graph = g.state.graph.lock().expect("the graph");
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let lost = g.until("the drain to count what overflowed", |g| {
        let n = dropped(g);
        (n > 0).then_some(n)
    });
    g.call("record stop", j!({}));

    let entry = mine(&third, &fast_name).pop().expect("one stream");
    // The samples are a bare stack of numbers, so what witnesses a LOSS is the sidecar: the
    // frames' own indices, whose gaps are the frames that never reached the file.
    let file = std::path::Path::new(&third).join(entry["file"].as_str().expect("a name"));
    let beside = std::fs::read_to_string(file.with_extension("jsonl")).expect("the sidecar");
    let indices: Vec<u64> = beside
        .lines()
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).expect("a json line");
            v["meta"]["index"].as_u64().expect("a frame index")
        })
        .collect();
    let gaps: u64 = indices.windows(2).map(|p| p[1] - p[0] - 1).sum();
    assert!(gaps > 0, "the file itself is missing frames: {} written", indices.len());
    assert_eq!(
        entry["dropped"].as_u64().expect("a count"),
        gaps,
        "every frame the subscriber overflowed is counted, and never silently: {lost} said at the stop"
    );

    // Step: what the service already DELIVERED is transported data. A disarm that dropped the
    // subscriber before reading it would lose it where no gap and no count could ever show it.
    let fourth = g.call("record start", j!({ "root": root.path() }))["folder"]
        .as_str()
        .expect("a folder")
        .to_string();
    g.until("the fourth recording to be writing", |g| (frames(g, &fast_name) > 0).then_some(()));
    let written = |g: &goofi_tests::Goofi| -> u64 {
        g.state.recorder.status().streams.iter().filter(|s| s.node == fast_name).map(|s| s.frames).sum()
    };
    // The drain resolves under the GRAPH lock, so holding it freezes the drain while the producer
    // fills the record service's buffer behind it.
    let graph = g.state.graph.lock().expect("the graph");
    std::thread::sleep(std::time::Duration::from_millis(200));
    let frozen = written(&g);
    std::thread::sleep(std::time::Duration::from_millis(80));
    assert_eq!(written(&g), frozen, "a held drain writes nothing, which is what makes this a probe");
    // The REAL op, parked on the lock this test holds, so it lands with the feed's buffer full —
    // the one state that tells a drain-then-close from a close.
    std::thread::scope(|scope| {
        scope.spawn(|| g.call("record disarm", j!({ "output": goofi_tests::ep(&fast_hex, "out") })));
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(graph);
    });

    g.until("the departing feed to be drained and closed", |g| (written(g) == 0).then_some(()));
    g.call("record stop", j!({}));
    let entry = mine(&fourth, &fast_name).pop().expect("one stream");
    let landed = entry["frames"].as_u64().expect("a count");
    assert!(
        landed >= frozen + 32,
        "the frames in flight at the disarm reached the file: {frozen} written under the hold, {landed} in the manifest"
    );
    // Step: the audio engine records through the same recorder, one block a frame, on a timeline
    // the SAMPLE COUNT makes — so two frames are a block apart whatever the drain's scheduling did.
    let osc = g.add("Osc");
    let osc_hex = goofi_tests::hex(osc);
    g.ready(osc);
    let gain = g.add("audio:Gain");
    g.set_param(gain, "gain", "gain", 0.5);
    g.link(osc, "out", gain, "input");
    g.ready(gain);
    let gain_hex = goofi_tests::hex(gain);
    let gain_name = name_of(&g, &gain_hex);
    g.call("record arm", j!({ "output": goofi_tests::ep(&osc_hex, "out") }));
    g.call("record arm", j!({ "output": goofi_tests::ep(&gain_hex, "out") }));
    let fifth = g.call("record start", j!({ "root": root.path() }))["folder"]
        .as_str()
        .expect("a folder")
        .to_string();
    let osc_name = name_of(&g, &osc_hex);
    g.until("the audio engine's blocks to reach the disk", |g| {
        goofi_tests::drive(g, 4_800);
        (frames(g, &osc_name) >= 64 && frames(g, &gain_name) >= 64).then_some(())
    });

    // A clean drive loses NOTHING, counted against what was DRIVEN rather than the file's own
    // numbering — which a mis-framed reader would keep self-consistent while it halved.
    let settled = |g: &goofi_tests::Goofi| -> u64 {
        g.until("the recorder's count to settle", |g| {
            let a = frames(g, &osc_name);
            std::thread::sleep(std::time::Duration::from_millis(60));
            (frames(g, &osc_name) == a).then_some(a)
        })
    };
    let before = settled(&g);
    goofi_tests::drive(&g, 64 * 480);
    // A wav counts SAMPLES, which is what a wav holds — 480 blocks of the engine's 64. Waited for
    // rather than slept on: the drain is a thread, and a busy machine parks it past any sleep.
    g.until("every block to reach the disk", |g| {
        (frames(g, &osc_name) >= before + 480 * 64).then_some(())
    });
    assert_eq!(settled(&g) - before, 480 * 64, "every block the clock rendered, and not one more");

    g.call("record stop", j!({}));

    // Every block one entry's file holds, read the way an analyst does: the WAV is one run of
    // samples and the SIDECAR is the index into it — each line's `n` is the block's own length,
    // which is the only thing that can say where one block ends and the next begins.
    let blocks_of = |folder: &str, entry: &serde_json::Value| -> Vec<(u64, f64, Vec<usize>, Vec<f32>)> {
        let path = std::path::Path::new(folder).join(entry["file"].as_str().expect("a name"));
        let bytes = std::fs::read(&path).expect("the audio stream");
        assert_eq!(&bytes[..4], b"RIFF", "an audio stream is a wav");
        assert_eq!(&bytes[8..12], b"WAVE", "…and it says so in its own header");
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 3, "IEEE float, which is what a sample is");
        let samples: Vec<f32> = bytes[44..]
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().expect("four bytes")))
            .collect();
        let beside = std::fs::read_to_string(path.with_extension("jsonl")).expect("the sidecar");
        let mut held = Vec::new();
        let mut at = 0usize;
        for line in beside.lines() {
            let v: serde_json::Value = serde_json::from_str(line).expect("a json line");
            let n = v["n"].as_u64().expect("the block's own length") as usize;
            held.push((
                v["meta"]["index"].as_u64().expect("every block is numbered"),
                v["t"].as_f64().expect("every block is dated"),
                vec![1, n],
                samples[at..at + n].to_vec(),
            ));
            at += n;
        }
        assert_eq!(at, samples.len(), "the sidecar accounts for every sample in the file");
        held
    };

    let entry = mine(&fifth, &osc_name).pop().expect("the audio stream's entry");
    assert_eq!(entry["timeline"], j!("derived"), "the sample count is the clock: {entry}");
    assert_eq!(entry["sfreq"], j!(48_000.0), "the device rate rides the frames: {entry}");
    let held = blocks_of(&fifth, &entry);
    assert!(held.len() >= 64, "the whole drive is on disk: {} blocks", held.len());
    assert_eq!(held[0].2, vec![1, 64], "one block of a mono output, as the engine renders it");

    // …and what reached it is the OSCILLATOR: a reader off by a few samples delivers the header's
    // own floats as audio, and the block number among them is nowhere near full scale.
    let peak = held.iter().flat_map(|b| b.3.iter()).fold(0f32, |m, x| m.max(x.abs()));
    assert!((peak - 1.0).abs() < 0.05, "the file holds the oscillator at full scale: peak {peak}");
    // EXACT, not approximate: every kept block lies on ONE line through its own number, so a block
    // that went missing moved none of the blocks around it. A loose assertion would prove nothing.
    let step = 64.0 / 48_000.0;
    let line = |blocks: &[(u64, f64, Vec<usize>, Vec<f32>)]| {
        for pair in blocks.windows(2) {
            let d = pair[1].1 - pair[0].1;
            let counted = (pair[1].0 - pair[0].0) as f64 * step;
            assert!((d - counted).abs() < 1e-9, "a block is dated by its number: {d} against {counted}");
        }
        blocks.windows(2).map(|p| p[1].0 - p[0].0 - 1).sum::<u64>()
    };
    assert_eq!(line(&held), 0, "an ordinary drive loses no block at all: {entry}");
    assert_eq!(entry["dropped"], j!(0), "…and the manifest says so too: {entry}");

    let gain_entry = mine(&fifth, &gain_name).pop().expect("the second audio stream's entry");
    let gained = blocks_of(&fifth, &gain_entry);
    assert_eq!(gain_entry["sfreq"], entry["sfreq"], "both recordings use the same sample rate");
    assert_eq!(gain_entry["dropped"], j!(0), "the second stream loses no block");
    assert_eq!(gained.len(), held.len(), "both files contain every simultaneous block");
    for (source, scaled) in held.iter().zip(&gained) {
        assert_eq!(scaled.0, source.0, "corresponding blocks have the same number");
        assert_eq!(scaled.1, source.1, "corresponding blocks have the same timestamp");
        assert_eq!(scaled.2, source.2, "corresponding blocks have the same sample count");
        for (i, (x, y)) in source.3.iter().zip(&scaled.3).enumerate() {
            assert_eq!(*y, *x * 0.5, "sample {i} in block {} stays aligned across both WAV files", source.0);
        }
    }
    g.call("record disarm", j!({ "output": goofi_tests::ep(&gain_hex, "out") }));

    // …and with no block missing the wav is ONE run of audio, so the crossings are counted across
    // the whole of it rather than per block, where every boundary would drop one.
    let run: Vec<f32> = held.iter().flat_map(|b| b.3.iter().copied()).collect();
    let crossings = run.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
    let expect = 880 * (run.len() - 1) / 48_000;
    assert!(crossings.abs_diff(expect) <= 2, "…and it is A4: {crossings} crossings against {expect}");

    // Step: the ANCHOR is derived from the count too, so a stream armed after a long wait is dated
    // by the block it begins at — where a t0 read at the first drain is the whole wait out.
    g.call("record disarm", j!({ "output": goofi_tests::ep(&osc_hex, "out") }));
    std::thread::sleep(std::time::Duration::from_millis(300));
    g.call("record arm", j!({ "output": goofi_tests::ep(&osc_hex, "out") }));
    let sixth = g.call("record start", j!({ "root": root.path() }))["folder"]
        .as_str()
        .expect("a folder")
        .to_string();
    g.until("the second audio recording to reach the disk", |g| {
        goofi_tests::drive(g, 4_800);
        (frames(g, &osc_name) >= 64).then_some(())
    });
    g.call("record stop", j!({}));
    let later = blocks_of(&sixth, &mine(&sixth, &osc_name).pop().expect("the second entry"));
    let apart = later[0].1 - held[0].1;
    let counted = (later[0].0 - held[0].0) as f64 * step;
    assert!(
        (apart - counted).abs() < 1e-6,
        "the two recordings are exactly the blocks between them apart: {apart} against {counted}, \
         with 0.3 s of wall clock in which nothing was rendered"
    );

    // …and the tie is pinned to the patch clock, not merely self-consistent: the file MINUS the
    // count is when the engine began — where a tie made at a drain is the walking above.
    let began = held[0].1 - held[0].0 as f64 * step;
    assert!(
        (0.0..0.5).contains(&began),
        "the block count is tied to the engine's own beginning: {began} s into the patch"
    );

    // Step: a block that does not fit the ring is one the recorder COUNTS. It carries its own
    // number, so the gap says how many went — one rebuilt at the drain would close it silently.
    let seventh = g.call("record start", j!({ "root": root.path() }))["folder"]
        .as_str()
        .expect("a folder")
        .to_string();
    let dropped = |g: &goofi_tests::Goofi| -> u64 {
        g.call("record status", j!({}))["streams"]
            .as_array()
            .map(|s| s.iter().filter(|e| e["node"] == j!(osc_name)).filter_map(|e| e["dropped"].as_u64()).sum())
            .unwrap_or(0)
    };
    // The file has to be OPEN across the loss, or a truncated head would read as a late start
    // rather than as a gap.
    g.until("the overflow recording to be writing", |g| {
        goofi_tests::drive(g, 4_800);
        (frames(g, &osc_name) > 0).then_some(())
    });
    // The ring holds ONE second and the control half empties it every 10 ms, so what decides an
    // overrun is the ratio of render speed to drain speed — not the length of the burst, which
    // scales both. Four seconds sat on that boundary and stopped overrunning at all once the
    // writer moved off the drain; 256 overruns on every run of eight, by a margin of 25% to 98%.
    goofi_tests::drive(&g, 48_000 * 256);
    g.until("the blocks that survived the overflow to reach the disk", |g| {
        goofi_tests::drive(g, 4_800);
        (dropped(g) > 0).then_some(())
    });
    g.call("record stop", j!({}));
    let overflowed = mine(&seventh, &osc_name).pop().expect("the overflowed entry");
    let lost = overflowed["dropped"].as_u64().expect("a count");
    assert!(lost > 0, "the manifest says what could not be held: {overflowed}");
    // The survivors are still where they were rendered — one line through every kept block, gap and
    // all — and the file's own numbering is missing exactly what the manifest counted.
    assert_eq!(line(&blocks_of(&seventh, &overflowed)), lost, "the count and the numbering agree");

    inside.store(false, Ordering::Relaxed);
    release.store(false, Ordering::Relaxed);
    g.state.recorder.set_encoders(std::sync::Arc::new(SlowEncoders {
        inside: inside.clone(), release: release.clone(),
    }));
    let video = g.add("graphics:Constant");
    g.set_param(video, "common", "width", 32);
    g.set_param(video, "common", "height", 16);
    g.ready(video);
    g.call("record arm", j!({ "output": goofi_tests::ep(video, "out") }));
    let last = g.call("record start", j!({ "root": root.path(), "name": "shutdown" }))["folder"]
        .as_str().expect("a folder").to_string();
    g.until("video frames before shutdown", |g| {
        goofi_tests::render(g, 1);
        g.call("record status", j!({}))["streams"].as_array()
            .is_some_and(|streams| streams.iter().any(|stream| stream["engine"] == "graphics" && stream["frames"].as_u64().unwrap_or(0) > 0))
            .then_some(())
    });
    g.state.graph.lock().unwrap().shutdown();
    assert_eq!(g.state.graph.lock().unwrap().node_count(), 0);
    std::thread::scope(|scope| {
        let (done, finished) = std::sync::mpsc::channel();
        let state = &g.state;
        scope.spawn(move || {
            state.stop_recording();
            done.send(()).unwrap();
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while !inside.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let draining = inside.load(Ordering::Relaxed) && finished.try_recv().is_err();
        release.store(true, Ordering::Relaxed);
        finished.recv_timeout(std::time::Duration::from_secs(8)).expect("shutdown drains the recorder");
        assert!(draining, "shutdown waits for video after the engines have stopped");
    });
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(&last).join("manifest.json")).unwrap(),
    ).unwrap();
    assert!(manifest["stopped_utc"].is_string(), "shutdown finalizes the manifest");

}

#[test]
fn an_armed_signal_slot_loses_no_tick_to_the_viewer_plane() {
    // The recorder's own service, drained by hand where Task 5 will put the recorder. Contiguity
    // of `index` is the oracle: a frame count alone would pass against the latest-wins wire.
    let g = goofi_tests::Goofi::new();
    let fast = g.add("_TestConst");
    let hex = goofi_tests::hex(fast);
    g.set_param(fast, "common", "max_frequency", 200.0);
    g.ready(fast);
    g.call("record arm", j!({ "output": goofi_tests::ep(&hex, "out") }));

    let service = {
        let graph = g.state.graph.lock().unwrap();
        goofi_transport::record_service(
            &goofi_transport::service_base(graph.instance(), fast, graph.node_generation(fast)),
            "out",
        )
    };
    let node = goofi_transport::iox_node().expect("an iceoryx2 node");
    let sub = goofi_transport::open_record_subscriber(&node, &service, goofi_transport::record_shape("signal"))
        .expect("the recorder's end");

    let mut seen: Vec<u64> = Vec::new();
    g.until("the armed slot to publish a run of frames", |_| {
        while let Ok(Some(sample)) = sub.receive() {
            let frame = goofi_codec::decode(sample.payload()).expect("a frame decodes");
            seen.push(frame.meta().index().expect("the engine stamps every frame"));
        }
        (seen.len() >= 32).then_some(())
    });
    for pair in seen.windows(2) {
        assert_eq!(pair[1], pair[0] + 1, "the recording service loses no tick: {seen:?}");
    }

    // Step: a frame over the recording service's ceiling is a COUNTED drop the node says, never a
    // segment quietly grown to hold it — which is what holds an armed slot to `RECORD_BUDGET`.
    let wide = g.add("_TestRamp");
    let wide_out = goofi_tests::ep(goofi_tests::hex(wide), "out");
    g.set_param(wide, "ramp", "channels", 2);
    g.set_param(wide, "ramp", "length", 600_000);
    g.set_param(wide, "common", "max_frequency", 5.0);
    g.ready(wide);
    g.call("record arm", j!({ "output": &wide_out }));
    let said = g.until("the node to wear what the recording cost", |g| g.error(wide));
    assert!(said.starts_with("recording dropped"), "the drop is counted, not swallowed: {said}");
    assert!(
        said.contains(&goofi_transport::RECORD_SLICE.to_string()),
        "and the ceiling it broke is named: {said}"
    );

    // A re-arm is what clears the complaint, and a frame under the ceiling records again.
    g.set_param(wide, "ramp", "length", 512);
    g.call("record disarm", j!({ "output": &wide_out }));
    g.call("record arm", j!({ "output": &wide_out }));
    g.until("the recording complaint to clear", |g| g.error(wide).is_none().then_some(()));
}

/// A stream's sidecar, read as the instants it carries — the one sidecar shape every kind has.
fn beside_instants(file: &std::path::Path) -> Vec<f64> {
    let text = std::fs::read_to_string(file.with_extension("jsonl")).expect("the sidecar");
    text.lines()
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).expect("a json line");
            v["t"].as_f64().expect("an instant")
        })
        .collect()
}

#[test]
fn h264_records_full_resolution_and_pads_odd_sizes() {
    use goofi_record::video::{Encoders, FfmpegEncoders};
    let root = tempfile::tempdir().expect("video folder");
    let encoders = FfmpegEncoders;
    encoders.probe().expect("FFmpeg is installed");
    for (width, height) in [(3840u32, 2160u32), (63, 31), (1, 1)] {
        let path = root.path().join(format!("{width}x{height}.mkv"));
        let mut encoder = encoders.open(&path, (width, height), 60.0, Default::default()).expect("open video");
        assert!(encoder.write(&[0]).is_err(), "a partial frame must not corrupt the pipe");
        let mut frame = vec![0u8; width as usize * height as usize * 4];
        for pixel in frame.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[128, 64, 192, 255]);
        }
        for _ in 0..12 {
            encoder.write(&frame).expect("write full-resolution frame");
        }
        encoder.finish().expect("finish video");
        encoder.finish().expect("finish is idempotent");
        assert!(encoder.write(&frame).is_err(), "a closed encoder cannot restart");
        let probe = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
            .args(["-show_entries", "stream=codec_name,pix_fmt,width,height,nb_read_frames", "-of", "json"])
            .arg(&path).output().expect("inspect video");
        assert!(probe.status.success(), "{}", String::from_utf8_lossy(&probe.stderr));
        let probe: serde_json::Value = serde_json::from_slice(&probe.stdout).expect("probe JSON");
        let stream = &probe["streams"][0];
        assert_eq!(stream["codec_name"], "h264");
        assert_eq!(stream["pix_fmt"], "yuv420p");
        assert_eq!(stream["width"], width.next_multiple_of(2));
        assert_eq!(stream["height"], height.next_multiple_of(2));
        assert_eq!(stream["nb_read_frames"], "12");
        if width == 3840 {
            assert!(std::fs::metadata(&path).unwrap().len() < 1_000_000,
                "a short flat-colour 4K clip stays small");
        }
    }
}

#[test]
fn video_quality_is_saved_undone_and_applied_to_each_file() {
    let g = goofi_tests::Goofi::new();
    let root = tempfile::tempdir().expect("recording root");
    let uid = g.add("graphics:Constant");
    g.set_param(uid, "common", "width", 32);
    g.set_param(uid, "common", "height", 16);
    g.ready(uid);
    let output = goofi_tests::ep(uid, "out");
    g.call("record arm", j!({ "output": output }));
    let quality = || g.doc()["nodes"][goofi_tests::hex(uid)]["record"][0]["quality"].clone();
    assert_eq!(quality(), "high");
    g.call("record quality", j!({ "output": output, "quality": "small" }));
    assert_eq!(quality(), "small");
    g.call("undo", j!({}));
    assert_eq!(quality(), "high");
    g.call("redo", j!({}));
    assert_eq!(quality(), "small");
    assert!(g.refuse("record quality", j!({ "output": output, "quality": "invalid" })).contains("quality"));
    let path = root.path().join("quality.gfi");
    g.call("session save", j!({ "path": path }));
    g.call("session load", j!({ "path": path }));
    assert_eq!(quality(), "small", "the patch keeps the output's quality");
    g.ready(uid);
    let folder = g.call("record start", j!({ "root": root.path() }))["folder"].as_str().unwrap().to_string();
    let frames = || g.state.recorder.status().streams.iter().map(|stream| stream.frames).sum::<u64>();
    g.until("the small-quality video to write frames", |_| {
        goofi_tests::render(&g, 1);
        (frames() >= 4).then_some(())
    });
    let before = frames();
    g.call("record quality", j!({ "output": output, "quality": "very_high" }));
    g.until("the new quality to open a new file", |_| {
        goofi_tests::render(&g, 1);
        (frames() >= before + 4).then_some(())
    });
    g.call("record stop", j!({}));
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(&folder).join("manifest.json")).unwrap(),
    ).unwrap();
    let streams = manifest["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 2, "each quality has a separate video file: {manifest}");
    assert_eq!(streams[0]["quality"], "small");
    assert_eq!(streams[0]["closed_because"], "quality changed");
    assert_eq!(streams[1]["quality"], "very_high");
    for stream in streams {
        let file = std::path::Path::new(&folder).join(stream["file"].as_str().unwrap());
        assert!(file.exists());
        let start = stream["t0_patch"].as_f64().unwrap();
        assert!(beside_instants(&file).iter().all(|at| *at >= start), "no earlier readback enters the new file");
        assert!(stream.get("error").is_none(), "{stream}");
    }
}

#[test]
fn video_quality_changes_the_encoded_picture() {
    use goofi_core::record::VideoQuality;
    use goofi_record::video::{Encoders, FfmpegEncoders};
    let root = tempfile::tempdir().unwrap();
    let mut state = 17u32;
    let mut frame = vec![0u8; 128 * 128 * 4];
    for pixel in frame.chunks_exact_mut(4) {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let grey = (state >> 24) as u8;
        pixel.copy_from_slice(&[grey, grey, grey, 255]);
    }
    let mut errors = Vec::new();
    for quality in [VideoQuality::Small, VideoQuality::VeryHigh] {
        let file = root.path().join(format!("{quality:?}.mkv"));
        let mut encoder = FfmpegEncoders.open(&file, (128, 128), 60.0, quality).unwrap();
        for _ in 0..4 { encoder.write(&frame).unwrap(); }
        encoder.finish().unwrap();
        let decoded = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"]).arg(&file)
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .output().unwrap();
        assert!(decoded.status.success());
        assert_eq!(decoded.stdout.len(), frame.len());
        let error: u64 = decoded.stdout.chunks_exact(4).zip(frame.chunks_exact(4))
            .map(|(out, input)| i64::from(out[0]).abs_diff(i64::from(input[0])).pow(2)).sum();
        errors.push(error);
    }
    assert!(errors[1] < errors[0], "very high quality preserves more of the supplied picture: {errors:?}");
}
