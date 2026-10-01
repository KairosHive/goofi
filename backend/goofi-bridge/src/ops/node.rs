//! One node, several nodes, and the wires between them.

use serde_json::{json, Value};

use super::{op, Any, EffectOp, Endpoint, NodeRef, ParamAddr, ReadOp, WriteOp};
use crate::{inspect, named, param_state_update, schemas, vocab, AppState, Caller, Event, Txn};
use goofi_graph::{Graph, Uid};

op!(State, "node state", 1, StateArgs {
    pub node: NodeRef,
    pub slot: Option<String>,
    pub params: Option<bool>,
    pub error: Option<bool>,
},
    "Read one node: its params (values, ranges, expression bindings), each output slot's name and kind and whether the node is emitting on it, and its error. `slot` narrows to one output; `--no-params` and `--no-error` drop a section. The FRAMES are not here: `node snapshot` reads one raw, and `/data/<node>/<slot>` streams them exactly as a viewer sees them.",
    "{text: string}");

op!(Snapshot, "node snapshot", 1, SnapshotArgs {
    pub output: Endpoint,
    pub raw: Option<bool>,
},
    "The output's latest frame, once — the analysis read, addressed `node/slot`. A facade or a boundary port resolves to the stream behind it, exactly as a viewer's does. It reads the cache the slot's reducer already keeps, so it never wakes the node and never touches the viewers' shared stream. An ARRAY answers its shape and its range, which is what tells silence from signal; `--raw` answers the numbers themselves as base64 NPY. STRING and TABLE answer plain JSON, a table's ARRAY members reading the same way. A slot asked about before anything was cached answers `{frame: null}` with the reason — asking is also what opens the slot's feed, so ask again after the node's next emit.",
    "{meta, shape, range: {min, max, mean}} for ARRAY, or {meta, npy_b64} under `--raw`; {meta, value} for STRING/TABLE; {frame: null, reason} before the first cached frame");

op!(Add, "node add", 1, AddArgs {
    #[serde(rename = "type")]
    pub ty: String,
    pub pos: Option<[f64; 2]>,
    pub name: Option<String>,
    pub inst_id: Option<NodeRef>,
    pub member_uid: Option<NodeRef>,
    pub param: Option<Vec<Value>>,
},
    "Create a node of `type`. `inst_id` births it inside that sub-patch; absent = root. `name` is a letter then letters or digits and not a Python keyword — one that is taken or illegal is refused, never silently swapped — and an omitted one is minted. Each `--param` is one birth param, self-addressed: `{\"name\": \"group/param\", …}` carrying `node param edit`'s fields — inside a JSON flag under bash, spell nested strings with ESCAPED double quotes (`\"nd(\\\"other\\\").out.sfreq\"`); a single-quoted `nd('x')` inside a single-quoted shell token loses its quotes silently. `member_uid` asks for a CHOSEN uid, so a caller rebuilding a graph it already knows — or wiring a batch it is still building — keeps its uid-keyed bindings; naming one the patch already holds answers with that node rather than a second one.\n\n\
                   The boundary types ({boundary_types}) create a PORT of the sub-patch named by `inst_id`, which is required for them. A port is a node in every way an op can see — it is named, moved, wired and removed by the same ops — but it never runs, so it takes no params. To COPY a node rather than build one, read it with `nodes copy` and put it back with `nodes paste`.",
    "{name, uid, input_slots, output_slots, params} — the node as born, so it can be wired and tuned without a follow-up read. `name` is what every op and nd() address it by; the uid is for a caller keying its own records.");

op!(Edit, "node edit", 1, EditArgs {
    pub node: NodeRef,
    pub name: Option<String>,
    pub pos: Option<[f64; 2]>,
    pub viewer: Option<Vec<Value>>,
},
    "Edit a node's own record: rename it, move it, set viewers — any of them, in one step and one undo. An omitted field is left alone. Params are `node param edit`'s. A sub-patch boundary port takes every field: its name is in the one namespace nd() reads, so a collision is refused exactly as a leaf's is, and its `value` slot takes a viewer exactly as a leaf's output does.\n\n\
                   A `name` is a letter then letters or digits, and not a Python keyword, for every kind of node. An expression reads a name as an ATTRIBUTE — a sub-patch's slot in `nd('chain').out.drain` — and a reference spells `name.slot`, so one that cannot be read there breaks every source naming it, and the rewrite that follows the NEXT rename can no longer find what it broke.\n\n\
                   Each `--viewer` is one slot's inline view, `{\"slot\": \"out\", \"kind\": …, \"settings\": …}`, merged slot by slot so only the slots named move; `{\"slot\": \"out\", \"clear\": true}` removes that slot's stored view. `kind` is one of: {viewer_kinds}.",
    "{ok: true}");

