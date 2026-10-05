//! The GOOF v2 binary frame codec: the browser data plane. The child boundary is [`rpc`].
//!
//! Frame: `magic "GOOF" | u8 version | u8 dtype tag | u32 meta_len | u32 body_len | meta | body`,
//! little-endian, with the meta dict projected from the typed `Meta` plus derived shape/dtype.


use std::borrow::Cow;

pub mod rpc;
use std::hash::{DefaultHasher, Hasher};
use std::mem::MaybeUninit;
use std::ops::Range;
use std::sync::Arc;

use goofi_core::{Coord, Data, MetaValue, Value, META_CHANNELS, META_EMIT, META_INDEX, META_SOURCE, META_TIME, META_UFREQ};
use rmpv::Value as Mp;

pub const MAGIC: &[u8; 4] = b"GOOF";
pub const VERSION: u8 = 2;
pub const HEADER_SIZE: usize = 14;

/// Why a value cannot cross as a frame: a count or a length the format has no field wide enough
/// for, or a part serde refused. Every encoder checks its lengths before it puts a byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    TooLong { what: &'static str, len: usize, max: usize },
    Serialize(String),
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodeError::TooLong { what, len, max } => write!(f, "{what} of {len} exceeds the wire's {max}"),
            EncodeError::Serialize(why) => write!(f, "does not serialize: {why}"),
        }
    }
}

impl std::error::Error for EncodeError {}

fn u8_of(what: &'static str, n: usize) -> Result<[u8; 1], EncodeError> {
    u8::try_from(n).map(|v| [v]).map_err(|_| EncodeError::TooLong { what, len: n, max: u8::MAX as usize })
}

pub(crate) fn u16_of(what: &'static str, n: usize) -> Result<[u8; 2], EncodeError> {
    u16::try_from(n).map(u16::to_le_bytes).map_err(|_| EncodeError::TooLong { what, len: n, max: u16::MAX as usize })
}

pub(crate) fn u32_of(what: &'static str, n: usize) -> Result<[u8; 4], EncodeError> {
    u32::try_from(n).map(u32::to_le_bytes).map_err(|_| EncodeError::TooLong { what, len: n, max: u32::MAX as usize })
}

pub(crate) fn packed<T: serde::Serialize + ?Sized>(what: &str, value: &T) -> Result<Vec<u8>, EncodeError> {
    rmp_serde::to_vec(value).map_err(|e| EncodeError::Serialize(format!("{what}: {e}")))
}

/// Where an encoder writes. A frame's samples are offered apart from the bytes around them, so
/// a sink that can hand memory on by reference does, and every other sink copies them.
pub trait Out<'a> {
    fn put(&mut self, bytes: &[u8]);
    fn put_samples(&mut self, samples: &'a [u8]) {
        self.put(samples);
    }
}

impl<'a> Out<'a> for Vec<u8> {
    fn put(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

/// A sink that only counts: how a frame's length is known before its loan is taken.
struct Count(usize);

impl<'a> Out<'a> for Count {
    fn put(&mut self, bytes: &[u8]) {
        self.0 += bytes.len();
    }
}

/// A sink over uninitialised memory — an iceoryx2 loan — sized by [`encoded_len`] beforehand.
pub struct Fill<'m> {
    buf: &'m mut [MaybeUninit<u8>],
    at: usize,
}

impl<'m> Fill<'m> {
    pub fn new(buf: &'m mut [MaybeUninit<u8>]) -> Fill<'m> {
        Fill { buf, at: 0 }
    }
    /// Every byte of the loan was written — the one state in which it may be sent.
    pub fn full(&self) -> bool {
        self.at == self.buf.len()
    }
}

impl<'a, 'm> Out<'a> for Fill<'m> {
    fn put(&mut self, bytes: &[u8]) {
        let end = self.at + bytes.len();
        // SAFETY: `MaybeUninit<u8>` and `u8` share a layout, and the range is bounds-checked.
        let dst = &mut self.buf[self.at..end];
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst.as_mut_ptr() as *mut u8, bytes.len()) };
        self.at = end;
    }
}

/// A run of bytes that may hand samples on BY REFERENCE: a request crosses the node boundary as
/// these, concatenated, so a big input is never copied into it. Small samples ride the copy.
#[derive(Default)]
pub struct Segments<'a> {
    done: Vec<Cow<'a, [u8]>>,
    head: Vec<u8>,
}

