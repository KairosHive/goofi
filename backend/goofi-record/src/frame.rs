//! What one frame is, in the terms a FILE takes.
//!
//! This is the one place that decides which kind of file a frame belongs in, so the drain that
//! opens a stream and the recorder that writes it cannot disagree about what arrived.

use goofi_core::{Data, Value};

use crate::stream::{Kind, Written};

/// A frame read into the shape its file takes: which stream holds it, its payload, and the meta
/// its sidecar carries.
pub struct Incoming<'a> {
    pub kind: Kind,
    pub written: Written<'a>,
    /// The sidecar's half of the frame, and what the drain reads its instant and index from.
    pub meta: Option<goofi_core::Meta>,
}

/// Read a frame. `rate` is the audio engine's word that this stream is audio and how fast it
/// runs — a signal node's array is an array however many samples a second it carries.
pub fn read(frame: &Data, rate: Option<f64>) -> Result<Incoming<'_>, String> {
    let meta = Some(frame.meta().clone());
    match frame.value() {
        Value::Array(a) => match rate {
            // WAV is interleaved and the engine's blocks are planar, so audio is the one
            // stream whose samples are rearranged rather than appended.
            Some(rate) => {
                let channels = *a.shape().first().unwrap_or(&1);
                Ok(Incoming { kind: Kind::Audio { rate, channels }, written: Written::Blocks { planar: a.as_bytes(), channels }, meta })
            }
            None => Ok(Incoming { kind: Kind::Array, written: Written::Rows { samples: a.as_bytes(), shape: a.shape().to_vec() }, meta }),
        },
        Value::Str(text) => Ok(Incoming { kind: Kind::Text, written: Written::Text(text), meta }),
        Value::Table(map) => {
            let cells: Vec<(String, String)> = map.iter().map(|(name, child)| (name.clone(), cell(child))).collect();
            let columns = cells.iter().map(|(k, _)| k.clone()).collect();
            Ok(Incoming { kind: Kind::Table { columns }, written: Written::Cells(cells), meta })
        }
        Value::Texture(_) => Err("a texture submission is not recorded".into()),
        Value::Functional(_) => Err("a functional is not recorded".into()),
    }
}

/// How many bytes a frame's samples take: what a wav's room is measured against.
pub fn size(frame: &Data) -> usize {
    match frame.value() {
        Value::Array(a) => a.as_bytes().len(),
        _ => 0,
    }
}

/// A table's cell as text. A column holding one number is that number; anything wider is the
/// shape it has, because a CSV cell holds one value and a reader must not be told otherwise.
fn cell(d: &Data) -> String {
    match d.value() {
        Value::Str(s) => s.to_string(),
        Value::Table(_) => "<table>".into(),
        Value::Texture(_) => "<texture submission>".into(),
        Value::Functional(_) => "<functional>".into(),
        Value::Array(a) => match (a.shape().iter().product::<usize>(), a.values().next()) {
            (1, Some(v)) => v.to_string(),
            _ => format!("<{:?}>", a.shape()),
        },
    }
}
