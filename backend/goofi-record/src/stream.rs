//! One open stream file, and what a reader must be told about it.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use crate::video::{Encoders, Video};

/// What a stream's file is: GOOF frames end to end, or a video the encoder owns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Frames,
    Video { size: (u32, u32), fps: f64 },
}

impl Kind {
    /// What names the file. A video's container is the encoder backend's to name, so only the
    /// frame stream answers here.
    pub fn extension(&self, encoders: &dyn Encoders) -> &'static str {
        match self {
            Kind::Frames => "goof",
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
    frames: u64,
    sink: Sink,
    synced: Instant,
}

/// Where a stream's frames land: the file itself, or the encoder that owns it.
enum Sink {
    Frames(BufWriter<File>),
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
        let sink = match kind {
            Kind::Frames => {
                Sink::Frames(BufWriter::new(File::create_new(folder.join(&file)).map_err(|e| e.to_string())?))
            }
            Kind::Video { size, fps } => Sink::Video(Video::spawn(encoders, folder, &file, size, fps)?),
        };
        Ok(Stream {
            file,
            kind,
            meta,
            t0_patch,
            t0_utc,
            frames: 0,
            dropped: 0,
            dropped_at: None,
            drift: None,
            fill: 0.0,
            sink,
            synced: Instant::now(),
        })
    }

    /// How many frames this stream holds. A video's own encoder is the one that knows, because a
    /// frame handed to it is not yet a frame in the file.
    pub fn frames(&self) -> u64 {
        match &self.sink {
            Sink::Frames(_) => self.frames,
            Sink::Video(v) => v.encoded(),
        }
    }

    /// How full this stream's feeding buffer is. A video's queue into the encoder knows its own.
    pub fn fill(&self) -> f32 {
        match &self.sink {
            Sink::Frames(_) => self.fill,
            Sink::Video(v) => v.fill(),
        }
    }

    /// What a drain last saw of the buffer feeding it. A video answers from its own queue.
    pub fn set_fill(&mut self, fill: f32) {
        if let Sink::Frames(_) = self.sink {
            self.fill = fill;
        }
    }

    /// Frames this stream lost. A video adds the one an encoder that died mid-write took with it,
    /// which no caller can see from the outside.
    pub fn lost(&self) -> u64 {
        match &self.sink {
            Sink::Frames(_) => self.dropped,
            Sink::Video(v) => self.dropped + v.lost(),
        }
    }

    /// Hand one readback to the encoder, with the instant it was rendered at. `false` is a drop
    /// the caller counts; a video stream never stalls the engine that feeds it.
    pub fn write_video(&mut self, texels: &[u8], at: f64) -> bool {
        match &self.sink {
            Sink::Video(v) => v.push(texels, at),
            Sink::Frames(_) => false,
        }
    }

    /// Append one frame, and reach the disk on a one-second cadence so a crash costs a second.
    pub fn write(&mut self, frame: &[u8]) -> Result<(), String> {
        let Sink::Frames(writer) = &mut self.sink else {
            return Err("a video stream takes texels, never encoded frames".into());
        };
        writer.write_all(frame).map_err(|e| e.to_string())?;
        self.frames += 1;
        if self.synced.elapsed() >= SYNC_EVERY {
            self.sync()?;
        }
        Ok(())
    }

    /// Reach the disk. For a video that is the encoder finishing, which is what closes the
    /// container — so nothing after it may write.
    pub fn sync(&mut self) -> Result<(), String> {
        match &mut self.sink {
            Sink::Frames(writer) => {
                writer.flush().map_err(|e| e.to_string())?;
                writer.get_ref().sync_data().map_err(|e| e.to_string())?;
            }
            Sink::Video(v) => v.finish()?,
        }
        self.synced = Instant::now();
        Ok(())
    }
}
