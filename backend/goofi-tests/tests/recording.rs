//! Recording: every engine's frames, on one timeline.

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

#[test]
fn arming_survives_a_rewire_and_rides_the_document() {
    let g = goofi_tests::Goofi::new();
    let src_uid = g.add("_TestConst");
    let src = goofi_tests::hex(src_uid);
    let dst = goofi_tests::hex(g.add("_TestSink"));

    g.call("record arm", j!({ "output": goofi_tests::ep(&src, "out") }));
    assert_eq!(g.doc()["nodes"][&src]["record"], j!(["out"]), "arming is the node's own record");

    let status = g.call("record status", j!({}));
    assert_eq!(status["running"], j!(false), "arming a slot does not begin a recording");

    g.call("link add", j!({ "from": goofi_tests::ep(&src, "out"), "to": goofi_tests::ep(&dst, "input") }));
    assert_eq!(g.doc()["nodes"][&src]["record"], j!(["out"]), "a re-wire cannot disarm a recording");

    g.call("undo", j!({}));
    g.call("undo", j!({}));
    assert_eq!(g.doc()["nodes"][&src]["record"], j!([]), "arming has an exact inverse");

    g.call("redo", j!({}));
    assert_eq!(g.doc()["nodes"][&src]["record"], j!(["out"]), "and the arm comes back");

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

    g.call("record disarm", j!({ "output": goofi_tests::ep(&src, "out") }));
    assert_eq!(g.doc()["nodes"][&src]["record"], j!([]), "disarming empties the node's record");

    g.call("record stop", j!({}));
    assert_eq!(g.call("record status", j!({}))["running"], j!(false));
    assert!(std::path::Path::new(&folder).join("manifest.json").exists(), "the folder holds a manifest");

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
    let third = g.call("record start", j!({ "root": root.path() }))["folder"]
        .as_str()
        .expect("a folder")
        .to_string();
    let fast_name = name_of(&g, &fast_hex);
    let lost = g.until("the drain to fall behind and say so", |g| {
        let status = g.call("record status", j!({}));
        let n: u64 = status["streams"]
            .as_array()
            .map(|s| s.iter().filter(|e| e["node"] == j!(fast_name)).filter_map(|e| e["dropped"].as_u64()).sum())
            .unwrap_or(0);
        (n > 0).then_some(n)
    });
    g.call("record stop", j!({}));

    let entry = mine(&third, &fast_name).pop().expect("one stream");
    let bytes = std::fs::read(std::path::Path::new(&third).join(entry["file"].as_str().expect("a name")))
        .expect("the stream");
    let mut rest = &bytes[..];
    let mut indices: Vec<u64> = Vec::new();
    while !rest.is_empty() {
        let (_, meta, body) = goofi_codec::split_frame(rest).expect("a whole frame");
        let used = 14 + meta.len() + body.len();
        indices.push(goofi_codec::decode(&rest[..used]).expect("a frame").meta().index().expect("an index"));
        rest = &rest[used..];
    }
    let gaps: u64 = indices.windows(2).map(|p| p[1] - p[0] - 1).sum();
    assert!(gaps > 0, "the file itself is missing frames: {} written", indices.len());
    assert_eq!(
        entry["dropped"].as_u64().expect("a count"),
        gaps,
        "every frame the subscriber overflowed is counted, and never silently: {lost} said at the stop"
    );
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
    let sub = goofi_transport::open_record_subscriber(&node, &service).expect("the recorder's end");

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
