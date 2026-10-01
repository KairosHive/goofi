//! `GraphDoc` — goofi's control-plane document, and the deltas that keep a browser replica equal
//! to it: path operations, each a `put` of one value at a path or a `del` of the path.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One step of a delta. A path is `[root]` or `[root, key]`: the roots are maps keyed by uid,
/// link key or variable name, and the arrangement moves whole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    Put { path: Vec<String>, value: Value },
    Del { path: Vec<String> },
}

/// The ops that take `before` to `after`: an entry of a root map re-sent whole when it moved,
/// any other root re-sent whole. Shallow by design — a record is the unit a reader derives from.
pub fn diff_ops(before: &Value, after: &Value) -> Vec<Op> {
    let (Some(b), Some(a)) = (before.as_object(), after.as_object()) else {
        return vec![Op::Put { path: Vec::new(), value: after.clone() }];
    };
    let mut ops = Vec::new();
    for (root, av) in a {
        match (b.get(root), av) {
            // Per-key ops keep old keys in place and append new ones; any other order goes whole.
            (Some(Value::Object(bm)), Value::Object(am)) if !keeps_order(bm, am) => {
                ops.push(Op::Put { path: vec![root.clone()], value: av.clone() });
            }
            (Some(Value::Object(bm)), Value::Object(am)) => {
                for (key, v) in am {
                    if bm.get(key) != Some(v) {
                        ops.push(Op::Put { path: vec![root.clone(), key.clone()], value: v.clone() });
                    }
                }
                for key in bm.keys().filter(|k| !am.contains_key(*k)) {
                    ops.push(Op::Del { path: vec![root.clone(), key.clone()] });
                }
            }
            (Some(bv), _) if bv == av => {}
            _ => ops.push(Op::Put { path: vec![root.clone()], value: av.clone() }),
        }
    }
    for root in b.keys().filter(|k| !a.contains_key(*k)) {
        ops.push(Op::Del { path: vec![root.clone()] });
    }
    ops
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
