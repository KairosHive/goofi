//! One open stream file, and what a reader must be told about it.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use crate::beside::{Beside, Extent};
use crate::csv::Csv;
use crate::npy::Npy;
use crate::video::{Encoders, Video};
use crate::wav::Wav;

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
    Video { size: (u32, u32), fps: f64 },
}

impl Kind {
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
    /// One array frame, appended as it stands, and the shape it came in — which the flat file
    /// cannot hold, so the sidecar carries it.
    Rows { samples: &'a [u8], shape: Vec<usize> },
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
    /// The last frame number this file holds; the gap to the next one is what went missing.
    numbered: Option<u64>,
    /// What shapes this file has held. The sidecar's line is written off it, and the manifest
    /// projects it: one shape while every frame agreed, none once one did not.
    shapes: Shapes,
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
            Kind::Array => Sink::Array(Npy::create(&path)?),
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
            numbered: None,
            shapes: Shapes::None,
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

    /// The one shape every frame of this file had, which is what lets a reader fold the flat
    /// values back in one line. Absent where a shape moved: the sidecar is the index then.
    pub fn frame(&self) -> Option<Vec<usize>> {
        match &self.shapes {
            Shapes::One(shape) => Some(shape.clone()),
            _ => None,
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
    /// refused frame: a table with other columns, a full wav. An array always takes one — its
    /// file is flat, so a reshape is not a reason to open another.
    pub fn takes(&self, kind: &Kind, bytes: usize) -> bool {
        match (&self.sink, kind) {
            (Sink::Audio(w), Kind::Audio { .. }) => &self.kind == kind && w.room_for(bytes),
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

    /// Append one frame, and reach the disk on a one-second cadence so a crash costs a second. The
    /// frame's own NUMBER is what says whether any went missing before it, so `dropped` is the gap
    /// in this file's numbering and can never be a loss the file itself cannot show.
    pub fn write(
        &mut self,
        at: f64,
        written: Written<'_>,
        meta: Option<&goofi_core::Meta>,
    ) -> Result<(), String> {
        let extent = match (&mut self.sink, written) {
            (Sink::Array(n), Written::Rows { samples, shape }) => {
                n.write(samples)?;
                Extent::Shape(self.shapes.took(shape))
            }
            (Sink::Audio(w), Written::Blocks { planar, channels }) => {
                let was = w.frames();
                w.write_planar(planar, channels)?;
                Extent::Rows(w.frames() - was)
            }
            (Sink::Table(c), Written::Cells(cells)) => {
                c.write(at, &cells)?;
                Extent::Rows(1)
            }
            (Sink::Text(f), Written::Text(t)) => {
                use std::io::Write;
                writeln!(f, "{}", t.replace('\n', "\\n")).map_err(|e| e.to_string())?;
                self.lines += 1;
                Extent::Rows(1)
            }
            _ => return Err("the frame is not the shape this stream holds".into()),
        };
        if let Some(n) = meta.and_then(|m| m.index()) {
            let missed = self.numbered.replace(n).map_or(0, |l| n.saturating_sub(l).saturating_sub(1));
            if missed > 0 {
                self.dropped += missed;
                self.dropped_at = Some(at);
            }
        }
        if let Some(beside) = &mut self.beside {
            beside.line(at, extent, meta)?;
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
