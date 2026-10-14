//! A goofi recording read back: the frames of one stream file at the instants its `.jsonl` sidecar
//! gives, for a playback node to emit in order. The inverse of what `goofi-record` writes.

use crate::{Axes, Axis, Coord, Data, Meta, MetaValue, META_CHANNELS, META_SFREQ};
use indexmap::IndexMap;
use serde_json::Value as Json;
use std::path::Path;

/// Meta keys the engine stamps on every frame it emits: a replay never carries them over.
const STAMPS: [&str; 6] = ["time", "index", "emit", "source", "ufreq", "drift"];

/// Which playback node a file belongs to, by the extension the recorder gave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Array,
    Text,
    Table,
}

impl Kind {
    fn of(path: &Path) -> Result<Kind, String> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("npy") => Ok(Kind::Array),
            Some("txt") => Ok(Kind::Text),
            Some("csv") => Ok(Kind::Table),
            _ => Err(format!("{} is not a recorded .npy, .txt or .csv file", path.display())),
        }
    }
}

/// What stands in for a missing sidecar: the frame shape when the manifest has none, and the rate
/// that spaces the frames when nothing else does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fallback {
    pub channels: usize,
    pub sfreq: f64,
}

/// Every frame of one stream file, at seconds from its first.
pub struct Recording {
    pub kind: Kind,
    pub frames: Vec<(f64, Data)>,
}

/// A whole `<f4` NPY file back: its shape and its samples, refused when it is anything else.
pub fn read_npy(file: &[u8]) -> Result<(Vec<usize>, Vec<u8>), String> {
    let short = || "not an NPY file".to_string();
    if file.len() < 10 || &file[..8] != b"\x93NUMPY\x01\x00" {
        return Err(short());
    }
    let head = 10 + u16::from_le_bytes([file[8], file[9]]) as usize;
    let dict = std::str::from_utf8(file.get(10..head).ok_or_else(short)?).map_err(|_| short())?;
    if !dict.contains("'<f4'") || dict.contains("True") {
        return Err("only a C-ordered `<f4` array is a variable".into());
    }
    let (_, tail) = dict.split_once("'shape': (").ok_or_else(short)?;
    let (dims, _) = tail.split_once(')').ok_or_else(short)?;
    let shape: Vec<usize> = dims.split(',').map(str::trim).filter(|d| !d.is_empty()).map(|d| d.parse().map_err(|_| short())).collect::<Result<_, _>>()?;
    let samples = file[head..].to_vec();
    if shape.iter().product::<usize>() * 4 != samples.len() {
        return Err("the file's size does not match its shape".into());
    }
    Ok((shape, samples))
}