/// Samples below this ride inside the copied run: a segment costs a descriptor and a bounds check.
const SEGMENT_MIN: usize = 4096;

impl<'a> Segments<'a> {
    pub fn finish(mut self) -> Vec<Cow<'a, [u8]>> {
        if !self.head.is_empty() {
            self.done.push(Cow::Owned(std::mem::take(&mut self.head)));
        }
        self.done
    }
}

impl<'a> Out<'a> for Segments<'a> {
    fn put(&mut self, bytes: &[u8]) {
        self.head.extend_from_slice(bytes);
    }
    fn put_samples(&mut self, samples: &'a [u8]) {
        if samples.len() < SEGMENT_MIN {
            return self.put(samples);
        }
        if !self.head.is_empty() {
            self.done.push(Cow::Owned(std::mem::take(&mut self.head)));
        }
        self.done.push(Cow::Borrowed(samples));
    }
}

/// Encode a `Data` into a fresh GOOF v2 frame.
pub fn encode(d: &Data) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::with_capacity(encoded_len(d)?);
    encode_into(d, &mut out)?;
    Ok(out)
}

/// How many bytes [`encode_into`] writes for `d`.
pub fn encoded_len(d: &Data) -> Result<usize, EncodeError> {
    let mut count = Count(0);
    encode_into(d, &mut count)?;
    Ok(count.0)
}

/// Write `d`'s frame to `out`, the body in place: a frame is never built, then copied. An `Err`
/// is decided before the first byte is put, so it leaves `out` as it was.
pub fn encode_into<'a>(d: &'a Data, out: &mut impl Out<'a>) -> Result<(), EncodeError> {
    let meta = pack_meta(d)?;
    let body = Body::of(d)?;
    put_header(out, d.dtype_tag(), &meta, body.len())?;
    body.write(out)
}

/// The header every frame carries, and its packed meta; the lengths are checked before a byte is put.
fn put_header<'a>(out: &mut impl Out<'a>, dtype_tag: u8, meta: &[u8], body_len: usize) -> Result<(), EncodeError> {
    let meta_len = u32_of("meta", meta.len())?;
    let body_len = u32_of("body", body_len)?;
    out.put(MAGIC);
    out.put(&[VERSION, dtype_tag]);
    out.put(&meta_len);
    out.put(&body_len);
    out.put(meta);
    Ok(())
}

