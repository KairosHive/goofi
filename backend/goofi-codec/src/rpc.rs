//! The call a node answers across a process boundary: `[entry][now]` then a codec request, and
//! the response. Hosted Rust and subprocess Python children speak exactly this.

use std::borrow::Cow;
use std::sync::Arc;

use goofi_core::Data;

use crate::{decode_at, decode_owned, encode_into, encoded_len, packed, u16_of, u32_of, Cursor, EncodeError, Out, Segments};

/// Which entry of a node a call is for: the first byte of every call frame.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Setup = 0,
    Process = 1,
    ParamChanged = 2,
    Refresh = 3,
    Pulse = 4,
    /// The node goes: the child releases what it holds, answers, and exits.
    Stop = 5,
}

impl Entry {
    pub fn from_u8(byte: u8) -> Option<Entry> {
        [Entry::Setup, Entry::Process, Entry::ParamChanged, Entry::Refresh, Entry::Pulse, Entry::Stop]
            .into_iter()
            .find(|e| *e as u8 == byte)
    }
}

/// A call frame's head, `[u8 entry][f64 now LE]`; the request's runs follow it.
pub fn call_head(entry: Entry, now: f64) -> [u8; 9] {
    let mut head = [0u8; 9];
    head[0] = entry as u8;
    head[1..].copy_from_slice(&now.to_le_bytes());
    head
}

/// A call frame as the child reads it: the entry, its `now`, and the request after them.
pub fn split_call(frame: &[u8]) -> Result<(Entry, f64, &[u8]), String> {
    let (&byte, rest) = frame.split_first().ok_or("an empty call")?;
    let entry = Entry::from_u8(byte).ok_or_else(|| format!("unknown entry {byte}"))?;
    if rest.len() < 8 {
        return Err("a call with no clock".into());
    }
    Ok((entry, f64::from_le_bytes(rest[..8].try_into().unwrap()), &rest[8..]))
}

/// The reply to an entry that answers nothing.
pub fn done() -> Vec<u8> {
    let mut out = Vec::new();
    encode_response(&[], &[], &mut out).expect("an empty response encodes");
    out
}

/// `group -> name -> Param`, spelled out because the codec has no goofi-node dep.
pub type ParamMap = indexmap::IndexMap<String, indexmap::IndexMap<String, goofi_core::Param>>;

/// The slot entries of a run request: `(slot, source, frame)`, the source empty on a single slot.
pub type SourcedSlots = Vec<(String, String, Data)>;

/// The bytes a request is: runs of copied bytes around samples handed on by reference.
pub type Runs<'a> = Vec<Cow<'a, [u8]>>;

/// One named slot's head: `[u16 name_len][name][u16 src_len][src][u32 frame_len]`.
fn slot_head(name: &str, source: &str, frame_len: usize) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::with_capacity(8 + name.len() + source.len());
    for (what, text) in [("slot name", name), ("slot source", source)] {
        out.extend_from_slice(&u16_of(what, text.len())?);
        out.extend_from_slice(text.as_bytes());
    }
    out.extend_from_slice(&u32_of("slot frame", frame_len)?);
    Ok(out)
}

/// Append a named-slot list: `[u16 n]` then n × `[u16 name_len][name][u16 src_len][src]
/// [u32 frame_len][GOOF frame]`; `src` is the `node.slot` a multi-slot frame came from, else empty.
pub fn encode_slots<'a>(slots: &[(&str, &str, &'a Data)], out: &mut impl Out<'a>) -> Result<(), EncodeError> {
    let count = u16_of("slot count", slots.len())?;
    let mut heads = Vec::with_capacity(slots.len());
    for (name, source, d) in slots {
        heads.push(slot_head(name, source, encoded_len(d)?)?);
    }
    out.put(&count);
    for (head, (_, _, d)) in heads.iter().zip(slots) {
        out.put(head);
        encode_into(d, out)?;
    }
    Ok(())
}

