//! One goofi session owns every process-scoped resource; its held `<id>.alive` lock is the ONE
//! aliveness answer. Everything ephemeral — the lock, the record, the iceoryx2 root — lives in
//! one machine-wide directory every sweep reads, so a boot under any `GOOFI_HOME` sees the same
//! answer and never sweeps a live session. Only a crash's workspace is kept, under recovery.

use std::fs::{self, File};
use std::sync::OnceLock;
use std::io;
use std::path::{Path, PathBuf};

use crate::layout;

/// The env var a spawned process reads to JOIN its parent's session rather than hold its own.
pub const ENV: &str = "GOOFI_SESSION";

/// One session as its record spells it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Record {
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
/// leaves here is moved to `layout::recovery` by the next boot, so it outlives a reboot.
pub fn workspaces_base() -> PathBuf {
    std::env::temp_dir().join("goofi-workspaces")
}

/// The workspace directory of session `id`.
pub fn workspace_dir(id: &str) -> PathBuf {
    workspaces_base().join(id)
}

/// `bytes` random bytes, hex: the one random source every id and nonce is minted from.
pub fn nonce_hex(bytes: usize) -> Result<String, String> {
    let mut nonce = vec![0u8; bytes];
    getrandom::fill(&mut nonce).map_err(|e| format!("the OS random source: {e}"))?;
    Ok(nonce.iter().map(|b| format!("{b:02x}")).collect())
}

/// A 64-bit random session id. Short on purpose: it is a path segment under the socket cap above.
pub fn fresh_id() -> Result<String, String> {
    nonce_hex(8)
}

static ID: OnceLock<String> = OnceLock::new();

/// The session this process runs under — the first one held or joined — read-only, for what
/// a child is told and what a part file is named by.
pub fn id() -> Option<&'static str> {
    ID.get().map(String::as_str)
}

/// What names a part file written beside a cache entry: `s<session id>`, so the boot pass can
/// sweep a crash's leftover, and no bare content key reads as one. No session yet: `p<pid>`.
pub fn tag() -> String {
    id().map(|id| format!("s{id}")).unwrap_or_else(|| format!("p{}", std::process::id()))
}