/// A frame's body, its every length checked before a byte of it is written.
enum Body<'a> {
    /// `[u8 ndim][u8 dtype_str_len][dtype_str][ndim × u32 shape]`, then the samples themselves.
    Array { head: Vec<u8>, samples: &'a [u8] },
    Str(&'a [u8]),
    Texture(Vec<u8>),
    /// Per entry: the key's length, the value frame's length, and that frame's length as bytes.
    Table { map: &'a indexmap::IndexMap<String, Data>, count: [u8; 4], heads: Vec<([u8; 2], usize, [u8; 4])> },
}

impl<'a> Body<'a> {
    fn of(d: &'a Data) -> Result<Body<'a>, EncodeError> {
        Ok(match d.value() {
            Value::Texture(t) => Body::Texture(packed("texture", &**t)?),
            Value::Array(store) => Body::Array { head: array_head_bytes(b"<f4", store.shape())?, samples: store.as_bytes() },
            Value::Str(s) => Body::Str(s.as_bytes()),
            Value::Table(map) => {
                let mut heads = Vec::with_capacity(map.len());
                for (key, value) in map.iter() {
                    let len = encoded_len(value)?;
                    heads.push((u16_of("table key", key.len())?, len, u32_of("table value", len)?));
                }
                Body::Table { map, count: u32_of("table count", map.len())?, heads }
            }
        })
    }

    fn len(&self) -> usize {
        match self {
            Body::Array { head, samples } => head.len() + samples.len(),
            Body::Str(s) => s.len(),
            Body::Texture(t) => t.len(),
            Body::Table { map, heads, .. } => 4 + map.keys().zip(heads).map(|(k, (_, len, _))| 2 + k.len() + 4 + len).sum::<usize>(),
        }
    }

    fn write(self, out: &mut impl Out<'a>) -> Result<(), EncodeError> {
        match self {
            Body::Array { head, samples } => {
                out.put(&head);
                out.put_samples(samples);
            }
            Body::Str(s) => out.put(s),
            Body::Texture(t) => out.put(&t),
            Body::Table { map, count, heads } => {
                out.put(&count);
                for ((key, value), (klen, _, vlen)) in map.iter().zip(heads) {
                    out.put(&klen);
                    out.put(key.as_bytes());
                    out.put(&vlen);
                    encode_into(value, out)?;
                }
            }
        }
        Ok(())
    }
}

/// An 8-bit array frame for the browser hop, where `Data` itself stays f32: the same header and
/// the same meta, with a `|u1` body.
pub fn encode_u8(shape: &[usize], texels: &[u8], meta: &goofi_core::Meta) -> Result<Vec<u8>, EncodeError> {
    goofi_core::samples::SampleSpan::validate_shape(meta, shape).map_err(EncodeError::Serialize)?;
    let mut body = array_head_bytes(b"|u1", shape)?;
    body.extend_from_slice(texels);
    frame(0, pack_array_meta(meta, shape, "uint8")?, body)
}

/// A half-float array frame for a line viewer's hop, where `Data` itself stays f32: the same
/// header and the same meta, with a `<f2` body. `None` for a finite sample beyond a half's range.
pub fn encode_f16(d: &Data) -> Result<Option<Vec<u8>>, EncodeError> {
    goofi_core::samples::SampleSpan::validate(d).map_err(EncodeError::Serialize)?;
    let Value::Array(store) = d.value() else { return Ok(None) };
    let mut halves = Vec::with_capacity(store.as_bytes().len() / 2);
    for v in store.values() {
        if v.is_finite() && v.abs() > half::f16::MAX.to_f32() {
            return Ok(None);
        }
        halves.extend_from_slice(&half::f16::from_f32(v).to_le_bytes());
    }
    let mut body = array_head_bytes(b"<f2", store.shape())?;
    body.extend_from_slice(&halves);
    frame(0, pack_array_meta(d.meta(), store.shape(), "float16")?, body).map(Some)
}

/// The tag of a frame that carries the engine's per-emit stamps alone, with no body: sent when
/// the frame they belong to already reached the viewers, and merged into it there.
pub const STAMPS_TAG: u8 = 4;
/// What the engine writes afresh on every emit, and what [`content_hash`] leaves out.
const STAMP_KEYS: [&str; 5] = [META_TIME, META_INDEX, META_UFREQ, META_EMIT, META_SOURCE];

/// A 64-bit hash of what a frame SAYS: its kind, its body, and its meta without the engine's
/// per-emit stamps. A table's child stamps are part of its body and stay in the hash.
pub fn content_hash(d: &Data) -> Result<u64, EncodeError> {
    let mut h = DefaultHasher::new();
    hash_into(d, &mut h, true)?;
    Ok(h.finish())
}

/// A 64-bit hash of the stamps alone, the other half of [`content_hash`].
pub fn stamp_hash(d: &Data) -> Result<u64, EncodeError> {
    let mut h = DefaultHasher::new();
    h.write(&pack(stamps(d.meta()))?);
    Ok(h.finish())
}

/// The stamps of `meta` as a frame of their own, under [`STAMPS_TAG`].
pub fn encode_stamps(meta: &goofi_core::Meta) -> Result<Vec<u8>, EncodeError> {
    frame(STAMPS_TAG, pack(stamps(meta))?, Vec::new())
}

/// Whether `frame` is a stamps frame: its meta is read with [`frame_meta`], and it has no data.
pub fn is_stamps(frame: &[u8]) -> bool {
    split_frame(frame).is_ok_and(|(tag, _, _)| tag == STAMPS_TAG)
}

fn stamps(meta: &goofi_core::Meta) -> Vec<(Mp, Mp)> {
    carried(meta).into_iter().filter(|(k, _)| k.as_str().is_some_and(|k| STAMP_KEYS.contains(&k))).collect()
}

fn hash_into(d: &Data, h: &mut DefaultHasher, root: bool) -> Result<(), EncodeError> {
    h.write_u8(d.dtype_tag());
    let mut said: Vec<(Mp, Mp)> = carried(d.meta())
        .into_iter()
        .filter(|(k, _)| !root || !k.as_str().is_some_and(|k| STAMP_KEYS.contains(&k)))
        .collect();
    // By key, so a frame that sets the same keys in another order says the same thing.
    said.sort_by(|(a, _), (b, _)| a.as_str().cmp(&b.as_str()));
    said.push((Mp::from(META_CHANNELS), channels_to_mp(d.meta().channels())));
    h.write(&pack(said)?);
    match d.value() {
        Value::Texture(t) => h.write(&packed("texture", &**t)?),
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
                hash_into(value, h, false)?;
            }
        }
    }
    Ok(())
}

/// A whole frame around a packed meta and a body.
fn frame(dtype_tag: u8, meta: Vec<u8>, body: Vec<u8>) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::with_capacity(HEADER_SIZE + meta.len() + body.len());
    put_header(&mut out, dtype_tag, &meta, body.len())?;
    out.extend_from_slice(&body);
    Ok(out)
}

