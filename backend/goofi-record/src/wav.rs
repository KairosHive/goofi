//! A `.wav` an audio stream is appended to. IEEE float32, which is what a goofi sample already
//! is, so the samples are written and never converted.
//!
//! RIFF counts its bytes in 32 bits, so a take has a CEILING rather than a length: at the limit
//! the stream opens a new file, exactly as a shape change or a resize does. RF64 lifts the ceiling
//! and is read by far less than plain WAV, which is the whole reason a recording is a WAV at all.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

/// The most sample bytes one file takes, leaving the headers inside the 32-bit count.
pub const CEILING: u64 = u32::MAX as u64 - 1024;

const HEADER: u64 = 44;

pub struct Wav {
    file: BufWriter<File>,
    channels: u16,
    bytes: u64,
    /// Reused across blocks: the interleave is per sample and a fresh buffer per block put the
    /// allocator on the path of a real-time drain.
    scratch: Vec<u8>,
}

impl Wav {
    pub fn create(path: &Path, rate: f64, channels: usize) -> Result<Wav, String> {
        let channels = u16::try_from(channels).map_err(|_| "too many channels for a wav")?;
        if channels == 0 {
            return Err("a wav holds at least one channel".into());
        }
        let file = File::create_new(path).map_err(|e| e.to_string())?;
        let mut wav = Wav { file: BufWriter::new(file), channels, bytes: 0, scratch: Vec::new() };
        wav.head(rate)?;
        wav.sizes()?;
        Ok(wav)
    }

    /// Whether this file can still take a block of `bytes`. A full file is a new file, never a
    /// truncated count.
    pub fn room_for(&self, bytes: usize) -> bool {
        self.bytes + bytes as u64 <= CEILING
    }

    /// One PLANAR block, interleaved into the only order a wav has.
    pub fn write_planar(&mut self, planar: &[u8], channels: usize) -> Result<(), String> {
        let samples = planar.len() / (4 * channels.max(1));
        self.scratch.clear();
        self.scratch.reserve(planar.len());
        for i in 0..samples {
            for c in 0..channels {
                let at = (c * samples + i) * 4;
                self.scratch.extend_from_slice(&planar[at..at + 4]);
            }
        }
        self.file.write_all(&self.scratch).map_err(|e| e.to_string())?;
        self.bytes += self.scratch.len() as u64;
        Ok(())
    }

    /// Frames, in WAV's sense: one sample per channel.
    pub fn frames(&self) -> u64 {
        self.bytes / (4 * self.channels as u64)
    }

    pub fn sync(&mut self) -> Result<(), String> {
        let end = self.file.stream_position().map_err(|e| e.to_string())?;
        self.sizes()?;
        self.file.seek(SeekFrom::Start(end)).map_err(|e| e.to_string())?;
        self.file.flush().map_err(|e| e.to_string())?;
        self.file.get_ref().sync_data().map_err(|e| e.to_string())
    }

    fn head(&mut self, rate: f64) -> Result<(), String> {
        let block = 4 * self.channels as u32;
        let mut h = Vec::with_capacity(HEADER as usize);
        h.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        h.extend_from_slice(&self.channels.to_le_bytes());
        h.extend_from_slice(&(rate.round() as u32).to_le_bytes());
        h.extend_from_slice(&(rate.round() as u32 * block).to_le_bytes());
        h.extend_from_slice(&(block as u16).to_le_bytes());
        h.extend_from_slice(&32u16.to_le_bytes());
        h.extend_from_slice(b"data\0\0\0\0");
        self.file.write_all(&h).map_err(|e| e.to_string())
    }

    fn sizes(&mut self) -> Result<(), String> {
        let data = self.bytes as u32;
        self.file.seek(SeekFrom::Start(4)).map_err(|e| e.to_string())?;
        self.file.write_all(&(HEADER as u32 - 8 + data).to_le_bytes()).map_err(|e| e.to_string())?;
        self.file.seek(SeekFrom::Start(HEADER - 4)).map_err(|e| e.to_string())?;
        self.file.write_all(&data.to_le_bytes()).map_err(|e| e.to_string())
    }
}
