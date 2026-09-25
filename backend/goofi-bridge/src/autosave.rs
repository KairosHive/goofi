//! The autosave: the open patch written beside its mount, in the `.gfi` layout unpacked, whenever
//! it holds unsaved work — so a crash leaves a recovery, and the next manager can offer it. It
//! carries the manifest and the workspace files; a node's opaque state is persisted by a save.
//! The worker parks on `AppState::changed`, which every settle, every dirty transition and a
//! watcher on the mount pulse, and writes once the pulses go quiet. Without a watcher it looks every [`CAP`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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
            state.changed.wait_timeout(if watch.at.is_some() { PARK } else { CAP });
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

/// Every live mount, as given and symlink-free, and the waker its event pulses. ONE watcher serves
/// them all. FSEvents names the real path: a macOS temp mount is `/var/…`, its events `/private/var/…`.
static MOUNTS: Mutex<Vec<(PathBuf, PathBuf, Arc<goofi_node::DrainWaker>)>> = Mutex::new(Vec::new());

/// The process's one watcher, made on first use and again after a refusal: the instances a
/// machine grants run out while other programs hold them.
static WATCHER: Mutex<Option<notify::RecommendedWatcher>> = Mutex::new(None);

fn watch(mount: &Path) -> bool {
    let mut watcher = WATCHER.lock().unwrap();
    if watcher.is_none() {
        match notify::recommended_watcher(|event: notify::Result<notify::Event>| {
            let hit = |at: &[&PathBuf]| {
                event.as_ref().map_or(true, |e| e.paths.is_empty() || e.paths.iter().any(|p| at.iter().any(|m| p.starts_with(m))))
            };
            for (_, _, changed) in MOUNTS.lock().unwrap().iter().filter(|(m, real, _)| hit(&[m, real])) {
                changed.notify();
            }
        }) {
            Ok(made) => *watcher = Some(made),
            Err(e) => {
                static SAID: std::sync::Once = std::sync::Once::new();
                SAID.call_once(|| {
                    let why = format!("autosave: no watcher on the workspace, so it is walked every {CAP:?}: {e}");
                    goofi_core::log::record(goofi_core::log::Source::component("bridge"), goofi_core::log::Level::Warning, None, why);
                });
            }
        }
    }
    watcher.as_mut().is_some_and(|w| w.watch(mount, notify::RecursiveMode::Recursive).is_ok())
}

/// This manager's mount on the shared watcher, re-aimed when a load replaces it. The tick then
/// walks, so an ignored file's churn costs a walk and never a write.
struct Watch {
    changed: Arc<goofi_node::DrainWaker>,
    at: Option<PathBuf>,
}

impl Watch {
    fn new(state: &AppState) -> Watch {
        let mut w = Watch { changed: state.changed.clone(), at: None };
        w.follow(state);
        w
    }

    fn follow(&mut self, state: &AppState) {
        let mount = state.mount();
        if self.at.as_ref() == Some(&mount) {
            return;
        }
        self.release();
        if watch(&mount) {
            let real = goofi_core::path::canonical(&mount).unwrap_or_else(|_| mount.clone());
            MOUNTS.lock().unwrap().push((mount.clone(), real, self.changed.clone()));
            self.at = Some(mount);
        }
    }

    fn release(&mut self) {
        let Some(old) = self.at.take() else { return };
        MOUNTS.lock().unwrap().retain(|(m, _, _)| *m != old);
        if let Some(w) = WATCHER.lock().unwrap().as_mut() {
            let _ = w.unwatch(&old);
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.release();
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
    let manifest = state.graph.lock().unwrap().serialize();
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

/// What a recovery holds: its path, the patch's home, and when the autosave was taken.
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

/// Every `(session id, nonce directory)` under `base`, sessions filtered by `keep`.
fn nonces(base: &Path, keep: impl Fn(&str) -> bool) -> Vec<(String, PathBuf)> {
    let Ok(sessions) = std::fs::read_dir(base) else { return Vec::new() };
    let mut out = Vec::new();
    for session in sessions.flatten() {
        let Some(id) = session.file_name().to_str().map(str::to_string) else { continue };
        if !session.path().is_dir() || !keep(&id) {
            continue;
        }
        let Ok(dirs) = std::fs::read_dir(session.path()) else { continue };
        out.extend(dirs.flatten().map(|n| n.path()).filter(|p| p.is_dir()).map(|p| (id.clone(), p)));
    }
    out.sort();
    out
}

/// Move a tree across filesystems if it must: temp is often one of its own.
fn move_tree(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    archive::copy_tree(from, to)?;
    std::fs::remove_dir_all(from).map_err(|e| format!("{}: {e}", from.display()))
}

/// The boot pass over the workspaces: a dead session's directory that carries an autosave is
/// moved to the recovery base for safekeeping, one that carries none held no unsaved work and
/// goes. Nothing a living session owns is touched. Answers how many went either way.
pub fn sweep_dead() -> usize {
    let mut swept = 0;
    for (id, dir) in nonces(&goofi_core::session::workspaces_base(), |id| !goofi_core::session::alive(id)) {
        let Some(nonce) = dir.file_name() else { continue };
        let done = if archive::has_manifest(&dir) {
            move_tree(&dir, &goofi_core::session::recovery_base().join(&id).join(nonce)).is_ok()
        } else {
            std::fs::remove_dir_all(&dir).is_ok()
        };
        swept += usize::from(done);
        let _ = std::fs::remove_dir(goofi_core::session::workspace_dir(&id));
    }
    swept
}

/// Every recovery on this machine, oldest session first.
pub fn recoverable() -> Vec<Value> {
    nonces(&goofi_core::session::recovery_base(), |_| true)
        .into_iter()
        .filter(|(_, dir)| archive::has_manifest(dir))
        .map(|(_, dir)| entry(&dir))
        .collect()
}

/// The recovery a caller names, checked: a nonce directory under the recovery base, with an
/// autosave in it. Anything else is refused — this is the one path an op removes wholesale.
pub fn recovery(workspace: &str) -> Result<PathBuf, String> {
    let dir = PathBuf::from(crate::fsbrowse::resolve(workspace));
    let base = goofi_core::path::canonical(&goofi_core::session::recovery_base()).map_err(|e| e.to_string())?;
    let under = dir.parent().and_then(Path::parent) == Some(base.as_path());
    if !under {
        return Err(format!("{workspace}: not a recovery goofi keeps"));
    }
    if !archive::has_manifest(&dir) {
        return Err(format!("{workspace}: no autosave to recover"));
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
