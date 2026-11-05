//! The terminal screen goofi's binaries draw: the banner, a spinner per step that becomes a `✓`
//! or `✗` line, a bar per folder being scanned, and the outcome. The one sink of [`progress`].
//! Everything is drawn on the terminal the log capture saved, and in color only on a terminal.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use console::style;
use goofi_supervisor::log;
use goofi_supervisor::progress::{self, Progress};
use goofi_supervisor::sync::Mutex;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle, TermLike};

pub use console::Color;

const BANNER: [&str; 6] = [
    " ██████╗  ██████╗  ██████╗ ███████╗██╗",
    "██╔════╝ ██╔═══██╗██╔═══██╗██╔════╝██║",
    "██║  ███╗██║   ██║██║   ██║█████╗  ██║",
    "██║   ██║██║   ██║██║   ██║██╔══╝  ██║",
    "╚██████╔╝╚██████╔╝╚██████╔╝██║     ██║",
    " ╚═════╝  ╚═════╝  ╚═════╝ ╚═╝     ╚═╝",
];

/// The step in progress, and the lines that print under it once it is closed.
struct Step {
    name: String,
    bar: ProgressBar,
    started: Instant,
    under: Vec<String>,
}

#[derive(Default)]
struct State {
    step: Option<Step>,
    folders: HashMap<PathBuf, ProgressBar>,
    failed_at: Option<String>,
}

/// `Some` while a [`Screen`] is up: reports outside one are logged, not drawn.
static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Whether output to a terminal (`terminal`) is colored: `GOOFI_COLOR=always|never` decides,
/// else a terminal is, unless `NO_COLOR` is set or `TERM=dumb`.
pub fn color(terminal: bool) -> bool {
    match std::env::var("GOOFI_COLOR").ok().as_deref() {
        Some("always") => true,
        Some("never") => false,
        _ => terminal && std::env::var_os("NO_COLOR").is_none() && std::env::var("TERM").map_or(true, |t| t != "dumb"),
    }
}

/// The saved terminal as indicatif's drawing surface, so bars survive the stdio capture.
#[derive(Debug)]
struct Terminal;

impl TermLike for Terminal {
    fn width(&self) -> u16 { log::terminal_width().unwrap_or(80) }
    fn move_cursor_up(&self, n: usize) -> std::io::Result<()> { if n > 0 { self.write_str(&format!("\x1b[{n}A")) } else { Ok(()) } }
    fn move_cursor_down(&self, n: usize) -> std::io::Result<()> { if n > 0 { self.write_str(&format!("\x1b[{n}B")) } else { Ok(()) } }
    fn move_cursor_right(&self, n: usize) -> std::io::Result<()> { if n > 0 { self.write_str(&format!("\x1b[{n}C")) } else { Ok(()) } }
    fn move_cursor_left(&self, n: usize) -> std::io::Result<()> { if n > 0 { self.write_str(&format!("\x1b[{n}D")) } else { Ok(()) } }
    fn write_line(&self, s: &str) -> std::io::Result<()> { self.write_str(s)?; self.write_str("\n") }
    fn write_str(&self, s: &str) -> std::io::Result<()> { log::terminal_write(s) }
    fn clear_line(&self) -> std::io::Result<()> { self.write_str("\r\x1b[2K") }
    fn flush(&self) -> std::io::Result<()> { Ok(()) }
}

fn screen() -> &'static MultiProgress {
    static SCREEN: OnceLock<MultiProgress> = OnceLock::new();
    SCREEN.get_or_init(|| {
        let target = match log::terminal_width() {
            Some(_) => ProgressDrawTarget::term_like(Box::new(Terminal)),
            None => ProgressDrawTarget::hidden(),
        };
        MultiProgress::with_draw_target(target)
    })
}

/// One line on the saved terminal, above any bars.
pub fn say(text: &str) {
    let screen = screen();
    if screen.is_hidden() || screen.println(text).is_err() {
        let _ = log::terminal_line(text);
    }
}

/// Run `f` with the bars lifted, for a prompt they must not draw over.
pub fn suspend<R>(f: impl FnOnce() -> R) -> R {
    screen().suspend(f)
}

/// `text` dimmed, for a fact beside the main line.
pub fn dim(text: &str) -> String {
    style(text).dim().to_string()
}

/// `text` in the color of a warning.
pub fn warn(text: &str) -> String {
    style(text).yellow().to_string()
}

/// The mark a closed step carries: a green `✓`, or a red `✗`.
pub fn mark(passed: bool) -> String {
    match passed {
        true => style("✓").green().to_string(),
        false => style("✗").red().bold().to_string(),
    }
}

/// The GOOFI banner in `color`, with `under` beneath it when it is not empty.
pub fn banner(color: Color, under: &str) {
    say("");
    for line in BANNER {
        say(&format!("  {}", style(line).fg(color).bold()));
    }
    if !under.is_empty() {
        say(&format!("  {}", style(under).dim()));
    }
    say("");
}

/// The address a browser opens, as the Electron shell reads it: `goofi → <url>`.
pub fn url(url: &str) {
    say(&format!("  goofi → {}", style(url).cyan().bold().underlined()));
}

/// The screen while it is up: every step reported from [`Screen::begin`] to its outcome.
pub struct Screen {
    started: Instant,
}

impl Screen {
    pub fn begin() -> Screen {
        let colored = color(log::terminal_width().is_some());
        console::set_colors_enabled(colored);
        console::set_colors_enabled_stderr(colored);
        let _ = progress::set_sink(|p| match p {
            Progress::Report(m) => report(m),
            Progress::Note(m) => under(format!("    {} {}", style("·").dim(), style(m).dim())),
            Progress::Scanning(folder, files) => scanning(&folder, files),
            Progress::Reading(file) => reading(&file),
            Progress::Scanned(folder, file) => scanned(&folder, &file),
            Progress::Indexed(folder, nodes, unavailable) => indexed(&folder, nodes, unavailable),
        });
        *STATE.lock() = Some(State::default());
        Screen { started: Instant::now() }
    }

