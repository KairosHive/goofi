//! A video stream: the frames one armed graphics slot renders, encoded, and the sidecar of
//! instants that makes the video alignable frame by frame with every other stream.
//!
//! [`Video`] is the stream and knows no codec: a queue the engine hands frames to, the sidecar
//! every stream has, and the counters. Everything a codec needs is behind [`Encoder`], and `Ffmpeg` is the
//! one implementation of it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One video file being written. A frame is `width * height` tight-packed texels of four 8-bit
/// unsigned RGBA channels, row 0 the top — the graphics engine's readback exactly as
/// the device wrote it. What a codec wants instead is that codec's own business.
pub trait Encoder: Send {
    fn write(&mut self, rows: &[u8]) -> Result<(), String>;
    /// Close the file. Called once, and what makes the container complete.
    fn finish(&mut self) -> Result<(), String>;
}

/// Where a recording's video streams come from. One implementation ships; the recorder holds it
/// behind this so the codec is a swap and nothing above it changes.
pub trait Encoders: Send + Sync {
    /// Whether this machine can encode at all — the backend's own precondition, which
    /// `record start` asks before it refuses a recording of nothing but video.
    fn probe(&self) -> Result<(), String>;
    /// The container this backend writes, which is what names the file.
    fn extension(&self) -> &'static str;
    fn open(&self, file: &Path, size: (u32, u32), fps: f64) -> Result<Box<dyn Encoder>, String>;
}

/// Constant-quality H.264 in Matroska, through an `ffmpeg` child on stdin.
pub struct FfmpegEncoders;

impl Encoders for FfmpegEncoders {
    fn probe(&self) -> Result<(), String> {
        Command::new("ffmpeg")
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| MISSING.to_string())
            .and_then(|s| if s.success() { Ok(()) } else { Err(MISSING.to_string()) })
    }

    fn extension(&self) -> &'static str {
        "mkv"
    }

    fn open(&self, file: &Path, size: (u32, u32), fps: f64) -> Result<Box<dyn Encoder>, String> {
        if size.0 == 0 || size.1 == 0 || !fps.is_finite() || fps <= 0.0 {
            return Err("video needs a nonzero size and a positive finite frame rate".into());
        }
        Ok(Box::new(Ffmpeg {
            child: None, stdin: None, file: file.to_path_buf(), size, fps, closed: false, stderr: None,
        }))
    }
}

/// What `record start` says when a graphics slot is armed and this machine has no encoder.
pub const MISSING: &str = "`ffmpeg` is not on PATH, and a graphics slot is armed. \
                           Install the `ffmpeg` package, or disarm the graphics slot.";

/// A video is a viewable projection of the float texture, not measurement data.
pub const CLIP: &str = "H.264 in Matroska: constant quality, 8-bit YUV 4:2:0, values outside \
                        [0,1] clipped, alpha not kept. Odd dimensions are padded on the right \
                        or bottom to even dimensions.";

static CHILDREN: Mutex<Vec<Child>> = Mutex::new(Vec::new());
static STOPPING: AtomicBool = AtomicBool::new(false);

/// Stop encoder processes without waiting for queued frames.
pub fn kill_encoders() {
    STOPPING.store(true, Ordering::Relaxed);
    for child in CHILDREN.lock().unwrap_or_else(|e| e.into_inner()).iter_mut() {
        let _ = child.kill();
    }
}

fn track(mut child: Child) -> Result<u32, String> {
    let mut children = CHILDREN.lock().unwrap_or_else(|e| e.into_inner());
    if STOPPING.load(Ordering::Relaxed) {
        let _ = child.kill();
        let _ = child.wait();
        return Err("video encoders are stopping".into());
    }
    let id = child.id();
    children.push(child);
    Ok(id)
}