fn read_slot_head<'a>(cur: &mut Cursor<'_, 'a>) -> std::result::Result<(String, String, usize), String> {
    let nlen = cur.u16("slot name length")?;
    let name = std::str::from_utf8(cur.take(nlen, "slot name")?).map_err(|e| e.to_string())?.to_string();
    let slen = cur.u16("slot source length")?;
    let source = std::str::from_utf8(cur.take(slen, "slot source")?).map_err(|e| e.to_string())?.to_string();
    let flen = cur.u32("slot frame length")?;
    Ok((name, source, flen))
}

/// Decode the named-slot list written by [`encode_slots`], each frame copied out of the request.
fn decode_slots(cur: &mut Cursor<'_, '_>) -> std::result::Result<SourcedSlots, String> {
    let n = cur.u16("slot count")?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (name, source, flen) = read_slot_head(cur)?;
        let data = decode_owned(cur.gather(flen, "slot frame")?)?;
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

fn encode_params(params: &ParamMap, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    let pbytes = packed("params", params)?;
    out.extend_from_slice(&u32_of("params", pbytes.len())?);
    out.extend_from_slice(&pbytes);
    Ok(())
}

/// Encode a tick request: `[0][u32 params_len][params msgpack][slots]`, each slot with its source.
pub fn encode_request<'a>(params: &ParamMap, slots: &[(&str, &str, &'a Data)]) -> Result<Runs<'a>, EncodeError> {
    let mut out = Segments::default();
    let mut head = vec![0u8];
    encode_params(params, &mut head)?;
    out.put(&head);
    encode_slots(slots, &mut out)?;
    Ok(out.finish())
}

/// Encode a refresh request: `[1][u32 params_len][params msgpack][(group, name) msgpack]`.
pub fn encode_refresh_request(params: &ParamMap, group: &str, name: &str) -> Result<Vec<u8>, EncodeError> {
    encode_keyed_request(1, params, group, name)
}

/// Encode a pulse request: `[2][u32 params_len][params msgpack][(group, name) msgpack]`.
pub fn encode_pulse_request(params: &ParamMap, group: &str, name: &str) -> Result<Vec<u8>, EncodeError> {
    encode_keyed_request(2, params, group, name)
}

fn encode_keyed_request(tag: u8, params: &ParamMap, group: &str, name: &str) -> Result<Vec<u8>, EncodeError> {
    let mut out = vec![tag];
    encode_params(params, &mut out)?;
    out.extend_from_slice(&packed("param key", &(group, name))?);
    Ok(out)
}

/// Decode a request written by [`encode_request`], [`encode_refresh_request`] or
/// [`encode_pulse_request`], from the segments it crossed as.
pub fn decode_request(segments: &[&[u8]]) -> std::result::Result<Request, String> {
    let mut cur = Cursor::new(segments);
    let tag = cur.u8("request tag")? as u8;
    let plen = cur.u32("params length")?;
    let pbytes = cur.take(plen, "params blob")?;
    let params: ParamMap = rmp_serde::from_slice(pbytes).map_err(|e| e.to_string())?;
    match tag {
        0 => {
            let slots = decode_slots(&mut cur)?;
            match cur.done() {
                true => Ok(Request::Process { params, slots }),
                false => Err("bytes after the last slot".into()),
            }
        }
        1 | 2 => {
            let (group, name): (String, String) =
                rmp_serde::from_slice(cur.rest()?).map_err(|e| e.to_string())?;
            Ok(if tag == 1 { Request::Refresh { params, group, name } } else { Request::Pulse { params, group, name } })
        }
        other => Err(format!("unknown request tag {other}")),
    }
}

