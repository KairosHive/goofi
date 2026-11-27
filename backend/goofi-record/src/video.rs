//! A video stream: the frames one armed graphics slot renders, encoded. Everything a codec needs
//! is behind [`Encoder`], and `Ffmpeg` is the one implementation of it; [`Video`] is the format
//! the stream writes through and knows no codec.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command};
use std::sync::LazyLock;
use std::time::Duration;

use goofi_core::record::{Frame, VideoQuality};
use goofi_supervisor::child::{Child, Out};
use goofi_supervisor::sync::Mutex;

use crate::stream::{Added, Format};

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
    fn open(&self, file: &Path, size: (u32, u32), fps: f64, quality: VideoQuality) -> Result<Box<dyn Encoder>, String>;
    /// A take is about to record a picture of this size: whatever choosing an encoder for it
    /// costs is spent now, before the take begins, and not in its first frames.
    fn prepare(&self, _size: (u32, u32), _fps: f64, _quality: VideoQuality) {}
}

/// Constant-quality H.264 in Matroska, through an `ffmpeg` child on stdin.
pub struct FfmpegEncoders;

impl Encoders for FfmpegEncoders {
    fn probe(&self) -> Result<(), String> {
        goofi_supervisor::child::output("ffmpeg probe", ffmpeg_bare().arg("-version"), Duration::from_secs(10))
            .map_err(|_| MISSING.to_string())
            .and_then(|out| if out.status.success() { Ok(()) } else { Err(MISSING.to_string()) })
    }