/// `[u8 ndim][u8 dtype_str_len][dtype_str][ndim × u32 shape]`: what stands before an array's samples.
fn array_head_bytes(dtype_str: &[u8], shape: &[usize]) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::with_capacity(2 + dtype_str.len() + 4 * shape.len());
    out.extend_from_slice(&u8_of("array rank", shape.len())?);
    out.extend_from_slice(&u8_of("dtype string", dtype_str.len())?);
    out.extend_from_slice(dtype_str);
    for &dim in shape {
        out.extend_from_slice(&u32_of("array dim", dim)?);
    }
    Ok(out)
}

/// Meta names the wire derives from the `Data` itself, so they are never taken from `Meta`.
const DERIVED_KEYS: [&str; 2] = ["shape", "dtype"];

/// Serialize a `Data`'s `Meta` to the msgpack map used in a GOOF frame.
fn pack_meta(d: &Data) -> Result<Vec<u8>, EncodeError> {
    goofi_core::samples::SampleSpan::validate(d).map_err(EncodeError::Serialize)?;
    let meta = d.meta();
    match d.value() {
        Value::Texture(_) => pack(carried(meta)),
        Value::Array(store) => pack_array_meta(meta, store.shape(), "float32"),
        Value::Str(_) | Value::Table(_) => {
            let mut entries = carried(meta);
            let dtype = if matches!(d.value(), Value::Str(_)) { "str" } else { "table" };
            entries.push((Mp::from("dtype"), Mp::from(dtype)));
            pack(entries)
        }
    }
}

/// An array frame's meta: what the `Meta` carries, plus the shape, dtype and axes the wire
/// derives. Shared with [`encode_u8`], whose frame has no `Data` to read them off.
fn pack_array_meta(meta: &goofi_core::Meta, shape: &[usize], dtype: &str) -> Result<Vec<u8>, EncodeError> {
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

fn pack(entries: Vec<(Mp, Mp)>) -> Result<Vec<u8>, EncodeError> {
    let mut buf = Vec::new();
    rmpv::encode::write_value(&mut buf, &Mp::Map(entries)).map_err(|e| EncodeError::Serialize(format!("meta: {e}")))?;
    Ok(buf)
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
    let (tag, meta, body) = split_at(frame)?;
    Ok((tag, &frame[meta], &frame[body]))
}

/// The header's answer as RANGES of `frame`, so a decoder can view the bytes rather than copy them.
fn split_at(frame: &[u8]) -> std::result::Result<(u8, Range<usize>, Range<usize>), String> {
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
    let meta_len = u32::from_le_bytes([frame[6], frame[7], frame[8], frame[9]]) as usize;
    let body_len = u32::from_le_bytes([frame[10], frame[11], frame[12], frame[13]]) as usize;
    let meta_end = HEADER_SIZE.checked_add(meta_len).ok_or("metadata length overflow")?;
    let body_end = meta_end.checked_add(body_len).ok_or("body length overflow")?;
    if frame.len() < body_end {
        return Err(format!(
            "frame truncated: need {body_end}, have {}",
            frame.len()
        ));
    }
    Ok((tag, HEADER_SIZE..meta_end, meta_end..body_end))
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
    let segments = [body];
    let mut cur = Cursor::new(&segments);
    let (dtype, shape) = read_array_head(&mut cur).ok()?;
    Some((dtype, shape, cur.rest().ok()?))
}

/// `[u8 ndim][u8 dtype_len][dtype][ndim × u32 shape]`, read off the front of an array body.
fn read_array_head<'a>(cur: &mut Cursor<'_, 'a>) -> std::result::Result<(&'a [u8], Vec<usize>), String> {
    let ndim = cur.u8("array ndim")?;
    let dslen = cur.u8("array dtype len")?;
    let dtype = cur.take(dslen, "array dtype string")?;
    let shape = (0..ndim).map(|_| cur.u32("array shape")).collect::<std::result::Result<_, _>>()?;
    Ok((dtype, shape))
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
    decode_owned(frame.to_vec())
}