op!(ParamEdit, "node param edit", 2, ParamEditArgs {
    pub node: NodeRef,
    pub param: ParamAddr,
    pub value: Option<Any>,
    pub expression: Option<String>,
    pub reference: Option<String>,
    pub mode: Option<String>,
    pub triggers: Option<bool>,
},
    "Set ONE param, addressed `group/param`. `value` is coerced to the param's declared type — a fraction into an int rounds, a value of the wrong kind falls back to that type's zero; the declared min/max are the editor's range, NOT a clamp. A param has ONE active source, named by `mode`: `constant` (the value), `expression` (Python over nd(), variables and me, at control rate), or `reference` (one producer output spelled `node.slot`, no Python, at the producer's rate). Giving an `expression` or a `reference` implies its mode, so binding one is a single flag; the other two are RETAINED across a mode switch, an empty text clears that text (and the mode, if it was the active one), and a mode or trigger given alone edits what is already there. A `value` on a driven param switches it to `constant`. A reference's producer slot must match the param: a number or bool references any output that may feed an ARRAY input and that holds one element, or selects one flat element with `node.slot[index]`. A string references a STRING output.\n\n\
                       `triggers` defaults false, and that is almost always right: a binding re-evaluates on its own — when a referenced node emits, when a referenced param (`nd('x').params.<group>.<param>`, `me.params.…`) is edited, or on each of the node's own runs for a ref-less one — and the node reads the fresh value on its next normal run. `triggers: true` ALSO wakes the node's process() on every evaluation, making the reference its clock. Reach for it only when the node would otherwise not run (a trigger input with no wire into it) and you want the referenced node to drive it. Never on a ref-less expression (`t`, `variables.x`): that free-runs the node at its common.max_frequency.",
    "{value, error} — the value as STORED, with its bind error: a compile failure, an unknown producer, or a slot of the wrong kind.");

op!(ParamRequest, "node param request", 2, ParamRequestArgs {
    pub node: NodeRef,
    pub param: ParamAddr,
    pub request: String,
},
    "One request to a node's own thread, addressed `group/param`: `refresh` asks a refreshable string param (a device or stream picker) to re-enumerate its options — read them back with `node state`; `pulse` fires a pulse param once: a reset, a trigger, a clear. A request holds no value, so it is never an edit of the document, and this reply only says it was dispatched.",
    "{ok: true} — the request was dispatched to the node's own thread");

op!(Remove, "node remove", 1, RemoveArgs {
    pub node: NodeRef,
},
    "Delete whatever `node` names — a leaf, a boundary port or a whole sub-patch. A sub-patch takes everything inside it, to any depth: nested sub-patches, their members and their ports. A port of an enclosing sub-patch that exposed the deleted node STAYS, unwired — a port is a node, and it outlives what was behind it exactly as an unconnected node outlives the cable it lost. Idempotent: a uid naming no node succeeds having deleted nothing, and says so.",
    "{removed: bool} — false when it named nothing");

op!(Baseline, "node baseline", 1, BaselineArgs {
    pub node: NodeRef,
},
    "Take the touched filter's zero point to be what this node holds NOW — what the inspector's Clear button does. A param reads as touched when its value, its expression or its reference differs from that zero, and with no zero recorded the zero is the type's own declared default. A plugin's declared default is its FACTORY default, so loading a preset moves hundreds of params at once and the filter that exists to show the few in play fills with everything the preset moved; this is how that is reset to the few that follow. It edits no param and breaks no binding: an expression or a reference keeps driving exactly as it did, and the Expression and Reference filters still find it. Undoable.",
    "{ok: true, cleared: int} — how many params the new zero point covers");

op!(Restart, "node restart", 1, RestartArgs {
    pub node: NodeRef,
},
    "Respawn a node in place, keeping its uid, name, params, links and scope. Recovery, not an edit — `setup()` runs again.",
    "{ok: true}");

