//! One open stream file, the sidecar beside it, and what a reader must be told about it. Every
//! kind of file is behind one [`Format`]: a frame goes in, the file says what it added, and the
//! sidecar and the counts are kept here for all of them alike.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use goofi_core::record::{Frame, VideoQuality};
use goofi_core::Value;

use crate::beside::{Beside, Extent};
use crate::csv::Csv;
use crate::manifest::Entry;
use crate::npy::Npy;
use crate::video::{Encoders, Video};
use crate::wav::Wav;
use crate::StreamId;

/// What a stream's file IS, which is the shape of the thing being recorded rather than a choice:
/// an array is a `.npy`, a table is a `.csv`, text is its own lines, audio is a `.wav`, a texture
/// is a video. Nothing writes the wire format to disk.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Every array, whatever its shape: the file is FLAT and the sidecar's shape line splits it,
    /// so a node whose shape moves — a filling `Buffer`, a peak count — stays in one file.
    Array,
    Table { columns: Vec<String> },
    Text,
    Audio { rate: f64, channels: usize },
    Video { size: (u32, u32), fps: f64, quality: VideoQuality },
}

impl Kind {
    /// The kind a frame belongs in. `rate` is the audio engine's word that this stream is audio
    /// and how fast it runs — a signal node's array is an array however many samples a second it
    /// carries. `quality` is what a video of it is encoded at.
    pub fn of(frame: &Frame, rate: Option<f64>, quality: VideoQuality) -> Result<Kind, String> {
        match frame {
            Frame::Texels(t) => Ok(Kind::Video { size: (t.width, t.height), fps: t.meta.sfreq().unwrap_or_default(), quality }),
            Frame::Data(d) => match (d.value(), rate) {
                (Value::Array(a), Some(rate)) => Ok(Kind::Audio { rate, channels: *a.shape().first().unwrap_or(&1) }),
                (Value::Array(_), None) => Ok(Kind::Array),
                (Value::Str(_), _) => Ok(Kind::Text),
                (Value::Table(map), _) => Ok(Kind::Table { columns: map.keys().cloned().collect() }),
                (Value::Texture(_), _) => Err("a texture submission is not recorded".into()),
                (Value::Functional(_), _) => Err("a functional is not recorded".into()),
            },
        }
    }

    pub fn extension(&self, encoders: &dyn Encoders) -> &'static str {
        match self {
            Kind::Array => "npy",
            Kind::Table { .. } => "csv",
            Kind::Text => "txt",
            Kind::Audio { .. } => "wav",
            Kind::Video { .. } => encoders.extension(),
        }
    }
}

/// How many bytes a frame's samples take: what a wav's room is measured against.
pub fn size(frame: &Frame) -> usize {
    match frame {
        Frame::Data(d) => match d.value() {
            Value::Array(a) => a.as_bytes().len(),
            _ => 0,
        },
        Frame::Texels(t) => t.rgba.len(),
    }
}

/// Whether the stream's own counter is the clock, or its tick reads are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Timeline {
    Measured,
    Derived,
}

impl Timeline {
    pub fn name(&self) -> &'static str {
        match self {
            Timeline::Measured => "measured",
            Timeline::Derived => "derived",
        }
    }
}

/// What a file says one frame added to it: the shape it came in, which the sidecar writes where
/// it MOVED, or how many of the file's own units it took, where no shape can say it.
pub enum Added {
    Shape(Vec<usize>),
    Rows(u64),
}

/// One kind of file: a frame goes in and the file says what it added. Opened for one kind, and
/// never asked to take another.
pub trait Format {
    fn write(&mut self, frame: &Frame, at: f64) -> Result<Added, String>;
    /// Frames the file holds, in the file's own units.
    fn frames(&self) -> u64;
    /// Whether a frame of `bytes` still fits: a file with a ceiling says no, and opens the next.
    fn room_for(&self, _bytes: usize) -> bool {
        true
    }
    /// Reach the disk, on the cadence.
    fn sync(&mut self) -> Result<(), String>;
    /// Close the file, which for a video is what completes the container.
    fn finish(&mut self) -> Result<(), String>;
}

