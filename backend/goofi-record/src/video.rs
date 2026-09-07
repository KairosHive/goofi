//! A video stream: the frames one armed graphics slot renders, encoded, and the sidecar of
//! instants that makes the video alignable frame by frame with every other stream.
//!
//! [`Video`] is the stream and knows no codec: a queue the engine hands frames to, the `.times`
//! file, and the counters. Everything a codec needs is behind [`Encoder`], and `Ffmpeg` is the
//! one implementation of it.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One video file being written. A frame is `width * height` tight-packed texels of four 16-bit
/// UNSIGNED channels, little-endian, row 0 the top — the graphics engine's readback exactly as
/// the device wrote it. What a codec wants instead is that codec's own business.
pub trait Encoder: Send {
    fn write(&mut self, rows: &[u8]) -> Result<(), String>;
    /// Close the file. Called once, and what makes the container complete.
    fn finish(&mut self) -> Result<(), String>;
}

/// The encoder a recording uses.
pub fn open(file: &Path, size: (u32, u32), fps: f64) -> Result<Box<dyn Encoder>, String> {
    Ffmpeg::spawn(file, size, fps).map(|e| Box::new(e) as Box<dyn Encoder>)
}

/// Whether this machine can encode at all — the ffmpeg backend's own precondition, which
/// `record start` asks before it opens a folder.
pub fn probe() -> Result<(), String> {
    Command::new("ffmpeg")
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| MISSING.to_string())
        .and_then(|s| if s.success() { Ok(()) } else { Err(MISSING.to_string()) })
}

/// What `record start` says when a graphics slot is armed and this machine has no encoder.
pub const MISSING: &str = "`ffmpeg` is not on PATH, and a graphics slot is armed. \
                           Install the `ffmpeg` package, or disarm the graphics slot.";

/// What the manifest states about a video stream, in words. Every texture in the graphics engine
/// is 16-bit FLOAT and no video codec takes float, so the recorded window is the one a
/// 16-bit UNSIGNED texel holds.
pub const CLIP: &str = "FFV1 in Matroska at rgba64le: lossless within [0,1], and a value outside \
                        that range is clipped to it.";

/// FFV1 in Matroska, through an `ffmpeg` child on stdin. FFV1 is lossless and Matroska is the
/// container that carries it; MP4 cannot.
struct Ffmpeg {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    file: PathBuf,
}

impl Ffmpeg {
    fn spawn(file: &Path, (w, h): (u32, u32), fps: f64) -> Result<Ffmpeg, String> {
        let mut child = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba64le"])
            .args(["-s", &format!("{w}x{h}"), "-r", &format!("{fps}")])
            .args(["-i", "-", "-an", "-c:v", "ffv1"])
            .arg(file)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|_| MISSING.to_string())?;
        let stdin = child.stdin.take().ok_or("the encoder took no stdin")?;
        Ok(Ffmpeg { child: Some(child), stdin: Some(stdin), file: file.to_path_buf() })
    }
}