op!(Editor, "node editor", 1, EditorArgs {
    pub node: NodeRef,
    pub show: Option<bool>,
},
    "Open a node's own editor window — a plugin's GUI — on the machine goofi runs on, never in the page; `--no-show` closes it. Only a type whose palette row says `editor: true` has one, and a machine with no display has none.",
    "{changed: bool} — false when the editor was already open, or already closed");

op!(NodesInspect, "nodes inspect", 1, NodesInspectArgs {
    pub scope: Option<NodeRef>,
},
    "Read one scope as a mermaid flowchart — nodes, sub-patches, boundary ports and wires. A node's mermaid id is its NAME, which is what every other op takes. No arg = the root scope. Scope-wide and nothing more: what is broken is the whole patch's business, so `session status` answers that.",
    "{text: string}");

op!(NodesCopy, "nodes copy", 1, NodesCopyArgs {
    pub nodes: Vec<NodeRef>,
},
    "Read `nodes` and everything they hold — a sub-patch's members, their ports and the nested sub-patches below them, to any depth — as a self-contained fragment. A link rides only when BOTH its ends are in the fragment. The shape is the `.gfi`'s own, so a fragment is a patch's worth of nodes in the format a patch is written in, and `nodes paste` is what puts one back.",
    "{doc: {nodes, links}} — the fragment: node records keyed by uid, links keyed by `out.slot>in.slot`");

op!(NodesPaste, "nodes paste", 0, NodesPasteArgs {
    pub doc: Value,
    pub pos: Option<[f64; 2]>,
    pub inst_id: Option<NodeRef>,
},
    "Add a `nodes copy` fragment on FRESH uids and fresh names, so it lands beside whatever it was copied from rather than colliding with it. `pos` shifts the whole fragment by that offset; `inst_id` puts its roots inside that sub-patch, absent = root. A record naming a scope that is IN the fragment keeps the shape it was copied with. One command, so it is one undo step.",
    "{rename: {old_uid: new_uid}, warnings: string[]} — every record's uid in the fragment mapped to the one it was created at, and what the paste dropped");

op!(NodesGroup, "nodes group", 1, NodesGroupArgs {
    pub nodes: Vec<NodeRef>,
    pub pos: Option<[f64; 2]>,
},
    "Collapse nodes into a new sub-patch, returning it. `nodes` must share one scope, and one of them may itself be a sub-patch. Every wire that ends up CROSSING the new boundary mints a port to carry it, so nothing is disconnected and nothing stops running; a wire buried in a nested member mints a port there too, so it can reach the new boundary.",
    "{name, inst_id} — the sub-patch as born; `name` is what every op takes, `inst_id` the uid behind it");

op!(NodesUngroup, "nodes ungroup", 1, NodesUngroupArgs {
    pub subpatch: NodeRef,
},
    "Dissolve a sub-patch, returning its members to the parent scope. Its ports go with it and every wire they carried stands, because a port keeps its wire against the node behind it. A port of an ENCLOSING sub-patch that exposed one of these follows down onto what it exposed.",
    "{ok: true}");

op!(LinkAdd, "link add", 2, LinkAddArgs {
    pub from: Endpoint,
    pub to: Endpoint,
},
    "Wire `from` (an output, as `node/slot`) to `to` (an input). Refuses a dtype mismatch, naming both ends; refuses an end that names no node — so a reply means the wire is really there.\n\n\
                   A link never crosses a sub-patch boundary, and the two acts that look like it are ordinary links in different scopes. From the OUTSIDE you wire a node to the sub-patch's facade, naming a port's uid as the slot; the wire is stored against the PORT, whether or not anything is behind it yet. From the INSIDE you wire a port to a member, both of them in that sub-patch. A port carries one slot, `value`, on both of its sides.",
    "{from, to, dtype} — the wire as made, both ends named, with a facade endpoint resolved to the PORT it named.");

op!(LinkRemove, "link remove", 2, LinkRemoveArgs {
    pub from: Endpoint,
    pub to: Endpoint,
},
    "Remove one wire, addressed by both of its endpoints — a boundary port's inner wire included. Idempotent, like `node remove`.",
    "{removed: bool} — false when there was no such wire");

