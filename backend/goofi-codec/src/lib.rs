//! The GOOF v2 binary frame codec: the browser data plane, and the subprocess boundary.
//!
//! Frame: `magic "GOOF" | u8 version | u8 dtype tag | u32 meta_len | u32 body_len | meta | body`,
//! little-endian, with the meta dict projected from the typed `Meta` plus derived shape/dtype.


use std::hash::{DefaultHasher, Hasher};

use goofi_core::{Coord, Data, MetaValue, Value, META_CHANNELS, META_INDEX, META_TIME, META_UFREQ};
use rmpv::Value as Mp;

pub const MAGIC: &[u8; 4] = b"GOOF";
pub const VERSION: u8 = 2;
pub const HEADER_SIZE: usize = 14;

/// Encode a `Data` into a fresh GOOF v2 frame.
pub fn encode(d: &Data) -> Vec<u8> {
    let mut body = Vec::new();
    write_body(d, &mut body);
    frame(d.dtype_tag(), pack_meta(d), body)
}

/// An 8-bit array frame for the browser hop, where `Data` itself stays f32: the same header and
/// the same meta, with a `|u1` body.
pub fn encode_u8(shape: &[usize], texels: &[u8], meta: &goofi_core::Meta) -> Vec<u8> {
    let mut body = Vec::new();
    array_body(b"|u1", shape, texels, &mut body);
    frame(0, pack_array_meta(meta, shape, "uint8"), body)
}

/// A half-float array frame for a line viewer's hop, where `Data` itself stays f32: the same
/// header and the same meta, with a `<f2` body. `None` for a finite sample beyond a half's range.
pub fn encode_f16(d: &Data) -> Option<Vec<u8>> {
    let Value::Array(store) = d.value() else { return None };
    let mut halves = Vec::with_capacity(store.as_bytes().len() / 2);
    for b in store.as_bytes().chunks_exact(4) {
        let v = f32::from_le_bytes(b.try_into().expect("four bytes"));
        if v.is_finite() && v.abs() > half::f16::MAX.to_f32() {
            return None;
        }
        halves.extend_from_slice(&half::f16::from_f32(v).to_le_bytes());
    }
    let mut body = Vec::new();
    array_body(b"<f2", store.shape(), &halves, &mut body);
    Some(frame(0, pack_array_meta(d.meta(), store.shape(), "float16"), body))
}

/// The tag of a frame that carries the engine's per-emit stamps alone, with no body: sent when
/// the frame they belong to already reached the viewers, and merged into it there.
pub const STAMPS_TAG: u8 = 4;
/// What the engine writes afresh on every emit, and what [`content_hash`] leaves out.
const STAMP_KEYS: [&str; 3] = [META_TIME, META_INDEX, META_UFREQ];

/// A 64-bit hash of what a frame SAYS: its kind, its body, and its meta without the engine's
/// per-emit stamps — so a held value emitted again hashes as the frame before it.
pub fn content_hash(d: &Data) -> u64 {
    let mut h = DefaultHasher::new();
    hash_into(d, &mut h);
    h.finish()
}

/// A 64-bit hash of the stamps alone, the other half of [`content_hash`].
pub fn stamp_hash(d: &Data) -> u64 {
    let mut h = DefaultHasher::new();
    h.write(&pack(stamps(d.meta())));
    h.finish()
}

/// The stamps of `meta` as a frame of their own, under [`STAMPS_TAG`].
pub fn encode_stamps(meta: &goofi_core::Meta) -> Vec<u8> {
    frame(STAMPS_TAG, pack(stamps(meta)), Vec::new())
}

/// Whether `frame` is a stamps frame: its meta is read with [`frame_meta`], and it has no data.
pub fn is_stamps(frame: &[u8]) -> bool {
    split_frame(frame).is_ok_and(|(tag, _, _)| tag == STAMPS_TAG)
}