/// Reap the child. A trial has a deadline; a recording drains until it is complete or killed.
fn wait_encoder(id: u32, deadline: Option<Instant>) -> Result<Option<ExitStatus>, String> {
    loop {
        {
            let mut children = CHILDREN.lock().unwrap_or_else(|e| e.into_inner());
            let index = children.iter().position(|child| child.id() == id)
                .ok_or("the encoder is no longer running")?;
            match children[index].try_wait() {
                Ok(Some(status)) => {
                    let _ = children.swap_remove(index).wait();
                    return Ok(Some(status));
                }
                Ok(None) if !deadline.is_some_and(|end| Instant::now() >= end) => {}
                result => {
                    let mut child = children.swap_remove(index);
                    let _ = child.kill();
                    let _ = child.wait();
                    return result.map(|_| None).map_err(|e| e.to_string());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Ffmpeg {
    child: Option<u32>,
    stdin: Option<ChildStdin>,
    file: PathBuf,
    size: (u32, u32),
    fps: f64,
    closed: bool,
    stderr: Option<std::thread::JoinHandle<String>>,
}

/// Settings shared by the trial encode and the recording.
struct Preset {
    codec: &'static str,
    options: &'static [&'static str],
    device: Option<PathBuf>,
}

impl Preset {
    fn candidates() -> Vec<Self> {
        let mut presets = Vec::new();
        #[cfg(target_os = "macos")]
        presets.push(Self {
            codec: "h264_videotoolbox",
            options: &["-q:v", "65", "-allow_sw", "0"], device: None,
        });
        #[cfg(not(target_os = "macos"))]
        presets.push(Self {
            codec: "h264_nvenc",
            options: &["-preset", "p5", "-tune", "hq", "-rc", "constqp", "-qp", "23"],
            device: None,
        });
        #[cfg(target_os = "windows")]
        presets.push(Self {
            codec: "h264_amf",
            options: &["-quality", "quality", "-rc", "cqp", "-qp_i", "23", "-qp_p", "23"],
            device: None,
        });
        #[cfg(not(target_os = "macos"))]
        presets.push(Self {
            codec: "h264_qsv",
            options: &["-preset", "medium", "-global_quality", "23"], device: None,
        });
        #[cfg(target_os = "linux")]
        if let Ok(entries) = std::fs::read_dir("/dev/dri") {
            let mut devices: Vec<_> = entries.flatten().map(|entry| entry.path())
                .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("renderD")))
                .collect();
            devices.sort();
            presets.extend(devices.into_iter().map(|device| Self {
                codec: "h264_vaapi", options: &["-rc_mode", "CQP", "-qp", "23"], device: Some(device),
            }));
        }
        presets.push(Self {
            codec: "libx264", options: &["-preset", "veryfast", "-crf", "23"], device: None,
        });
        presets
    }

    fn input(&self, command: &mut Command) {
        if let Some(device) = &self.device {
            command.arg("-vaapi_device").arg(device);
        }
    }

    fn output(&self, command: &mut Command, fps: f64) {
        let filter = "pad=ceil(iw/2)*2:ceil(ih/2)*2,scale=out_color_matrix=bt709:out_range=tv";
        command.args(["-vf", &if self.device.is_some() {
            format!("{filter},format=nv12,hwupload")
        } else {
            format!("{filter},format=yuv420p")
        }]);
        command.args(["-an", "-c:v", self.codec]).args(self.options)
            .args(["-g", &format!("{:.0}", (fps * 2.0).max(1.0)), "-profile:v", "high"])
            .args(["-colorspace", "bt709", "-color_range", "tv"]);
    }

    /// Encode at the requested size. An encoder in FFmpeg's list can still lack a usable device.
    fn works(&self, size: (u32, u32), fps: f64) -> Result<bool, String> {
        let mut command = ffmpeg();
        self.input(&mut command);
        command.args(["-f", "lavfi", "-i", &format!(
            "nullsrc=size={}x{}:rate={fps},format=rgba", size.0, size.1,
        )]);
        self.output(&mut command, fps);
        let child = command.args(["-frames:v", "1", "-f", "null", "-"])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().map_err(|e| format!("{MISSING} ({e})"))?;
        let id = track(child)?;
        Ok(wait_encoder(id, Some(Instant::now() + Duration::from_secs(5)))?
            .is_some_and(|status| status.success()))
    }
}

fn ffmpeg() -> Command {
    let mut command = Command::new("ffmpeg");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.args(["-hide_banner", "-loglevel", "error", "-y"]);
    command
}

impl Ffmpeg {
    /// Select and start the encoder on the writer thread, without blocking the graphics clock.
    fn start(&mut self) -> Result<(), String> {
        let mut selected = None;
        for preset in Preset::candidates() {
            if STOPPING.load(Ordering::Relaxed) {
                return Err("video encoders are stopping".into());
            }
            if preset.works(self.size, self.fps)? {
                selected = Some(preset);
                break;
            }
        }
        let preset = selected.ok_or("FFmpeg has no working H.264 encoder for this frame size; install an FFmpeg build with libx264 or a supported hardware encoder")?;
        let mut command = ffmpeg();
        preset.input(&mut command);
        command.args(["-f", "rawvideo", "-pix_fmt", "rgba"])
            .args(["-s", &format!("{}x{}", self.size.0, self.size.1), "-r", &format!("{}", self.fps)])
            .args(["-i", "-"]);
        preset.output(&mut command, self.fps);
        let mut child = command.arg(&self.file)
            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped())
            .spawn().map_err(|e| format!("could not start FFmpeg: {e}"))?;
        let mut stderr = child.stderr.take().ok_or("FFmpeg has no error pipe")?;
        let errors = std::thread::Builder::new().name("goofi-record-errors".into()).spawn(move || {
            let mut message = Vec::new();
            let mut buffer = [0u8; 1024];
            while let Ok(n) = stderr.read(&mut buffer) {
                if n == 0 { break; }
                let keep = n.min(8192 - message.len());
                message.extend_from_slice(&buffer[..keep]);
            }
            String::from_utf8_lossy(&message).trim().to_string()
        });
        match errors {
            Ok(errors) => self.stderr = Some(errors),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("could not read FFmpeg errors: {error}"));
            }
        }
        self.stdin = child.stdin.take();
        self.child = Some(track(child)?);
        Ok(())
    }
}