/// What a node emits on one output: a frame of its own, or one of its inputs unchanged, which
/// crosses back as that input's NAME since the host still holds the frame it sent.
pub enum Emitted<'a> {
    Frame(&'a Data),
    Input { slot: &'a str, index: usize },
}

/// [`Emitted`] as the host reads it back.
pub enum Output {
    Frame(Data),
    Input { slot: String, index: usize },
}

/// Outputs and input clears from a successful process call.
pub struct ProcessOutput {
    pub outputs: Vec<(String, Output)>,
    pub clear_inputs: Vec<String>,
}

pub enum Response {
    Process(ProcessOutput),
    NodeError(String),
    Options(Option<Vec<String>>),
}

/// `[0][u32 clears_len][clears msgpack]`, then the slot list, where an input handed back unchanged
/// is a slot whose `src` names it (`slot`, or `slot#index` on a multi slot) over an empty frame.
pub fn encode_response<'a>(outputs: &[(&str, Emitted<'a>)], clear_inputs: &[String], out: &mut impl Out<'a>) -> Result<(), EncodeError> {
    let clears = packed("input clears", clear_inputs)?;
    let clears_len = u32_of("input clears", clears.len())?;
    let count = u16_of("output count", outputs.len())?;
    let mut heads = Vec::with_capacity(outputs.len());
    for (name, emitted) in outputs {
        heads.push(match emitted {
            Emitted::Frame(d) => slot_head(name, "", encoded_len(d)?)?,
            Emitted::Input { slot, index: 0 } => slot_head(name, slot, 0)?,
            Emitted::Input { slot, index } => slot_head(name, &format!("{slot}#{index}"), 0)?,
        });
    }
    out.put(&[0u8]);
    out.put(&clears_len);
    out.put(&clears);
    out.put(&count);
    for (head, (_, emitted)) in heads.iter().zip(outputs) {
        out.put(head);
        if let Emitted::Frame(d) = emitted {
            encode_into(d, out)?;
        }
    }
    Ok(())
}

/// Encode a node-error response: `[1][utf8 message]`.
pub fn encode_error_response(msg: &str) -> Vec<u8> {
    let mut out = vec![1u8];
    out.extend_from_slice(msg.as_bytes());
    out
}

/// Encode a refresh response: `[2][Option<Vec<String>> msgpack]`.
pub fn encode_options_response(options: &Option<Vec<String>>) -> Result<Vec<u8>, EncodeError> {
    let mut out = vec![2u8];
    out.extend_from_slice(&packed("options", options)?);
    Ok(out)
}

/// Decode a response, which the outputs then keep: each frame is a view of the reply it came in.
/// The outer `Err` is a malformed reply, never a node-reported one.
pub fn decode_response(reply: Vec<u8>) -> std::result::Result<Response, String> {
    let (&tag, rest) = reply.split_first().ok_or("empty response frame")?;
    match tag {
        0 => {
            let buf = Arc::new(reply);
            let outputs = {
                let segments = [&buf[1..]];
                let mut cur = Cursor::new(&segments);
                let len = cur.u32("input clears length")?;
                let clear_inputs = rmp_serde::from_slice(cur.take(len, "input clears")?).map_err(|e| e.to_string())?;
                let n = cur.u16("slot count")?;
                let mut outputs = Vec::with_capacity(n);
                for _ in 0..n {
                    let (name, source, flen) = read_slot_head(&mut cur)?;
                    let start = 1 + cur.off;
                    cur.take(flen, "slot frame")?;
                    let output = match (flen, source.split_once('#')) {
                        (0, Some((slot, index))) => {
                            Output::Input { slot: slot.to_string(), index: index.parse().map_err(|_| "a bad input index")? }
                        }
                        (0, None) if !source.is_empty() => Output::Input { slot: source, index: 0 },
                        _ => Output::Frame(decode_at(&buf, start..start + flen, 0)?),
                    };
                    outputs.push((name, output));
                }
                ProcessOutput { outputs, clear_inputs }
            };
            Ok(Response::Process(outputs))
        },
        1 => Ok(Response::NodeError(String::from_utf8_lossy(rest).into_owned())),
        2 => Ok(Response::Options(rmp_serde::from_slice(rest).map_err(|e| e.to_string())?)),
        other => Err(format!("unknown response tag {other}")),
    }
}
