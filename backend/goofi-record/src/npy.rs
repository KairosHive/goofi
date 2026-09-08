//! NPY: the one place goofi spells that format, for a whole array in memory and for a file a
//! stream is appended to alike.
//!
//! The header is padded ASCII and the samples follow it, so a frame is `write_all` of the bytes
//! the wire already holds and the count is patched IN PLACE on the sync cadence. What that buys is
//! the property the recorder is built on: a writer that is killed leaves every whole frame
//! readable, because the frames are contiguous and the count follows from the file's size.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

/// The widest a frame count can ever be written, so an appended file's header is sized once and
/// the count can never outgrow the room reserved for it.
const COUNT: usize = 20;

/// A whole array as an NPY file.
pub fn bytes(shape: &[usize], samples: &[u8]) -> Vec<u8> {
    let mut out = header(shape, head_len(shape, 0));
    out.extend_from_slice(samples);
    out
}

/// How long the header for `shape` is, with `reserve` bytes left for its first dimension to grow.
fn head_len(shape: &[usize], reserve: usize) -> usize {
    (10 + dict(shape).len() + reserve + 1).div_ceil(64) * 64
}

/// The NPY v1.0 header for `shape`, occupying exactly `total` bytes.
fn header(shape: &[usize], total: usize) -> Vec<u8> {
    let dict = dict(shape);
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"\x93NUMPY\x01\x00");
    out.extend_from_slice(&((total - 10) as u16).to_le_bytes());
    out.extend_from_slice(dict.as_bytes());
    out.resize(total - 1, b' ');
    out.push(b'\n');
    out
}

fn dict(shape: &[usize]) -> String {
    let dims: String = shape.iter().map(|d| format!("{d},")).collect();
    format!("{{'descr': '<f4', 'fortran_order': False, 'shape': ({dims}), }}")
}

/// One `.npy` a stream is appended to: the shape of ONE frame, and the count of them the header
/// carries.
pub struct Npy {
    file: BufWriter<File>,
    frame: Vec<usize>,
    frames: u64,
    head: usize,
}

impl Npy {
    pub fn create(path: &Path, frame: &[usize]) -> Result<Npy, String> {
        let file = File::create_new(path).map_err(|e| e.to_string())?;
        let head = head_len(&stacked(u64::MAX, frame), COUNT);
        let mut npy = Npy { file: BufWriter::new(file), frame: frame.to_vec(), frames: 0, head };
        npy.head()?;
        Ok(npy)
    }

    /// One frame's samples, which are `<f4` already and are appended untouched.
    pub fn write(&mut self, samples: &[u8]) -> Result<(), String> {
        if samples.len() != self.frame.iter().product::<usize>() * 4 {
            return Err("the frame is not the shape this file holds".into());
        }
        self.file.write_all(samples).map_err(|e| e.to_string())?;
        self.frames += 1;
        Ok(())
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Reach the disk, and leave the header saying what the file holds.
    pub fn sync(&mut self) -> Result<(), String> {
        let end = self.file.stream_position().map_err(|e| e.to_string())?;
        self.head()?;
        self.file.seek(SeekFrom::Start(end)).map_err(|e| e.to_string())?;
        self.file.flush().map_err(|e| e.to_string())?;
        self.file.get_ref().sync_data().map_err(|e| e.to_string())
    }

    fn head(&mut self) -> Result<(), String> {
        let head = header(&stacked(self.frames, &self.frame), self.head);
        self.file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        self.file.write_all(&head).map_err(|e| e.to_string())
    }
}

/// The file's own shape: how many frames, then one frame's.
fn stacked(frames: u64, frame: &[usize]) -> Vec<usize> {
    std::iter::once(frames as usize).chain(frame.iter().copied()).collect()
}