fn stamps(meta: &goofi_core::Meta) -> Vec<(Mp, Mp)> {
    carried(meta).into_iter().filter(|(k, _)| k.as_str().is_some_and(|k| STAMP_KEYS.contains(&k))).collect()
}

fn hash_into(d: &Data, h: &mut DefaultHasher) {
    h.write_u8(d.dtype_tag());
    let mut said: Vec<(Mp, Mp)> = carried(d.meta())
        .into_iter()
        .filter(|(k, _)| !k.as_str().is_some_and(|k| STAMP_KEYS.contains(&k)))
        .collect();
    // By key, so a frame that sets the same keys in another order says the same thing.
    said.sort_by(|(a, _), (b, _)| a.as_str().cmp(&b.as_str()));
    said.push((Mp::from(META_CHANNELS), channels_to_mp(d.meta().channels())));
    h.write(&pack(said));
    match d.value() {
        Value::Texture(t) => h.write(&rmp_serde::to_vec(&**t).expect("texture serialization")),
        Value::Array(store) => {
            h.write_usize(store.shape().len());
            for &dim in store.shape() {
                h.write_usize(dim);
            }
            h.write(store.as_bytes());
        }
        Value::Str(s) => h.write(s.as_bytes()),
        Value::Table(map) => {
            h.write_usize(map.len());
            for (key, value) in map.iter() {
                h.write(key.as_bytes());
                hash_into(value, h);
            }
        }
    }
}

/// The header every frame carries, around a packed meta and a body.
fn frame(dtype_tag: u8, meta: Vec<u8>, body: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_SIZE + meta.len() + body.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(dtype_tag);
    out.extend_from_slice(&(meta.len() as u32).to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&meta);
    out.extend_from_slice(&body);
    out
}

fn write_body(d: &Data, out: &mut Vec<u8>) {
    match d.value() {
        Value::Texture(t) => out.extend(rmp_serde::to_vec(&**t).expect("texture serialization")),
        Value::Array(store) => array_body(b"<f4", store.shape(), store.as_bytes(), out),
        Value::Str(s) => out.extend_from_slice(s.as_bytes()),
        Value::Table(map) => {
            out.extend_from_slice(&(map.len() as u32).to_le_bytes());
            for (key, value) in map.iter() {
                let kb = key.as_bytes();
                out.extend_from_slice(&(kb.len() as u16).to_le_bytes());
                out.extend_from_slice(kb);
                let frame = encode(value);
                out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
                out.extend_from_slice(&frame);
            }
        }
    }
}

/// `[u8 ndim][u8 dtype_str_len][dtype_str][ndim × u32 shape][raw bytes]`.
fn array_body(dtype_str: &[u8], shape: &[usize], samples: &[u8], out: &mut Vec<u8>) {
    out.push(shape.len() as u8);
    out.push(dtype_str.len() as u8);
    out.extend_from_slice(dtype_str);
    for &dim in shape {
        out.extend_from_slice(&(dim as u32).to_le_bytes());
    }
    out.extend_from_slice(samples);
}

/// Meta names the wire derives from the `Data` itself, so they are never taken from `Meta`.
const DERIVED_KEYS: [&str; 2] = ["shape", "dtype"];

/// Serialize a `Data`'s `Meta` to the msgpack map used in a GOOF frame.
fn pack_meta(d: &Data) -> Vec<u8> {
    let meta = d.meta();
    match d.value() {
        Value::Texture(_) => pack(carried(meta)),
        Value::Array(store) => pack_array_meta(meta, store.shape(), "float32"),
        Value::Str(_) => {
            let mut entries = carried(meta);
            entries.push((Mp::from("dtype"), Mp::from("str")));
            pack(entries)
        }
        Value::Table(_) => {
            let mut entries = carried(meta);
            entries.push((Mp::from("dtype"), Mp::from("table")));
            pack(entries)
        }
    }
}