impl Encoder for Ffmpeg {
    fn write(&mut self, rows: &[u8]) -> Result<(), String> {
        if self.closed {
            return Err("the encoder is closed".into());
        }
        let expected = u64::from(self.size.0).checked_mul(u64::from(self.size.1))
            .and_then(|pixels| pixels.checked_mul(4)).ok_or("video frame size exceeds the byte limit")?;
        if rows.len() as u64 != expected {
            return Err(format!("video frame has {} bytes; expected {expected}", rows.len()));
        }
        if self.child.is_none() {
            self.start()?;
        }
        let stdin = self.stdin.as_mut().ok_or("the encoder is closed")?;
        if let Err(error) = stdin.write_all(rows) {
            return Err(self.finish().err().unwrap_or_else(|| error.to_string()));
        }
        Ok(())
    }

    /// The pipe is closed FIRST: ffmpeg finalizes the container on end of input, and a child
    /// waited for with its stdin still open never reaches that.
    fn finish(&mut self) -> Result<(), String> {
        self.closed = true;
        drop(self.stdin.take());
        let status = self.child.take().map(|id| wait_encoder(id, None)).transpose();
        let error = self.stderr.take().and_then(|reader| reader.join().ok()).unwrap_or_default();
        match status? {
            None => Ok(()),
            Some(Some(status)) if status.success() => Ok(()),
            Some(Some(status)) => Err(format!("ffmpeg left {} unfinished: {status}: {error}", self.file.display())),
            Some(None) => Err("the encoder did not finish".into()),
        }
    }
}