/// Decode a frame that is the whole of `buf`, which the `Data` then keeps: an f32 array's samples
/// are a view of the bytes they arrived in, never copied out of them.
pub fn decode_owned(buf: Vec<u8>) -> std::result::Result<Data, String> {
    let range = 0..buf.len();
    decode_at(&Arc::new(buf), range, 0)
}

const MAX_DEPTH: usize = 64;

pub(crate) fn decode_at(buf: &Arc<Vec<u8>>, at: Range<usize>, depth: usize) -> std::result::Result<Data, String> {
    if depth >= MAX_DEPTH { return Err("frame nesting exceeds 64 levels".into()); }
    let frame = &buf[at.clone()];
    let (tag, meta, body) = split_at(frame)?;
    let meta = parse_meta(&frame[meta])?;
    let body = at.start + body.start..at.start + body.end;
    let data = match tag {
        0 => decode_array_body(buf, body, meta),
        1 => {
            let s = std::str::from_utf8(&buf[body]).map_err(|e| e.to_string())?;
            Ok(Data::string(s, meta))
        }
        2 => decode_table(buf, body, meta, depth),
        3 => Data::texture(rmp_serde::from_slice(&buf[body]).map_err(|e| e.to_string())?, meta),
        STAMPS_TAG => Err("a stamps frame carries no data".into()),
        other => Err(format!("unknown dtype tag {other}")),
    }?;
    goofi_core::samples::SampleSpan::validate(&data)?;
    Ok(data)
}

/// A forward-only, bounds-checked reader over byte runs read as one: a hostile frame yields
/// `Err`, never a panic. A field never straddles two runs; a whole frame may, and is gathered.
pub(crate) struct Cursor<'s, 'a> {
    segments: &'s [&'a [u8]],
    seg: usize,
    pub(crate) off: usize,
}

impl<'s, 'a> Cursor<'s, 'a> {
    pub(crate) fn new(segments: &'s [&'a [u8]]) -> Cursor<'s, 'a> {
        Cursor { segments, seg: 0, off: 0 }
    }
    /// Where the next byte is: its segment, and its offset within it.
    fn at(&mut self) -> (usize, usize) {
        while self.seg < self.segments.len() && self.off == self.segments[self.seg].len() {
            self.seg += 1;
            self.off = 0;
        }
        (self.seg, self.off)
    }
    /// The next `n` bytes, advancing past them; `what` names them in the error.
    pub(crate) fn take(&mut self, n: usize, what: &str) -> std::result::Result<&'a [u8], String> {
        let (seg, off) = self.at();
        if n == 0 {
            return Ok(&[]);
        }
        let segment = self.segments.get(seg).ok_or_else(|| format!("{what} truncated"))?;
        let end = off.checked_add(n).ok_or_else(|| format!("{what} length overflow"))?;
        let s = segment.get(off..end).ok_or_else(|| format!("{what} truncated"))?;
        self.off = end;
        Ok(s)
    }
    /// The next `n` bytes gathered into a buffer of their own, across runs: a frame's head and its
    /// samples are two runs, and the one copy here is the copy that owns the frame.
    pub(crate) fn gather(&mut self, n: usize, what: &str) -> std::result::Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            let (seg, off) = self.at();
            let segment = self.segments.get(seg).ok_or_else(|| format!("{what} truncated"))?;
            let piece = &segment[off..segment.len().min(off + n - out.len())];
            out.extend_from_slice(piece);
            self.off = off + piece.len();
        }
        Ok(out)
    }
    pub(crate) fn u8(&mut self, what: &str) -> std::result::Result<usize, String> {
        Ok(self.take(1, what)?[0] as usize)
    }
    pub(crate) fn u16(&mut self, what: &str) -> std::result::Result<usize, String> {
        let b = self.take(2, what)?;
        Ok(u16::from_le_bytes([b[0], b[1]]) as usize)
    }
    pub(crate) fn u32(&mut self, what: &str) -> std::result::Result<usize, String> {
        let b = self.take(4, what)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    /// Everything left, which must lie in one segment.
    pub(crate) fn rest(mut self) -> std::result::Result<&'a [u8], String> {
        let (seg, off) = self.at();
        let mut segments = self.segments.iter().skip(seg);
        let rest = segments.next().map_or(&[][..], |s| &s[off..]);
        match segments.any(|s| !s.is_empty()) {
            true => Err("a trailing run straddles segments".into()),
            false => Ok(rest),
        }
    }
    pub(crate) fn done(&mut self) -> bool {
        let (seg, _) = self.at();
        seg == self.segments.len()
    }
}