const SYNC_EVERY: Duration = Duration::from_secs(1);

pub struct Stream {
    pub file: String,
    pub kind: Kind,
    sfreq: Option<f64>,
    timeline: Timeline,
    channels: Option<usize>,
    pub t0_patch: f64,
    pub t0_utc: SystemTime,
    pub dropped: u64,
    pub dropped_at: Option<f64>,
    /// The last frame number this file holds; the gap to the next one is what went missing.
    numbered: Option<u64>,
    /// What shapes this file has held. The sidecar's line is written off it, and the manifest
    /// projects it: one shape while every frame agreed, none once one did not.
    shapes: Shapes,
    /// What a derived timeline last measured itself against patch time.
    pub drift: Option<f64>,
    sink: Box<dyn Format + Send>,
    beside: Beside,
    synced: Instant,
}

/// What shapes one file has held: nothing yet, one that every frame agreed on, or several — in
/// which case the last is kept, because the next frame is compared against it.
enum Shapes {
    None,
    One(Vec<usize>),
    Many(Vec<usize>),
}

impl Shapes {
    /// Take `shape` in, and answer it back where it MOVED — the one place the sidecar writes a
    /// shape, since a reader carries the last one forward.
    fn took(&mut self, shape: Vec<usize>) -> Option<&[usize]> {
        if self.last() == Some(&shape[..]) {
            return None;
        }
        *self = if matches!(self, Shapes::None) { Shapes::One(shape) } else { Shapes::Many(shape) };
        self.last()
    }

    fn last(&self) -> Option<&[usize]> {
        match self {
            Shapes::None => None,
            Shapes::One(held) | Shapes::Many(held) => Some(held),
        }
    }
}