/// An array frame's meta: what the `Meta` carries, plus the shape, dtype and axes the wire
/// derives. Shared with [`encode_u8`], whose frame has no `Data` to read them off.
fn pack_array_meta(meta: &goofi_core::Meta, shape: &[usize], dtype: &str) -> Vec<u8> {
    let mut entries = carried(meta);
    let dims: Vec<Mp> = shape.iter().map(|&d| Mp::from(d as u64)).collect();
    entries.push((Mp::from("shape"), Mp::Array(dims)));
    entries.push((Mp::from("dtype"), Mp::from(dtype)));
    entries.push((Mp::from("channels"), channels_to_mp(meta.channels())));
    pack(entries)
}

/// Every key the `Meta` itself carries. `channels` and the derived names are projected beside
/// them; dropping them here is what keeps the map from carrying one key twice.
fn carried(meta: &goofi_core::Meta) -> Vec<(Mp, Mp)> {
    meta.iter()
        .filter(|(k, v)| {
            *k != goofi_core::META_CHANNELS && !DERIVED_KEYS.contains(&k.as_str()) && !matches!(v, MetaValue::Null)
        })
        .map(|(k, v)| (Mp::from(k.as_str()), mv_to_mp(v)))
        .collect()
}

fn pack(entries: Vec<(Mp, Mp)>) -> Vec<u8> {
    let mut buf = Vec::new();
    rmpv::encode::write_value(&mut buf, &Mp::Map(entries)).expect("msgpack meta encode");
    buf
}

fn channels_to_mp(ch: &goofi_core::Axes) -> Mp {
    // Python-compat: positional axes cross as a dim-keyed dict, and axis names have no wire slot.
    let entries = ch
        .dims()
        .map(|(dim, coords)| (Mp::from(dim), Mp::Array(coords.iter().map(coord_to_mp).collect())))
        .collect();
    Mp::Map(entries)
}

fn coord_to_mp(c: &Coord) -> Mp {
    match c {
        Coord::Num(n) => Mp::from(*n),
        Coord::Str(s) => Mp::from(s.as_ref()),
    }
}

fn mv_to_mp(v: &MetaValue) -> Mp {
    match v {
        MetaValue::Null => Mp::Nil,
        MetaValue::Bool(b) => Mp::from(*b),
        MetaValue::Int(i) => Mp::from(*i),
        MetaValue::Uint(u) => Mp::from(*u),
        MetaValue::Float(f) => Mp::from(*f),
        MetaValue::Str(s) => Mp::from(s.as_str()),
        MetaValue::Bytes(b) => Mp::Binary(b.clone()),
        MetaValue::List(l) => Mp::Array(l.iter().map(mv_to_mp).collect()),
        MetaValue::Map(m) => {
            Mp::Map(m.iter().map(|(k, v)| (Mp::from(k.as_str()), mv_to_mp(v))).collect())
        }
        // `channels` is projected via channels_to_mp, never through here.
        MetaValue::Axes(_) => Mp::Nil,
    }
}

/// Split a frame into `(dtype_tag, meta_bytes, body_bytes)`, validating the header.
pub fn split_frame(frame: &[u8]) -> std::result::Result<(u8, &[u8], &[u8]), String> {
    if frame.len() < HEADER_SIZE {
        return Err(format!("frame too small: {} bytes", frame.len()));
    }
    if &frame[0..4] != MAGIC {
        return Err(format!("bad magic {:?}", &frame[0..4]));
    }
    if frame[4] != VERSION {
        return Err(format!("bad version {}", frame[4]));
    }
    let tag = frame[5];
    let meta_len = u32::from_le_bytes(frame[6..10].try_into().unwrap()) as usize;
    let body_len = u32::from_le_bytes(frame[10..14].try_into().unwrap()) as usize;
    let meta_end = HEADER_SIZE.checked_add(meta_len).ok_or("metadata length overflow")?;
    let body_end = meta_end.checked_add(body_len).ok_or("body length overflow")?;
    if frame.len() < body_end {
        return Err(format!(
            "frame truncated: need {body_end}, have {}",
            frame.len()
        ));
    }
    Ok((tag, &frame[HEADER_SIZE..meta_end], &frame[meta_end..body_end]))
}

