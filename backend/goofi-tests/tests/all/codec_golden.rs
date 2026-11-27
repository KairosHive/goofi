//! The GOOF frame, byte for byte: the layout the codec writes is spelled out here, every frame
//! survives its round trip, and the browser's parity fixture is written from the same cases so the
//! hand-mirrored TypeScript decoder is held to the one encoder.

use goofi_codec::{encode_f16, encode_u8, split_frame, ALIGN, HEADER_SIZE};
use goofi_core::{Axes, Axis, Coord, Data, Meta, MetaValue, Value};
use indexmap::IndexMap;

fn encode(d: &Data) -> Vec<u8> {
    goofi_codec::encode(d).expect("a frame that crosses")
}

fn arr(shape: &[usize], buf: Vec<u8>, meta: Meta) -> Data {
    Data::array_f32(shape.to_vec(), buf, meta).unwrap()
}

/// Values a half float holds exactly: one, its largest, its smallest normal and its smallest subnormal.
const HALVES: [f32; 4] = [1.0, 65504.0, 6.103_515_6e-5, 5.960_464_5e-8];

fn le_bytes(vals: &[f32]) -> Vec<u8> {
    vals.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A case, by which encoder writes it: the wire's f32 `Data`, the viewer hop's texels, which no
/// `Data` can hold, or the same hop's half floats of an f32 `Data`.
enum Case {
    Frame(Data),
    Texels(Vec<usize>, Vec<u8>, Meta),
    Halves(Data),
}

impl Case {
    fn encode(&self) -> Vec<u8> {
        match self {
            Case::Frame(d) => encode(d),
            Case::Texels(shape, texels, meta) => encode_u8(shape, texels, meta).expect("a frame that crosses"),
            Case::Halves(d) => encode_f16(d).expect("a frame that crosses").expect("every sample fits a half"),
        }
    }

    /// What the browser must decode the frame to: the kind, the meta as JSON, and the body as
    /// the wire dtype, shape and values an array has, the text a string has, the entries a table has.
    fn decoded(&self) -> serde_json::Value {
        match self {
            Case::Frame(d) => decoded_of(d),
            Case::Texels(shape, texels, meta) => serde_json::json!({
                "dtype": "ARRAY", "meta": meta_json(meta, true), "arrayDtype": "|u1", "shape": shape,
                "values": texels,
            }),
            // Widened on arrival: the browser never learns a second float width.
            Case::Halves(d) => decoded_of(d),
        }
    }
}

fn decoded_of(d: &Data) -> serde_json::Value {
    match d.value() {
        Value::Array(store) => serde_json::json!({
            "dtype": "ARRAY", "meta": meta_json(d.meta(), true), "arrayDtype": "<f4", "shape": store.shape(),
            "values": store.values().map(json_number).collect::<Vec<_>>(),
        }),
        Value::Str(s) => serde_json::json!({ "dtype": "STRING", "meta": meta_json(d.meta(), false), "value": &**s }),
        Value::Table(map) => serde_json::json!({
            "dtype": "TABLE", "meta": meta_json(d.meta(), false),
            "entries": map.iter().map(|(k, v)| (k.clone(), decoded_of(v))).collect::<serde_json::Map<_, _>>(),
        }),
        other => panic!("no browser decoding for {other:?}"),
    }
}

/// JSON has no NaN or infinity: spelled as the strings `Number` reads.
fn json_number(v: f32) -> serde_json::Value {
    match serde_json::Number::from_f64(f64::from(v)) {
        Some(n) if v.is_finite() => serde_json::Value::Number(n),
        _ if v.is_nan() => "NaN".into(),
        _ => if v > 0.0 { "Infinity" } else { "-Infinity" }.into(),
    }
}

/// The meta as the browser's msgpack decoder hands it over: the keys set, and for an array the
/// `channels` dict beside them, as `pack_array_meta` projects it.
fn meta_json(meta: &Meta, array: bool) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (k, v) in meta.iter() {
        if k == goofi_core::META_CHANNELS || matches!(v, MetaValue::Null) {
            continue;
        }
        out.insert(k.clone(), mv_json(v));
    }
    if array {
        let channels: serde_json::Map<String, serde_json::Value> = meta
            .channels()
            .dims()
            .map(|(dim, coords)| {
                let coords = coords.iter().map(|c| match c {
                    Coord::Num(n) => serde_json::json!(n),
                    Coord::Str(s) => serde_json::json!(s.as_ref()),
                }).collect::<Vec<_>>();
                (dim.to_string(), serde_json::Value::Array(coords))
            })
            .collect();
        out.insert("channels".into(), serde_json::Value::Object(channels));
    }
    serde_json::Value::Object(out)
}

