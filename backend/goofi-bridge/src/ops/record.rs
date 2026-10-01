//! Recording: arming outputs on the patch, and the one recorder every engine writes to.

use serde_json::{json, Value};

use super::{op, EffectOp, Endpoint, NoArgs, ReadOp, WriteOp};
use crate::{vocab, AppState, Caller, Event, Txn};
use goofi_graph::{Graph, Uid};

op!(Status, "record status", 0, NoArgs,
    "Whether a recording runs, where it writes, and every armed stream's health: frames written, frames dropped, and how full its buffer is. The one read a panel, an agent and a test all use.",
    "{running: bool, folder: string | null, elapsed: number | null, streams: [{node, slot, engine, file, frames, dropped, fill}], error: null} — `error` is what the `record_changed` event puts a failed finalize in; a status read always answers null");

op!(Arm, "record arm", 1, ArmArgs {
    pub output: Endpoint,
},
    "Capture this output slot, addressed `node/slot`. Arming rides the node's own record, so it is undone, saved and copied with the node, and a re-wire elsewhere cannot disarm it. Arming while a recording runs opens a new file for that stream at once. `changed` is false when the slot was already armed, which records no command.",
    "{ok: true, changed: bool}");

op!(Quality, "record quality", 2, QualityArgs {
    pub output: Endpoint,
    pub quality: String,
},
    "Set an armed video output's quality: small, high (default), or very_high. Saved with the patch and undoable. During recording, a change starts a new video file.",
    "{ok: true, changed: bool}");

op!(Disarm, "record disarm", 1, DisarmArgs {
    pub output: Endpoint,
},
    "Stop capturing this output slot. A file open for it is closed and named in the manifest. `changed` is false when the slot was not armed, which records no command.",
    "{ok: true, changed: bool}");

op!(Start, "record start", 1, StartArgs {
    pub name: Option<String>,
    pub root: Option<String>,
    pub annotations: Option<Value>,
},
    "Begin a recording. `name` names the folder, which otherwise carries the UTC of this moment; `root` overrides the recordings folder for this one recording. Either one absent is read from `variables.record.name` and `variables.record.root`. Refused when nothing is armed, and refused when one already runs.",
    "{folder: string}");

op!(Stop, "record stop", 0, NoArgs,
    "End the recording: every file is closed and the manifest is finalized.",
    "{folder: string}");

/// The recorder's name for one armed output slot: the node's identity, plus the engine behind it.
pub(crate) fn stream_id(g: &Graph, uid: Uid, slot: &str) -> goofi_record::StreamId {
    let engine = g.node_type(uid).and_then(|ty| g.type_engine(&ty)).unwrap_or("signal");
    goofi_record::StreamId { uid, node: crate::named(g, uid), slot: slot.to_string(), engine }
}

/// The slot half of an endpoint, for a label: the node is where the user is looking already.
fn slot_of(output: &Endpoint) -> &str {
    output.0.rsplit('/').next().unwrap_or(&output.0)
}

fn set_armed(tx: &mut Txn, output: &Endpoint, arm: bool) -> Result<Value, String> {
    let (uid, slot) = output.resolve(&tx.g, "output")?;
    let slot = vocab::resolve_slot(&tx.g, uid, &slot)?;
    let mut record = tx.g.recorded(uid).unwrap_or(&[]).to_vec();
    let held = record.iter().position(|s| s.slot == slot);
    match (arm, held) {
        (true, None) => record.push(goofi_core::record::RecordedOutput { slot: slot.clone(), quality: Default::default(), serial: 0 }),
        (false, Some(i)) => {
            record.remove(i);
        }
        _ => return Ok(json!({ "ok": true, "changed": false })),
    }
    // The op arms and nothing else. Each engine's own drain observes the settled set and closes
    // the stream after it has read what was already delivered — closing here raced that.
    tx.apply(goofi_graph::Command::SetRecorded { uid, record })?;
    Ok(json!({ "ok": true, "changed": true }))
}

impl ReadOp for Status {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(record_state_at(tx.state, tx.g.time().now()))
    }
}

impl WriteOp for Arm {
    fn run(tx: &mut Txn, a: ArmArgs) -> Result<Value, String> {
        set_armed(tx, &a.output, true)
    }

    fn label(a: &ArmArgs, _: &Value) -> String {
        format!("Arm {}", slot_of(&a.output))
    }
}

