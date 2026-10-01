//! The vocabulary and the history: the registry read as data, completion, undo, redo, and the
//! batch that runs several steps as one.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{op, EffectOp, Handler, NoArgs, ReadOp};
use crate::{AppState, Txn};

// ---- op list (Read)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpListArgs {
    pub doc: Option<bool>,
}

op!(OpList, "op list", 0, OpListArgs, Value,
    "Every op this server speaks: its name, its arguments (`!` marks a required one) and its kind — a `write` is undoable and may ride in a batch, an `effect` runs alone. Every argument is reachable as `--name value`, which is all a caller needs to write one. What an op DOES is `<op> --help`; `--doc` answers the whole vocabulary explained, which is the manual and costs like one.",
    "{ops: [{op, args, kind}]}, each row also carrying {positional, doc, result} under `--doc`");

// ---- op complete (Read)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpCompleteArgs {
    pub line: Option<String>,
}

op!(OpComplete, "op complete", 1, OpCompleteArgs, Value,
    "What can come NEXT on a partial command line — the shell completion read. Each candidate is a word with a one-line doc: a group or op word mid-phrase, a flag once the op is named, or a value for a flag with a known vocabulary (a panel type, a live node's name). The line's last word, when partial, filters the candidates.",
    "{text: string} — one candidate per line, `word<TAB>doc`");

// ---- undo (Effect)
op!(Undo, "undo", 0, NoArgs, Value,
    "Undo this actor's last graph command. Each actor — a browser tab, a shell, the MCP — has its own stack.",
    "{changed: bool, can_undo: bool, can_redo: bool}");

// ---- redo (Effect)
op!(Redo, "redo", 0, NoArgs, Value,
    "Redo this actor's last undone graph command.",
    "{changed: bool, can_undo: bool, can_redo: bool}");

// ---- compound (Effect)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompoundArgs {
    pub ops: Value,
}

op!(Compound, "compound", 0, CompoundArgs, Value,
    "Run several steps in order as ONE undo step and one settled decision: viewers see no intermediate document, and the unsaved dot moves once. `ops` is a list of `{op, payload}`; a step is a read or an undoable write, and an effect is refused — it runs as its own call. A refused step takes back the ones that already landed, so the call either happens whole or not at all.\n\n\
               A read step sees the earlier steps' writes on the GRAPH — but the document settles only when the batch does, so `session state` and `session status` inside a batch answer the document the batch found. Read them after it, not inside it.",
    "the steps' own replies, as a bare JSON list in order");

impl ReadOp for OpList {
    /// The registry itself, as data a caller derives a whole client from.
    fn run(tx: &mut Txn, a: OpListArgs) -> Result<Value, String> {
        let doc = a.doc.unwrap_or(false);
        let ops: Vec<Value> = tx.state
            .ops()
            .iter()
            .map(|o| {
                let mut row = json!({ "op": o.name, "args": o.args(), "kind": o.kind().name() });
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
    fn run(state: &AppState, _: NoArgs, actor: &str) -> Result<Value, String> {
        let mut tx = Txn::begin(state, actor, false);
        let changed = tx.history.undo(&mut tx.g, actor)?;
        flipped(tx, changed)
    }
}

impl EffectOp for Redo {
    fn run(state: &AppState, _: NoArgs, actor: &str) -> Result<Value, String> {
        let mut tx = Txn::begin(state, actor, false);
        let changed = tx.history.redo(&mut tx.g, actor)?;
        flipped(tx, changed)
    }
}

/// A flip's reply and tail. Only a flip that CHANGED something raises the dot: an empty stack
/// is not an edit.
fn flipped(mut tx: Txn, changed: bool) -> Result<Value, String> {
    let result = json!({ "changed": changed, "can_undo": tx.history.can_undo(tx.actor), "can_redo": tx.history.can_redo(tx.actor) });
    if changed {
        tx.touch();
    }
    tx.commit();
    Ok(result)
}

impl EffectOp for Compound {
    /// Several steps as ONE undo step, decided from SETTLED state: the handlers run directly, so
    /// no step re-mirrors or dirties on its own — the batch does each exactly once when it
    /// settles, and viewers never see an intermediate document.
    fn run(state: &AppState, a: CompoundArgs, actor: &str) -> Result<Value, String> {
        let steps = a.ops.as_array().ok_or("compound: `ops` is a list of {op, payload}")?;
        // Every row is resolved BEFORE anything lands: a Read rides for its result, a Write can be
        // taken back, and an Effect owns consequences a rollback cannot reach — refused whole.
        let mut resolved = Vec::with_capacity(steps.len());
        for (i, step) in steps.iter().enumerate() {
            let name = step.get("op").and_then(|v| v.as_str()).unwrap_or_default();
            let op = state.find_op(name).ok_or_else(|| format!("compound: step {i}: unknown op `{name}`"))?;
            if state.plugins.has_hooks(name) {
                return Err(format!("compound: `{name}` uses a plugin and must run alone"));
            }
            if !matches!(op.handler, Handler::Read(_) | Handler::Write(_)) {
                return Err(format!(
                    "compound: step {i} `{name}` is not a step — a read or an undoable write \
                     rides a batch; an effect runs as the only command"
                ));
            }
            resolved.push((op, step.get("payload").cloned().unwrap_or_else(|| json!({}))));
        }
        // One transaction for every step: the graph is held throughout, so nothing is delivered or
        // projected between two steps, and a refused step drops it, which takes the others back.
        let mut tx = Txn::begin(state, actor, false);
        let mut results = Vec::with_capacity(resolved.len());
        let mut labels = Vec::new();
        for (i, (op, arg)) in resolved.iter().enumerate() {
            let ran = match op.handler {
                Handler::Read(f) => f(&mut tx, arg),
                Handler::Write(f) => f(&mut tx, arg).map(|(v, label)| {
                    labels.push(label);
                    v
                }),
                _ => unreachable!("resolved above"),
            };
            match ran {
                Ok(r) => results.push(r),
                Err(e) => return Err(format!("compound: step {i} `{}` was refused: {e}", op.name)),
            }
        }
        tx.label(match labels.as_slice() {
            [one] => one.clone(),
            many => format!("{} edits", many.len()),
        });
        tx.commit();
        // The steps' own replies, in order — a BARE list, the shape every batch door answers.
        Ok(Value::Array(results))
    }
}
