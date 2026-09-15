//! The autosave: the open patch written beside its mount, in the `.gfi` layout unpacked, whenever
//! it holds unsaved work — so a crash leaves a recovery, and the next manager can offer it. It
//! carries the manifest and the workspace files; a node's opaque state is persisted by a save.
//! Nothing polls: the worker parks on `AppState::changed`, which every settle, every dirty
//! transition and a watcher on the mount pulse, and writes once the pulses go quiet.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use goofi_graph::archive;
use notify::Watcher;
use serde_json::{json, Value};

use crate::AppState;

/// How long the pulses must be quiet before a write: a drag settles within it.
const QUIET: Duration = Duration::from_millis(500);
/// The most a write waits under pulses that never go quiet — a script editing at a steady pace.
const CAP: Duration = Duration::from_secs(5);
/// A park with no ceiling worth naming: the stop pulses the waker too.
const PARK: Duration = Duration::from_secs(86_400);

/// The sidecar beside the manifest: the patch's home and when the autosave was taken.
const SIDECAR: &str = "autosave.json";

/// What the last autosave wrote: the manifest and the workspace, so a tick where nothing moved
/// writes nothing.
type Stamp = (String, archive::Fingerprint);

/// The directory an autosave lives in: the mount's nonce directory, `patch.yaml` beside `workspace/`.
fn dir_of(mount: &Path) -> PathBuf {
    mount.parent().unwrap_or(mount).to_path_buf()
}

/// Start the worker; it ends with `state.stopping`, before the mount goes.
pub(crate) fn spawn(state: AppState) {
    let owner = state.clone();
    let worker = goofi_core::worker::spawn("goofi-autosave", move || {
        let mut last: Option<Stamp> = None;
        let mut watch = Watch::new(&state);
        loop {
            state.changed.wait_timeout(PARK);
            // Debounce: wait for a quiet window, but no longer than the cap since the first pulse.
            let first = Instant::now();
            loop {
                let left = (first + CAP).saturating_duration_since(Instant::now());
                if left.is_zero() || !state.changed.wait_timeout(QUIET.min(left)) {
                    break;
                }
            }
            if state.stopping.stopped() {
                return;
            }
            watch.follow(&state);
            tick(&state, &mut last);
        }
    });
    if let Ok(worker) = worker {
        owner.workers.lock().unwrap().push(worker);
    }
}

/// The watcher on the mount, re-aimed when a load replaces it. Every event pulses `changed`; the
/// tick then walks, so an ignored file's churn costs a walk and never a write.
struct Watch {
    watcher: Option<notify::RecommendedWatcher>,
    at: Option<PathBuf>,
}

impl Watch {
    fn new(state: &AppState) -> Watch {
        let changed = state.changed.clone();
        let watcher = notify::recommended_watcher(move |_: notify::Result<notify::Event>| changed.notify());
        let mut w = Watch { watcher: watcher.ok(), at: None };
        w.follow(state);
        w
    }

    fn follow(&mut self, state: &AppState) {
        let mount = state.mount();
        if self.at.as_ref() == Some(&mount) {
            return;
        }
        let Some(watcher) = self.watcher.as_mut() else { return };
        if let Some(old) = self.at.take() {
            let _ = watcher.unwatch(&old);
        }
        if watcher.watch(&mount, notify::RecursiveMode::Recursive).is_ok() {
            self.at = Some(mount);
        }
    }
}

