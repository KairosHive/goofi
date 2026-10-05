//! The vocabulary and the history: the registry read as data, completion, undo, redo, and the
//! batch that runs several steps as one.

use serde_json::{json, Value};

use super::{op, EffectOp, Handler, NoArgs, ReadOp};
use crate::{AppState, Caller, Txn};
use goofi_graph::{CommandHistory, Flip, Graph};

op!(OpList, "op list", 0, OpListArgs {
    pub doc: Option<bool>,
},
    "Every op this server speaks: its name, its arguments (`!` marks a required one) and its kind — a `write` is undoable and may ride in a batch, an `effect` runs alone. Every argument is reachable as `--name value`, which is all a caller needs to write one. What an op DOES is `<op> --help`; `--doc` answers the whole vocabulary explained, which is the manual and costs like one.",
    "{ops: [{op, args, kind}]}, each row also carrying {positional, doc, result} under `--doc`");

op!(OpComplete, "op complete", 1, OpCompleteArgs {
    pub line: Option<String>,
},
    "What can come NEXT on a partial command line — the shell completion read. Each candidate is a word with a one-line doc: a group or op word mid-phrase, a flag once the op is named, or a value for a flag with a known vocabulary (a panel type, a live node's name). The line's last word, when partial, filters the candidates.",
    "{text: string} — one candidate per line, `word<TAB>doc`");

op!(Undo, "undo", 0, NoArgs,
    "Undo this actor's last graph command. Each actor — a browser tab, a shell, the MCP — has its own stack.",
    "{changed: bool, context, stale: string | null, undo: string | null, redo: string | null} — `context` is what the actor sent with the step; `stale` names an entry that no longer applied and was dropped; `undo`/`redo` are the labels now on top");

op!(Redo, "redo", 0, NoArgs,
    "Redo this actor's last undone graph command.",
    "{changed: bool, context, stale: string | null, undo: string | null, redo: string | null}");

op!(Compound, "compound", 0, CompoundArgs {
    pub ops: Value,
},
    "Run several steps in order as ONE undo step and one settled decision: viewers see no intermediate document, and the unsaved dot moves once. `ops` is a list of `{op, payload}`; a step is a read or an undoable write, and an effect is refused — it runs as its own call. A refused step takes back the ones that already landed, so the call either happens whole or not at all.\n\n\
               A read step sees the earlier steps' writes on the GRAPH — but the document settles only when the batch does, so `session state` and `session status` inside a batch answer the document the batch found. Read them after it, not inside it.",
    "the steps' own replies, as a bare JSON list in order");

impl ReadOp for OpList {
    fn run(tx: &mut Txn, a: OpListArgs) -> Result<Value, String> {
        let doc = a.doc.unwrap_or(false);
        let ops: Vec<Value> = tx.state
            .ops()
            .iter()
            .map(|o| {
                let mut row = json!({ "op": o.name, "args": o.args(), "kind": o.handler.name() });
                if doc {
                    row["positional"] = json!(o.positional);
                    row["doc"] = json!(o.doc());
                    row["result"] = json!(o.result);
                    row["schema"] = o.schema();
                }
                row
            })
            .collect();
        Ok(json!({ "ops": ops }))
    }
}

impl ReadOp for OpComplete {
    fn run(tx: &mut Txn, a: OpCompleteArgs) -> Result<Value, String> {
        let rows: Vec<String> = crate::phrase::complete(&tx.state.ops(), Some(tx.state), a.line.as_deref().unwrap_or_default())
            .into_iter()
            .map(|(word, doc)| format!("{word}\t{doc}"))
            .collect();
        Ok(json!({ "text": rows.join("\n") }))
    }
}

impl EffectOp for Undo {
    fn run(state: &AppState, _: NoArgs, caller: &Caller) -> Result<Value, String> {
        flip(state, caller, CommandHistory::undo)
    }
}

impl EffectOp for Redo {
    fn run(state: &AppState, _: NoArgs, caller: &Caller) -> Result<Value, String> {
        flip(state, caller, CommandHistory::redo)
    }
}

/// A flip's reply and tail. Only a flip that CHANGED something raises the dot: an empty stack
/// is not an edit. The reply carries the flipped entry's context and the labels now on top.
fn flip(state: &AppState, caller: &Caller, f: fn(&mut CommandHistory, &mut Graph, &str) -> Flip) -> Result<Value, String> {
    let mut tx = Txn::begin(state, caller, false, true);
    let flip = f(&mut tx.history, &mut tx.g, &caller.actor);
    let (undo, redo) = tx.history.labels(&caller.actor);
    let result = json!({ "changed": flip.changed, "context": flip.context, "stale": flip.stale, "undo": undo, "redo": redo });
    if flip.changed {
        tx.touch();
    }
    tx.commit();
    Ok(result)
}

impl EffectOp for Compound {
    /// The handlers run directly, so no step re-mirrors or dirties on its own: the batch does
    /// each exactly once when it settles.
    fn run(state: &AppState, a: CompoundArgs, caller: &Caller) -> Result<Value, String> {
        let steps = a.ops.as_array().ok_or("`ops` is a list of {op, payload}")?;
        // Every row is resolved BEFORE anything lands: a Read rides for its result, a Write can be
        // taken back, and an Effect owns consequences a rollback cannot reach — refused whole.
        let mut resolved = Vec::with_capacity(steps.len());
        for (i, step) in steps.iter().enumerate() {
            let name = step.get("op").and_then(|v| v.as_str()).unwrap_or_default();
            let op = state.find_op(name).ok_or_else(|| format!("step {i}: unknown op `{name}`"))?;
            if state.plugins.has_hooks(name) {
                return Err(format!("`{name}` uses a plugin and must run alone"));
            }
            let (Handler::Read(f) | Handler::Write(f)) = op.handler else {
                return Err(format!(
                    "step {i} `{name}` is not a step — a read or an undoable write \
                     rides a batch; an effect runs as the only command"
                ));
            };
            resolved.push((op, f, step.get("payload").cloned().unwrap_or_else(|| json!({}))));
        }
        // One transaction for every step: the graph is held throughout, so nothing is delivered or
        // projected between two steps, and a refused step drops it, which takes the others back.
        let mut tx = Txn::begin(state, caller, false, true);
        let mut results = Vec::with_capacity(resolved.len());
        for (i, (op, f, arg)) in resolved.iter().enumerate() {
            match f(&mut tx, arg) {
                Ok(r) => results.push(r),
                Err(e) => return Err(format!("step {i} `{}` was refused: {e}", op.name)),
            }
        }
        tx.commit();
        // The steps' own replies, in order — a BARE list, the shape every batch door answers.
        Ok(Value::Array(results))
    }
}