fn mv_json(v: &MetaValue) -> serde_json::Value {
    match v {
        MetaValue::Null | MetaValue::Axes(_) => serde_json::Value::Null,
        MetaValue::Bool(b) => serde_json::json!(b),
        MetaValue::Int(i) => serde_json::json!(i),
        MetaValue::Uint(u) => serde_json::json!(u),
        MetaValue::Float(f) => serde_json::json!(f),
        MetaValue::Str(s) => serde_json::json!(s),
        MetaValue::Bytes(b) => serde_json::json!(b),
        MetaValue::List(l) => serde_json::Value::Array(l.iter().map(mv_json).collect()),
        MetaValue::Map(m) => serde_json::Value::Object(m.iter().map(|(k, v)| (k.clone(), mv_json(v))).collect()),
    }
}

fn build_cases() -> Vec<(&'static str, Case)> {
    let meta_sfreq_ch = Meta::new()
        .with_sfreq(Some(250.0))
        .with_channels(Axes::new().with(0, Axis::coords(vec![Coord::Str("Fz".into()), Coord::Str("Cz".into())])));
    let meta_index = Meta::new().with_index(Some(42));

    // Every kind a meta value comes in but bytes, which JSON cannot spell for the browser's half.
    let meta_kinds = Meta::new()
        .with("flag", MetaValue::Bool(true))
        .with("count", MetaValue::Int(-3))
        .with("ratio", MetaValue::Float(0.5))
        .with("unit", MetaValue::Str("uV".into()))
        .with("list", MetaValue::List(vec![MetaValue::Int(1), MetaValue::Str("a".into())]))
        .with("map", MetaValue::Map([("k".to_string(), MetaValue::Float(2.5))].into_iter().collect()));
    let meta_freqs = Meta::new().with_channels(Axes::new().with(1, Axis::coords(vec![Coord::Num(0.5), Coord::Num(8.0)])));

    let mut table = IndexMap::new();
    table.insert("a".to_string(), arr(&[2], le_bytes(&[1.0, 2.0]), Meta::empty()));
    table.insert("b".to_string(), Data::string("x", Meta::empty()));

    vec![
        ("f32_1d", arr(&[3], le_bytes(&[1.0, 2.0, 3.0]), Meta::empty())),
        // 0-d scalar promotes to shape (1,)
        ("scalar_0d", arr(&[], le_bytes(&[3.0]), Meta::empty())),
        ("empty_array", arr(&[0], vec![], Meta::empty())),
        (
            "with_sfreq_channels",
            arr(&[2, 3], le_bytes(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]), meta_sfreq_ch),
        ),
        ("with_index", arr(&[4], le_bytes(&[1.0, 2.0, 3.0, 4.0]), meta_index)),
        ("with_meta_kinds", arr(&[1], le_bytes(&[1.0]), meta_kinds)),
        // A numeric axis on the second dim alone: the first carries no labels and no key.
        ("with_numeric_coords", arr(&[2, 2], le_bytes(&[0.0, 1.0, 2.0, 3.0]), meta_freqs)),
        ("f32_3d", arr(&[2, 1, 2], le_bytes(&[0.0, 1.0, 2.0, 3.0]), Meta::empty())),
        ("nonfinite", arr(&[3], le_bytes(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY]), Meta::empty())),
        ("string", Data::string("hello world éà", Meta::empty())),
        ("string_empty", Data::string("", Meta::empty())),
        ("table", Data::table(table, Meta::empty())),
        ("table_empty", Data::table(IndexMap::new(), Meta::empty())),
        ("table_special_keys", Data::table(
            ["__proto__", "constructor", "toString"].into_iter()
                .map(|key| (key.to_string(), Data::string(key, Meta::empty()))).collect(),
            Meta::empty(),
        )),
    ]
    .into_iter()
    .map(|(name, d)| (name, Case::Frame(d)))
    .chain([
        ("u8_image", Case::Texels(vec![1, 2, 3], vec![0, 255, 128, 255, 0, 128], Meta::empty())),
        ("f16_line", Case::Halves(arr(&[4], le_bytes(&HALVES), Meta::empty()))),
    ])
    .collect()
}