impl Stream {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        encoders: &dyn Encoders,
        folder: &Path,
        file: String,
        kind: Kind,
        sfreq: Option<f64>,
        timeline: Timeline,
        channels: Option<usize>,
        t0_patch: f64,
        t0_utc: SystemTime,
    ) -> Result<Stream, String> {
        let path = folder.join(&file);
        let sink: Box<dyn Format + Send> = match &kind {
            Kind::Array => Box::new(Npy::create(&path)?),
            Kind::Table { columns } => Box::new(Csv::create(&path, columns)?),
            Kind::Text => Box::new(Text(std::io::BufWriter::new(std::fs::File::create_new(&path).map_err(|e| e.to_string())?), 0)),
            Kind::Audio { rate, channels } => Box::new(Wav::create(&path, *rate, *channels)?),
            Kind::Video { size, fps, quality } => Box::new(Video::open(encoders, &path, *size, *fps, *quality)?),
        };
        let beside = Beside::create(&path)?;
        Ok(Stream {
            file,
            kind,
            sfreq,
            timeline,
            channels,
            t0_patch,
            t0_utc,
            dropped: 0,
            dropped_at: None,
            numbered: None,
            shapes: Shapes::None,
            drift: None,
            sink,
            beside,
            synced: Instant::now(),
        })
    }

    /// How many frames this stream holds, in its file's own units.
    pub fn frames(&self) -> u64 {
        self.sink.frames()
    }

    /// The one shape every frame of this file had, which is what lets a reader fold the flat
    /// values back in one line. Absent where a shape moved: the sidecar is the index then.
    pub fn frame(&self) -> Option<Vec<usize>> {
        match &self.shapes {
            Shapes::One(shape) => Some(shape.clone()),
            _ => None,
        }
    }

    /// Whether this stream can still take what the frame brings. A `false` is a NEW file, never a
    /// refused frame: a table with other columns, a full wav, a video of another size.
    pub fn takes(&self, kind: &Kind, bytes: usize) -> bool {
        self.kind == *kind && self.sink.room_for(bytes)
    }

    /// Why a frame of `kind` opens a new file after this one.
    pub fn reopened_because(&self, kind: &Kind) -> &'static str {
        match (&self.kind, kind) {
            (Kind::Video { size, .. }, Kind::Video { size: next, .. }) if size != next => "resized",
            (Kind::Video { quality, .. }, Kind::Video { quality: next, .. }) if quality != next => "quality changed",
            _ => "reopened",
        }
    }

    /// Append one frame, and reach the disk on a one-second cadence so a crash costs a second. The
    /// frame's own NUMBER is what says whether any went missing before it, so `dropped` is the gap
    /// in this file's numbering and can never be a loss the file itself cannot show.
    pub fn write(&mut self, at: f64, frame: &Frame) -> Result<(), String> {
        let meta = frame.meta();
        let extent = match self.sink.write(frame, at)? {
            Added::Shape(shape) => Extent::Shape(self.shapes.took(shape)),
            Added::Rows(n) => Extent::Rows(n),
        };
        if let Some(n) = meta.index() {
            let missed = self.numbered.replace(n).map_or(0, |l| n.saturating_sub(l).saturating_sub(1));
            if missed > 0 {
                self.dropped += missed;
                self.dropped_at = Some(at);
            }
        }
        if let Some(goofi_core::MetaValue::Float(d)) = meta.get(goofi_core::META_DRIFT) {
            self.drift = Some(*d);
        }
        self.beside.line(at, extent, Some(meta))?;
        if self.synced.elapsed() >= SYNC_EVERY {
            self.sink.sync()?;
            self.beside.sync()?;
            self.synced = Instant::now();
        }
        Ok(())
    }

    /// Close the file and the sidecar. For a video that is the encoder finishing, which is what
    /// closes the container — so nothing after it may write.
    pub fn finish(&mut self) -> Result<(), String> {
        let closed = self.sink.finish();
        self.beside.sync()?;
        closed
    }

    /// This stream's manifest entry, with its counts so far.
    pub fn entry(&self, id: &StreamId, because: Option<String>, error: Option<String>) -> Entry {
        Entry {
            drift: self.drift,
            frame: self.frame(),
            frames: self.frames(),
            dropped: self.dropped,
            dropped_at: self.dropped_at,
            closed_because: because,
            error,
            ..Entry::base(id, &self.file, &self.kind, self.sfreq, self.timeline, self.channels, self.t0_patch, self.t0_utc)
        }
    }

    /// Refresh the counts an open entry carries.
    pub fn count_into(&self, entry: &mut Entry) {
        entry.frames = self.frames();
        entry.dropped = self.dropped;
        entry.dropped_at = self.dropped_at;
        entry.frame = self.frame();
        entry.drift = self.drift;
    }
}

/// A text stream: one line per frame.
struct Text(std::io::BufWriter<std::fs::File>, u64);

impl Format for Text {
    fn write(&mut self, frame: &Frame, _at: f64) -> Result<Added, String> {
        use std::io::Write;
        let Frame::Data(d) = frame else { return Err("the frame is not the shape this stream holds".into()) };
        let Value::Str(text) = d.value() else { return Err("the frame is not the shape this stream holds".into()) };
        writeln!(self.0, "{}", text.replace('\n', "\\n")).map_err(|e| e.to_string())?;
        self.1 += 1;
        Ok(Added::Rows(1))
    }

    fn frames(&self) -> u64 {
        self.1
    }

    fn sync(&mut self) -> Result<(), String> {
        sync_file(&mut self.0)
    }

    fn finish(&mut self) -> Result<(), String> {
        self.sync()
    }
}

/// The frame as an array, or the error every format answers a frame of another shape with.
pub(crate) fn array(frame: &Frame) -> Result<&goofi_core::ArrayStore, String> {
    match frame {
        Frame::Data(d) => match d.value() {
            Value::Array(a) => Ok(a),
            _ => Err("the frame is not the shape this stream holds".into()),
        },
        Frame::Texels(_) => Err("the frame is not the shape this stream holds".into()),
    }
}

/// Flush `file` and reach the disk.
pub(crate) fn sync_file(file: &mut std::io::BufWriter<std::fs::File>) -> Result<(), String> {
    use std::io::Write;
    file.flush().map_err(|e| e.to_string())?;
    file.get_ref().sync_data().map_err(|e| e.to_string())
}
