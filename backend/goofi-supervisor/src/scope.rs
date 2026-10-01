//! The one index of every resource this process holds: a [`Lease`] IS the entry, so the index
//! cannot drift from what exists. One per process; `session status` lists it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::child::Child;
use crate::sync::Mutex;
use crate::worker::Worker;

/// What kind of thing a resource is, in the order a shutdown releases them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An OS process this one spawned.
    Child,
    /// A thread of this process that outlives the call that started it.
    Worker,
    /// An iceoryx2 node, with the ports it owns.
    Port,
    /// A file or directory this process made and will remove.
    Path,
    /// A hardware door: an audio stream, a MIDI port, a GPU device, a window loop.
    Device,
}

/// One held resource as the index sees it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Entry {
    pub id: u64,
    pub kind: Kind,
    pub name: String,
    /// Seconds this resource has been held.
    pub held_s: f64,
    #[serde(skip)]
    since: Instant,
}

static ENTRIES: Mutex<BTreeMap<u64, Entry>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU64 = AtomicU64::new(1);

/// One resource's presence in the index, for as long as this value lives. Held by the owner of
/// the resource, or inside the thread that is the resource.
#[must_use = "a lease dropped at once records nothing"]
pub struct Lease {
    id: u64,
}

impl Drop for Lease {
    fn drop(&mut self) {
        ENTRIES.lock().remove(&self.id);
    }
}

/// A value and its entry in the index, which goes when the value does. For a handle another
/// crate owns — a device stream, a MIDI port — that has no room of its own for a lease.
pub struct Leased<T> {
    value: T,
    _lease: Lease,
}

impl<T> std::ops::Deref for Leased<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

/// Enter `value` as a resource of `kind` named `name`.
pub fn leased<T>(kind: Kind, name: impl Into<String>, value: T) -> Leased<T> {
    Leased { value, _lease: lease(kind, name) }
}

/// Enter a resource of `kind` named `name`. The name is for a reader: a command line, a thread
/// name, a service name, a path.
pub fn lease(kind: Kind, name: impl Into<String>) -> Lease {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let entry = Entry { id, kind, name: name.into(), held_s: 0.0, since: Instant::now() };
    ENTRIES.lock().insert(id, entry);
    Lease { id }
}

/// Everything held right now, oldest first within a kind, kinds in release order.
pub fn inventory() -> Vec<Entry> {
    let now = Instant::now();
    let mut out: Vec<Entry> = ENTRIES
        .lock()
        .values()
        .map(|e| Entry { held_s: now.duration_since(e.since).as_secs_f64(), ..e.clone() })
        .collect();
    out.sort_by_key(|e| (e.kind, e.id));
    out
}

/// A path this process made: in the index while it lives, and removed when it goes.
pub struct PathLease {
    path: PathBuf,
    _lease: Lease,
}

impl PathLease {
    pub fn new(path: PathBuf) -> PathLease {
        let lease = lease(Kind::Path, path.display().to_string());
        PathLease { path, _lease: lease }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PathLease {
    fn drop(&mut self) {
        let _ = if self.path.is_dir() { std::fs::remove_dir_all(&self.path) } else { std::fs::remove_file(&self.path) };
    }
}

/// An owner of what one part of the process holds: the threads it started, the children it
/// spawned, and the steps that finish its work. `close` releases them in release order: the
/// finishes, then the children on one shared deadline, then the threads.
#[derive(Default)]
pub struct Scope {
    finishes: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    children: Mutex<Vec<Child>>,
    workers: Mutex<Vec<Worker>>,
}

impl Scope {
    /// A thread of this scope's own, joined at its close. One that ended is let go at once.
    pub fn adopt(&self, worker: Worker) {
        let mut workers = self.workers.lock();
        workers.retain(|w| !w.is_done());
        workers.push(worker);
    }

    pub fn adopt_child(&self, child: Child) {
        self.children.lock().push(child);
    }

    /// A step to run first at the close, before anything is stopped.
    pub fn finish(&self, step: impl FnOnce() + Send + 'static) {
        self.finishes.lock().push(Box::new(step));
    }

    /// Release everything, within `within` for the children and the threads together.
    pub fn close(&self, within: Duration) {
        let deadline = Instant::now() + within;
        for step in std::mem::take(&mut *self.finishes.lock()).into_iter().rev() {
            step();
        }
        for mut child in std::mem::take(&mut *self.children.lock()) {
            child.stop(deadline.saturating_duration_since(Instant::now()));
        }
        for worker in std::mem::take(&mut *self.workers.lock()) {
            let _ = worker.join_within(deadline.saturating_duration_since(Instant::now()));
        }
    }
}