/// One look: unsaved work that moved since the last look is written; a clean patch has its
/// autosave removed, so what a crash leaves is exactly what was not on disk.
fn tick(state: &AppState, last: &mut Option<Stamp>) {
    let mount = state.mount();
    let dir = dir_of(&mount);
    let seen = archive::fingerprint(&mount);
    if !state.dirty_against(&seen) {
        if last.take().is_some() || archive::has_manifest(&dir) {
            let _ = std::fs::remove_file(dir.join(archive::MANIFEST));
            let _ = std::fs::remove_file(dir.join(SIDECAR));
        }
        return;
    }
    // The manifest is the op path's, not read off the graph here: the graph lock is the audio
    // drain's too, and a serialize held against it loses blocks.
    let manifest = state.manifest.lock().unwrap().clone();
    if last.as_ref().is_some_and(|(m, fp)| *m == manifest && *fp == seen) {
        return;
    }
    let at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let sidecar = json!({ "home": state.save_path(), "at": at });
    let written = archive::write_manifest(&dir, &manifest)
        .and_then(|()| std::fs::write(dir.join(SIDECAR), sidecar.to_string()).map_err(|e| e.to_string()));
    if let Err(e) = written {
        goofi_core::log::record(goofi_core::log::Source::component("bridge"), goofi_core::log::Level::Error, None, format!("autosave: {e}"));
        return;
    }
    *last = Some((manifest, seen));
}

/// What a dead session's nonce directory holds: the recovery's path, the patch's home, and when
/// the autosave was taken.
fn entry(dir: &Path) -> Value {
    let sidecar: Value = std::fs::read(dir.join(SIDECAR))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    json!({
        "workspace": goofi_core::path::to_slash(dir),
        "home": sidecar.get("home").cloned().unwrap_or(Value::Null),
        "at": sidecar.get("at").cloned().unwrap_or(Value::Null),
    })
}

/// Every dead session's nonce directory, `(session id, directory)`, alive ones never enumerated.
fn dead() -> Vec<(String, PathBuf)> {
    let Ok(sessions) = std::fs::read_dir(goofi_core::session::workspaces_base()) else { return Vec::new() };
    let mut out = Vec::new();
    for session in sessions.flatten() {
        let Some(id) = session.file_name().to_str().map(str::to_string) else { continue };
        if !session.path().is_dir() || goofi_core::session::alive(&id) {
            continue;
        }
        let Ok(nonces) = std::fs::read_dir(session.path()) else { continue };
        out.extend(nonces.flatten().map(|n| n.path()).filter(|p| p.is_dir()).map(|p| (id.clone(), p)));
    }
    out.sort();
    out
}

/// The boot pass over the workspaces: a dead session's directory that carries no autosave held
/// no unsaved work and goes; one that does is a recovery and stays. Answers how many went.
pub fn sweep_dead() -> usize {
    let mut swept = 0;
    for (id, dir) in dead() {
        if !archive::has_manifest(&dir) && std::fs::remove_dir_all(&dir).is_ok() {
            swept += 1;
        }
        let _ = std::fs::remove_dir(goofi_core::session::workspace_dir(&id));
    }
    swept
}

/// Every recovery on this machine, oldest session first.
pub fn recoverable() -> Vec<Value> {
    dead().into_iter().filter(|(_, dir)| archive::has_manifest(dir)).map(|(_, dir)| entry(&dir)).collect()
}

/// The recovery a caller names, checked: a nonce directory under the workspaces base, its
/// session dead. Anything else is refused — this is the one path an op removes wholesale.
pub fn recovery(workspace: &str) -> Result<PathBuf, String> {
    let dir = PathBuf::from(crate::fsbrowse::resolve(workspace));
    let base = goofi_core::path::canonical(&goofi_core::session::workspaces_base()).map_err(|e| e.to_string())?;
    let id = dir
        .parent()
        .filter(|session| session.parent() == Some(base.as_path()))
        .and_then(|session| session.file_name())
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{workspace}: not a workspace goofi left behind"))?;
    if !archive::has_manifest(&dir) {
        return Err(format!("{workspace}: no autosave to recover"));
    }
    if goofi_core::session::alive(id) {
        return Err(format!("{workspace}: that workspace belongs to a running goofi"));
    }
    Ok(dir)
}

/// Remove one recovery, and its session's directory once it holds nothing.
pub fn discard(dir: &Path) -> Result<(), String> {
    std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if let Some(session) = dir.parent() {
        let _ = std::fs::remove_dir(session);
    }
    Ok(())
}

/// The home a recovery's sidecar names, if it names one.
pub fn home_of(dir: &Path) -> Option<String> {
    entry(dir)["home"].as_str().map(str::to_string)
}
