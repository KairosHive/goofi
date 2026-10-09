//! `GraphDoc` — goofi's control-plane document, and the deltas that keep a browser replica equal
//! to it: path operations, each a `put` of one value at a path or a `del` of the path.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One step of a delta. A path descends the document's maps, `[root, key, …]`, to the value it
/// moves: a `put` lands a value there, a `del` removes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    Put { path: Vec<String>, value: Value },
    Del { path: Vec<String> },
}

/// The ops that take `before` to `after`, as deep as the maps go: a map whose kept keys stay in
/// order is descended, so a moved literal rides alone; any other value is re-sent whole. The roots
/// are named, not ordered, so every op has a root in its path.
pub fn diff_ops(before: &Value, after: &Value) -> Vec<Op> {
    let empty = Map::new();
    let (b, a) = (before.as_object().unwrap_or(&empty), after.as_object().unwrap_or(&empty));
    let mut ops = Vec::new();
    for (root, av) in a {
        match b.get(root) {
            Some(bv) => diff_into(&mut ops, vec![root.clone()], bv, av),
            None => ops.push(Op::Put { path: vec![root.clone()], value: av.clone() }),
        }
    }
    for root in b.keys().filter(|k| !a.contains_key(*k)) {
        ops.push(Op::Del { path: vec![root.clone()] });
    }
    ops
}

fn diff_into(ops: &mut Vec<Op>, path: Vec<String>, before: &Value, after: &Value) {
    let at = |key: &str| path.iter().cloned().chain([key.to_string()]).collect::<Vec<_>>();
    match (before.as_object(), after.as_object()) {
        // Per-key ops keep old keys in place and append new ones; any other order goes whole.
        (Some(b), Some(a)) if keeps_order(b, a) => {
            for (key, v) in a {
                match b.get(key) {
                    Some(bv) if bv == v => {}
                    Some(bv) => diff_into(ops, at(key), bv, v),
                    None => ops.push(Op::Put { path: at(key), value: v.clone() }),
                }
            }
            for key in b.keys().filter(|k| !a.contains_key(*k)) {
                ops.push(Op::Del { path: at(key) });
            }
        }
        _ if before == after => {}
        _ => ops.push(Op::Put { path, value: after.clone() }),
    }
}

/// Whether per-key ops on `before` leave its keys in `after`'s order.
fn keeps_order(before: &Map<String, Value>, after: &Map<String, Value>) -> bool {
    let kept = before.keys().filter(|k| after.contains_key(*k));
    let added = after.keys().filter(|k| !before.contains_key(*k));
    kept.chain(added).eq(after.keys())
}

/// Apply ops in place. A `put` makes the maps on its way; a `del` of what is absent is nothing.
pub fn apply_ops(target: &mut Value, ops: &[Op]) {
    for op in ops {
        match op {
            Op::Put { path, value } => {
                let mut cur = &mut *target;
                for seg in path {
                    if !cur.is_object() {
                        *cur = Value::Object(Map::new());
                    }
                    cur = cur.as_object_mut().expect("just made it an object").entry(seg.clone()).or_insert(Value::Null);
                }
                *cur = value.clone();
            }
            Op::Del { path } => {
                let Some((last, parents)) = path.split_last() else {
                    *target = Value::Object(Map::new());
                    continue;
                };
                let mut cur = Some(&mut *target);
                for seg in parents {
                    cur = cur.and_then(|c| c.get_mut(seg));
                }
                if let Some(m) = cur.and_then(Value::as_object_mut) {
                    m.shift_remove(last);
                }
            }
        }
    }
}

/// What a replica did with a delta.
#[derive(Debug, PartialEq, Eq)]
pub enum Receipt {
    Applied,
    Stale,
    /// This replica is missing everything between `at` and `from`, and can only be re-seeded.
    Gap { from: u64, at: u64 },
}

/// The control-plane document, and the version every change advances.
pub struct GraphDoc {
    state: Value,
    version: u64,
}

impl GraphDoc {
    pub fn new() -> GraphDoc {
        GraphDoc { state: Value::Object(Map::new()), version: 0 }
    }

    pub fn to_json(&self) -> Value {
        self.state.clone()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Take the document to `target`, answering the ops that get a replica there, or `None`
    /// when nothing changed; the version advances only on a real change.
    pub fn reconcile_root(&mut self, target: Value) -> Option<Vec<Op>> {
        let ops = diff_ops(&self.state, &target);
        if ops.is_empty() {
            return None;
        }
        self.state = target;
        self.version += 1;
        Some(ops)
    }

    /// Apply a delta a peer produced. A result already held is stale and skipped; one reaching
    /// forward of this replica is a gap and refused.
    pub fn apply_patch(&mut self, from: u64, to: u64, ops: &[Op]) -> Receipt {
        if to <= self.version {
            return Receipt::Stale;
        }
        if from != self.version {
            return Receipt::Gap { from, at: self.version };
        }
        apply_ops(&mut self.state, ops);
        self.version = to;
        Receipt::Applied
    }

    /// Adopt a peer's whole document.
    pub fn reset_to(&mut self, version: u64, state: Value) {
        self.state = state;
        self.version = version;
    }

    /// Read the JSON value at a path, or `None` if any segment is missing.
    pub fn read_at(&self, path: &[&str]) -> Option<Value> {
        let mut cur = &self.state;
        for seg in path {
            cur = cur.get(seg)?;
        }
        Some(cur.clone())
    }

    pub fn node_ids(&self) -> Vec<String> {
        self.state["nodes"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default()
    }
}

impl Default for GraphDoc {
    fn default() -> GraphDoc {
        GraphDoc::new()
    }
}