impl ReadOp for State {
    fn run(tx: &mut Txn, a: StateArgs) -> Result<Value, String> {
        let uid = a.node.resolve(&tx.g)?;
        let text = inspect::node(&tx.g, uid, a.slot.as_deref(), a.params.unwrap_or(true), a.error.unwrap_or(true))?;
        Ok(json!({ "text": text }))
    }
}

impl ReadOp for Snapshot {
    fn run(tx: &mut Txn, a: SnapshotArgs) -> Result<Value, String> {
        // The address resolves exactly as a viewer's does: a facade or a port names the stream
        // BEHIND it, and one with nothing behind it yet is the unwired state, never an error.
        let key = {
            let (uid, slot) = a.output.resolve(&tx.g, "output")?;
            if !tx.g.exists(uid) {
                return Err(format!("no node {}", named(&tx.g, uid)));
            }
            let slot = vocab::resolve_slot(&tx.g, uid, &slot)?;
            crate::stream_behind(&tx.g, uid, &slot)
        };
        let Some(key) = key else {
            return Ok(json!({
                "frame": null,
                "reason": "nothing is behind this port yet — wire its inside, then ask again",
            }));
        };
        match tx.state.reducers.latest(key) {
            Some(d) => Ok(frame_json(&d, a.raw.unwrap_or(false))),
            None => Ok(json!({
                "frame": null,
                "reason": "nothing cached for this slot yet — its feed is now open, so ask again \
                           after the node's next emit",
            })),
        }
    }
}

/// A frame as the snapshot answers it: an ARRAY as its shape and range unless `raw` asks for the
/// numbers, STRING as its text, TABLE recursing.
fn frame_json(d: &goofi_core::Data, raw: bool) -> Value {
    let meta = goofi_core::meta_json(d.meta());
    match d.value() {
        goofi_core::Value::Array(s) if raw => {
            use base64::Engine;
            let npy = base64::engine::general_purpose::STANDARD
                .encode(goofi_record::npy::bytes(s.shape(), s.as_bytes()));
            json!({ "meta": meta, "npy_b64": npy })
        }
        goofi_core::Value::Array(s) => json!({ "meta": meta, "shape": s.shape(), "range": range_json(s) }),
        goofi_core::Value::Str(s) => json!({ "meta": meta, "value": &**s }),
        goofi_core::Value::Texture(t) => json!({ "meta": meta, "texture_size": t.size() }),
        goofi_core::Value::Table(t) => json!({ "meta": meta,
            "value": Value::Object(t.iter().map(|(k, v)| (k.clone(), frame_json(v, raw))).collect()) }),
    }
}

/// What an array holds, in three numbers — enough to tell silence from signal without the frame.
fn range_json(s: &goofi_core::ArrayStore) -> Value {
    let (min, max, sum, n) = s.values().map(f64::from).fold((f64::INFINITY, f64::NEG_INFINITY, 0.0, 0u64), |(lo, hi, sum, n), v| {
        (lo.min(v), hi.max(v), sum + v, n + 1)
    });
    match n {
        0 => Value::Null,
        n => json!({ "min": min, "max": max, "mean": sum / n as f64 }),
    }
}

/// A chosen name is refused when it collides or when an expression could not read it as an
/// attribute, so a caller told nothing cannot get a node under a name it never asked for.
fn check_name(g: &Graph, name: &str, except: Option<Uid>) -> Result<(), String> {
    if g.name_taken(name, except) {
        return Err(format!("the name `{name}` is taken"));
    }
    if !goofi_core::variables::is_valid_name(name) {
        return Err(format!("`{name}` is not a legal name: {}", goofi_core::variables::NAME_RULE));
    }
    Ok(())
}