impl Encoder for Ffmpeg {
    fn write(&mut self, rows: &[u8]) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("the encoder is closed")?;
        stdin.write_all(rows).map_err(|e| e.to_string())
    }

    /// The pipe is closed FIRST: ffmpeg finalizes the container on end of input, and a child
    /// waited for with its stdin still open never reaches that.
    fn finish(&mut self) -> Result<(), String> {
        drop(self.stdin.take());
        let Some(mut child) = self.child.take() else { return Ok(()) };
        match child.wait() {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("ffmpeg left {} unfinished: {status}", self.file.display())),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// How many frames may wait for the encoder. The engine renders in real time and an HD frame is
/// 16 MB, so a deeper queue only postpones the same loss and hides it behind memory.
const QUEUE: usize = 2;

const FLUSH_EVERY: Duration = Duration::from_secs(1);

type Job = (Vec<u8>, f64);

/// Frames the encoder has finished with, kept so a 16 MB readback is copied and never allocated.
type Free = Arc<Mutex<Vec<Vec<u8>>>>;

pub struct Video {
    frames: Option<SyncSender<Job>>,
    writer: Option<std::thread::JoinHandle<()>>,
    counts: Counts,
    free: Free,
}

impl Video {
    /// Open the encoder onto `file`, and the `.times` sidecar beside it.
    pub fn spawn(folder: &Path, file: &str, size: (u32, u32), fps: f64) -> Result<Video, String> {
        let out = folder.join(file);
        let times = File::create(out.with_extension("times")).map_err(|e| e.to_string())?;
        let encoder = open(&out, size, fps)?;
        let (tx, rx) = sync_channel(QUEUE);
        let counts = Counts::default();
        let free = Free::default();
        let writer = {
            let (counts, free) = (counts.clone(), free.clone());
            std::thread::Builder::new()
                .name("goofi-record-video".into())
                .spawn(move || encode(rx, encoder, BufWriter::new(times), &counts, &free))
                .map_err(|e| e.to_string())?
        };
        Ok(Video { frames: Some(tx), writer: Some(writer), counts, free })
    }

    /// Hand one readback to the encoder, with the patch instant it was rendered at. `false` is a
    /// frame the encoder could not keep up with, or a dead encoder — a drop, never a stall.
    pub fn push(&self, texels: &[u8], at: f64) -> bool {
        let Some(tx) = &self.frames else { return false };
        if self.counts.dead.load(Ordering::Relaxed) {
            return false;
        }
        let mut buffer = self.free.lock().expect("the free frames").pop().unwrap_or_default();
        buffer.clear();
        buffer.extend_from_slice(texels);
        match tx.try_send((buffer, at)) {
            Ok(()) => true,
            Err(TrySendError::Full((buffer, _))) => {
                give_back(&self.free, buffer);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    /// Frames the encoder took, which is what the manifest counts — never what was handed over.
    pub fn encoded(&self) -> u64 {
        self.counts.encoded.load(Ordering::Relaxed)
    }

    /// Frames accepted and then lost, which only an encoder that died mid-write can cost.
    pub fn lost(&self) -> u64 {
        self.counts.lost.load(Ordering::Relaxed)
    }

    /// Close the queue and wait for the writer, which is what finishes the file. Idempotent.
    pub fn finish(&mut self) -> Result<(), String> {
        drop(self.frames.take());
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        match self.counts.error.lock().expect("the encoder's error").take() {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[derive(Clone, Default)]
struct Counts {
    encoded: Arc<AtomicU64>,
    lost: Arc<AtomicU64>,
    dead: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
}

fn give_back(free: &Free, buffer: Vec<u8>) {
    let mut free = free.lock().expect("the free frames");
    if free.len() < QUEUE {
        free.push(buffer);
    }
}

/// The one thread that touches the encoder: the render thread hands frames over and never waits
/// on a disk. Its buffers go back to `free` as the queue drains.
fn encode(
    rx: Receiver<Job>,
    mut encoder: Box<dyn Encoder>,
    mut times: BufWriter<File>,
    counts: &Counts,
    free: &Free,
) {
    let mut flushed = Instant::now();
    let died = |counts: &Counts, why: String| {
        counts.dead.store(true, Ordering::Relaxed);
        *counts.error.lock().expect("the encoder's error") = Some(why);
    };
    for (buffer, at) in rx {
        if let Err(why) = encoder.write(&buffer) {
            counts.lost.fetch_add(1, Ordering::Relaxed);
            died(counts, why);
            break;
        }
        if let Err(e) = times.write_all(&at.to_le_bytes()) {
            died(counts, e.to_string());
            break;
        }
        counts.encoded.fetch_add(1, Ordering::Relaxed);
        give_back(free, buffer);
        if flushed.elapsed() >= FLUSH_EVERY {
            let _ = times.flush();
            flushed = Instant::now();
        }
    }
    let _ = times.flush();
    let _ = times.get_ref().sync_data();
    if let Err(why) = encoder.finish() {
        died(counts, why);
    }
}
