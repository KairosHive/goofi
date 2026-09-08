//! What one wire frame is, in the terms a FILE takes.
//!
//! This is the one place that decides which kind of file a frame belongs in, so the drain that
//! opens a stream and the recorder that writes it cannot disagree about what arrived.

use crate::stream::{Kind, Written};

/// A frame read into the shape its file takes: which stream holds it, its payload, and the meta
/// its sidecar carries.
pub struct Incoming<'a> {
    pub kind: Kind,
    pub written: Written<'a>,
    /// The sidecar's half of the frame, and what the drain reads its instant and index from —
    /// parsed ONCE here rather than again by every caller that wants one field of it.
    pub meta: Option<goofi_core::Meta>,
}

/// Read a frame. `rate` is the audio engine's word that this stream is audio and how fast it
/// runs — a signal node's array is an array however many samples a second it carries.
pub fn read<'a>(frame: &'a [u8], rate: Option<f64>) -> Result<Incoming<'a>, String> {
    let (tag, _, body) = goofi_codec::split_frame(frame)?;
    let meta = goofi_codec::frame_meta(frame).ok();
    match tag {
        0 => {
            let (shape, samples) =
                goofi_codec::array_view(body).ok_or("an array the recorder cannot borrow")?;
            match rate {
                // WAV is interleaved and the engine's blocks are planar, so audio is the one
                // stream whose samples are rearranged rather than appended.
                Some(rate) => {
                    let channels = *shape.first().unwrap_or(&1);
                    Ok(Incoming {
                        kind: Kind::Audio { rate, channels },
                        written: Written::Blocks { planar: samples, channels },
                        meta,
                    })
                }
                None => Ok(Incoming {
                    kind: Kind::Array,
                    written: Written::Rows { samples, shape },
                    meta,
                }),
            }
        }
        1 => {
            let text = std::str::from_utf8(body).map_err(|e| e.to_string())?;
            Ok(Incoming { kind: Kind::Text, written: Written::Text(text), meta })
        }
        2 => {
            let cells = cells(frame)?;
            let columns = cells.iter().map(|(k, _)| k.clone()).collect();
            Ok(Incoming { kind: Kind::Table { columns }, written: Written::Cells(cells), meta })
        }
        other => Err(format!("unknown dtype tag {other}")),
    }
}


/// A table's columns as text. A column holding one number is that number; anything wider is the
/// shape it has, because a CSV cell holds one value and a reader must not be told otherwise.
fn cells(frame: &[u8]) -> Result<Vec<(String, String)>, String> {
    let data = goofi_codec::decode(frame)?;
    let goofi_core::Value::Table(map) = data.value() else {
        return Err("a table frame that is not a table".into());
    };
    Ok(map
        .iter()
        .map(|(name, child)| (name.clone(), cell(child)))
        .collect())
}

fn cell(d: &goofi_core::Data) -> String {
    match d.value() {
        goofi_core::Value::Str(s) => s.to_string(),
        goofi_core::Value::Table(_) => "<table>".into(),
        goofi_core::Value::Array(a) => {
            let mut it = a.as_bytes().chunks_exact(4);
            match (a.shape().iter().product::<usize>(), it.next()) {
                (1, Some(b)) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]).to_string(),
                _ => format!("<{:?}>", a.shape()),
            }
        }
    }
}