impl WriteOp for Add {
    fn run(tx: &mut Txn, a: AddArgs) -> Result<Value, String> {
        // A CHOSEN uid and name, so a caller reconstructing a known graph keeps its uid-keyed
        // bindings. Not the undo path, which is manager-owned.
        let restore = a.member_uid.and_then(|m| Uid::from_hex(&m.0));
        let name = a.name.filter(|n| !n.is_empty());
        if let Some(n) = &name {
            check_name(&tx.g, n, None)?;
        }
        // Never silently rooted on a bad `inst_id`: the canvas draws only the entered scope, so a
        // rooted node would be invisible exactly where the user placed it.
        let scope = a.inst_id.map(|s| s.resolve(&tx.g)).transpose()?;
        // Inline params are applied AFTER: RemoveNode's inverse captures the LIVE node, so an
        // undo→redo restores them without threading them through the command.
        let cmd = goofi_graph::Command::AddNode {
            type_name: a.ty,
            pos: a.pos.unwrap_or([0.0, 0.0]),
            uid: restore,
            name,
            params: None,
            sources: vec![],
            viewers: None,
            baseline: None,
            record: None,
            scope,
        };
        let uid = match tx.apply(cmd)? {
            goofi_graph::Outcome::Uid(u) => u,
            _ => return Err("no uid returned".into()),
        };
        // Applied UNDER THE GRAPH LOCK, so the node is born configured before the resync mirrors it
        // into the doc.
        if let Some(entries) = a.param {
            let bag = param_entries_bag(&entries)?;
            for cmd in goofi_graph::param_commands(&tx.g, uid, &bag)? {
                cmd.execute(&mut tx.g, goofi_graph::Ctx::Fresh)?;
            }
        }
        // A bare uid: the node itself arrives via the doc mirror.
        tx.emit(Event::NodeAdded { uid: uid.to_hex() });
        // The REPLY answers a caller with no doc replica. Read off the GRAPH, not a manifest: a
        // facade and a port have none.
        let slots = |v: Vec<(String, String, goofi_core::SlotType)>| {
            Value::Object(v.into_iter().map(|(k, _, t)| (k, json!(t.name()))).collect())
        };
        Ok(json!({
            "name": named(&tx.g, uid),
            "uid": uid.to_hex(),
            "input_slots": slots(tx.g.slots(uid, goofi_graph::subpatch::Dir::In)),
            "output_slots": slots(tx.g.slots(uid, goofi_graph::subpatch::Dir::Out)),
            "params": tx.g.params(uid).map(|p| schemas::param_value_map(&p)).unwrap_or_else(|| json!({})),
        }))
    }

    fn label(a: &AddArgs, _: &Value) -> String {
        format!("Add {}", goofi_node::split_type_id(&a.ty).1)
    }
}