/// A frame's META alone — what a recorder reads to name a file and to count a gap, without
/// paying for the body it is about to write through untouched.
pub fn frame_meta(frame: &[u8]) -> std::result::Result<goofi_core::Meta, String> {
    let (_, meta, _) = split_frame(frame)?;
    parse_meta(meta)
}

/// An array body's dtype string, shape and samples, BORROWED and whatever the dtype — what a
/// reader needs to plan for a frame without decoding one.
pub fn array_head(body: &[u8]) -> Option<(&[u8], Vec<usize>, &[u8])> {
    let mut cur = Cursor::new(body);
    let ndim = cur.u8("array ndim").ok()?;
    let dslen = cur.u8("array dtype len").ok()?;
    let dtype = cur.take(dslen, "array dtype string").ok()?;
    let mut shape = Vec::with_capacity(ndim);
    for _ in 0..ndim {
        shape.push(cur.u32("array shape").ok()?);
    }
    Some((dtype, shape, cur.rest()))
}

/// An array body's shape and its samples, BORROWED. The engine's own arrays are `<f4` already, so
/// a recorder appends the bytes it was handed; a foreign dtype answers `None` and takes [`decode`].
pub fn array_view(body: &[u8]) -> Option<(Vec<usize>, &[u8])> {
    let (dtype, shape, samples) = array_head(body)?;
    if dtype != b"<f4" {
        return None;
    }
    let bytes = shape.iter().try_fold(4usize, |n, &d| n.checked_mul(d))?;
    (samples.len() == bytes).then_some((shape, samples))
}

/// Decode a GOOF v2 frame into a `Data`. The inverse of [`encode`].
pub fn decode(frame: &[u8]) -> std::result::Result<Data, String> {
    decode_at(frame, 0)
}

const MAX_DEPTH: usize = 64;

fn decode_at(frame: &[u8], depth: usize) -> std::result::Result<Data, String> {
    if depth >= MAX_DEPTH { return Err("frame nesting exceeds 64 levels".into()); }
    let (tag, meta_bytes, body) = split_frame(frame)?;
    let meta = parse_meta(meta_bytes)?;
    match tag {
        0 => decode_array_body(body, meta),
        1 => {
            let s = std::str::from_utf8(body).map_err(|e| e.to_string())?;
            Ok(Data::string(s, meta))
        }
        2 => decode_table(body, meta, depth),
        3 => Data::texture(rmp_serde::from_slice(body).map_err(|e| e.to_string())?, meta),
        STAMPS_TAG => Err("a stamps frame carries no data".into()),
        other => Err(format!("unknown dtype tag {other}")),
    }
}

/// A forward-only reader over a body slice: every read is bounds-checked, so a truncated or
/// hostile frame yields `Err` rather than a panic.
struct Cursor<'a> {
    body: &'a [u8],
    off: usize,
}

impl<'a> Cursor<'a> {
    fn new(body: &'a [u8]) -> Cursor<'a> {
        Cursor { body, off: 0 }
    }
    /// The next `n` bytes, advancing past them; `what` names them in the error.
    fn take(&mut self, n: usize, what: &str) -> std::result::Result<&'a [u8], String> {
        let end = self.off.checked_add(n).ok_or_else(|| format!("{what} length overflow"))?;
        let s = self.body.get(self.off..end).ok_or_else(|| format!("{what} truncated"))?;
        self.off = end;
        Ok(s)
    }
    fn u8(&mut self, what: &str) -> std::result::Result<usize, String> {
        Ok(self.take(1, what)?[0] as usize)
    }
    fn u16(&mut self, what: &str) -> std::result::Result<usize, String> {
        Ok(u16::from_le_bytes(self.take(2, what)?.try_into().unwrap()) as usize)
    }
    fn u32(&mut self, what: &str) -> std::result::Result<usize, String> {
        Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap()) as usize)
    }
    fn rest(self) -> &'a [u8] {
        &self.body[self.off..]
    }
}