/// Decode an array body: the ingest boundary, where a foreign source dtype is cast to f32, and a
/// frame already f32 is viewed where it lies.
fn decode_array_body(buf: &Arc<Vec<u8>>, body: Range<usize>, meta: goofi_core::Meta) -> std::result::Result<Data, String> {
    let segments = [&buf[body.clone()]];
    let mut cur = Cursor::new(&segments);
    let (dtype, shape) = read_array_head(&mut cur)?;
    let dstr = std::str::from_utf8(dtype).map_err(|e| e.to_string())?;
    let src = goofi_core::SrcDtype::from_numpy_typestr(dstr)
        .ok_or_else(|| format!("unsupported dtype `{dstr}`"))?;
    let samples = body.start + cur.off..body.end;
    // The shape×4 overflow guard lives in `array_shared`, deliberately.
    if src == goofi_core::SrcDtype::F32 {
        return Data::array_shared(shape, buf.clone(), samples, meta).map_err(|e| e.to_string());
    }
    let (f32_bytes, _did_cast) = goofi_core::cast_to_f32(src, &buf[samples]).map_err(|e| e.to_string())?;
    Data::array_f32(shape, f32_bytes, meta).map_err(|e| e.to_string())
}

fn decode_table(buf: &Arc<Vec<u8>>, body: Range<usize>, meta: goofi_core::Meta, depth: usize) -> std::result::Result<Data, String> {
    let segments = [&buf[body.clone()]];
    let mut cur = Cursor::new(&segments);
    let n = cur.u32("table count")?;
    let mut map: indexmap::IndexMap<String, Data> = indexmap::IndexMap::new();
    for _ in 0..n {
        let klen = cur.u16("table key length")?;
        let key = std::str::from_utf8(cur.take(klen, "table key")?)
            .map_err(|e| e.to_string())?
            .to_string();
        let vlen = cur.u32("table value length")?;
        let start = body.start + cur.off;
        cur.take(vlen, "table value frame")?;
        let child = decode_at(buf, start..start + vlen, depth + 1)?;
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
            "channels" => meta.set_channels(parse_channels(&val)?),
            other => meta.set(other, mp_to_mv(&val)),
        }
    }
    Ok(meta)
}

fn parse_channels(v: &Mp) -> std::result::Result<goofi_core::Axes, String> {
    // Entries may arrive out of order, so `with_checked` pads up to the max labeled dim.
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
                axes = axes
                    .with_checked(dim, goofi_core::Axis::coords(coords))
                    .ok_or_else(|| format!("channels dim{dim} exceeds the array rank bound"))?;
            }
        }
    }
    Ok(axes)
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

/// The frontend's frame constants, generated from the tags above and checked into the tree.
pub fn typescript() -> String {
    let names = goofi_core::DTYPE_NAMES;
    let tags = names.iter().enumerate().map(|(i, n)| format!("\t{i}: '{n}'")).collect::<Vec<_>>().join(",\n");
    let union = names.iter().map(|n| format!("'{n}'")).collect::<Vec<_>>().join(" | ");
    format!(
        "// GENERATED from backend/goofi-codec/src/lib.rs — do not edit by hand. The GOOF header's\n\
         // version, size and tags are declared once, in the codec. Regenerate by running\n\
         // `cargo test -p goofi-tests contracts::`, which rewrites this file when it drifts.\n\
         export const VERSION = {VERSION};\n\
         export const HEADER_SIZE = {HEADER_SIZE};\n\
         /** The tag of a frame that carries a held frame's per-emit stamps and no body. */\n\
         export const STAMPS_TAG = {STAMPS_TAG};\n\
         export type DataType = {union};\n\
         export const DTYPE_TAG: Record<number, DataType> = {{\n{tags}\n}};\n"
    )
}