impl Drop for Ffmpeg {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// How many frames may wait for the encoder. The engine renders in real time and an HD frame is
/// 8 MB, so a deeper queue only postpones the same loss and hides it behind memory.
const QUEUE: usize = 2;

const FLUSH_EVERY: Duration = Duration::from_secs(1);

type Job = (Vec<u8>, f64);

/// Reuse completed frame buffers to limit allocations.
type Free = Arc<Mutex<Vec<Vec<u8>>>>;

pub struct Video {
    frames: Option<SyncSender<Job>>,
    writer: Option<std::thread::JoinHandle<()>>,
    counts: Counts,
    free: Free,
}

impl Video {
    /// Open the encoder handle and sidecar. Encoding starts on the writer thread.
    pub fn spawn(
        encoders: &dyn Encoders,
        folder: &Path,
        file: &str,
        size: (u32, u32),
        fps: f64,
    ) -> Result<Video, String> {
        let out = folder.join(file);
        let encoder = encoders.open(&out, size, fps)?;
        let beside = crate::beside::Beside::create(&out)?;
        let (tx, rx) = sync_channel(QUEUE);
        let counts = Counts::default();
        let free = Free::default();
        let writer = {
            let (counts, free) = (counts.clone(), free.clone());
            std::thread::Builder::new()
                .name("goofi-record-video".into())
                .spawn(move || encode(rx, encoder, beside, &counts, &free))
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
        if self.counts.queued.load(Ordering::Relaxed) >= QUEUE as u64 {
            return false;
        }
        let mut buffer = self.free.lock().expect("the free frames").pop().unwrap_or_default();
        buffer.clear();
        buffer.extend_from_slice(texels);
        self.counts.queued.fetch_add(1, Ordering::Relaxed);
        match tx.try_send((buffer, at)) {
            Ok(()) => true,
            Err(TrySendError::Full((buffer, _))) => {
                self.counts.queued.fetch_sub(1, Ordering::Relaxed);
                give_back(&self.free, buffer);
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                self.counts.queued.fetch_sub(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Frames the encoder took, which is what the manifest counts — never what was handed over.
    pub fn encoded(&self) -> u64 {
        self.counts.encoded.load(Ordering::Relaxed)
    }

    /// Frames accepted and then lost, which an encoder that died mid-write costs, and a frame
    /// the sidecar could not account for.
    pub fn lost(&self) -> u64 {
        self.counts.lost.load(Ordering::Relaxed)
    }

    /// How full the queue into the encoder is, which is this stream's whole buffer health.
    pub fn fill(&self) -> f32 {
        self.counts.queued.load(Ordering::Relaxed) as f32 / QUEUE as f32
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
    queued: Arc<AtomicU64>,
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
    mut beside: crate::beside::Beside,
    counts: &Counts,
    free: &Free,
) {
    let mut flushed = Instant::now();
    // The FIRST error is what killed the stream; `finish` on a dead encoder only says so again.
    let died = |counts: &Counts, why: String| {
        counts.dead.store(true, Ordering::Relaxed);
        counts.error.lock().expect("the encoder's error").get_or_insert(why);
    };
    for (buffer, at) in rx.iter() {
        counts.queued.fetch_sub(1, Ordering::Relaxed);
        if let Err(why) = encoder.write(&buffer) {
            counts.lost.fetch_add(1, Ordering::Relaxed);
            died(counts, why);
            break;
        }
        // A frame the sidecar could not account for is a frame nothing can align, so it is LOST
        // rather than counted — the container holds it and the manifest says it was not kept.
        if let Err(e) = beside.line(at, crate::beside::Extent::Rows(1), None) {
            counts.lost.fetch_add(1, Ordering::Relaxed);
            died(counts, e);
            break;
        }
        counts.encoded.fetch_add(1, Ordering::Relaxed);
        give_back(free, buffer);
        if flushed.elapsed() >= FLUSH_EVERY {
            let _ = beside.sync();
            flushed = Instant::now();
        }
    }
    // A dead encoder still owes its counters: what the queue holds is LOST, never forgotten, or
    // `fill` reads a queue that never empties.
    for (buffer, _) in rx.try_iter() {
        counts.queued.fetch_sub(1, Ordering::Relaxed);
        counts.lost.fetch_add(1, Ordering::Relaxed);
        give_back(free, buffer);
    }
    let _ = beside.sync();
    if let Err(why) = encoder.finish() {
        died(counts, why);
    }
}