/// Read the stream at `path`, with its sidecar when there is one, else its folder's manifest and
/// `fallback`.
pub fn open(path: &Path, fallback: Fallback) -> Result<Recording, String> {
    let kind = Kind::of(path)?;
    let file = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let lines = sidecar(path)?;
    let entry = manifest_entry(path);
    let at = |k: usize, rate: f64| k as f64 / rate.max(f64::MIN_POSITIVE);
    let mut frames = match kind {
        Kind::Array => {
            let (_, samples) = read_npy(&file).map_err(|e| format!("{}: {e}", path.display()))?;
            match lines {
                Some(lines) => arrays(&samples, &lines)?,
                None => {
                    let frame = entry.as_ref().and_then(|e| e.get("frame")).and_then(shape_of).unwrap_or_else(|| vec![fallback.channels.max(1)]);
                    let sfreq = entry.as_ref().and_then(|e| e.get("sfreq")).and_then(Json::as_f64).unwrap_or(fallback.sfreq);
                    let per = if frame.len() >= 2 { *frame.last().unwrap() as f64 / sfreq } else { 1.0 / sfreq };
                    let meta = Meta::new().with_sfreq(entry.as_ref().and_then(|e| e.get("sfreq")).and_then(Json::as_f64));
                    let size = frame.iter().product::<usize>() * 4;
                    if size == 0 {
                        return Err("a frame shape with no values".into());
                    }
                    samples
                        .chunks_exact(size)
                        .enumerate()
                        .map(|(k, chunk)| Ok((k as f64 * per, array(frame.clone(), chunk, meta.clone())?)))
                        .collect::<Result<_, String>>()?
                }
            }
        }
        Kind::Text => {
            let text = String::from_utf8(file).map_err(|e| format!("{}: {e}", path.display()))?;
            let texts = text.lines().map(|l| l.replace("\\n", "\n"));
            match lines {
                Some(lines) => texts.zip(&lines).map(|(t, l)| (l.t, Data::string(t, l.meta.clone()))).collect(),
                None => texts.enumerate().map(|(k, t)| (at(k, fallback.sfreq), Data::string(t, Meta::new()))).collect(),
            }
        }
        Kind::Table => {
            let text = String::from_utf8(file).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut rows = csv_rows(&text).into_iter();
            let head = rows.next().ok_or("a CSV without a header")?;
            if head.first().map(String::as_str) != Some("t") {
                return Err("a recorded CSV starts with the `t` column".into());
            }
            let mut out = Vec::new();
            for (k, row) in rows.enumerate() {
                let t = row.first().and_then(|t| t.parse::<f64>().ok()).ok_or_else(|| format!("row {} has no time", k + 1))?;
                let meta = lines.as_ref().and_then(|l| l.get(k)).map_or_else(Meta::new, |l| l.meta.clone());
                let cells: IndexMap<String, Data> =
                    head.iter().skip(1).zip(row.iter().skip(1).chain(std::iter::repeat(&String::new()))).map(|(c, v)| (c.clone(), cell(v))).collect();
                out.push((t, Data::table(cells, meta)));
            }
            out
        }
    };
    if let Some(&(first, _)) = frames.first() {
        frames.iter_mut().for_each(|(t, _)| *t -= first);
    }
    Ok(Recording { kind, frames })
}

/// One sidecar line, its meta carried forward from the lines before it.
struct Line {
    t: f64,
    shape: Option<Vec<usize>>,
    meta: Meta,
}

/// The sidecar beside `path`, or `None` when it is absent.
fn sidecar(path: &Path) -> Result<Option<Vec<Line>>, String> {
    let beside = path.with_extension("jsonl");
    let Ok(text) = std::fs::read_to_string(&beside) else { return Ok(None) };
    let mut said: IndexMap<String, Json> = IndexMap::new();
    let mut out = Vec::new();
    for (k, raw) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let line: Json = serde_json::from_str(raw).map_err(|e| format!("{} line {}: {e}", beside.display(), k + 1))?;
        let t = line.get("t").and_then(Json::as_f64).ok_or_else(|| format!("{} line {}: no `t`", beside.display(), k + 1))?;
        if let Some(Json::Object(delta)) = line.get("meta") {
            for (key, v) in delta {
                match v {
                    Json::Null => said.shift_remove(key),
                    v => said.insert(key.clone(), v.clone()),
                };
            }
        }
        out.push(Line { t, shape: line.get("shape").and_then(shape_of), meta: meta_of(&said) });
    }
    Ok(Some(out))
}

/// This file's entry in its folder's `manifest.json`, when both are there.
fn manifest_entry(path: &Path) -> Option<Json> {
    let manifest: Json = serde_json::from_slice(&std::fs::read(path.parent()?.join("manifest.json")).ok()?).ok()?;
    let name = path.file_name()?.to_str()?;
    manifest.get("streams")?.as_array()?.iter().find(|s| s.get("file").and_then(Json::as_str) == Some(name)).cloned()
}