/// `--param` entries `{name: "group/param", …fields}`, folded into the `{group: {param: fields}}`
/// bag the engine path reads.
fn param_entries_bag(list: &[Value]) -> Result<Value, String> {
    let mut bag = serde_json::Map::new();
    for e in list {
        let mut o = e.as_object().cloned().ok_or(r#"a param entry is {"name": "group/param", …}"#)?;
        let addr = match o.remove("name") {
            Some(Value::String(s)) => s,
            _ => return Err(r#"a param entry names its param: {"name": "group/param", …}"#.into()),
        };
        let (group, name) = addr.split_once('/').ok_or_else(|| format!("`{addr}` is not `group/param`"))?;
        let taken = bag
            .entry(group.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(name.to_string(), Value::Object(o));
        if taken.is_some() {
            return Err(format!("`{addr}` is named twice"));
        }
    }
    Ok(Value::Object(bag))
}

impl WriteOp for Edit {
    fn run(tx: &mut Txn, a: EditArgs) -> Result<Value, String> {
        let uid = a.node.resolve(&tx.g)?;
        // The rename command tolerates a collision as a no-op so a stale replay converges; the
        // user-facing error therefore belongs here, at the forward RPC.
        if let Some(n) = &a.name {
            check_name(&tx.g, n, Some(uid))?;
        }
        // Viewer entries MERGE slot by slot, and the command sets the whole blob so its inverse is
        // exact. Only the PATCH is checked: a stale slot already stored is inert.
        let viewers = match &a.viewer {
            Some(entries) => {
                let patch = viewer_entries_patch(entries)?;
                vocab::check_viewers(&tx.g, uid, &patch)?;
                let mut whole = tx.g.viewers(uid).cloned().ok_or("no such node")?;
                merge_into(&mut whole, &Value::Object(patch));
                Some(whole)
            }
            None => None,
        };
        if a.name.is_none() && a.pos.is_none() && viewers.is_none() {
            return Err("give a name, pos or viewer".into());
        }
        let out = tx.apply(goofi_graph::Command::EditNode { uid, name: a.name, pos: a.pos, viewers })?;
        // The runtime `error` is doc-invisible, so echo every referrer a rename rewrote.
        if let goofi_graph::Outcome::Nodes(referrers) = out {
            for r in referrers {
                tx.echo(r);
            }
        }
        Ok(json!({ "ok": true, "name": named(&tx.g, uid) }))
    }

    fn label(a: &EditArgs, o: &Value) -> String {
        let name = o["name"].as_str().unwrap_or(&a.node.0);
        match (&a.name, &a.pos) {
            (Some(_), _) => format!("Rename to {name}"),
            (None, Some(_)) => format!("Move {name}"),
            _ => format!("Set view on {name}"),
        }
    }
}

/// Merge `patch` into `target`: an object merges into an object, `null` removes a key, anything
/// else replaces — how a viewer edit folds into the blob a node wears.
fn merge_into(target: &mut Value, patch: &Value) {
    let Value::Object(p) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }
    let t = target.as_object_mut().expect("just made it an object");
    for (k, pv) in p {
        if pv.is_null() {
            t.shift_remove(k);
        } else {
            merge_into(t.entry(k.clone()).or_insert(Value::Null), pv);
        }
    }
}

/// `--viewer` entries `{slot, …view}` — or `{slot, clear: true}` — as the merge patch the stored
/// blob takes, where clearing is the patch's `null`.
fn viewer_entries_patch(list: &[Value]) -> Result<serde_json::Map<String, Value>, String> {
    let mut patch = serde_json::Map::new();
    for e in list {
        let mut o = e
            .as_object()
            .cloned()
            .ok_or(r#"a viewer entry is {"slot", …} or {"slot", "clear": true}"#)?;
        let slot = match o.remove("slot") {
            Some(Value::String(s)) => s,
            _ => return Err("a viewer entry names its slot".into()),
        };
        let taken = match o.remove("clear") {
            Some(Value::Bool(true)) => patch.insert(slot.clone(), Value::Null),
            None => patch.insert(slot.clone(), Value::Object(o)),
            _ => return Err("`clear` is only ever true — omit it to set".into()),
        };
        if taken.is_some() {
            return Err(format!("slot `{slot}` is named twice"));
        }
    }
    Ok(patch)
}

impl WriteOp for ParamEdit {
    fn run(tx: &mut Txn, a: ParamEditArgs) -> Result<Value, String> {
        let uid = a.node.resolve(&tx.g)?;
        let (group, name) = a.param.split()?;
        let mut entry = serde_json::Map::new();
        let given = [
            ("value", a.value.map(|v| v.0)),
            ("expression", a.expression.map(Value::String)),
            ("reference", a.reference.map(Value::String)),
            ("mode", a.mode.map(Value::String)),
            ("triggers", a.triggers.map(Value::Bool)),
        ];
        for (key, v) in given {
            if let Some(v) = v.filter(|v| !v.is_null()) {
                entry.insert(key.into(), v);
            }
        }
        // A value rides the doc patch and its error the live plane; a source edit changes the
        // descriptor, which only the echo carries.
        let describes = entry.keys().any(|k| k != "value");
        let bag = json!({ &group: { &name: entry } });
        let cmd = goofi_graph::param_commands(&tx.g, uid, &bag)
            ?
            .pop()
            .ok_or("nothing to change")?;
        tx.apply(cmd)?;
        if describes {
            tx.echo(uid);
        }
        Ok(json!({
            "value": tx.g.params(uid)
                .and_then(|p| goofi_node::param(&p, &group, &name).cloned())
                .map(|p| goofi_graph::param_value_json(&p)),
            "error": tx.g.param_source(uid, &group, &name).and_then(|s| s.error),
        }))
    }

    fn label(a: &ParamEditArgs, _: &Value) -> String {
        let param = a.param.0.rsplit('/').next().unwrap_or(&a.param.0);
        match (&a.expression, &a.reference) {
            (Some(_), _) | (_, Some(_)) => format!("Set {param} source"),
            _ => format!("Set {param}"),
        }
    }
}

impl EffectOp for ParamRequest {
    /// NOT a command: a request holds no state, so there is nothing to undo. A refresh's options
    /// do not ride the reply: `node state` reports them once the hook has run.
    fn run(state: &AppState, a: ParamRequestArgs, _: &Caller) -> Result<Value, String> {
        let kind: goofi_node::RequestKind = serde_json::from_value(json!(a.request))
            .map_err(|_| "`request` is `refresh` or `pulse`".to_string())?;
        {
            let mut g = state.graph.lock();
            let uid = a.node.resolve(&g)?;
            let (group, name) = a.param.split()?;
            g.request(uid, &group, &name, kind)?;
        }
        if kind == goofi_node::RequestKind::Refresh {
            crate::resync_and_broadcast(state);
        }
        Ok(json!({ "ok": true }))
    }
}

impl WriteOp for Remove {
    fn run(tx: &mut Txn, a: RemoveArgs) -> Result<Value, String> {
        let uid = a.node.resolve(&tx.g)?;
        // The command is idempotent, so a uid naming nothing succeeds; the reply says which of the
        // two happened.
        let existed = tx.g.exists(uid);
        let name = named(&tx.g, uid);
        tx.apply(goofi_graph::Command::RemoveNode { uid })?;
        Ok(json!({ "removed": existed, "name": name }))
    }

    fn label(_: &RemoveArgs, o: &Value) -> String {
        format!("Delete {}", o["name"].as_str().unwrap_or_default())
    }
}

impl WriteOp for Baseline {
    fn run(tx: &mut Txn, a: BaselineArgs) -> Result<Value, String> {
        let uid = a.node.resolve(&tx.g)?;
        // `None` means "snapshot what is there", which the command does under the history lock so
        // the inverse captures the blob it replaced.
        tx.apply(goofi_graph::Command::SetBaseline { uid, baseline: None })?;
        let cleared = tx.g.baseline(uid).and_then(|b| b.as_object()).map_or(0, serde_json::Map::len);
        Ok(json!({ "ok": true, "cleared": cleared, "name": named(&tx.g, uid) }))
    }

    fn label(_: &BaselineArgs, o: &Value) -> String {
        format!("Clear non-default on {}", o["name"].as_str().unwrap_or_default())
    }
}

impl EffectOp for Restart {
    /// Recovery, not an edit, so it is NOT routed through the command history.
    fn run(state: &AppState, a: RestartArgs, _: &Caller) -> Result<Value, String> {
        {
            let mut g = state.graph.lock();
            let uid = a.node.resolve(&g)?;
            g.restart_node(uid)?;
            // Pushed at once, so the red border lifts on the click rather than on the sweep.
            state.events.send(param_state_update(&g, uid, &[]));
        }
        crate::resync_and_broadcast(state);
        Ok(json!({ "ok": true }))
    }
}

impl EffectOp for Editor {
    /// Neither an edit nor recovery: a window on the machine goofi runs on, opened or closed.
    fn run(state: &AppState, a: EditorArgs, _: &Caller) -> Result<Value, String> {
        let action = {
            let mut g = state.graph.lock();
            let uid = a.node.resolve(&g)?;
            g.node_editor(uid, a.show.unwrap_or(true))?
        };
        Ok(json!({ "changed": action()? }))
    }
}

impl ReadOp for NodesInspect {
    fn run(tx: &mut Txn, a: NodesInspectArgs) -> Result<Value, String> {
        let scope = a.scope.map(|s| s.resolve(&tx.g)).transpose()?;
        Ok(json!({ "text": inspect::patch(&tx.g, scope)? }))
    }
}

impl ReadOp for NodesCopy {
    fn run(tx: &mut Txn, a: NodesCopyArgs) -> Result<Value, String> {
        let uids = NodeRef::resolve_all(&a.nodes, &tx.g)?;
        Ok(json!({ "doc": tx.g.fragment(&tx.g.subtree_of(&uids)) }))
    }
}

impl WriteOp for NodesPaste {
    const LABEL: &str = "Paste nodes";
    fn run(tx: &mut Txn, a: NodesPasteArgs) -> Result<Value, String> {
        let scope = a.inst_id.map(|s| s.resolve(&tx.g)).transpose()?;
        let doc = serde_json::from_value(a.doc).map_err(|e| format!("doc: {e}"))?;
        let (doc, warnings) = tx.g.admit(doc)?;
        let (cmd, rename) = tx.g.import_fragment(&doc, scope, a.pos.unwrap_or([0.0, 0.0]))?;
        tx.apply(cmd)?;
        for uid in rename.values() {
            tx.emit(Event::NodeAdded { uid: uid.clone() });
        }
        Ok(json!({ "rename": rename, "warnings": warnings }))
    }
}

impl WriteOp for NodesGroup {
    const LABEL: &str = "Group nodes";
    fn run(tx: &mut Txn, a: NodesGroupArgs) -> Result<Value, String> {
        let uids = NodeRef::resolve_all(&a.nodes, &tx.g)?;
        let out = tx.apply(goofi_graph::Command::Group { members: uids, pos: a.pos.unwrap_or([0.0, 0.0]), restore: None })?;
        let inst = match out {
            goofi_graph::Outcome::Uid(u) => u,
            _ => return Err("no scope uid returned".into()),
        };
        Ok(json!({ "name": named(&tx.g, inst), "inst_id": inst.to_hex() }))
    }
}

impl WriteOp for NodesUngroup {
    const LABEL: &str = "Ungroup";
    fn run(tx: &mut Txn, a: NodesUngroupArgs) -> Result<Value, String> {
        let inst = a.subpatch.resolve(&tx.g)?;
        tx.apply(goofi_graph::Command::Expand { scope: inst })?;
        Ok(json!({ "ok": true }))
    }
}

/// Resolve a link endpoint AND refuse one that names nothing wirable — the check a caller-initiated
/// `add_link` gets and a REPLAY does not, since a replay must converge rather than wedge the stack.
fn wirable_endpoint(g: &Graph, uid: Uid, slot: &str, which: &str) -> Result<(Uid, String), String> {
    let (node, slot) = g.normalise(uid, slot);
    if g.wirable(node) {
        return Ok((node, slot));
    }
    // A FACADE is a node that exists and simply has no slot by that name — saying it names nothing
    // sends a caller looking for the wrong mistake.
    match g.is_facade(uid) {
        true => Err(format!("`{which}` names sub-patch {} — name one of its ports as the slot", uid.to_hex())),
        false => Err(format!("`{which}` names no node in this patch: {}", uid.to_hex())),
    }
}

impl WriteOp for LinkAdd {
    const LABEL: &str = "Connect";
    fn run(tx: &mut Txn, a: LinkAddArgs) -> Result<Value, String> {
        let (a_uid, so) = a.from.resolve(&tx.g, "from")?;
        let (b_uid, si) = a.to.resolve(&tx.g, "to")?;
        let (a_uid, so) = wirable_endpoint(&tx.g, a_uid, &so, "from")?;
        let (b_uid, si) = wirable_endpoint(&tx.g, b_uid, &si, "to")?;
        tx.apply(goofi_graph::Command::AddLink { node_out: a_uid, slot_out: so.clone(), node_in: b_uid, slot_in: si.clone() })?;
        // The wire AS MADE, not as named: a boundary endpoint resolves to its inner leaf, and the
        // agreed dtype gates the next link to this output.
        let dtype = vocab::output_slots(&tx.g, a_uid)
            .into_iter()
            .find(|(key, _, _)| *key == so)
            .map(|(_, _, dtype)| dtype);
        Ok(json!({
            "from": format!("{}/{so}", named(&tx.g, a_uid)),
            "to": format!("{}/{si}", named(&tx.g, b_uid)),
            "dtype": dtype,
        }))
    }
}

impl WriteOp for LinkRemove {
    const LABEL: &str = "Disconnect";
    fn run(tx: &mut Txn, a: LinkRemoveArgs) -> Result<Value, String> {
        let (a_uid, so) = a.from.resolve(&tx.g, "from")?;
        let (b_uid, si) = a.to.resolve(&tx.g, "to")?;
        let (a_uid, so) = tx.g.normalise(a_uid, &so);
        let (b_uid, si) = tx.g.normalise(b_uid, &si);
        // Idempotent for the same reason `remove_node` is, and answered the same way.
        let existed = tx.g.has_link(a_uid, &so, b_uid, &si);
        tx.apply(goofi_graph::Command::RemoveLink { node_out: a_uid, slot_out: so, node_in: b_uid, slot_in: si })?;
        Ok(json!({ "removed": existed }))
    }
}
