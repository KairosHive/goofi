//! One goofi session owns every process-scoped resource; its held `alive.lock` is the ONE
//! aliveness answer. Everything ephemeral — the lock, the record, the iceoryx2 root — lives in
//! one machine-wide directory every sweep reads, so a boot under any `GOOFI_HOME` sees the same
//! answer and never sweeps a live session. Only a crash's workspace is kept, under recovery.

use std::fs::{self, File};
use std::sync::OnceLock;
use std::io;
use std::path::{Path, PathBuf};

use crate::home;

/// The env var a spawned process reads to JOIN its parent's session rather than hold its own.
pub const ENV: &str = "GOOFI_SESSION";

/// One session as its record spells it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub id: String,
    /// The HTTP base every route hangs off; empty until the server has bound.
    #[serde(default)]
    pub url: String,
}

/// Where every session's ephemeral directory lives. A FIXED short path, not the user's temp dir:
/// a unix socket path under it is capped at 108 bytes, half of which a macOS `$TMPDIR` spends and
/// all of which `%LOCALAPPDATA%\Temp` can (iceoryx2 emulates the socket on Windows, cap and all).
/// On Windows it sits beside `C:\Temp\iceoryx2`, which iceoryx2 keeps its shared memory in.
pub fn system_base() -> PathBuf {
    if cfg!(unix) {
        PathBuf::from("/tmp/goofi-system")
    } else {
        PathBuf::from(r"C:\Temp\goofi-system")
    }
}

/// The ephemeral directory of session `id`.
pub fn system_dir(id: &str) -> PathBuf {
    system_base().join(id)
}

/// Where every live session's workspace lives: ephemeral, so the OS temp directory. What a crash
/// leaves here is moved to [`recovery_base`] by the next boot, so it outlives a reboot.
pub fn workspaces_base() -> PathBuf {
    std::env::temp_dir().join("goofi-workspaces")
}

/// Where a dead session's autosaved workspace is kept for the user to recover or discard.
pub fn recovery_base() -> PathBuf {
    home::system().join("recovery")
}

/// The workspace directory of session `id`.
pub fn workspace_dir(id: &str) -> PathBuf {
    workspaces_base().join(id)
}

/// A 64-bit random id, hex. Short on purpose: it is a path segment under the socket cap above.
pub fn fresh_id() -> String {
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).expect("the OS random source");
    format!("{:016x}", u64::from_be_bytes(nonce))
}

static CURRENT: OnceLock<String> = OnceLock::new();

/// Name the session this process runs under — decided once, by whoever holds or joins it.
/// A second decision is ignored: the first is the one every child was told.
pub fn decide(id: &str) {
    let _ = CURRENT.set(id.to_string());
}

/// The session this process runs under, once decided.
pub fn current() -> Option<&'static str> {
    CURRENT.get().map(String::as_str)
}

/// What names a part file written beside a cache entry: `s<session id>`, so the boot pass can
/// sweep a crash's leftover, and no bare content key reads as one. No session yet: `p<pid>`.
pub fn tag() -> String {
    current().map(|id| format!("s{id}")).unwrap_or_else(|| format!("p{}", std::process::id()))
}

