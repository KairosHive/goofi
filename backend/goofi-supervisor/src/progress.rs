//! Progress a library reports and a binary shows: one sink the binary installs. With none
//! installed a step is still logged, and nothing is drawn.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One step of a boot or a load as the startup screen draws it.
pub enum Progress {
    /// A step begins; the heartbeat's subject from now on.
    Report(String),
    /// A fact under the current step.
    Note(String),
    /// A scan of a folder with this many files to read begins.
    Scanning(PathBuf, usize),
    /// A file is being read.
    Reading(PathBuf),
    /// One file of a folder was read and decided.
    Scanned(PathBuf, PathBuf),
    /// A folder's scan is over: how many nodes it put in the library, and how many are unavailable.
    Indexed(PathBuf, usize, usize),
}

static SINK: OnceLock<Box<dyn Fn(Progress) + Send + Sync>> = OnceLock::new();

/// Install the one sink; a second is refused.
pub fn set_sink(sink: impl Fn(Progress) + Send + Sync + 'static) -> Result<(), String> {
    SINK.set(Box::new(sink)).map_err(|_| "the progress sink is already set".to_string())
}

fn send(p: Progress) {
    if let Some(sink) = SINK.get() {
        sink(p);
    }
}

/// A step begins: logged always, so a load's steps reach the console, and shown while a screen is up.
pub fn report(message: impl Into<String>) {
    let message = message.into();
    crate::log::record(crate::log::Source::component("goofi"), crate::log::Level::Info, None, message.clone());
    send(Progress::Report(message));
}

pub fn note(message: impl Into<String>) {
    send(Progress::Note(message.into()));
}

pub fn scanning(folder: &Path, files: usize) {
    send(Progress::Scanning(folder.to_path_buf(), files));
}

pub fn reading(file: &Path) {
    send(Progress::Reading(file.to_path_buf()));
}

pub fn scanned(folder: &Path, file: &Path) {
    send(Progress::Scanned(folder.to_path_buf(), file.to_path_buf()));
}

pub fn indexed(folder: &Path, nodes: usize, unavailable: usize) {
    send(Progress::Indexed(folder.to_path_buf(), nodes, unavailable));
}
