//! Recording: every engine's frames, on one timeline.

use goofi_core::time::{stamp, Time};

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
fn a_recording_is_a_folder_of_decodable_frames() {
    let dir = tempfile::tempdir().expect("a temp root");
    let time = std::sync::Arc::new(Time::new());
    let rec = goofi_record::Recorder::new(time.clone());
    rec.start(dir.path(), "probe", None).expect("started");
    let id = goofi_record::StreamId {
        uid: goofi_node::Uid(1),
        node: "src".into(),
        slot: "out".into(),
        engine: "signal",
    };
    rec.open(&id, goofi_record::Kind::Frames, 0.0, goofi_record::StreamMeta::measured(Some(256.0)))
        .expect("opened");
    for i in 0..8u64 {
        let mut meta = goofi_core::Meta::empty();
        meta.set_time(Some(i as f64 / 256.0));
        meta.set_index(Some(i));
        let frame = goofi_core::Data::array_f32(vec![4], vec![0u8; 16], meta).expect("a frame");
        rec.write(&id, &goofi_codec::encode(&frame)).expect("written");
    }
    // Step: a re-arm at the very same patch instant is a NEW file, never the last one truncated.
    rec.open(&id, goofi_record::Kind::Frames, 0.0, goofi_record::StreamMeta::measured(Some(256.0)))
        .expect("re-opened at the same patch instant");
    let frame = goofi_core::Data::array_f32(vec![4], vec![0u8; 16], goofi_core::Meta::empty())
        .expect("a frame");
    rec.write(&id, &goofi_codec::encode(&frame)).expect("written to the second file");
    let folder = rec.stop().expect("the manifest written").expect("a folder");

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("manifest.json")).expect("a manifest"))
            .expect("json");
    assert_eq!(manifest["streams"][0]["frames"], 8);
    assert_eq!(manifest["streams"][1]["frames"], 1);
    assert_ne!(
        manifest["streams"][0]["file"], manifest["streams"][1]["file"],
        "two entries never name one file"
    );
    assert!(manifest["origin_utc"].is_string(), "the one anchor every file adds to");

    let file = folder.join(manifest["streams"][0]["file"].as_str().expect("a name"));
    assert!(
        file.file_name().unwrap().to_string_lossy().contains("src-out__"),
        "the name carries node, slot and the first sample's UTC"
    );
    let bytes = std::fs::read(&file).expect("the stream");
    let mut rest = &bytes[..];
    let mut seen = 0;
    while !rest.is_empty() {
        let (_, meta, body) = goofi_codec::split_frame(rest).expect("a whole frame");
        let used = 14 + meta.len() + body.len();
        assert!(goofi_codec::decode(&rest[..used]).expect("a frame decodes").as_array().is_ok());
        rest = &rest[used..];
        seen += 1;
    }
    assert_eq!(seen, 8, "every frame is on disk, end to end");
}
