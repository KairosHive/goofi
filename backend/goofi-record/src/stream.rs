//! One open stream file, and what a reader must be told about it.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

/// What a stream's file is: GOOF frames end to end, or a video the encoder owns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Frames,
    Video { size: (u32, u32), fps: f64 },
}

impl Kind {
    pub fn extension(&self) -> &'static str {
        match self {
            Kind::Frames => "goof",
            Kind::Video { .. } => "mkv",
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

pub struct Stream {
    pub file: String,
    pub kind: Kind,
    pub meta: StreamMeta,
    pub t0_patch: f64,
    pub t0_utc: SystemTime,
    pub frames: u64,
    pub dropped: u64,
    pub dropped_at: Option<f64>,
    pub fill: f32,
    writer: BufWriter<File>,
    synced: Instant,
}

impl Stream {
    pub fn create(
        folder: &Path,
        file: String,
        kind: Kind,
        meta: StreamMeta,
        t0_patch: f64,
        t0_utc: SystemTime,
    ) -> Result<Stream, String> {
        let made = File::create_new(folder.join(&file)).map_err(|e| e.to_string())?;
        let writer = BufWriter::new(made);
        Ok(Stream {
            file,
            kind,
            meta,
            t0_patch,
            t0_utc,
            frames: 0,
            dropped: 0,
            dropped_at: None,
            fill: 0.0,
            writer,
            synced: Instant::now(),
        })
    }

    /// Append one frame, and reach the disk on a one-second cadence so a crash costs a second.
    pub fn write(&mut self, frame: &[u8]) -> Result<(), String> {
        self.writer.write_all(frame).map_err(|e| e.to_string())?;
        self.frames += 1;
        if self.synced.elapsed() >= SYNC_EVERY {
            self.sync()?;
        }
        Ok(())
    }

    pub fn sync(&mut self) -> Result<(), String> {
        self.writer.flush().map_err(|e| e.to_string())?;
        self.writer.get_ref().sync_data().map_err(|e| e.to_string())?;
        self.synced = Instant::now();
        Ok(())
    }
}
