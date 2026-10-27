//! One history of snapshots, for every client, tab, shell and agent alike. A write pushes the
//! snapshot from before it; undo and redo swap a snapshot in and let the settle deliver what
//! follows: births, removals, re-derivations, the document's ops. A drag in flight is a layer
//! over the committed state: its steps publish without pushing, a write that lands meanwhile
//! records the state without the drag and the drag then stands on what the write left, and the
//! commit that ends the drag pushes the snapshot from before its first step.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::Value;

use crate::{Graph, Patch, Uid};

/// The patch and the variable store's configuration as they stood at one instant: pointers to
/// what the patch shares, and the store's entries, whose values are shared too.
#[derive(Clone)]
pub struct Snapshot {
    pub(crate) patch: Patch,
    pub(crate) variables: goofi_core::variables::Snapshot,
}

/// What a write says of itself, for the entry it becomes.
pub struct Step {
    /// What the button says it takes back.
    pub label: String,
    /// Where the actor was when it did it, opaque to the manager; handed back on the flip.
    pub context: Value,
    /// A token the actor put on several calls it meant as one step; a neighbour sharing it merges.
    pub group: Option<String>,
}

/// One step, and what an undo of it puts back.
struct Entry {
    step: Step,
    before: Snapshot,
}

/// The records a drag's steps moved: a node, a variable, or a root held whole. What the drag's
/// way back puts back, and what a write landing meanwhile keeps out of its own.
#[derive(Default)]
struct Cut {
    nodes: HashSet<Uid>,
    links: bool,
    arrangement: bool,
    machines: bool,
    variables: Vec<String>,
    groups: bool,
}

impl Cut {
    /// What `after` holds differently from `before`, added to the cut.
    fn widen(&mut self, before: &Snapshot, after: &Snapshot) {
        let (p, q) = (&before.patch, &after.patch);
        for uid in p.nodes.keys().chain(q.nodes.keys()) {
            if !matches!((p.nodes.get(uid), q.nodes.get(uid)), (Some(a), Some(b)) if Arc::ptr_eq(a, b)) {
                self.nodes.insert(*uid);
            }
        }
        self.links |= !Arc::ptr_eq(&p.links, &q.links);
        self.arrangement |= !Arc::ptr_eq(&p.arrangement, &q.arrangement);
        self.machines |= !Arc::ptr_eq(&p.machines, &q.machines);
        let (names, groups) = before.variables.differing(&after.variables);
        for name in names {
            if !self.variables.contains(&name) {
                self.variables.push(name);
            }
        }
        self.groups |= groups;
    }

    /// This cut less `other`'s records.
    fn minus(&self, other: &Cut) -> Cut {
        Cut {
            nodes: self.nodes.difference(&other.nodes).copied().collect(),
            links: self.links && !other.links,
            arrangement: self.arrangement && !other.arrangement,
            machines: self.machines && !other.machines,
            variables: self.variables.iter().filter(|n| !other.variables.contains(n)).cloned().collect(),
            groups: self.groups && !other.groups,
        }
    }

    /// `into`, with the cut's records as `from` holds them.
    fn take(&self, into: &mut Snapshot, from: &Snapshot) {
        for uid in &self.nodes {
            match from.patch.nodes.get(uid) {
                Some(e) => {
                    into.patch.nodes.insert(*uid, e.clone());
                }
                None => {
                    into.patch.nodes.shift_remove(uid);
                }
            }
        }
        if self.links {
            into.patch.links = from.patch.links.clone();
        }
        if self.arrangement {
            into.patch.arrangement = from.patch.arrangement.clone();
        }
        if self.machines {
            into.patch.machines = from.patch.machines.clone();
        }
        into.variables.take(&from.variables, &self.variables, self.groups);
    }
}

/// A drag in flight: the way back to before its first step, per actor and key, and the cut its
/// steps have moved so far.
struct Preview {
    actor: String,
    key: String,
    before: Snapshot,
    cut: Cut,
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