/// Decode an array body: the ingest boundary, where a foreign source dtype is cast to f32.
fn decode_array_body(body: &[u8], meta: goofi_core::Meta) -> std::result::Result<Data, String> {
    let mut cur = Cursor::new(body);
    let ndim = cur.u8("array ndim")?;
    let dslen = cur.u8("array dtype len")?;
    let dstr = std::str::from_utf8(cur.take(dslen, "array dtype string")?).map_err(|e| e.to_string())?;
    let src = goofi_core::SrcDtype::from_numpy_typestr(dstr)
        .ok_or_else(|| format!("unsupported dtype `{dstr}`"))?;
    let mut shape = Vec::with_capacity(ndim);
    for _ in 0..ndim {
        shape.push(cur.u32("array shape")?);
    }
    // The shape×4 overflow guard lives in `array_f32`, deliberately.
    let (f32_bytes, _did_cast) = goofi_core::cast_to_f32(src, cur.rest()).map_err(|e| e.to_string())?;
    Data::array_f32(shape, f32_bytes, meta).map_err(|e| e.to_string())
}

fn decode_table(body: &[u8], meta: goofi_core::Meta, depth: usize) -> std::result::Result<Data, String> {
    let mut cur = Cursor::new(body);
    let n = cur.u32("table count")?;
    let mut map: indexmap::IndexMap<String, Data> = indexmap::IndexMap::new();
    for _ in 0..n {
        let klen = cur.u16("table key length")?;
        let key = std::str::from_utf8(cur.take(klen, "table key")?)
            .map_err(|e| e.to_string())?
            .to_string();
        let vlen = cur.u32("table value length")?;
        let child = decode_at(cur.take(vlen, "table value frame")?, depth + 1)?;
        map.insert(key, child);
    }
    Ok(Data::table(map, meta))
}

/// Parse the msgpack meta map written by [`pack_meta`] back into a typed `Meta`.
fn parse_meta(bytes: &[u8]) -> std::result::Result<goofi_core::Meta, String> {
    let mut meta = goofi_core::Meta::empty();
    if bytes.is_empty() {
        return Ok(meta);
    }
    let mut cur = bytes;
    let v = rmpv::decode::read_value_with_max_depth(&mut cur, MAX_DEPTH).map_err(|e| e.to_string())?;
    let Mp::Map(entries) = v else {
        return Ok(meta);
    };
    for (k, val) in entries {
        let Some(key) = k.as_str() else { continue };
        match key {
            // shape/dtype are derived from the body — ignore the redundant keys.
            "shape" | "dtype" => {}
            "channels" => meta.set_channels(parse_channels(&val)),
            other => meta.set(other, mp_to_mv(&val)),
        }
    }
    Ok(meta)
}

fn parse_channels(v: &Mp) -> goofi_core::Axes {
    // Entries may arrive out of order, so `with` pads up to the max labeled dim.
    let mut axes = goofi_core::Axes::new();
    if let Mp::Map(entries) = v {
        for (k, list) in entries {
            let Some(dim) = k
                .as_str()
                .and_then(|s| s.strip_prefix("dim"))
                .and_then(|d| d.parse::<usize>().ok())
            else {
                continue;
            };
            if let Mp::Array(items) = list {
                let coords: Vec<Coord> = items.iter().map(mp_to_coord).collect();
                axes = axes.with(dim, goofi_core::Axis::coords(coords));
            }
        }
    }
    axes
}

fn mp_to_coord(v: &Mp) -> Coord {
    match v {
        Mp::String(s) => Coord::Str(s.as_str().unwrap_or("").into()),
        other => Coord::Num(other.as_f64().unwrap_or(0.0)),
    }
}

