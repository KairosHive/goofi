//! The startup screen: a line per step, a progress bar per folder being scanned, and a
//! heartbeat when a step takes long. Everything is drawn on the terminal the log capture saved.
//! It is the one sink of [`goofi_supervisor::progress`]; the libraries report, this draws.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, OnceLock};
use goofi_supervisor::sync::Mutex;
use std::time::{Duration, Instant};

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle, TermLike};

static ACTIVE: Mutex<Option<(String, Instant)>> = Mutex::new(None);
static FOLDERS: Mutex<Option<HashMap<PathBuf, ProgressBar>>> = Mutex::new(None);

pub struct Startup {
    stop: mpsc::Sender<()>,
    worker: Option<goofi_supervisor::worker::Worker>,
    started: Instant,
}

/// The saved terminal as indicatif's drawing surface, so bars survive the stdio capture.
#[derive(Debug)]
struct Terminal;

impl TermLike for Terminal {
    fn width(&self) -> u16 { goofi_supervisor::log::terminal_width().unwrap_or(80) }
    fn move_cursor_up(&self, n: usize) -> std::io::Result<()> { if n > 0 { self.write_str(&format!("\x1b[{n}A")) } else { Ok(()) } }
    fn move_cursor_down(&self, n: usize) -> std::io::Result<()> { if n > 0 { self.write_str(&format!("\x1b[{n}B")) } else { Ok(()) } }
    fn move_cursor_right(&self, n: usize) -> std::io::Result<()> { if n > 0 { self.write_str(&format!("\x1b[{n}C")) } else { Ok(()) } }
    fn move_cursor_left(&self, n: usize) -> std::io::Result<()> { if n > 0 { self.write_str(&format!("\x1b[{n}D")) } else { Ok(()) } }
    fn write_line(&self, s: &str) -> std::io::Result<()> { self.write_str(s)?; self.write_str("\n") }
    fn write_str(&self, s: &str) -> std::io::Result<()> { goofi_supervisor::log::terminal_write(s.as_bytes()) }
    fn clear_line(&self) -> std::io::Result<()> { self.write_str("\r\x1b[2K") }
    fn flush(&self) -> std::io::Result<()> { Ok(()) }
}

fn screen() -> &'static MultiProgress {
    static SCREEN: OnceLock<MultiProgress> = OnceLock::new();
    SCREEN.get_or_init(|| {
        let target = match goofi_supervisor::log::terminal_width() {
            Some(_) => ProgressDrawTarget::term_like(Box::new(Terminal)),
            None => ProgressDrawTarget::hidden(),
        };
        MultiProgress::with_draw_target(target)
    })
}

fn line(mark: &str, message: &str) {
    let text = format!("  {mark} {message}");
    let screen = screen();
    if screen.is_hidden() || screen.println(&text).is_err() {
        let _ = goofi_supervisor::log::terminal_line(&text);
    }
}

impl Startup {
    pub fn begin(version: &str) -> Self {
        use goofi_supervisor::progress::Progress;
        let _ = goofi_supervisor::progress::set_sink(|p| match p {
            Progress::Report(m) => report(&m),
            Progress::Note(m) => note(&m),
            Progress::Scanning(folder, files) => scanning(&folder, files),
            Progress::Reading(file) => reading(&file),
            Progress::Scanned(folder, file) => scanned(&folder, &file),
            Progress::Indexed(folder, nodes, unavailable) => indexed(&folder, nodes, unavailable),
        });
        let started = Instant::now();
        line("goofi", &format!("{version} · starting"));
        *ACTIVE.lock() = Some(("Starting".into(), started));
        *FOLDERS.lock() = Some(HashMap::new());
        let (stop, receive) = mpsc::channel();
        let worker = goofi_supervisor::worker::thread("goofi-startup").spawn(move || {
            while receive.recv_timeout(Duration::from_secs(5)) == Err(mpsc::RecvTimeoutError::Timeout) {
                let active = ACTIVE.lock();
                if let Some((message, since)) = active.as_ref().filter(|(_, since)| since.elapsed() >= Duration::from_secs(5)) {
                    line("…", &format!("{message} · {:.0}s elapsed", since.elapsed().as_secs_f64()));
                }
            }
        }).ok();
        Self { stop, worker, started }
    }

    pub fn finish(self, message: &str) {
        let elapsed = self.started.elapsed();
        drop(self);
        line("✓", &format!("{message} in {:.1}s", elapsed.as_secs_f64()));
    }
}

impl Drop for Startup {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        *ACTIVE.lock() = None;
        *FOLDERS.lock() = None;
    }
}

/// A step begins: printed, and the heartbeat's subject from now on, while the screen is up.
fn report(message: &str) {
    let mut active = ACTIVE.lock();
    if let Some(current) = active.as_mut() {
        line(">", message);
        *current = (message.to_string(), Instant::now());
    }
}

/// A fact under the current step, printed during startup only.
fn note(message: &str) {
    if ACTIVE.lock().is_some() {
        line("·", message);
    }
}

/// A folder as the screen names it: under `.goofi` by its path from there, else from `~`.
fn shown(path: &Path) -> String {
    if path.starts_with(goofi_supervisor::home::system().join("shipped")) {
        return format!("shipped/{}", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
    }
    if let Ok(rest) = path.strip_prefix(goofi_supervisor::home::dir()) {
        return format!(".goofi/{}", rest.display());
    }
    match std::env::home_dir().and_then(|h| path.strip_prefix(h).ok().map(|r| r.to_path_buf())) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// A scan of `folder` with `files` to read begins: a bar until [`indexed`] replaces it.
fn scanning(folder: &Path, files: usize) {
    let mut folders = FOLDERS.lock();
    let Some(folders) = folders.as_mut() else { return };
    let bar = screen().add(ProgressBar::new(files as u64));
    let style = ProgressStyle::with_template("  ⋯ {prefix} {bar:24} {pos}/{len} {msg}").expect("a template").progress_chars("━╸ ");
    bar.set_style(style);
    bar.set_prefix(shown(folder));
    bar.enable_steady_tick(Duration::from_millis(100));
    folders.insert(folder.to_path_buf(), bar);
}

/// `file` is being read: its folder's bar names it.
fn reading(file: &Path) {
    let Some(folder) = file.parent() else { return };
    if let Some(bar) = FOLDERS.lock().as_ref().and_then(|f| f.get(folder)) {
        bar.set_message(file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    }
}

/// One file of `folder` was read and decided.
fn scanned(folder: &Path, file: &Path) {
    if let Some(bar) = FOLDERS.lock().as_ref().and_then(|f| f.get(folder)) {
        bar.set_message(file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        bar.inc(1);
    }
}

/// The scan of `folder` is over: its bar becomes the count of nodes it put in the library.
fn indexed(folder: &Path, nodes: usize, unavailable: usize) {
    let bar = FOLDERS.lock().as_mut().and_then(|f| f.remove(folder));
    if let Some(bar) = bar {
        bar.finish_and_clear();
        screen().remove(&bar);
        let greyed = if unavailable > 0 { format!(" · {unavailable} unavailable") } else { String::new() };
        let noun = if nodes == 1 { "node" } else { "nodes" };
        line("✓", &format!("{} · {nodes} {noun}{greyed}", shown(folder)));
    }
}