#[test]
fn a_frame_is_laid_out_as_the_codec_says_and_survives_its_round_trip() {
    // The layout, byte for byte, on one frame: the magic, the tag, the two lengths, the meta,
    // then the body on the boundary, and the samples on it too after the array head.
    let d = arr(&[2], le_bytes(&[1.0, 2.0]), Meta::new().with_sfreq(Some(250.0)));
    let frame = encode(&d);
    assert_eq!(&frame[..4], b"GOOF");
    assert_eq!(frame[4], 0, "an array's tag");
    let meta_len = u32::from_le_bytes(frame[5..9].try_into().unwrap()) as usize;
    let body_len = u32::from_le_bytes(frame[9..13].try_into().unwrap()) as usize;
    assert_eq!(HEADER_SIZE, 13);
    let (tag, meta, body) = split_frame(&frame).expect("a valid frame");
    assert_eq!((tag, meta.len(), body.len()), (0, meta_len, body_len));
    let body_at = (HEADER_SIZE + meta_len).div_ceil(ALIGN) * ALIGN;
    assert_eq!(body.as_ptr() as usize - frame.as_ptr() as usize, body_at, "the body starts on the boundary");
    assert!(frame[HEADER_SIZE + meta_len..body_at].iter().all(|b| *b == 0), "the pad is zero");
    assert_eq!(frame.len(), body_at + body_len, "nothing after the body");
    // The array head: rank, the dtype string's length, the string, its pad, then the shape.
    assert_eq!(&body[..5], b"\x01\x03<f4");
    assert_eq!(&body[5..8], &[0, 0, 0], "the head's text is padded to the boundary");
    assert_eq!(&body[8..12], &2u32.to_le_bytes());
    assert_eq!(&body[12..], &le_bytes(&[1.0, 2.0]), "the samples, on the boundary");
    // The meta carries what is set and nothing the body already says.
    let meta: rmpv::Value = rmpv::decode::read_value(&mut &meta[..]).expect("msgpack");
    let keys: Vec<&str> = meta.as_map().unwrap().iter().map(|(k, _)| k.as_str().unwrap()).collect();
    assert_eq!(keys, vec!["sfreq", "channels"], "{meta}");

    for (name, case) in build_cases() {
        let frame = case.encode();
        split_frame(&frame).unwrap_or_else(|e| panic!("[{name}] {e}"));
        if let Case::Frame(d) = &case {
            let back = goofi_codec::decode(&frame).unwrap_or_else(|e| panic!("[{name}] {e}"));
            assert_eq!(encode(&back), frame, "[{name}] did not survive the round trip");
            assert_eq!(back.meta().sfreq(), d.meta().sfreq(), "[{name}] sfreq");
            assert_eq!(back.meta().channels(), d.meta().channels(), "[{name}] axis labels ride the frame");
            for (key, value) in d.meta().iter().filter(|(_, v)| !matches!(v, MetaValue::Null | MetaValue::Axes(_))) {
                assert_eq!(back.meta().get(key).map(mv_json), Some(mv_json(value)), "[{name}] meta `{key}`");
            }
        }
        // The half-float hop comes back as the f32 it was made from: these values it holds exactly.
        if let Case::Halves(d) = &case {
            let back = goofi_codec::decode(&frame).unwrap_or_else(|e| panic!("[{name}] {e}"));
            assert_eq!(encode(&back), encode(d), "[{name}] did not survive the half-float hop");
        }
    }

    // What a frame SAYS does not depend on the order its meta keys were set in.
    let said = |keys: [(&str, f64); 2]| {
        let meta = keys.iter().fold(Meta::new(), |m, (k, v)| m.with(*k, MetaValue::Float(*v)));
        goofi_codec::content_hash(&arr(&[2], le_bytes(&[1.0, 2.0]), meta)).unwrap()
    };
    assert_eq!(said([("zz", 1.0), ("aa", 2.0)]), said([("aa", 2.0), ("zz", 1.0)]), "a key order changed the hash");
}