fn mp_to_mv(v: &Mp) -> MetaValue {
    match v {
        Mp::Nil => MetaValue::Null,
        Mp::Boolean(b) => MetaValue::Bool(*b),
        Mp::Integer(i) => {
            if let Some(s) = i.as_i64() {
                MetaValue::Int(s)
            } else if let Some(u) = i.as_u64() {
                MetaValue::Uint(u)
            } else {
                MetaValue::Null
            }
        }
        Mp::F32(f) => MetaValue::Float(*f as f64),
        Mp::F64(f) => MetaValue::Float(*f),
        Mp::String(s) => MetaValue::Str(s.as_str().unwrap_or("").to_string()),
        Mp::Binary(b) => MetaValue::Bytes(b.clone()),
        Mp::Array(a) => MetaValue::List(a.iter().map(mp_to_mv).collect()),
        Mp::Map(m) => MetaValue::Map(
            m.iter()
                .filter_map(|(k, v)| k.as_str().map(|ks| (ks.to_string(), mp_to_mv(v))))
                .collect(),
        ),
        Mp::Ext(_, _) => MetaValue::Null,
    }
}

/// `group -> name -> Param`, spelled out because the codec has no goofi-node dep.
pub type ParamMap = indexmap::IndexMap<String, indexmap::IndexMap<String, goofi_core::Param>>;

/// The slot entries of a run request: `(slot, source, frame)`, the source empty on a single slot.
pub type SourcedSlots = Vec<(String, String, Data)>;

/// Append a named-slot list: `[u16 n]` then n × `[u16 name_len][name][u16 src_len][src]
/// [u32 frame_len][GOOF frame]`; `src` is the `node.slot` a multi-slot frame came from, else empty.
pub fn encode_slots(slots: &[(&str, &str, &Data)], out: &mut Vec<u8>) {
    out.extend_from_slice(&(slots.len() as u16).to_le_bytes());
    for (name, source, d) in slots {
        for text in [name, source] {
            let tb = text.as_bytes();
            out.extend_from_slice(&(tb.len() as u16).to_le_bytes());
            out.extend_from_slice(tb);
        }
        let frame = encode(d);
        out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        out.extend_from_slice(&frame);
    }
}

/// Decode the named-slot list written by [`encode_slots`].
pub fn decode_slots(body: &[u8]) -> std::result::Result<SourcedSlots, String> {
    let mut cur = Cursor::new(body);
    let n = cur.u16("slot count")?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let nlen = cur.u16("slot name length")?;
        let name = std::str::from_utf8(cur.take(nlen, "slot name")?).map_err(|e| e.to_string())?.to_string();
        let slen = cur.u16("slot source length")?;
        let source = std::str::from_utf8(cur.take(slen, "slot source")?).map_err(|e| e.to_string())?.to_string();
        let flen = cur.u32("slot frame length")?;
        let data = decode(cur.take(flen, "slot frame")?)?;
        out.push((name, source, data));
    }
    Ok(out)
}

/// A decoded subprocess request, always carrying the node's live params: one tick, the ⟳ on
/// one string param, or a pulse on one pulse param.
pub enum Request {
    Process { params: ParamMap, slots: SourcedSlots },
    Refresh { params: ParamMap, group: String, name: String },
    Pulse { params: ParamMap, group: String, name: String },
}