impl WriteOp for Disarm {
    fn run(tx: &mut Txn, a: DisarmArgs) -> Result<Value, String> {
        set_armed(tx, &a.output, false)
    }

    fn label(a: &DisarmArgs, _: &Value) -> String {
        format!("Disarm {}", slot_of(&a.output))
    }
}

impl WriteOp for Quality {
    fn run(tx: &mut Txn, a: QualityArgs) -> Result<Value, String> {
        let (uid, slot) = a.output.resolve(&tx.g, "output")?;
        let slot = vocab::resolve_slot(&tx.g, uid, &slot)?;
        if tx.g.node_type(uid).and_then(|ty| tx.g.type_engine(&ty)) != Some("graphics") {
            return Err("quality settings apply to video outputs only".into());
        }
        let quality = serde_json::from_value::<goofi_core::record::VideoQuality>(json!(a.quality))
            .map_err(|_| "expected small, high, or very_high")?;
        let mut record = tx.g.recorded(uid).unwrap_or(&[]).to_vec();
        let output = record.iter_mut().find(|output| output.slot == slot).ok_or("arm the output first")?;
        if output.quality == quality {
            return Ok(json!({ "ok": true, "changed": false }));
        }
        output.quality = quality;
        tx.apply(goofi_graph::Command::SetRecorded { uid, record })?;
        Ok(json!({ "ok": true, "changed": true }))
    }

    fn label(a: &QualityArgs, _: &Value) -> String {
        format!("Set {} recording quality", slot_of(&a.output))
    }
}

/// A `record start` argument, given or otherwise read from `variables.record.<key>`.
fn record_arg(g: &Graph, given: Option<&str>, key: &str) -> Option<String> {
    given.filter(|s| !s.is_empty()).map(str::to_string).or_else(|| match g.variables().get(&format!("record.{key}")) {
        Some(goofi_core::variables::VariableValue::Str(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    })
}

impl EffectOp for Start {
    fn run(state: &AppState, a: StartArgs, _: &Caller) -> Result<Value, String> {
        let g = state.graph.lock();
        let armed: Vec<Uid> = g.all_uids().into_iter().filter(|u| !g.recorded(*u).unwrap_or(&[]).is_empty()).collect();
        if armed.is_empty() {
            return Err("nothing is armed — `record arm <node>/<slot>` first".into());
        }
        // A recording is many streams across three engines, so a missing video encoder costs the
        // graphics stream alone — unless every armed stream is one, which would record nothing.
        if armed.iter().all(|u| stream_id(&g, *u, "").engine == "graphics") {
            state.recorder.can_encode()?;
        }
        let root = record_arg(&g, a.root.as_deref(), "root")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(goofi_supervisor::home::recordings);
        let name = record_arg(&g, a.name.as_deref(), "name").unwrap_or_default();
        let patch = state.save_path().map(std::path::PathBuf::from);
        // Capture preparation and the stream drain need the graph to make progress.
        drop(g);
        let folder = state
            .recorder
            .start(&root, &name, patch.as_deref(), a.annotations.as_ref())
            ?;
        state.events.send(record_changed(state));
        Ok(json!({ "folder": folder.to_string_lossy() }))
    }
}

impl EffectOp for Stop {
    fn run(state: &AppState, _: NoArgs, _: &Caller) -> Result<Value, String> {
        let folder = state.recorder.stop()?.ok_or("no recording runs")?;
        state.events.send(record_changed(state));
        Ok(json!({ "folder": folder.to_string_lossy() }))
    }
}

/// The session's recording state — RUNTIME, so it rides this read and the event, never the document.
pub(crate) fn record_state(state: &AppState) -> Value {
    let now = state.graph.lock().time().now();
    record_state_at(state, now)
}

/// Project recorder status without taking the graph lock.
pub(crate) fn record_state_at(state: &AppState, now: f64) -> Value {
    let s = state.recorder.status();
    let elapsed = s.started.map(|t0| now - t0);
    json!({
        "running": s.running,
        "folder": s.folder.map(|f| f.to_string_lossy().into_owned()),
        "elapsed": elapsed,
        "streams": s.streams,
        "error": Value::Null,
    })
}

pub(crate) fn record_changed(state: &AppState) -> Event {
    Event::RecordChanged(record_state(state))
}