    fn extension(&self) -> &'static str {
        "mkv"
    }

    fn prepare(&self, size: (u32, u32), fps: f64, quality: VideoQuality) {
        let _ = Preset::chosen(size, fps, quality);
    }

    fn open(&self, file: &Path, size: (u32, u32), fps: f64, quality: VideoQuality) -> Result<Box<dyn Encoder>, String> {
        if size.0 == 0 || size.1 == 0 || !fps.is_finite() || fps <= 0.0 {
            return Err("video needs a nonzero size and a positive finite frame rate".into());
        }
        Ok(Box::new(Ffmpeg {
            child: None, stdin: None, file: file.to_path_buf(), size, fps, quality, closed: false, stderr: None,
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

struct Ffmpeg {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    file: PathBuf,
    size: (u32, u32),
    fps: f64,
    quality: VideoQuality,
    closed: bool,
    stderr: Option<goofi_supervisor::worker::Worker<String>>,
}

/// Settings shared by the trial encode and the recording.
struct Preset {
    codec: &'static str,
    options: &'static [&'static str],
    device: Option<PathBuf>,
}

impl Preset {
    fn candidates() -> Vec<Self> {
        let mut presets = vec![
            #[cfg(target_os = "macos")]
            Self {
                codec: "h264_videotoolbox",
                options: &["-allow_sw", "0"], device: None,
            },
            #[cfg(not(target_os = "macos"))]
            Self {
                codec: "h264_nvenc",
                options: &["-preset", "p5", "-tune", "hq", "-rc", "constqp"],
                device: None,
            },
            #[cfg(target_os = "windows")]
            Self {
                codec: "h264_amf",
                options: &["-quality", "quality", "-rc", "cqp"],
                device: None,
            },
            #[cfg(not(target_os = "macos"))]
            Self {
                codec: "h264_qsv",
                options: &["-preset", "medium"], device: None,
            },
            ];
        #[cfg(target_os = "linux")]
        if let Ok(entries) = std::fs::read_dir("/dev/dri") {
            let mut devices: Vec<_> = entries.flatten().map(|entry| entry.path())
                .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("renderD")))
                .collect();
            devices.sort();
            presets.extend(devices.into_iter().map(|device| Self {
                codec: "h264_vaapi", options: &["-rc_mode", "CQP"], device: Some(device),
            }));
        }
        presets.push(Self {
            codec: "libx264", options: &["-preset", "veryfast"], device: None,
        });
        presets
    }

    fn input(&self, command: &mut Command) {
        if let Some(device) = &self.device {
            command.arg("-vaapi_device").arg(device);
        }
    }

    fn output(&self, command: &mut Command, fps: f64, quality: VideoQuality) {
        let q = quality.quantizer().to_string();
        match self.codec {
            "h264_videotoolbox" => { command.args(["-q:v", &quality.apple_quality().to_string()]); }
            "h264_amf" => { command.args(["-qp_i", &q, "-qp_p", &q]); }
            "h264_qsv" => { command.args(["-global_quality", &q]); }
            "libx264" => { command.args(["-crf", &q]); }
            _ => { command.args(["-qp", &q]); }
        }
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
    fn works(&self, size: (u32, u32), fps: f64, quality: VideoQuality) -> Result<bool, String> {
        let mut command = ffmpeg();
        self.input(&mut command);
        command.args(["-f", "lavfi", "-i", &format!(
            "nullsrc=size={}x{}:rate={fps},format=rgba", size.0, size.1,
        )]);
        self.output(&mut command, fps, quality);
        // A trial that fails is the expected answer for most candidates, not something to log.
        let mut child = goofi_supervisor::child::run(format!("ffmpeg trial {}", self.codec), command.args(["-frames:v", "1", "-f", "null", "-"]))
            .windowless()
            .stdout(Out::Null)
            .stderr(Out::Null)
            .spawn()
            .map_err(|e| format!("{MISSING} ({e})"))?;
        Ok(child.wait_within(Duration::from_secs(5)).is_some_and(|status| status.success()))
    }
}

/// The `ffmpeg` the layout names, with no arguments yet.
fn ffmpeg_bare() -> Command {
    goofi_supervisor::layout::command(goofi_supervisor::layout::Tool::Ffmpeg)
}

fn ffmpeg() -> Command {
    let mut command = ffmpeg_bare();
    command.args(["-hide_banner", "-loglevel", "error", "-y"]);
    command
}

/// The candidate that encoded at each frame size, so a later stream at that size opens with
/// no trial: a trial is a child per candidate, and an armed slot reopens on every resize.
static WORKING: LazyLock<Mutex<HashMap<(u32, u32), usize>>> = LazyLock::new(Default::default);

impl Preset {
    /// The candidate that encodes at `size`: the one a trial found before, or a trial of each now.
    fn chosen(size: (u32, u32), fps: f64, quality: VideoQuality) -> Result<Self, String> {
        let mut candidates = Self::candidates();
        let known = WORKING.lock().get(&size).copied().filter(|i| *i < candidates.len());
        let index = match known {
            Some(i) => i,
            None => {
                let mut found = None;
                for (i, preset) in candidates.iter().enumerate() {
                    if preset.works(size, fps, quality)? {
                        found = Some(i);
                        break;
                    }
                }
                let i = found.ok_or("FFmpeg has no working H.264 encoder for this frame size; install an FFmpeg build with libx264 or a supported hardware encoder")?;
                WORKING.lock().insert(size, i);
                i
            }
        };
        Ok(candidates.swap_remove(index))
    }
}

impl Ffmpeg {
    /// Start the encoder on the writer thread, without blocking the graphics clock.
    fn start(&mut self) -> Result<(), String> {
        let preset = &Preset::chosen(self.size, self.fps, self.quality)?;
        let mut command = ffmpeg();
        preset.input(&mut command);
        command.args(["-f", "rawvideo", "-pix_fmt", "rgba"])
            .args(["-s", &format!("{}x{}", self.size.0, self.size.1), "-r", &format!("{}", self.fps)])
            .args(["-i", "-"]);
        preset.output(&mut command, self.fps, self.quality);
        // stderr is read by the owner: an encoder's last words are the error the recording reports.
        let mut child = goofi_supervisor::child::run(format!("ffmpeg {}", self.file.display()), command.arg(&self.file))
            .windowless()
            .stdin_piped()
            .stdout(Out::Null)
            .stderr(Out::Pipe)
            .spawn()
            .map_err(|e| format!("could not start FFmpeg: {e}"))?;
        let mut stderr = child.stderr.take().ok_or("FFmpeg has no error pipe")?;
        let errors = goofi_supervisor::worker::thread("goofi-record-errors").spawn(move || {
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
            // `child` drops here: killed and reaped.
            Err(error) => return Err(format!("could not read FFmpeg errors: {error}")),
        }
        self.stdin = child.stdin.take();
        self.child = Some(child);
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
        // A recording drains until the container is complete, however long that takes.
        let status = self.child.take().map(|mut child| child.wait().map_err(|e| e.to_string())).transpose();
        let error = self.stderr.take().and_then(|reader| reader.join().ok()).unwrap_or_default();
        match status? {
            None => Ok(()),
            Some(status) if status.success() => Ok(()),
            Some(status) => Err(format!("ffmpeg left {} unfinished: {status}: {error}", self.file.display())),
        }
    }
}

impl Drop for Ffmpeg {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// One video file, written as a [`Format`]: the frames an armed texture slot read back or sent.
/// The file plays at `fps`, so each frame goes where its instant puts it. The worker that writes
/// it is this stream's own thread: a slow encoder costs this stream its queue's depth alone.
pub struct Video {
    encoder: Box<dyn Encoder>,
    fps: f64,
    encoded: u64,
    /// The first frame's instant: the file's zero.
    began: Option<f64>,
    /// The last picture encoded, which is what a hole in the source shows.
    held: Option<Frame>,
}

/// How far behind its instant the file is before a hole is filled, in frames: under one, so a
/// missing frame is seen, and over a half, so a late tick is not taken for one.
const HOLE: f64 = 0.75;

impl Video {
    pub fn open(encoders: &dyn Encoders, file: &Path, size: (u32, u32), fps: f64, quality: VideoQuality) -> Result<Video, String> {
        Ok(Video { encoder: encoders.open(file, size, fps, quality)?, fps, encoded: 0, began: None, held: None })
    }

    fn encode(&mut self, frame: &Frame) -> Result<(), String> {
        match frame {
            Frame::Texels(t) => self.encoder.write(&t.rgba)?,
            Frame::Data(d) => match d.as_texture().and_then(|texture| texture.pixels()) {
                Ok(p) => self.encoder.write(&p.to_rgba8())?,
                Err(_) => return Err("the frame is not the shape this stream holds".into()),
            },
        }
        self.encoded += 1;
        Ok(())
    }
}

impl Format for Video {
    /// A source that skipped or lost frames leaves a hole, which holds the picture before it; a
    /// frame a whole place early has no place, and is not encoded. The file so keeps real time.
    fn write(&mut self, frame: &Frame, at: f64) -> Result<Added, String> {
        let before = self.encoded;
        let behind = (at - *self.began.get_or_insert(at)) * self.fps - before as f64;
        if behind <= -1.0 {
            return Ok(Added::Rows(0));
        }
        if let Some(held) = self.held.take().filter(|_| behind >= HOLE) {
            for _ in 0..behind.round() as u64 {
                self.encode(&held)?;
            }
        }
        self.encode(frame)?;
        self.held = Some(frame.clone());
        Ok(Added::Rows(self.encoded - before))
    }

    /// Frames the encoder took, which is what the manifest counts.
    fn frames(&self) -> u64 {
        self.encoded
    }

    /// An encoder reaches its disk on its own; the container is complete only at the finish.
    fn sync(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        self.encoder.finish()
    }
}