#[test]
fn the_browser_fixture_is_what_the_encoder_writes() {
    // `tests/codec_golden.json`: every case as hex, with what the browser must decode it to. The
    // encoder is the source of truth, so a drift here rewrites the fixture for review.
    let entries: Vec<serde_json::Value> = build_cases()
        .iter()
        .map(|(name, case)| {
            let hex: String = case.encode().iter().map(|b| format!("{b:02x}")).collect();
            serde_json::json!({ "name": name, "hex": hex, "decoded": case.decoded() })
        })
        .collect();
    let want = serde_json::to_string_pretty(&serde_json::json!({ "entries": entries })).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/codec_golden.json");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(want.as_str()) {
        std::fs::write(&path, &want).expect("rewriting the fixture");
        panic!("tests/codec_golden.json was stale; it has been regenerated — review and commit it");
    }
}

/// A frame built by hand around `meta` and `body`, as the codec lays one out: the body on the
/// boundary after the meta.
fn framed(tag: u8, meta: &[u8], body: &[u8]) -> Vec<u8> {
    let mut frame = b"GOOF".to_vec();
    frame.push(tag);
    frame.extend_from_slice(&(meta.len() as u32).to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(meta);
    frame.resize((HEADER_SIZE + meta.len()).div_ceil(ALIGN) * ALIGN, 0);
    frame.extend_from_slice(body);
    frame
}

#[test]
fn a_request_carries_each_multi_frame_with_its_source_and_a_big_one_by_reference() {
    // A multi slot's entries cross under one name repeated, each with the `node.slot` that sent
    // it; a single slot's entry crosses with no source, and an output never has one.
    let a = &arr(&[3], le_bytes(&[1.0, 2.0, 3.0]), Meta::empty());
    let b = &arr(&[2], le_bytes(&[4.0, 5.0]), Meta::empty());
    let runs = goofi_codec::rpc::encode_request(&[("input", "alpha.out", a), ("input", "beta.out", b), ("gate", "", a)]).expect("a request");
    let slices: Vec<&[u8]> = runs.iter().map(|r| &**r).collect();
    let goofi_codec::rpc::Request::Process { slots, .. } = goofi_codec::rpc::decode_request(&slices).expect("a request") else {
        panic!("a run, not a refresh");
    };
    let named: Vec<(&str, &str)> = slots.iter().map(|(n, s, _)| (n.as_str(), s.as_str())).collect();
    assert_eq!(named, vec![("input", "alpha.out"), ("input", "beta.out"), ("gate", "")]);
    assert_eq!(encode(&slots[1].2), encode(b), "the second entry is beta's own frame");
    assert_eq!(runs.len(), 1, "small samples ride inside the one copied run");

    // A big frame's samples are a run of their own, the node's very bytes and never a copy of
    // them; what the runs say, read as one, is the same request.
    let big = &arr(&[4096], le_bytes(&[0.5; 4096]), Meta::empty());
    let runs = goofi_codec::rpc::encode_request(&[("input", "", big), ("gate", "", a)]).expect("a request");
    let samples = big.as_array().unwrap().as_bytes();
    assert!(runs.iter().any(|r| matches!(r, std::borrow::Cow::Borrowed(s) if s.as_ptr() == samples.as_ptr())), "by reference");
    let slices: Vec<&[u8]> = runs.iter().map(|r| &**r).collect();
    let whole = runs.concat();
    for request in [&slices[..], &[&whole[..]][..]] {
        let decoded = goofi_codec::rpc::decode_request(request).unwrap_or_else(|e| panic!("{e} ({} runs)", request.len()));
        let goofi_codec::rpc::Request::Process { slots, .. } = decoded else { panic!("a run") };
        assert_eq!((slots.len(), encode(&slots[0].2), encode(&slots[1].2)), (2, encode(big), encode(a)));
    }

    // The params cross on the setup, whole, and after that one at a time; a run carries none.
    let mut params = goofi_codec::rpc::ParamMap::new();
    params.entry("gain".into()).or_default().insert("factor".into(), goofi_core::Param::float(2.0, 0.0, 4.0));
    let setup = goofi_codec::rpc::encode_setup_request(&params).expect("a setup");
    let goofi_codec::rpc::Request::Setup { params: seeded } = goofi_codec::rpc::decode_request(&[&setup]).expect("a setup") else { panic!("a setup") };
    assert_eq!(seeded, params);
    let moved = goofi_codec::rpc::encode_param_request("gain", "factor", &goofi_core::Param::float(3.0, 0.0, 4.0)).expect("a param");
    let goofi_codec::rpc::Request::Param { group, name, value } = goofi_codec::rpc::decode_request(&[&moved]).expect("a param") else { panic!("a param") };
    assert_eq!((group.as_str(), name.as_str(), value), ("gain", "factor", goofi_core::Param::float(3.0, 0.0, 4.0)));

    // A reply carries a frame per output — or the NAME of the input an output is, unchanged,
    // with no bytes behind it, which the host resolves against what it sent.
    let mut reply = Vec::new();
    let emitted = [("out", goofi_codec::rpc::Emitted::Frame(b)), ("same", goofi_codec::rpc::Emitted::Input { slot: "input", index: 1 })];
    goofi_codec::rpc::encode_response(&emitted, &["input".into()], &mut reply).expect("a response");
    assert!(reply.len() < encode(b).len() + 64, "the reference carries no frame: {} bytes", reply.len());
    let goofi_codec::rpc::Response::Process(result) = goofi_codec::rpc::decode_response(reply).expect("a response") else {
        panic!("process output")
    };
    assert_eq!(result.clear_inputs, vec!["input"]);
    let outs = result.outputs;
    let goofi_codec::rpc::Output::Frame(frame) = &outs[0].1 else { panic!("a frame") };
    assert_eq!((outs[0].0.as_str(), encode(frame)), ("out", encode(b)));
    let goofi_codec::rpc::Output::Input { slot, index } = &outs[1].1 else { panic!("a reference") };
    assert_eq!((outs[1].0.as_str(), slot.as_str(), *index), ("same", "input", 1));
}

#[test]
fn malformed_and_deep_frames_are_refused_before_reduction() {
    // The decoder reads lengths out of the frame, and runs in the browser: it must refuse, never panic.
    let good = encode(&arr(&[2], vec![0u8; 8], Meta::new().with_sfreq(Some(1.0))));
    assert!(goofi_codec::decode(&good).is_ok(), "the fixture is a frame that DOES decode");
    assert!(goofi_codec::decode(b"NOPE").is_err(), "bad magic");
    for cut in 0..good.len() {
        // Every prefix — the shape a partially-flushed socket delivers.
        assert!(goofi_codec::decode(&good[..cut]).is_err(), "a truncated frame decoded at {cut}");
    }
    let mut wrong_tag = good.clone();
    wrong_tag[4] = 0x7f;
    assert!(goofi_codec::decode(&wrong_tag).is_err(), "an unknown dtype tag");
    // A channels dim beyond any array rank: a pad to it would overflow or allocate gigabytes.
    for key in ["dim18446744073709551615", "dim4000000000"] {
        let (tag, _, body) = goofi_codec::split_frame(&good).unwrap();
        let channels = rmpv::Value::Map(vec![(key.into(), rmpv::Value::Array(vec!["a".into()]))]);
        let mut meta = Vec::new();
        rmpv::encode::write_value(&mut meta, &rmpv::Value::Map(vec![("channels".into(), channels)])).unwrap();
        assert!(goofi_codec::decode(&framed(tag, &meta, body)).unwrap_err().contains("rank"), "{key} must be refused");
    }

    use goofi_core::reduce::reduce_axis;
    for shape in [vec![usize::MAX, 2], vec![10], vec![0, usize::MAX]] {
        assert!(reduce_axis(&[], &shape, 0, 1).is_none());
    }
    let bytes = le_bytes(&[1.0, 2.0, 3.0, 4.0]);
    assert!(reduce_axis(&bytes, &[4], 0, usize::MAX).is_none());
    assert!(reduce_axis(&bytes, &[4], usize::MAX, 1).is_none());
    let reduced = reduce_axis(&bytes, &[4], 0, 1).unwrap();
    assert_eq!(reduced.bytes, le_bytes(&[1.0]), "a subsample keeps entries the producer emitted");

    // Build nested table frames directly so the encoder cannot hide a decoder depth fault.
    let mut frame = encode(&Data::string("leaf", Meta::empty()));
    for _ in 0..70 {
        let mut body = 1u32.to_le_bytes().to_vec();
        body.extend_from_slice(&1u16.to_le_bytes());
        body.push(b'x');
        body.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        body.extend_from_slice(&frame);
        frame = framed(2, &[], &body);
    }
    assert!(goofi_codec::decode(&frame).unwrap_err().contains("nesting"));
    assert!(goofi_codec::decode(&encode(&Data::string("next frame", Meta::empty()))).is_ok());
}

#[test]
fn a_foreign_dtype_is_cast_to_f32_where_it_enters() {
    // A Python child hands over whatever dtype numpy made, and the decoder is the one place it
    // becomes the f32 every `Data` holds: one frame per dtype the ingest names, each sample a
    // value both widths hold exactly.
    fn body(dtype: &str, shape: &[u32], samples: &[u8]) -> Vec<u8> {
        let mut body = vec![shape.len() as u8, dtype.len() as u8];
        body.extend_from_slice(dtype.as_bytes());
        body.resize(body.len().div_ceil(ALIGN) * ALIGN, 0);
        for dim in shape {
            body.extend_from_slice(&dim.to_le_bytes());
        }
        body.extend_from_slice(samples);
        body
    }
    let decoded = |dtype: &str, shape: &[u32], samples: &[u8]| goofi_codec::decode(&framed(0, &[], &body(dtype, shape, samples)));
    fn bytes<const N: usize, T: Copy>(values: &[T], le: fn(T) -> [u8; N]) -> Vec<u8> {
        values.iter().flat_map(|v| le(*v)).collect()
    }
    let cases: Vec<(&str, Vec<u8>, [f32; 2])> = vec![
        ("<f8", bytes(&[1.5f64, -2.25], f64::to_le_bytes), [1.5, -2.25]),
        ("<f2", bytes(&[0x3c00u16, 0xc000], u16::to_le_bytes), [1.0, -2.0]),
        ("|i1", vec![0xff, 0x7f], [-1.0, 127.0]),
        ("<i2", bytes(&[-300i16, 300], i16::to_le_bytes), [-300.0, 300.0]),
        ("<i4", bytes(&[-70_000i32, 70_000], i32::to_le_bytes), [-70_000.0, 70_000.0]),
        ("<i8", bytes(&[-5_000_000_000i64, 5_000_000_000], i64::to_le_bytes), [-5.0e9, 5.0e9]),
        ("|u1", vec![0, 255], [0.0, 255.0]),
        ("<u2", bytes(&[1u16, 65_535], u16::to_le_bytes), [1.0, 65_535.0]),
        ("<u4", bytes(&[1u32, 4_000_000_000], u32::to_le_bytes), [1.0, 4.0e9]),
        ("<u8", bytes(&[1u64, 1 << 40], u64::to_le_bytes), [1.0, 1_099_511_627_776.0]),
        ("|b1", vec![0, 1], [0.0, 1.0]),
        // numpy spells the machine's own order `=`, and a bare typestring carries none.
        ("=i4", bytes(&[7i32, -7], i32::to_le_bytes), [7.0, -7.0]),
        ("i2", bytes(&[7i16, -7], i16::to_le_bytes), [7.0, -7.0]),
        ("<f4", le_bytes(&[0.25, -0.5]), [0.25, -0.5]),
    ];
    for (dtype, samples, want) in &cases {
        let d = decoded(dtype, &[2], samples).unwrap_or_else(|e| panic!("[{dtype}] {e}"));
        let store = d.as_array().unwrap_or_else(|e| panic!("[{dtype}] {e}"));
        assert_eq!((store.shape(), store.values().collect::<Vec<f32>>()), (&[2usize][..], want.to_vec()), "[{dtype}]");
        assert_eq!(encode(&d), encode(&arr(&[2], le_bytes(want), Meta::empty())), "[{dtype}] what leaves again is `<f4`");
    }
    // The shape is the frame's own, whatever the width of a sample.
    let image = decoded("|u1", &[2, 3], &[0, 1, 2, 3, 4, 255]).expect("a 2x3 of bytes");
    let store = image.as_array().unwrap();
    assert_eq!((store.shape(), store.values().collect::<Vec<f32>>()), (&[2usize, 3][..], vec![0.0, 1.0, 2.0, 3.0, 4.0, 255.0]));

    // What the ingest cannot read is refused, never guessed at: another byte order, a dtype that
    // is no number, a buffer that is no whole number of samples, or not the samples the shape counts.
    let big_endian = decoded(">f4", &[1], &1.0f32.to_be_bytes()).unwrap_err();
    assert!(big_endian.contains("unsupported dtype `>f4`"), "{big_endian}");
    assert!(decoded("<c8", &[1], &[0; 8]).unwrap_err().contains("unsupported dtype `<c8`"));
    assert!(decoded("<i2", &[2], &[1, 0, 2]).is_err(), "three bytes are not two 16-bit samples");
    assert!(decoded("<i2", &[3], &[1, 0, 2, 0]).is_err(), "two samples under a shape of three");
    assert!(decoded("<f4", &[3], &[0; 8]).is_err(), "and the same of f32 itself");
}

#[test]
fn a_frames_hash_tells_what_it_says_from_when_it_was_made() {
    use goofi_codec::{content_hash, encode_stamps, frame_meta, is_stamps, stamp_hash, wire_hashes};
    // A frame of two samples at a rate, as the engine stamps the run numbered `index` at `at`.
    let said = |value: f32, sfreq: f64, index: u64, at: f64| {
        let mut meta = Meta::new().with_sfreq(Some(sfreq)).with_index(Some(index));
        meta.set_procstart(Some(at));
        meta.set_procend(Some(at + 0.001));
        meta.set_ufreq(Some(30.0));
        arr(&[2], le_bytes(&[value, 2.0]), meta)
    };
    let content = |d: &Data| content_hash(d).expect("a frame that hashes");
    let stamps = |d: &Data| stamp_hash(d).expect("a frame that hashes");
    let wire = |d: &Data| wire_hashes(&encode(d)).expect("a frame off the wire");
    let (first, later) = (said(1.0, 250.0, 1, 0.5), said(1.0, 250.0, 2, 0.6));

    // The same samples at the same rate on a later run say the same thing, at another time.
    assert_eq!(content(&first), content(&later), "the stamps are no part of what a frame says");
    assert_ne!(stamps(&first), stamps(&later));
    assert_eq!(stamps(&first), stamps(&said(9.0, 1.0, 1, 0.5)), "and what it says is no part of its stamps");
    // A sample, a rate or an axis label that moved is another frame.
    let named = first.with_meta(first.meta().clone().with_channels(Axes::new().with(0, Axis::coords(vec![Coord::Str("Fz".into()), Coord::Str("Cz".into())]))));
    assert_ne!(content(&first), content(&named), "an axis label moved and the hash stood");
    for (what, other) in [("a sample", said(1.5, 250.0, 1, 0.5)), ("the rate", said(1.0, 500.0, 1, 0.5))] {
        assert_ne!(content(&first), content(&other), "{what} moved and the hash stood");
        assert_ne!(wire(&first).0, wire(&other).0, "{what} moved and the wire's hash stood");
    }
    // Read off the encoded bytes the two halves part the same way, and the meta comes with them.
    let ((said_first, at_first, meta), (said_later, at_later, _)) = (wire(&first), wire(&later));
    assert_eq!(said_first, said_later);
    assert_ne!(at_first, at_later);
    assert_eq!((meta.index(), meta.sfreq(), meta.procstart()), (Some(1), Some(250.0), Some(0.5)));
    // A table's child stamps are its body: equal values from a later run are another table.
    let table = |child: &Data| Data::table([("value".to_string(), child.clone())].into_iter().collect(), Meta::empty());
    assert_ne!(content(&table(&first)), content(&table(&later)));
    assert_ne!(wire(&table(&first)).0, wire(&table(&later)).0);
    assert_eq!(content(&table(&first)), content(&table(&first)));

    // The stamps travel alone as a frame with no body: it holds every stamp and nothing the
    // frame says, and it is no data to decode.
    let alone = encode_stamps(later.meta()).expect("a stamps frame");
    assert!(is_stamps(&alone) && !is_stamps(&encode(&later)) && !is_stamps(b"GOOF"));
    let carried = frame_meta(&alone).expect("its meta");
    assert_eq!((carried.index(), carried.procstart(), carried.procend(), carried.ufreq()), (Some(2), Some(0.6), Some(0.601), Some(30.0)));
    assert_eq!(carried.sfreq(), None, "the rate is the frame's, not the run's");
    assert!(goofi_codec::decode(&alone).unwrap_err().contains("carries no data"));
    assert!(wire_hashes(b"NOPE").is_err() && frame_meta(&encode(&first)[..8]).is_err());
}
