//! The pixels inside a drawing widget's value. A pad's value is a `data:image/png;base64,…` URL —
//! a STRING, so it crosses the wire and saves into the patch like every other global — and this is
//! the one door back to its texels.
//!
//! It reads what a browser canvas writes and says no to the rest: 8 bits a sample, no interlacing.
//! A decoder that covered the whole format would be a library, and nothing here produces one.

use std::io::Read;

/// A decoded picture, always four channels, row 0 the TOP — the row order every frame in goofi has.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The texels behind a `data:image/png;base64,…` URL, or bare base64.
pub fn decode(source: &str) -> Result<Image, String> {
    let body = source.rsplit_once("base64,").map_or(source, |(_, tail)| tail);
    let bytes = base64_decode(body.trim())?;
    chunks(&bytes)
}

fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|e| format!("the drawing is not base64: {e}"))
}

/// The PNG's own header, its pixel data inflated, and the two turned into RGBA.
fn chunks(bytes: &[u8]) -> Result<Image, String> {
    if bytes.first_chunk::<8>() != Some(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Err("the drawing is not a PNG".into());
    }
    let mut at = 8;
    let mut head: Option<(u32, u32, u8)> = None;
    let mut deflated: Vec<u8> = Vec::new();
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes")) as usize;
        let kind = &bytes[at + 4..at + 8];
        let from = at + 8;
        let to = from.checked_add(len).filter(|to| *to + 4 <= bytes.len()).ok_or("the drawing is truncated")?;
        match kind {
            b"IHDR" => {
                if len < 13 {
                    return Err("the drawing's header is short".into());
                }
                let w = u32::from_be_bytes(bytes[from..from + 4].try_into().expect("four bytes"));
                let h = u32::from_be_bytes(bytes[from + 4..from + 8].try_into().expect("four bytes"));
                let (depth, colour, interlace) = (bytes[from + 8], bytes[from + 9], bytes[from + 12]);
                if depth != 8 {
                    return Err(format!("the drawing is {depth} bits a sample; this reads 8"));
                }
                if interlace != 0 {
                    return Err("the drawing is interlaced; this reads the plain form".into());
                }
                if !matches!(colour, 0 | 2 | 4 | 6) {
                    return Err(format!("the drawing's colour type {colour} carries a palette; this reads the direct forms"));
                }
                head = Some((w, h, colour));
            }
            b"IDAT" => deflated.extend_from_slice(&bytes[from..to]),
            b"IEND" => break,
            _ => {}
        }
        at = to + 4;
    }
    let (w, h, colour) = head.ok_or("the drawing has no header")?;
    if w == 0 || h == 0 {
        return Err("the drawing has no pixels".into());
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&deflated[..])
        .read_to_end(&mut raw)
        .map_err(|e| format!("the drawing's pixels do not unpack: {e}"))?;
    unfilter(&raw, w, h, channels(colour)).map(|rgba| Image { width: w, height: h, rgba })
}

fn channels(colour: u8) -> usize {
    match colour {
        0 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    }
}

/// PNG stores each row as a filter byte and a difference from its neighbours; this walks that back
/// into samples and widens them to RGBA.
fn unfilter(raw: &[u8], w: u32, h: u32, bpp: usize) -> Result<Vec<u8>, String> {
    let stride = w as usize * bpp;
    if raw.len() < (stride + 1) * h as usize {
        return Err("the drawing holds fewer rows than it declares".into());
    }
    let mut out = vec![0u8; w as usize * h as usize * 4];
    let mut prev = vec![0u8; stride];
    let mut line = vec![0u8; stride];
    for y in 0..h as usize {
        let head = y * (stride + 1);
        let filter = raw[head];
        line.copy_from_slice(&raw[head + 1..head + 1 + stride]);
        for i in 0..stride {
            let a = if i >= bpp { line[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            line[i] = line[i].wrapping_add(match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                4 => paeth(a, b, c),
                other => return Err(format!("the drawing uses filter {other}, which is not one of the five")),
            });
        }
        for x in 0..w as usize {
            let s = &line[x * bpp..x * bpp + bpp];
            let rgba = match bpp {
                1 => [s[0], s[0], s[0], 255],
                2 => [s[0], s[0], s[0], s[1]],
                3 => [s[0], s[1], s[2], 255],
                _ => [s[0], s[1], s[2], s[3]],
            };
            out[(y * w as usize + x) * 4..][..4].copy_from_slice(&rgba);
        }
        prev.copy_from_slice(&line);
    }
    Ok(out)
}

/// The neighbour a Paeth-filtered byte was written against.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}
