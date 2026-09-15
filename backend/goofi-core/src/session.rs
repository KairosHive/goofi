//! One goofi session owns every process-scoped resource; its held `alive.lock` is the ONE
//! aliveness answer. It has a record, an ephemeral directory, and a workspace a crash keeps.

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

fn sessions_dir() -> PathBuf {
    home::system().join("sessions")
}

/// The record directory of session `id`.
pub fn entry(id: &str) -> PathBuf {
    sessions_dir().join(id)
}

/// Where every session's ephemeral directory lives. A FIXED short path on unix, not `$TMPDIR`:
/// a unix socket path under it is capped at 108 bytes, half of which a macOS `$TMPDIR` spends.
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

/// Where every session's workspace lives.
pub fn workspaces_base() -> PathBuf {
    std::env::temp_dir().join("goofi-workspaces")
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
        let s = Session { id: self.id.clone(), url: url.to_string() };
        let part = entry(&self.id).join("session.json.part");
        let _ = fs::write(&part, serde_json::to_vec_pretty(&s).expect("two strings"))
            .and_then(|()| fs::rename(&part, entry(&self.id).join("session.json")));
    }
}

impl Drop for Held {
    /// A clean shutdown: the record goes first, so no reader sees the session alive while its
    /// resources go. The ephemeral directory is the transport's to remove.
    fn drop(&mut self) {
        drop(self.lock.take());
        let _ = fs::remove_dir_all(entry(&self.id));
        // The workspace parent, when the owner has already taken its mount away: an empty
        // directory is nobody's work. A non-empty one stays, and `remove_dir` refuses it.
        let _ = fs::remove_dir(workspace_dir(&self.id));
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
    fs::create_dir_all(system_dir(id))?;
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

/// Sweep every ephemeral directory whose session is not alive. The record is locked and in place
/// before its directory is made, so a directory with no alive record is dead, whatever it holds.
pub fn sweep_dead_system(remove: RemoveTree) {
    let Ok(entries) = fs::read_dir(system_base()) else { return };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Some(id) = dir.file_name().and_then(|n| n.to_str()) else { continue };
        if !alive(id) {
            remove(&dir);
        }
    }
}

/// Sweep every workspace parent that holds nothing and belongs to no living session. A crash
/// leaves a workspace behind on purpose; an EMPTY one carries no work and only clutters.
pub fn sweep_empty_workspaces() {
    let Ok(entries) = fs::read_dir(workspaces_base()) else { return };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Some(id) = dir.file_name().and_then(|n| n.to_str()) else { continue };
        if !alive(id) {
            let _ = fs::remove_dir(&dir);
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
        assert!(system_dir("abc").is_dir());

        // A dead record: the lock file exists and nobody holds it.
        fs::create_dir_all(entry("gone")).unwrap();
        File::create(entry("gone").join("alive.lock")).unwrap();
        fs::create_dir_all(system_dir("gone")).unwrap();
        assert!(!alive("gone"));
        assert_eq!(sessions(remove).len(), 1, "the dead record is swept, the live one stays");
        assert!(!entry("gone").exists() && !system_dir("gone").exists());

        // A system dir whose record is gone entirely is swept by the boot pass.
        fs::create_dir_all(system_dir("orphan").join("iox")).unwrap();
        sweep_dead_system(remove);
        assert!(!system_dir("orphan").exists());
        assert!(system_dir("abc").exists(), "the live one is untouched");

        // Workspace parents: an empty one goes with its session, a crash's non-empty one stays.
        fs::create_dir_all(workspace_dir("abc")).unwrap();
        fs::create_dir_all(workspace_dir("crashed").join("mount")).unwrap();
        fs::create_dir_all(workspace_dir("empty")).unwrap();
        sweep_empty_workspaces();
        assert!(workspace_dir("abc").exists(), "alive: untouched");
        assert!(workspace_dir("crashed").exists(), "a workspace with content is the user's");
        assert!(!workspace_dir("empty").exists(), "an empty dead one is swept");
        let _ = fs::remove_dir_all(workspace_dir("crashed"));
        // The caches: a part file a crash left, a work directory, and a tree of another version
        // go; a live session's part and this version's tree stay.
        let live = hold("0123456789abcdef").unwrap();
        let out = home::system().join("build").join("out").join("k");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join(".node.so.s0123456789abcdef"), b"").unwrap();
        fs::write(out.join(".node.so.sfedcba9876543210"), b"").unwrap();
        fs::create_dir_all(home::system().join("build").join("plugins").join("x").join("work-sfedcba9876543210-0")).unwrap();
        // A content key truncated to 16 hex digits is NOT a session: the shipped tree's is one.
        fs::create_dir_all(home::system().join("shipped").join("9.9.9").join("fedcba9876543210")).unwrap();
        fs::create_dir_all(home::system().join("build").join("sdk").join("0.0.1")).unwrap();
        fs::create_dir_all(home::system().join("build").join("sdk").join("9.9.9")).unwrap();
        fs::create_dir_all(home::system().join("shipped").join("9.9.9")).unwrap();
        fs::write(out.join("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef.json"), b"").unwrap();
        sweep_system("9.9.9");
        assert!(out.join(".node.so.s0123456789abcdef").exists(), "a live session's part stays");
        assert!(!out.join(".node.so.sfedcba9876543210").exists(), "a dead session's part goes");
        assert!(!home::system().join("build").join("plugins").join("x").join("work-sfedcba9876543210-0").exists());
        assert!(home::system().join("shipped").join("9.9.9").join("fedcba9876543210").exists(), "a content key stays");
        assert!(!home::system().join("build").join("sdk").join("0.0.1").exists(), "another version's tree goes");
        assert!(home::system().join("build").join("sdk").join("9.9.9").exists() && home::system().join("shipped").join("9.9.9").exists());
        assert!(out.join("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef.json").exists(), "a content key is not a session id");
        drop(live);

        drop(held);
        assert!(!alive("abc") && !entry("abc").exists());
        let _ = fs::remove_dir_all(system_dir("abc"));
        assert!(!workspace_dir("abc").exists(), "the empty workspace parent went with the session");
    }
}