fn encode_params(params: &ParamMap, out: &mut Vec<u8>) {
    let pbytes = rmp_serde::to_vec(params).expect("param serialize (Param derives Serialize)");
    out.extend_from_slice(&(pbytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&pbytes);
}

/// Encode a tick request: `[0][u32 params_len][params msgpack][slots]`, each slot with its source.
pub fn encode_request(params: &ParamMap, slots: &[(&str, &str, &Data)]) -> Vec<u8> {
    let mut out = vec![0u8];
    encode_params(params, &mut out);
    encode_slots(slots, &mut out);
    out
}

/// Encode a refresh request: `[1][u32 params_len][params msgpack][(group, name) msgpack]`.
pub fn encode_refresh_request(params: &ParamMap, group: &str, name: &str) -> Vec<u8> {
    encode_keyed_request(1, params, group, name)
}

/// Encode a pulse request: `[2][u32 params_len][params msgpack][(group, name) msgpack]`.
pub fn encode_pulse_request(params: &ParamMap, group: &str, name: &str) -> Vec<u8> {
    encode_keyed_request(2, params, group, name)
}

fn encode_keyed_request(tag: u8, params: &ParamMap, group: &str, name: &str) -> Vec<u8> {
    let mut out = vec![tag];
    encode_params(params, &mut out);
    out.extend_from_slice(&rmp_serde::to_vec(&(group, name)).expect("two strings"));
    out
}

/// Decode a request frame written by [`encode_request`], [`encode_refresh_request`] or
/// [`encode_pulse_request`].
pub fn decode_request(buf: &[u8]) -> std::result::Result<Request, String> {
    let (&tag, rest) = buf.split_first().ok_or("empty request frame")?;
    let mut cur = Cursor::new(rest);
    let plen = cur.u32("params length")?;
    let pbytes = cur.take(plen, "params blob")?;
    let params: ParamMap = rmp_serde::from_slice(pbytes).map_err(|e| e.to_string())?;
    match tag {
        0 => Ok(Request::Process { params, slots: decode_slots(cur.rest())? }),
        1 | 2 => {
            let (group, name): (String, String) =
                rmp_serde::from_slice(cur.rest()).map_err(|e| e.to_string())?;
            Ok(if tag == 1 { Request::Refresh { params, group, name } } else { Request::Pulse { params, group, name } })
        }
        other => Err(format!("unknown request tag {other}")),
    }
}

/// Outputs and input clears from a successful process call.
pub struct ProcessOutput {
    pub outputs: Vec<(String, Data)>,
    pub clear_inputs: Vec<String>,
}

pub enum Response {
    Process(ProcessOutput),
    NodeError(String),
    Options(Option<Vec<String>>),
}

/// Encode outputs and input clears from a successful call.
pub fn encode_response(slots: &[(&str, &Data)], clear_inputs: &[String]) -> Vec<u8> {
    let mut out = vec![0u8];
    let clears = rmp_serde::to_vec(clear_inputs).expect("strings");
    out.extend_from_slice(&(clears.len() as u32).to_le_bytes());
    out.extend_from_slice(&clears);
    let unsourced: Vec<(&str, &str, &Data)> = slots.iter().map(|(name, d)| (*name, "", *d)).collect();
    encode_slots(&unsourced, &mut out);
    out
}

/// Encode a node-error response: `[1][utf8 message]`.
pub fn encode_error_response(msg: &str) -> Vec<u8> {
    let mut out = vec![1u8];
    out.extend_from_slice(msg.as_bytes());
    out
}

/// Encode a refresh response: `[2][Option<Vec<String>> msgpack]`.
pub fn encode_options_response(options: &Option<Vec<String>>) -> Vec<u8> {
    let mut out = vec![2u8];
    out.extend_from_slice(&rmp_serde::to_vec(options).expect("strings"));
    out
}

/// Decode a response frame; the outer `Err` is a malformed frame, never a node-reported one.
pub fn decode_response(buf: &[u8]) -> std::result::Result<Response, String> {
    let (&tag, rest) = buf.split_first().ok_or("empty response frame")?;
    match tag {
        0 => {
            let mut cur = Cursor::new(rest);
            let len = cur.u32("input clears length")?;
            let clear_inputs = rmp_serde::from_slice(cur.take(len, "input clears")?).map_err(|e| e.to_string())?;
            let outputs = decode_slots(cur.rest())?.into_iter().map(|(name, _, d)| (name, d)).collect();
            Ok(Response::Process(ProcessOutput { outputs, clear_inputs }))
        },
        1 => Ok(Response::NodeError(String::from_utf8_lossy(rest).into_owned())),
        2 => Ok(Response::Options(rmp_serde::from_slice(rest).map_err(|e| e.to_string())?)),
        other => Err(format!("unknown response tag {other}")),
    }
}
