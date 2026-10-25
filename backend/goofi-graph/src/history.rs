//! One history of snapshots, for every client, tab, shell and agent alike. A write pushes the
//! snapshot from before it; undo and redo swap a snapshot in and let the settle deliver what
//! follows: births, removals, re-derivations, the document's ops. A preview publishes without
//! pushing; the commit that ends a drag pushes the snapshot from before the drag's first step.

use serde_json::Value;

use crate::{Graph, Patch};

/// The patch and the variable store's configuration as they stood at one instant: pointers to
/// what the patch shares, and the store's entries, whose values are shared too.
#[derive(Clone)]
pub struct Snapshot {
    pub(crate) patch: Patch,
    pub(crate) variables: goofi_core::variables::Snapshot,
}

/// One step: what an undo of it puts back, and what the button says it takes back.
struct Entry {
    label: String,
    /// Where the actor was when it did it, opaque to the manager; handed back on the flip.
    context: Value,
    /// A token the actor put on several calls it meant as one step; a neighbour sharing it merges.
    group: Option<String>,
    before: Snapshot,
}

/// A drag in flight: the way back to before its first step, per actor and key.
struct Preview {
    actor: String,
    key: String,
    before: Snapshot,
}

#[derive(Default)]
pub struct History {
    past: Vec<Entry>,
    future: Vec<Entry>,
    previews: Vec<Preview>,
}

/// What an undo or a redo did, for the reply that carries it.
#[derive(Debug, Default)]
pub struct Flip {
    pub changed: bool,
    /// The flipped entry's navigation context, for the client to restore.
    pub context: Value,
}

impl History {
    pub fn new() -> History {
        History::default()
    }

    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
        self.previews.clear();
    }

    /// A write landed: the snapshot from before it is what an undo puts back, under `label`, with
    /// the actor's context. A step sharing the last entry's `group` token joins it. The redo
    /// stack is spent.
    pub fn record(&mut self, before: Snapshot, label: String, context: Value, group: Option<String>) {
        self.future.clear();
        if let (Some(token), Some(last)) = (&group, self.past.last_mut()) {
            if last.group.as_deref() == Some(token) {
                last.label = label;
                return;
            }
        }
        self.past.push(Entry { label, context, group, before });
    }

    /// A drag's first step keeps the way back; the ones after it change nothing here.
    pub fn preview(&mut self, actor: &str, key: &str, before: Snapshot) {
        if !self.previews.iter().any(|p| p.actor == actor && p.key == key) {
            self.previews.push(Preview { actor: actor.to_string(), key: key.to_string(), before });
        }
    }

    /// The commit that ends a drag: the snapshot from before its first step, if one is held.
    pub fn take_preview(&mut self, actor: &str, key: &str) -> Option<Snapshot> {
        let i = self.previews.iter().position(|p| p.actor == actor && p.key == key)?;
        Some(self.previews.remove(i).before)
    }

    /// Take back the actor's drags in flight — what its socket's close does, and what an undo or
    /// a redo does first. Answers whether anything moved.
    pub fn end_previews(&mut self, g: &mut Graph, actor: &str) -> bool {
        let mut mine = Vec::new();
        let mut i = 0;
        while i < self.previews.len() {
            if self.previews[i].actor == actor {
                mine.push(self.previews.remove(i));
            } else {
                i += 1;
            }
        }
        let Some(first) = mine.into_iter().next() else { return false };
        g.restore(&first.before);
        true
    }

    /// Put back the snapshot from before the last step; `changed` is false with nothing to undo.
    pub fn undo(&mut self, g: &mut Graph, actor: &str) -> Flip {
        let reverted = self.end_previews(g, actor);
        let Some(entry) = self.past.pop() else { return Flip { changed: reverted, context: Value::Null } };
        let now = g.snapshot();
        g.restore(&entry.before);
        let context = entry.context.clone();
        self.future.push(Entry { before: now, ..entry });
        Flip { changed: true, context }
    }

    /// Put back the snapshot the last undo took away; `changed` is false with nothing to redo.
    pub fn redo(&mut self, g: &mut Graph, actor: &str) -> Flip {
        let reverted = self.end_previews(g, actor);
        let Some(entry) = self.future.pop() else { return Flip { changed: reverted, context: Value::Null } };
        let now = g.snapshot();
        g.restore(&entry.before);
        let context = entry.context.clone();
        self.past.push(Entry { before: now, ..entry });
        Flip { changed: true, context }
    }

    /// What the next undo and redo would take back, by label; `None` where there is none.
    pub fn labels(&self) -> (Option<String>, Option<String>) {
        (self.past.last().map(|e| e.label.clone()), self.future.last().map(|e| e.label.clone()))
    }
}