/// Sweep the caches under `.goofi/system`: every part named by a dead session, and every
/// versioned tree that is not `version`'s. Cargo's own `target`, `crates` and `sdk` are not walked.
pub fn sweep_system(version: &str) {
    let system = home::system();
    for (dir, skip) in [("build", &["target", "crates", "sdk"][..]), ("shipped", &[][..])] {
        sweep_dead_parts(&system.join(dir), skip, 5);
    }
    for versioned in [system.join("shipped"), system.join("build").join("sdk")] {
        let Ok(entries) = fs::read_dir(versioned) else { continue };
        for entry in entries.flatten() {
            if entry.file_name() != *version {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

fn sweep_dead_parts(dir: &Path, skip: &[&str], depth: usize) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(id) = session_id_in(&name) {
            if !alive(id) {
                let _ = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            }
            continue;
        }
        if depth > 0 && path.is_dir() && !skip.contains(&name.as_str()) {
            sweep_dead_parts(&path, skip, depth - 1);
        }
    }
}

/// The session id a part name carries: one `.`- or `-`-separated segment spelled as [`tag`].
fn session_id_in(name: &str) -> Option<&str> {
    name.split(['.', '-'])
        .filter_map(|s| s.strip_prefix('s'))
        .find(|id| id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The session this process OWNS: alive exactly as long as this value lives.
pub struct Held {
    id: String,
    lock: Option<File>,
}

impl Held {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Record where this session serves. Written beside the record and renamed in, so a reader
    /// sees a whole file or none.
    pub fn record_url(&self, url: &str) {
        let _ = write_record(&self.id, url);
    }
}

fn write_record(id: &str, url: &str) -> io::Result<()> {
    let dir = system_dir(id);
    let part = dir.join("session.json.part");
    fs::write(&part, serde_json::to_vec_pretty(&Session { id: id.into(), url: url.into() })?)?;
    fs::rename(&part, dir.join("session.json"))
}

impl Drop for Held {
    /// A clean shutdown: the lock goes first, so no reader sees the session alive while its
    /// resources go. The ephemeral directory is the transport's to remove.
    fn drop(&mut self) {
        drop(self.lock.take());
        let _ = fs::remove_file(system_dir(&self.id).join("alive.lock"));
        // The workspace parent, when the owner has already taken its mount away: an empty
        // directory is nobody's work. A non-empty one stays, and `remove_dir` refuses it.
        let _ = fs::remove_dir(workspace_dir(&self.id));
    }
}

/// Hold a new session under `id`. Its directory is built under a part name and renamed into
/// place with its lock already held, so no sweep ever sees one that is neither locked nor dead.
pub fn hold(id: &str) -> io::Result<Held> {
    let at = |what: &'static str| move |e: io::Error| io::Error::new(e.kind(), format!("{what}: {e}"));
    let part = system_base().join(format!("{id}.part"));
    let _ = fs::remove_dir_all(&part);
    fs::create_dir_all(&part).map_err(at("create the part"))?;
    let lock = File::create(part.join("alive.lock")).map_err(at("create the lock"))?;
    // Windows refuses to move a folder while a file inside it is open, so there the lock is
    // taken once the folder is in place; the window before it is the time of one open.
    let held = (!cfg!(windows)).then_some(lock);
    if let Some(lock) = &held {
        lock.lock().map_err(at("take the lock"))?;
    }
    fs::rename(&part, system_dir(id)).map_err(at("move the part into place"))?;
    let lock = match held {
        Some(lock) => lock,
        None => {
            let lock = File::options().read(true).write(true).open(system_dir(id).join("alive.lock")).map_err(at("open the lock"))?;
            lock.lock().map_err(at("take the lock"))?;
            lock
        }
    };
    let held = Held { id: id.to_string(), lock: Some(lock) };
    write_record(id, "").map_err(at("write the record"))?;
    Ok(held)
}

/// Whether the session whose ephemeral directory is `dir` is alive: its lock is held by a living
/// process. A missing lock file is a dead session; a lock this process can take is nobody's.
pub fn alive_at(dir: &Path) -> bool {
    let Ok(file) = File::options().read(true).write(true).open(dir.join("alive.lock")) else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => false,
        Err(std::fs::TryLockError::WouldBlock) => true,
        // An error that is not the lock being held proves nothing; the record stays.
        Err(std::fs::TryLockError::Error(_)) => true,
    }
}

pub fn alive(id: &str) -> bool {
    alive_at(&system_dir(id))
}

/// Every alive session on the machine, its record read; a directory whose lock nobody holds is
/// dead — a part still being built holds its lock too — and `remove` takes it as it is met.
/// The lock is the FIRST thing in a directory, so one without it is dead, whatever it holds.
pub fn sessions(mut remove: impl FnMut(&Path)) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(system_base()) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            let _ = fs::remove_file(&dir);
            continue;
        }
        if !alive_at(&dir) {
            remove(&dir);
            continue;
        }
        let parsed = fs::read(dir.join("session.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Session>(&b).ok());
        if let Some(s) = parsed {
            out.push(s);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}
