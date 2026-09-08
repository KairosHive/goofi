//! One open stream file, and what a reader must be told about it.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use crate::beside::Beside;
use crate::csv::Csv;
use crate::npy::Npy;
use crate::video::{Encoders, Video};
use crate::wav::Wav;

/// What a stream's file IS, which is the shape of the thing being recorded rather than a choice:
/// an array is a `.npy`, a table is a `.csv`, text is its own lines, audio is a `.wav`, a texture
/// is a video. Nothing writes the wire format to disk.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// One frame's shape. The file holds a stack of them, so a frame of another shape is another
    /// file — the rule a resized texture already follows.
    Array { frame: Vec<usize> },
    Table { columns: Vec<String> },
    Text,
    Audio { rate: f64, channels: usize },
    Video { size: (u32, u32), fps: f64 },
}

impl Kind {
    pub fn extension(&self, encoders: &dyn Encoders) -> &'static str {
        match self {
            Kind::Array { .. } => "npy",
            Kind::Table { .. } => "csv",
            Kind::Text => "txt",
            Kind::Audio { .. } => "wav",
            Kind::Video { .. } => encoders.extension(),
        }
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

#[derive(Clone, Debug)]
pub struct StreamMeta {
    pub sfreq: Option<f64>,
    pub timeline: Timeline,
    pub channels: Option<usize>,
}

impl StreamMeta {
    pub fn measured(sfreq: Option<f64>) -> StreamMeta {
        StreamMeta { sfreq, timeline: Timeline::Measured, channels: None }
    }
    pub fn derived(sfreq: Option<f64>) -> StreamMeta {
        StreamMeta { sfreq, timeline: Timeline::Derived, channels: None }
    }
    pub fn with_channels(mut self, channels: usize) -> StreamMeta {
        self.channels = Some(channels);
        self
    }
}

const SYNC_EVERY: Duration = Duration::from_secs(1);

/// What one frame carries into a stream: the instant it was made, its `Meta` for the sidecar, and
/// the payload in the shape its own kind takes.
pub enum Written<'a> {
    /// One array frame, appended as it stands.
    Rows(&'a [u8]),
    /// One audio block, PLANAR as the engine renders it — the wav interleaves it on the way in,
    /// into a buffer of its own, so a block costs no allocation.
    Blocks { planar: &'a [u8], channels: usize },
    Cells(Vec<(String, String)>),
    Text(&'a str),
}

pub struct Stream {
    pub file: String,
    pub kind: Kind,
    pub meta: StreamMeta,
    pub t0_patch: f64,
    pub t0_utc: SystemTime,
    pub dropped: u64,
    pub dropped_at: Option<f64>,
    /// What a derived timeline last measured itself against patch time.
    pub drift: Option<f64>,
    fill: f32,
    lines: u64,
    sink: Sink,
    /// Absent for a video, whose encoder thread owns its own — a frame the encoder dropped must
    /// not get a line.
    beside: Option<Beside>,
    synced: Instant,
}

/// Where a stream's frames land.
enum Sink {
    Array(Npy),
    Table(Csv),
    Text(std::io::BufWriter<std::fs::File>),
    Audio(Wav),
    Video(Video),
}

impl Stream {
    pub fn create(
        encoders: &dyn Encoders,
        folder: &Path,
        file: String,
        kind: Kind,
        meta: StreamMeta,
        t0_patch: f64,
        t0_utc: SystemTime,
    ) -> Result<Stream, String> {
        let path = folder.join(&file);
        let sink = match &kind {
            Kind::Array { frame } => Sink::Array(Npy::create(&path, frame)?),
            Kind::Table { columns } => Sink::Table(Csv::create(&path, columns)?),
            Kind::Text => Sink::Text(std::io::BufWriter::new(
                std::fs::File::create_new(&path).map_err(|e| e.to_string())?,
            )),
            Kind::Audio { rate, channels } => Sink::Audio(Wav::create(&path, *rate, *channels)?),
            Kind::Video { size, fps } => {
                Sink::Video(Video::spawn(encoders, folder, &file, *size, *fps)?)
            }
        };
        let beside = match kind {
            Kind::Video { .. } => None,
            _ => Some(Beside::create(&path)?),
        };
        Ok(Stream {
            file,
            kind,
            meta,
            t0_patch,
            t0_utc,
            dropped: 0,
            dropped_at: None,
            drift: None,
            fill: 0.0,
            lines: 0,
            sink,
            beside,
            synced: Instant::now(),
        })
    }

    /// How many frames this stream holds. A video's own encoder is the one that knows, because a
    /// frame handed to it is not yet a frame in the file.
    pub fn frames(&self) -> u64 {
        match &self.sink {
            Sink::Array(n) => n.frames(),
            Sink::Table(c) => c.rows(),
            Sink::Text(_) => self.lines,
            Sink::Audio(w) => w.frames(),
            Sink::Video(v) => v.encoded(),
        }
    }

    /// How full this stream's feeding buffer is. A video's queue into the encoder knows its own.
    pub fn fill(&self) -> f32 {
        match &self.sink {
            Sink::Video(v) => v.fill(),
            _ => self.fill,
        }
    }

    /// What a drain last saw of the buffer feeding it. A video answers from its own queue.
    pub fn set_fill(&mut self, fill: f32) {
        if !matches!(self.sink, Sink::Video(_)) {
            self.fill = fill;
        }
    }

    /// Frames this stream lost. A video adds the one an encoder that died mid-write took with it,
    /// which no caller can see from the outside.
    pub fn lost(&self) -> u64 {
        match &self.sink {
            Sink::Video(v) => self.dropped + v.lost(),
            _ => self.dropped,
        }
    }

    /// Whether this stream can still take what the frame brings. A `false` is a NEW file, never a
    /// refused frame: an array that reshaped, a table with other columns, a full wav.
    pub fn takes(&self, kind: &Kind, bytes: usize) -> bool {
        match (&self.sink, kind) {
            (Sink::Audio(w), Kind::Audio { .. }) => w.room_for(bytes),
            _ => &self.kind == kind,
        }
    }

    /// Hand one readback to the encoder, with the instant it was rendered at. `false` is a drop
    /// the caller counts; a video stream never stalls the engine that feeds it.
    pub fn write_video(&mut self, texels: &[u8], at: f64) -> bool {
        match &self.sink {
            Sink::Video(v) => v.push(texels, at),
            _ => false,
        }
    }

    /// Append one frame, and reach the disk on a one-second cadence so a crash costs a second.
    pub fn write(
        &mut self,
        at: f64,
        written: Written<'_>,
        meta: Option<&goofi_core::Meta>,
    ) -> Result<(), String> {
        let rows = match (&mut self.sink, written) {
            (Sink::Array(n), Written::Rows(s)) => {
                n.write(s)?;
                1
            }
            (Sink::Audio(w), Written::Blocks { planar, channels }) => {
                let was = w.frames();
                w.write_planar(planar, channels)?;
                w.frames() - was
            }
            (Sink::Table(c), Written::Cells(cells)) => {
                c.write(at, &cells)?;
                1
            }
            (Sink::Text(f), Written::Text(t)) => {
                use std::io::Write;
                writeln!(f, "{}", t.replace('\n', "\\n")).map_err(|e| e.to_string())?;
                self.lines += 1;
                1
            }
            _ => return Err("the frame is not the shape this stream holds".into()),
        };
        if let Some(beside) = &mut self.beside {
            beside.line(at, rows, meta)?;
        }
        if self.synced.elapsed() >= SYNC_EVERY {
            self.sync()?;
        }
        Ok(())
    }

    /// Reach the disk. For a video that is the encoder finishing, which is what closes the
    /// container — so nothing after it may write.
    pub fn sync(&mut self) -> Result<(), String> {
        match &mut self.sink {
            Sink::Array(n) => n.sync()?,
            Sink::Table(c) => c.sync()?,
            Sink::Audio(w) => w.sync()?,
            Sink::Text(f) => {
                use std::io::Write;
                f.flush().map_err(|e| e.to_string())?;
                f.get_ref().sync_data().map_err(|e| e.to_string())?;
            }
            Sink::Video(v) => v.finish()?,
        }
        if let Some(beside) = &mut self.beside {
            beside.sync()?;
        }
        self.synced = Instant::now();
        Ok(())
    }
}