/// Sweep the runtime: every part named by a dead session under the build and shipped trees, and
/// every other version's tree whole. Cargo's own `target`, `crates` and `sdk` are not walked.
pub fn sweep_runtime() {
    let runtime = layout::runtime();
    sweep_dead_parts(&runtime.build(), &["target", "crates", "sdk"], 5);
    sweep_dead_parts(&runtime.shipped(), &[], 5);
    let Ok(entries) = fs::read_dir(runtime.root()) else { return };
    for entry in entries.flatten() {
        if entry.path().is_dir() && runtime.is_other_version(&entry.file_name()) {
            let _ = fs::remove_dir_all(entry.path());
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
                remove_tree(&path);
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
        .find(|id| is_id(id))
}

/// Whether `s` has the shape of a session id.
fn is_id(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The session this process runs under: HELD — its `<id>.alive` lock is the one aliveness
/// answer — or JOINED, a parent's. Releasing a held one is idempotent and runs on drop.
pub struct Session {
    id: String,
    lock: Option<File>,
}

/// Serialize session publication and cleanup so a sweep cannot unlink a lock before it is taken.
fn lock_catalog() -> io::Result<File> {
    fs::create_dir_all(system_base())?;
    let file = File::options().read(true).write(true).create(true).truncate(false)
        .open(system_base().join("catalog.lock"))?;
    file.lock()?;
    Ok(file)
}

impl Session {
    /// Hold a fresh session. The lock is a sibling `<id>.alive` taken BEFORE the directory
    /// exists, so no sweep ever sees a directory that is neither locked nor dead.
    pub fn hold() -> Result<Session, String> {
        let id = fresh_id()?;
        let at = |what: &'static str| move |e: io::Error| format!("{what}: {e}");
        let _catalog = lock_catalog().map_err(at("lock the session catalog"))?;
        let lock = File::create(alive_path(&id)).map_err(at("create the lock"))?;
        lock.lock().map_err(at("take the lock"))?;
        let dir = system_dir(&id);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(at("create the directory"))?;
        write_record(&id, "").map_err(at("write the record"))?;
        let _ = ID.set(id.clone());
        Ok(Session { id, lock: Some(lock) })
    }

    /// Join the session `GOOFI_SESSION` names, when a living process holds it.
    pub fn join_from_env() -> Result<Session, String> {
        let id = std::env::var(ENV).map_err(|_| format!("{ENV} is not set: not started by goofi"))?;
        if !alive(&id) {
            return Err(format!("{ENV}={id} names no live session"));
        }
        let _ = ID.set(id.clone());
        Ok(Session { id, lock: None })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Record where this session serves. Written beside the record and renamed in, so a reader
    /// sees a whole file or none.
    pub fn record_url(&self, url: &str) {
        let _ = write_record(&self.id, url);
    }

    /// Release what a held session owns: the lock first, so no reader sees it alive while its
    /// resources go; then its directory, its shared memory, and an empty workspace parent.
    pub fn release(&mut self) {
        let Some(lock) = self.lock.take() else { return };
        drop(lock);
        let _ = fs::remove_file(alive_path(&self.id));
        remove_tree(&system_dir(&self.id));
        sweep_shared_memory(|owner| owner == self.id);
        // A non-empty workspace is somebody's work: `remove_dir` refuses it.
        let _ = fs::remove_dir(workspace_dir(&self.id));
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.release();
    }
}

fn write_record(id: &str, url: &str) -> io::Result<()> {
    let dir = system_dir(id);
    let part = dir.join("session.json.part");
    fs::write(&part, serde_json::to_vec_pretty(&Record { id: id.into(), url: url.into() })?)?;
    fs::rename(&part, dir.join("session.json"))
}

/// The lock file of session `id`, beside its directory.
fn alive_path(id: &str) -> PathBuf {
    system_base().join(format!("{id}.alive"))
}

/// Whether session `id` is alive: its lock is held by a living process. A missing lock file is
/// a dead session; a lock this process can take is nobody's.
pub fn alive(id: &str) -> bool {
    let Ok(file) = File::options().read(true).write(true).open(alive_path(id)) else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => false,
        Err(std::fs::TryLockError::WouldBlock) => true,
        // An error that is not the lock being held proves nothing; the record stays.
        Err(std::fs::TryLockError::Error(_)) => true,
    }
}

/// Every alive session on the machine, its record read. A directory whose lock nobody holds is
/// dead and is left alone here: [`sweep_dead`] removes it.
pub fn sessions() -> Vec<Record> {
    let Ok(entries) = fs::read_dir(system_base()) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        let id = entry.file_name().to_string_lossy().into_owned();
        if !dir.is_dir() || !alive(&id) {
            continue;
        }
        let parsed = fs::read(dir.join("session.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Record>(&b).ok());
        if let Some(s) = parsed {
            out.push(s);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// What a sweep removed: directories of dead sessions, and shared-memory segments they left.
#[derive(Clone, Copy, Debug, Default)]
pub struct Swept {
    pub directories: usize,
    pub segments: usize,
}

/// The boot pass: every entry under the session base no lock holds (a dead session's directory,
/// its lock file, a stray) and every segment of a dead session. Workspaces are the manager's.
pub fn sweep_dead() -> Swept {
    let mut swept = Swept::default();
    let Ok(_catalog) = lock_catalog() else { return swept };
    for entry in fs::read_dir(system_base()).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "catalog.lock" {
            continue;
        }
        if !alive(name.strip_suffix(".alive").unwrap_or(&name)) {
            let path = entry.path();
            swept.directories += usize::from(path.is_dir());
            remove_tree(&path);
        }
    }
    let mut known = std::collections::HashMap::new();
    swept.segments = sweep_shared_memory(|id| !*known.entry(id.to_string()).or_insert_with(|| alive(id)));
    swept
}

/// The name every shared-memory segment of a session carries. Segments live in one platform
/// directory iceoryx2 cannot be pointed away from, so the prefix is what makes them a session's.
/// Short, because it is also part of a unix socket path capped at 108 bytes.
pub fn shm_prefix(id: &str) -> String {
    format!("g{id}_")
}

/// The directory the iceoryx2 platform layer keeps its shared memory in — a compile-time
/// constant there, restated here because a sweep by prefix has to know where to look.
fn shm_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Temp\iceoryx2\shm")
    } else {
        PathBuf::from("/dev/shm")
    }
}

/// The session id a segment name carries, when the name is one of ours.
fn shm_owner(name: &str) -> Option<&str> {
    let id = name.strip_prefix('g')?.split_once('_')?.0;
    is_id(id).then_some(id)
}

/// Remove every segment of ours whose owning session `dead` says so of.
fn sweep_shared_memory(mut dead: impl FnMut(&str) -> bool) -> usize {
    let Ok(entries) = fs::read_dir(shm_dir()) else { return 0 };
    let mut swept = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if shm_owner(&name.to_string_lossy()).is_some_and(&mut dead) && fs::remove_file(entry.path()).is_ok() {
            swept += 1;
        }
    }
    swept
}

/// Remove a tree a session owns.
#[cfg(not(windows))]
pub fn remove_tree(path: &Path) {
    let _ = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
}

/// On Windows iceoryx2's files carry a protected DACL (eclipse-iceoryx/iceoryx2#1869), so each
/// path is taken back by name, contents first, then unlinked.
#[cfg(windows)]
pub fn remove_tree(path: &Path) {
    let dir = path.is_dir();
    if dir {
        for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
            remove_tree(&entry.path());
        }
    }
    grant_owner(path);
    let _ = if dir { std::fs::remove_dir(path) } else { std::fs::remove_file(path) };
}

/// Grant OWNER RIGHTS full control on ONE path, explicitly. Naming the file is the whole point: a
/// protected DACL is precisely one that refuses an inherited ace, so a grant on the parent — an
/// `icacls /T` walk, which this replaces — never reaches the file it was meant for.
#[cfg(windows)]
fn grant_owner(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, GENERIC_ALL, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSidToSidW, EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SET_ACCESS,
        SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, NO_INHERITANCE, PROTECTED_DACL_SECURITY_INFORMATION,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // `S-1-3-4` is OWNER RIGHTS: it grants the object's own owner, which is this user, and nobody else.
    let sid_text: Vec<u16> = "S-1-3-4".encode_utf16().chain(std::iter::once(0)).collect();
    let mut sid = std::ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(sid_text.as_ptr(), &mut sid) } == 0 {
        return;
    }
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL,
        grfAccessMode: SET_ACCESS,
        grfInheritance: NO_INHERITANCE,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
            ptstrName: sid.cast(),
        },
    };
    let mut acl: *mut ACL = std::ptr::null_mut();
    if unsafe { SetEntriesInAclW(1, &access, std::ptr::null(), &mut acl) } == ERROR_SUCCESS {
        unsafe {
            SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                acl,
                std::ptr::null(),
            );
        }
    }
    unsafe {
        LocalFree(acl.cast());
        LocalFree(sid.cast());
    }
}