    /// A write landed, `before` to `after`: what an undo of `step` puts back is the snapshot from
    /// before it — from before the drag's first step, where `key` names the drag this write ends
    /// — without the drags in flight, which now stand on what the write left. A step sharing the
    /// last entry's `group` token joins it. The redo stack is spent.
    pub fn record(&mut self, actor: &str, key: Option<&str>, before: Snapshot, after: &Snapshot, step: Step) {
        let mut moved = Cut::default();
        moved.widen(&before, after);
        let before = key.and_then(|k| self.take_preview(actor, k)).unwrap_or(before);
        let before = self.committed(before);
        self.rebase(after, &moved);
        self.future.clear();
        if let (Some(token), Some(last)) = (&step.group, self.past.last_mut()) {
            if last.step.group.as_deref() == Some(token) {
                last.step.label = step.label;
                return;
            }
        }
        self.past.push(Entry { step, before });
    }

    /// A drag's step landed, `before` to `after`: the first step keeps the way back, which is the
    /// state without the other drags in flight; every step widens the cut.
    pub fn preview(&mut self, actor: &str, key: &str, before: Snapshot, after: &Snapshot) {
        if let Some(p) = self.previews.iter_mut().find(|p| p.actor == actor && p.key == key) {
            p.cut.widen(&before, after);
            return;
        }
        let mut cut = Cut::default();
        cut.widen(&before, after);
        let before = self.committed(before);
        self.previews.push(Preview { actor: actor.to_string(), key: key.to_string(), before, cut });
    }

    /// The commit that ends a drag: the snapshot from before its first step, if one is held.
    fn take_preview(&mut self, actor: &str, key: &str) -> Option<Snapshot> {
        let i = self.previews.iter().position(|p| p.actor == actor && p.key == key)?;
        Some(self.previews.remove(i).before)
    }

    /// `snapshot` with every drag in flight taken out: the committed state at that instant.
    fn committed(&self, mut snapshot: Snapshot) -> Snapshot {
        for p in &self.previews {
            p.cut.take(&mut snapshot, &p.before);
        }
        snapshot
    }

    /// The committed state moved to `after`, the change having `moved` its cut: each drag in
    /// flight now stands on it. A record the change moved stands as the change left it, under
    /// every drag; the rest of a drag's cut is still the way it was before its first step.
    fn rebase(&mut self, after: &Snapshot, moved: &Cut) {
        let befores: Vec<Snapshot> = (0..self.previews.len())
            .map(|i| {
                let mut before = after.clone();
                for (j, q) in self.previews.iter().enumerate() {
                    if j != i {
                        q.cut.minus(moved).take(&mut before, &q.before);
                    }
                }
                let p = &self.previews[i];
                p.cut.minus(moved).take(&mut before, &p.before);
                before
            })
            .collect();
        for (p, before) in self.previews.iter_mut().zip(befores) {
            p.before = before;
        }
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
        if mine.is_empty() {
            return false;
        }
        let mut back = g.snapshot();
        for p in &mine {
            p.cut.take(&mut back, &p.before);
        }
        g.restore(&back);
        true
    }

    /// Put back the snapshot from before the last step; `changed` is false with nothing to undo.
    pub fn undo(&mut self, g: &mut Graph, actor: &str) -> Flip {
        let reverted = self.end_previews(g, actor);
        let Some(entry) = self.past.pop() else { return Flip { changed: reverted, context: Value::Null } };
        let now = self.committed(g.snapshot());
        g.restore(&entry.before);
        let after = g.snapshot();
        let mut moved = Cut::default();
        moved.widen(&now, &after);
        self.rebase(&after, &moved);
        let context = entry.step.context.clone();
        self.future.push(Entry { before: now, ..entry });
        Flip { changed: true, context }
    }

    /// Put back the snapshot the last undo took away; `changed` is false with nothing to redo.
    pub fn redo(&mut self, g: &mut Graph, actor: &str) -> Flip {
        let reverted = self.end_previews(g, actor);
        let Some(entry) = self.future.pop() else { return Flip { changed: reverted, context: Value::Null } };
        let now = self.committed(g.snapshot());
        g.restore(&entry.before);
        let after = g.snapshot();
        let mut moved = Cut::default();
        moved.widen(&now, &after);
        self.rebase(&after, &moved);
        let context = entry.step.context.clone();
        self.past.push(Entry { before: now, ..entry });
        Flip { changed: true, context }
    }

    /// What the next undo and redo would take back, by label; `None` where there is none.
    pub fn labels(&self) -> (Option<String>, Option<String>) {
        (self.past.last().map(|e| e.step.label.clone()), self.future.last().map(|e| e.step.label.clone()))
    }
}
