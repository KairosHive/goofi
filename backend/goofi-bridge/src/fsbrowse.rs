//! Filesystem browsing for the Save/Load modal. Deliberately unjailed, and served without the
//! graph mutex. An unreadable directory lists EMPTY rather than erroring, so navigation never fails.

use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

/// The order a listing's rows come in, directories always first.
#[derive(Clone, Copy)]
pub enum Sort {
    Name,
    Modified,
    Size,
}

impl Sort {
    pub fn parse(s: &str) -> Result<Sort, String> {
        match s {
            "name" => Ok(Sort::Name),
            "modified" => Ok(Sort::Modified),
            "size" => Ok(Sort::Size),
            other => Err(format!("dir list: sort `{other}` is not one of name, modified, size")),
        }
    }
}

/// One directory level, shaped as the frontend's `DirListing`. A dot-name is left out unless
/// `hidden` asks for it — the browser drew none of them, and a home directory is mostly dotfiles.
pub fn list_dir(path: Option<&str>, hidden: bool, sort: Sort, reverse: bool) -> Value {
    let base = base_dir(path);
    let parent = base.parent().filter(|p| *p != base).map(display);
    json!({
        "path": display(&base),
        "parent": parent,
        "entries": entries(&base, hidden, sort, reverse),
        "roots": roots(),
    })
}

/// How many folders the sidebar offers under Recent.
const RECENT: usize = 5;

/// Put the folder of a patch just loaded or saved at the head of the recent list.
pub fn remember(patch: &str) {
    let Some(dir) = Path::new(patch).parent().map(display) else { return };
    let mut kept = vec![dir.clone()];
    kept.extend(recent().into_iter().filter(|d| *d != dir).take(RECENT - 1));
    let at = goofi_core::home::recent_folders();
    let part = at.with_extension(format!("{}.part", goofi_core::session::tag()));
    let _ = std::fs::create_dir_all(goofi_core::home::system())
        .and_then(|()| std::fs::write(&part, kept.join("\n")))
        .and_then(|()| std::fs::rename(&part, &at));
}

/// The recent folders, newest first, as stored; one gone from disk stays until pushed out.
fn recent() -> Vec<String> {
    let text = std::fs::read_to_string(goofi_core::home::recent_folders()).unwrap_or_default();
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

/// The recent folders that still exist, normalized as `path` is.
fn recent_dirs() -> Vec<PathBuf> {
    recent().iter().map(|d| normalize(Path::new(d))).filter(|d| d.is_dir()).collect()
}

/// Where a bare listing opens: the newest recent folder, else the working directory.
fn start() -> PathBuf {
    recent_dirs()
        .into_iter()
        .next()
        .or_else(|| std::env::current_dir().ok().map(|c| normalize(&c)))
        .unwrap_or_else(home)
}

/// Interpret a user-supplied path the way the browser does — `~` expanded, absolute, symlink-free.
pub fn resolve(path: &str) -> String {
    display(&normalize(&expand_tilde(path)))
}

/// The directory a request lands in, stepped up to the parent when the path names a file.
fn base_dir(path: Option<&str>) -> PathBuf {
    let base = match path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => normalize(&expand_tilde(p)),
        None => start(),
    };
    if base.is_file() {
        if let Some(parent) = base.parent() {
            return parent.to_path_buf();
        }
    }
    base
}

fn expand_tilde(path: &str) -> PathBuf {
    match path.strip_prefix('~') {
        // BOTH separators: `~\\x` leaves a rooted `\\x`, and `join` on one of those keeps the
        // drive and drops the home.
        Some(rest) => home().join(rest.trim_start_matches(['/', '\\'])),
        None => PathBuf::from(path),
    }
}

/// Absolute and symlink-free, including for a path not on disk yet: canonicalize its longest
/// existing ancestor and re-attach the rest.
fn normalize(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")).join(path)
    };
    if let Ok(real) = goofi_core::path::canonical(&abs) {
        return real;
    }
    // Resolve `.`/`..` before walking ancestors: `Path::file_name()` is None for `..`, which would
    // drop the component silently.
    let abs = lexical(&abs);
    let mut tail = Vec::new();
    let mut cur = abs.as_path();
    while let Some(parent) = cur.parent() {
        tail.push(cur.file_name().unwrap_or_default().to_os_string());
        if let Ok(mut real) = goofi_core::path::canonical(parent) {
            real.extend(tail.iter().rev());
            return real;
        }
        cur = parent;
    }
    abs
}

/// Resolve `.` and `..` textually, for paths that are NOT on disk.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn home() -> PathBuf {
    std::env::home_dir()
        .map(|h| normalize(&h))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
}

/// The one place a browsed path becomes a string for the client — `/`, on every platform.
fn display(path: &Path) -> String {
    goofi_core::path::to_slash(path)
}

/// The sidebar shortcuts, normalized through the same function as `path` so the "active"
/// highlight's string equality fires. A recent folder is labelled by its own name.
fn roots() -> Value {
    let mut fixed = vec![("Home".to_string(), home())];
    if let Ok(cwd) = std::env::current_dir() {
        let cwd = normalize(&cwd);
        if cwd != home() {
            fixed.push(("Working dir".to_string(), cwd));
        }
    }
    let recent: Vec<_> = recent_dirs()
        .into_iter()
        .filter(|d| fixed.iter().all(|(_, f)| f != d))
        .map(|d| (d.file_name().map_or_else(|| display(&d), |n| n.to_string_lossy().into_owned()), d))
        .collect();
    let row = |(label, path): (String, PathBuf), recent: bool| json!({ "label": label, "path": display(&path), "recent": recent });
    Value::Array(fixed.into_iter().map(|r| row(r, false)).chain(recent.into_iter().map(|r| row(r, true))).collect())
}

/// Directories first, then by `sort` with the case-insensitive name breaking ties; the browser
/// renders the array as given. `modified` is epoch milliseconds, and a directory has no `size`.
fn entries(base: &Path, hidden: bool, sort: Sort, reverse: bool) -> Value {
    let Ok(read) = std::fs::read_dir(base) else {
        return Value::Array(Vec::new());
    };
    let mut rows: Vec<(bool, u128, u64, String, Value)> = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        // `metadata()` follows symlinks (unlike `entry.file_type()`), so a link to a directory
        // browses as one.
        let Ok(meta) = path.metadata() else { continue };
        // A non-UTF-8 name cannot survive the JSON round trip, and lossy names collide.
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        if !hidden && name.starts_with('.') {
            continue;
        }
        let is_dir = meta.is_dir();
        let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        let size = (!is_dir).then_some(meta.len());
        let row = json!({
            "name": name,
            "path": display(&path),
            "kind": if is_dir { "dir" } else { "file" },
            "is_gfi": path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gfi")),
            "modified": modified.map(|d| d.as_millis() as u64),
            "size": size,
        });
        let millis = modified.map_or(0, |d| d.as_millis());
        rows.push((!is_dir, millis, size.unwrap_or(0), name.to_lowercase(), row));
    }
    rows.sort_by(|a, b| {
        let by = match sort {
            Sort::Name => std::cmp::Ordering::Equal,
            Sort::Modified => a.1.cmp(&b.1),
            Sort::Size => a.2.cmp(&b.2),
        };
        let by = by.then_with(|| a.3.cmp(&b.3));
        a.0.cmp(&b.0).then(if reverse { by.reverse() } else { by })
    });
    Value::Array(rows.into_iter().map(|row| row.4).collect())
}
