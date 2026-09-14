//! One goofi session: the identity every process-scoped resource is owned by, and the ONE
//! aliveness answer. A session holds a lock on `alive.lock` for its lifetime; the OS releases it
//! on any exit, a crash included, and nothing else is ever asked.
//!
//! Three locations belong to a session, and only these:
//! - `.goofi/sessions/<id>/` — the record: `session.json` (id, url) and `alive.lock`.
//! - `<temp>/goofi-system/<id>/` — ephemeral resources (the iceoryx2 root), safe to sweep whenever
//!   the record it `session` references is not alive.
//! - `<temp>/goofi-workspaces/<id>/` — the patch workspace, removed on a CLEAN shutdown only: what
//!   a crash leaves there is the user's work.

use std::fs::{self, File};
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

fn sessions_dir() -> PathBuf {
    home::dir().join("sessions")
}

/// The record directory of session `id`.
pub fn entry(id: &str) -> PathBuf {
    sessions_dir().join(id)
}

/// Where every session's ephemeral directory lives. A FIXED short path on unix, not `$TMPDIR`:
/// iceoryx2 names a unix domain socket under it, and that path is capped at 108 bytes — which a
/// macOS `$TMPDIR` alone spends half of.
pub fn system_base() -> PathBuf {
    if cfg!(unix) {
        PathBuf::from("/tmp/goofi-system")
    } else {
        std::env::temp_dir().join("goofi-system")
    }
}

/// The ephemeral directory of session `id`.
pub fn system_dir(id: &str) -> PathBuf {
    system_base().join(id)
}

/// The workspace directory of session `id`.
pub fn workspace_dir(id: &str) -> PathBuf {
    std::env::temp_dir().join("goofi-workspaces").join(id)
}

/// A 64-bit random id, hex. Short on purpose: it is a path segment under the socket cap above.
pub fn fresh_id() -> String {
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).expect("the OS random source");
    format!("{:016x}", u64::from_be_bytes(nonce))
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
        let s = Session { id: self.id.clone(), url: url.to_string() };
        let part = entry(&self.id).join("session.json.part");
        let _ = fs::write(&part, serde_json::to_vec_pretty(&s).expect("two strings"))
            .and_then(|()| fs::rename(&part, entry(&self.id).join("session.json")));
    }
}

impl Drop for Held {
    /// A clean shutdown: the record first, so no reader sees the session as alive while its
    /// resources go, then the ephemeral directory. The workspace is the owner's own call.
    fn drop(&mut self) {
        drop(self.lock.take());
        let _ = fs::remove_dir_all(entry(&self.id));
        let _ = fs::remove_dir_all(system_dir(&self.id));
    }
}

/// Hold a new session under `id`. The record is built under a part name and renamed into place
/// with its lock already held, so no reader ever sees an entry that is neither locked nor dead.
pub fn hold(id: &str) -> io::Result<Held> {
    let at = entry(id);
    let part = sessions_dir().join(format!("{id}.part"));
    let _ = fs::remove_dir_all(&part);
    fs::create_dir_all(&part)?;
    let lock = File::create(part.join("alive.lock"))?;
    lock.lock()?;
    fs::write(part.join("session.json"), serde_json::to_vec_pretty(&Session { id: id.into(), url: String::new() })?)?;
    fs::rename(&part, &at)?;
    let system = system_dir(id);
    fs::create_dir_all(&system)?;
    fs::write(system.join("session"), at.to_string_lossy().as_bytes())?;
    Ok(Held { id: id.to_string(), lock: Some(lock) })
}

/// Whether the session recorded at `entry` is alive: its lock is held by a living process. A
/// missing lock file is a dead session; a lock this process can take is one nobody holds.
pub fn alive_at(entry: &Path) -> bool {
    let Ok(file) = File::options().read(true).write(true).open(entry.join("alive.lock")) else {
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
    alive_at(&entry(id))
}

/// Remove a directory tree this session owns. Platform-specific where iceoryx2's own files refuse
/// their owner — see the transport crate, which supplies that removal.
pub type RemoveTree = fn(&Path);

/// Every alive session, its record read; dead records are swept as they are met, together with
/// the ephemeral directory each names. `remove` is how a tree goes.
pub fn sessions(remove: RemoveTree) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(sessions_dir()) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            let _ = fs::remove_file(&path);
            continue;
        }
        if !alive_at(&path) {
            if let Some(id) = path.file_name().and_then(|n| n.to_str()) {
                remove(&system_dir(id));
            }
            remove(&path);
            continue;
        }
        let parsed = fs::read(path.join("session.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Session>(&b).ok());
        if let Some(s) = parsed {
            out.push(s);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Sweep every ephemeral directory whose referenced record is not alive — a record that was
/// already swept, a home that is gone, or a session that died. A directory with no reference yet
/// is being born and is left alone.
pub fn sweep_dead_system(remove: RemoveTree) {
    let Ok(entries) = fs::read_dir(system_base()) else { return };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Ok(reference) = fs::read_to_string(dir.join("session")) else { continue };
        if !alive_at(Path::new(reference.trim())) {
            remove(&dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_session_is_alive_until_it_is_dropped_and_a_dead_one_is_swept() {
        home::test_home();
        let remove: RemoveTree = |p| {
            let _ = fs::remove_dir_all(p);
        };

        let held = hold("abc").unwrap();
        held.record_url("http://127.0.0.1:9999");
        assert!(alive("abc"), "held from within the same process still reads alive");
        assert_eq!(sessions(remove), vec![Session { id: "abc".into(), url: "http://127.0.0.1:9999".into() }]);
        assert!(system_dir("abc").join("session").exists());

        // A dead record: the lock file exists and nobody holds it.
        fs::create_dir_all(entry("gone")).unwrap();
        File::create(entry("gone").join("alive.lock")).unwrap();
        fs::create_dir_all(system_dir("gone")).unwrap();
        fs::write(system_dir("gone").join("session"), entry("gone").to_string_lossy().as_bytes()).unwrap();
        assert!(!alive("gone"));
        assert_eq!(sessions(remove).len(), 1, "the dead record is swept, the live one stays");
        assert!(!entry("gone").exists() && !system_dir("gone").exists());

        // A system dir whose record is gone entirely is swept by the boot pass.
        fs::create_dir_all(system_dir("orphan")).unwrap();
        fs::write(system_dir("orphan").join("session"), entry("orphan").to_string_lossy().as_bytes()).unwrap();
        sweep_dead_system(remove);
        assert!(!system_dir("orphan").exists());
        assert!(system_dir("abc").exists(), "the live one is untouched");

        drop(held);
        assert!(!alive("abc") && !entry("abc").exists() && !system_dir("abc").exists());
    }
}
