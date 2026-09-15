//! The one index of every resource this process holds open: a child process, a thread, an
//! iceoryx2 port, a file it will remove, a device. A resource is minted with a [`Lease`] and the
//! lease IS the entry — dropped, the entry goes — so the index is never a mirror that can drift
//! from what exists. `session status` answers it, and a shutdown reads it to know what is left.
//!
//! One index per process, rooted in the session the process runs under: an engine that becomes
//! a process of its own keeps one of these too, under the same session id.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

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

impl Lease {
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        ENTRIES.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.id);
    }
}

/// A value and its entry in the index, which goes when the value does. For a handle another
/// crate owns — a device stream, a MIDI port — that has no room of its own for a lease.
pub struct Leased<T> {
    pub value: T,
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
    ENTRIES.lock().unwrap_or_else(|e| e.into_inner()).insert(id, entry);
    Lease { id }
}

/// Everything held right now, oldest first within a kind, kinds in release order.
pub fn inventory() -> Vec<Entry> {
    let now = Instant::now();
    let mut out: Vec<Entry> = ENTRIES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .map(|e| Entry { held_s: now.duration_since(e.since).as_secs_f64(), ..e.clone() })
        .collect();
    out.sort_by_key(|e| (e.kind, e.id));
    out
}

/// How many of `kind` are held.
pub fn count(kind: Kind) -> usize {
    ENTRIES.lock().unwrap_or_else(|e| e.into_inner()).values().filter(|e| e.kind == kind).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lease_is_the_entry_and_goes_with_it() {
        let before = count(Kind::Path);
        let a = lease(Kind::Path, "/tmp/a");
        let b = lease(Kind::Child, "sleep 1");
        assert_eq!(count(Kind::Path), before + 1);
        let listed = inventory();
        let (ia, ib) = (
            listed.iter().position(|e| e.id == a.id()).unwrap(),
            listed.iter().position(|e| e.id == b.id()).unwrap(),
        );
        assert!(ib < ia, "children list before paths: the release order");
        assert_eq!(listed[ib].name, "sleep 1");
        drop(a);
        assert_eq!(count(Kind::Path), before);
        assert!(inventory().iter().any(|e| e.id == b.id()));
    }
}