    /// Close the step in progress as passed or failed.
    pub fn close(&self, passed: bool) {
        if let Some(state) = STATE.lock().as_mut() {
            close(state, passed);
        }
    }

    /// The end on success: `message` in green, with the time since [`Screen::begin`].
    pub fn ready(self, message: &str) {
        self.close(true);
        let took = format!("in {:.1}s", self.started.elapsed().as_secs_f64());
        say(&format!("  {} {}", style(format!("✓ {message}")).green().bold(), style(took).dim()));
    }

    /// The end on failure: `headline` and the step it failed at in red, then the error, its first
    /// line red and the rest dim.
    pub fn failed(self, headline: &str, error: &str) {
        self.close(false);
        let at = STATE.lock().as_ref().and_then(|s| s.failed_at.clone()).map(|s| format!(" at \"{s}\"")).unwrap_or_default();
        say(&format!("  {}", style(format!("✗ {headline}{at}.")).red().bold()));
        let mut lines = error.lines();
        say(&format!("    {}", style(lines.next().unwrap_or_default()).red()));
        for line in lines {
            say(&format!("    {}", style(line).dim()));
        }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        if let Some(state) = STATE.lock().take() {
            for bar in state.folders.values().chain(state.step.as_ref().map(|s| &s.bar)) {
                bar.finish_and_clear();
                screen().remove(bar);
            }
        }
    }
}

/// A step begins: the one before it passed.
fn report(name: String) {
    let mut state = STATE.lock();
    let Some(state) = state.as_mut() else { return };
    close(state, true);
    let bar = screen().insert(0, ProgressBar::new_spinner());
    bar.set_style(ProgressStyle::with_template("  {spinner:.cyan} {msg} {elapsed:.dim}").expect("a template"));
    bar.set_message(name.clone());
    bar.enable_steady_tick(Duration::from_millis(100));
    if screen().is_hidden() {
        let _ = log::terminal_line(&format!("  ⋯ {name}"));
    }
    state.step = Some(Step { name, bar, started: Instant::now(), under: Vec::new() });
}

fn close(state: &mut State, passed: bool) {
    let Some(step) = state.step.take() else { return };
    step.bar.finish_and_clear();
    screen().remove(&step.bar);
    let mark = mark(passed);
    let took = step.started.elapsed();
    let took = if took >= Duration::from_millis(100) { format!(" {}", style(format!("{:.1}s", took.as_secs_f64())).dim()) } else { String::new() };
    say(&format!("  {mark} {}{took}", step.name));
    for line in &step.under {
        say(line);
    }
    if !passed {
        state.failed_at = Some(step.name);
    }
}

/// A line under the step in progress, printed when it closes; outside a step, printed now.
fn under(line: String) {
    match STATE.lock().as_mut().map(|s| s.step.as_mut()) {
        Some(Some(step)) => step.under.push(line),
        Some(None) => say(&line),
        None => {}
    }
}

/// A folder as the screen names it: under `.goofi` by its path from there, else from `~`.
fn shown(path: &Path) -> String {
    if path.starts_with(goofi_supervisor::layout::runtime().shipped()) {
        return format!("shipped/{}", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
    }
    if let Ok(rest) = path.strip_prefix(goofi_supervisor::layout::home()) {
        return format!(".goofi/{}", rest.display());
    }
    match std::env::home_dir().and_then(|h| path.strip_prefix(h).ok().map(|r| r.to_path_buf())) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// A scan of `folder` with `files` to read begins: a bar until [`indexed`] replaces it.
fn scanning(folder: &Path, files: usize) {
    let mut state = STATE.lock();
    let Some(state) = state.as_mut() else { return };
    let bar = screen().add(ProgressBar::new(files as u64));
    let style = ProgressStyle::with_template("    {spinner:.cyan} {prefix} {bar:24.cyan} {pos}/{len} {msg:.dim}").expect("a template").progress_chars("━╸ ");
    bar.set_style(style);
    bar.set_prefix(shown(folder));
    bar.enable_steady_tick(Duration::from_millis(100));
    state.folders.insert(folder.to_path_buf(), bar);
}

fn file_name(file: &Path) -> String {
    file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `file` is being read: its folder's bar names it.
fn reading(file: &Path) {
    let Some(folder) = file.parent() else { return };
    if let Some(bar) = STATE.lock().as_ref().and_then(|s| s.folders.get(folder)) {
        bar.set_message(file_name(file));
    }
}

/// One file of `folder` was read and decided.
fn scanned(folder: &Path, file: &Path) {
    if let Some(bar) = STATE.lock().as_ref().and_then(|s| s.folders.get(folder)) {
        bar.set_message(file_name(file));
        bar.inc(1);
    }
}

/// The scan of `folder` is over: its bar becomes the count of nodes it put in the library.
fn indexed(folder: &Path, nodes: usize, unavailable: usize) {
    let bar = STATE.lock().as_mut().and_then(|s| s.folders.remove(folder));
    let Some(bar) = bar else { return };
    bar.finish_and_clear();
    screen().remove(&bar);
    let noun = if nodes == 1 { "node" } else { "nodes" };
    let greyed = if unavailable > 0 { format!(" · {}", style(format!("{unavailable} unavailable")).yellow()) } else { String::new() };
    under(format!("    {} {} {}{greyed}", style("✓").green(), shown(folder), style(format!("· {nodes} {noun}")).dim()));
}