/// The flat samples cut into one frame per line, each line's shape held until the next names one.
fn arrays(samples: &[u8], lines: &[Line]) -> Result<Vec<(f64, Data)>, String> {
    let (mut shape, mut at, mut out) = (None::<Vec<usize>>, 0, Vec::with_capacity(lines.len()));
    for line in lines {
        if line.shape.is_some() {
            shape.clone_from(&line.shape);
        }
        let shape = shape.clone().ok_or("the sidecar's first line names no shape")?;
        let size = shape.iter().product::<usize>() * 4;
        // A recording cut short holds fewer samples than its sidecar names: it ends there.
        let Some(chunk) = samples.get(at..at + size) else { break };
        out.push((line.t, array(shape, chunk, line.meta.clone())?));
        at += size;
    }
    Ok(out)
}

fn array(shape: Vec<usize>, chunk: &[u8], meta: Meta) -> Result<Data, String> {
    Data::array_f32(shape, chunk.to_vec(), meta).map_err(|e| e.to_string())
}

fn shape_of(v: &Json) -> Option<Vec<usize>> {
    v.as_array()?.iter().map(|d| d.as_u64().map(|d| d as usize)).collect()
}

/// The meta a frame carried, without the stamps the engine sets afresh.
fn meta_of(said: &IndexMap<String, Json>) -> Meta {
    let mut meta = Meta::new();
    for (key, v) in said.iter().filter(|(k, _)| !STAMPS.contains(&k.as_str())) {
        match key.as_str() {
            META_SFREQ => meta.set_sfreq(v.as_f64()),
            META_CHANNELS => meta.set_channels(axes_of(v)),
            _ => {
                if let Ok(v) = serde_json::from_value::<MetaValue>(v.clone()) {
                    meta.set(key.clone(), v);
                }
            }
        }
    }
    meta
}

/// `{"dimN": [coords]}` back into positional axes.
fn axes_of(v: &Json) -> Axes {
    let mut axes = Axes::new();
    for (dim, coords) in v.as_object().into_iter().flatten() {
        let (Some(i), Some(coords)) = (dim.strip_prefix("dim").and_then(|i| i.parse::<usize>().ok()), coords.as_array()) else { continue };
        let coords: Vec<Coord> = coords
            .iter()
            .filter_map(|c| match c {
                Json::Number(n) => n.as_f64().map(Coord::Num),
                Json::String(s) => Some(Coord::Str(s.as_str().into())),
                _ => None,
            })
            .collect();
        if let Some(with) = axes.clone().with_checked(i, Axis::coords(coords)) {
            axes = with;
        }
    }
    axes
}

/// A CSV cell as the table column it was: a number when it reads as one, else its text.
fn cell(v: &str) -> Data {
    match v.parse::<f32>() {
        Ok(n) if n.to_string() == v => Data::array_f32(vec![1], n.to_le_bytes().to_vec(), Meta::new()).unwrap_or_else(|_| Data::string(v, Meta::new())),
        _ => Data::string(v, Meta::new()),
    }
}

/// RFC 4180 rows: a quoted field may hold commas, doubled quotes and newlines.
fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut field, mut quoted) = (Vec::new(), Vec::new(), String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// A recording played against a clock: each run hands out the next frame once its instant has
/// come, one frame a run, so every frame leaves whole and in order however late the runs are.
pub struct Player {
    recording: Recording,
    next: usize,
    start: Option<f64>,
}

impl Player {
    pub fn new(recording: Recording) -> Player {
        Player { recording, next: 0, start: None }
    }

    pub fn kind(&self) -> Kind {
        self.recording.kind
    }

    /// The frame due at `now`, if any; at the end, `looped` starts over at once.
    pub fn due(&mut self, now: f64, looped: bool) -> Option<Data> {
        if self.next == self.recording.frames.len() {
            if !looped || self.recording.frames.is_empty() {
                return None;
            }
            (self.next, self.start) = (0, None);
        }
        let start = *self.start.get_or_insert(now);
        let (t, data) = &self.recording.frames[self.next];
        (*t <= now - start).then(|| {
            self.next += 1;
            data.clone()
        })
    }
}
