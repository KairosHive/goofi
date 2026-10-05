//! The state machines: build one through ops, and drive its playheads.

use serde_json::{json, Value};

use super::{op, Any, EffectOp, NoArgs, ReadOp, WriteOp};
use crate::machines::Effect;
use crate::{AppState, Caller, Txn};
use goofi_core::ease::Curve;
use goofi_core::Data;
use goofi_graph::machine::{Attribute, AttributeKind, Machine, Playhead, Seconds, Selection, State, Transition, Trigger, OWN};
use goofi_graph::{Command, Graph};

const VALUES: &str = "`values` is `{attribute: literal}`, each a number, a list of numbers (nested for a wider array), a bool or a string; an attribute a state leaves out is kept by a playhead entering it";

op!(List, "machine list", 0, NoArgs,
    "Every authored state machine, its standing runtime health, the resolved outgoing transition order for each state and the reserved runtime element names. Health reports expression failures and stopped playheads; successful reevaluation or recovery clears them. Live playhead state is read through its variables: state (resident or destination), prev (departure state during travel), progress (0..1), transition (active ID), and arrived (global clock ticks, as a decimal integer string; one second is 2^64 ticks).",
    "{machines, health: {machine: {expressions: [{playhead, transition, surface, expression, error}], playheads: {playhead: error}}}, outgoing: {machine: {state: [transition IDs]}}, runtime_elements: [reserved variable element names]}");

op!(Add, "machine add", 1, AddArgs {
    pub name: Option<String>,
},
    "Create an empty state machine. Without a name, use the first free machine0/machine1/... name. A machine is a set of states, the transitions between them, and the playheads that travel through them; a playhead writes the values its states hold into the variables `variables.<playhead>.<attribute>`, which any param reads as it reads a control panel's.",
    "{name} — the machine as stored");

op!(Remove, "machine remove", 1, RemoveArgs {
    pub machine: String,
},
    "Delete a machine, its playheads and their variable groups.",
    "{removed: true}");

op!(Rename, "machine rename", 2, RenameArgs {
    pub machine: String,
    pub to: String,
},
    "Rename a machine. Its playheads keep their names, so nothing that reads them moves.",
    "{name} — the name as stored");

op!(Edit, "machine edit", 1, EditArgs {
    pub machine: String,
    pub seed: Option<u64>,
},
    "Change a machine's seed for repeatable chance, selection and random-duration draws. Reset restarts the seeded run.",
    "{seed}");

op!(AttributeAdd, "machine attribute add", 2, AttributeAddArgs {
    pub machine: String,
    pub name: String,
    pub value: Any,
    #[schemars(with = "Option<Value>")]
    pub kind: Option<AttributeKind>,
},
    "Add an attribute: a value every state may set and every playhead carries as `variables.<playhead>.<name>`. `value` is its default. `kind` is what the attribute is, as a node's param is: `{type: num, vmin, vmax, int?, color?}` holding a number or a list of them (four for a colour), `{type: bool}` holding 0 or 1, or `{type: string, options?}` holding a text; left out, a number is a `num` in 0..1 and a text a `string`. The inspector draws each with that param's widget, and a playhead's variable wears the control-panel widget nearest it. Runtime element names from `machine list` are reserved and refused.",
    "{name, value} — the attribute and its default as stored");

op!(AttributeEdit, "machine attribute edit", 2, AttributeEditArgs {
    pub machine: String,
    pub name: String,
    pub value: Option<Any>,
    #[schemars(with = "Option<Value>")]
    pub kind: Option<AttributeKind>,
},
    "Change an attribute's default and/or its kind; the kind must hold the default and every state's value.",
    "{name}");

op!(AttributeRemove, "machine attribute remove", 2, AttributeRemoveArgs {
    pub machine: String,
    pub name: String,
},
    "Delete an attribute, from every state and every playhead with it.",
    "{removed: true}");

op!(AttributeRename, "machine attribute rename", 2, AttributeRenameArgs {
    pub machine: String,
    pub name: String,
    pub to: String,
},
    "Rename an attribute in every state, and rewrite every expression that reads it on a playhead.",
    "{name} — the name as stored");

op!(StateAdd, "machine state add", 2, StateAddArgs {
    pub machine: String,
    pub name: Option<String>,
    pub pos: Option<[f64; 2]>,
    pub values: Option<Value>,
    pub exit_values: Option<Value>,
    #[schemars(with = "Option<Value>")]
    pub selection: Option<Selection>,
    pub order: Option<Vec<String>>,
},
    "Add a state. Without a name, use the first free state0/state1/... name. `pos` is where its card sits on the canvas. `values` is `{attribute: literal}`, each a number, a list of numbers (nested for a wider array), a bool or a string; an attribute a state leaves out is kept by a playhead entering it.",
    "{name} — the state as stored");

op!(StateEdit, "machine state edit", 2, StateEditArgs {
    pub machine: String,
    pub name: String,
    pub pos: Option<[f64; 2]>,
    pub values: Option<Value>,
    pub exit_values: Option<Value>,
    #[schemars(with = "Option<Value>")]
    pub selection: Option<Selection>,
    pub order: Option<Vec<String>>,
},
    "Change a state's place, entry/exit values, selection policy (ordered/weighted/uniform), or preferred outgoing transition IDs in order. `values` keys set an attribute each, and a key set to `null` clears that attribute from the state, so a playhead entering it keeps what it holds. Previewable: a card drag sends `pos` as a preview.",
    "{name}");

op!(StateRemove, "machine state remove", 2, StateRemoveArgs {
    pub machine: String,
    pub name: String,
},
    "Delete a state and every transition into or out of it. A state a playhead starts in refuses: move the playhead first.",
    "{removed: true}");

op!(StateRename, "machine state rename", 2, StateRenameArgs {
    pub machine: String,
    pub name: String,
    pub to: String,
},
    "Rename a state; every transition and playhead start that names it follows.",
    "{name} — the name as stored");

op!(TransitionAdd, "machine transition add", 1, TransitionAddArgs {
    pub machine: String,
    pub from: String,
    pub to: String,
    #[schemars(with = "Option<Vec<Value>>")]
    pub triggers: Option<Vec<Trigger>>,
    #[schemars(with = "Option<Value>")]
    pub duration: Option<Seconds>,
    pub chance: Option<f64>,
    pub weight: Option<f64>,
    pub guard: Option<String>,
    pub internal: Option<bool>,
    pub values: Option<Value>,
    #[schemars(with = "Option<String>")]
    pub curve: Option<Curve>,
},
    "Add a transition from a state (or `*`, any state) to a state. A trigger offers it: manual; after {seconds}; when {expression, edge?} with rising/falling/change/level; event {name}; always; meet {policy?} with fifo/lifo/all; alone. Several triggers offer one transition, once. A guard expression can block any trigger. Chance in 0..1 defaults to 1; weight defaults to 1 and is used by weighted selection. A state's selection is ordered (first successful chance), weighted (relative shares), or uniform. State order lists preferred outgoing IDs. After seconds and travel duration accept a nonnegative number, an expression over variables and t, or a uniform random range {min, max}, sampled once when armed/taken. Failed or unselected positive timers retry from their deadline using that interval. Travel eases held values through curve step/linear/in/out/in_out/smooth; strings and unlike shapes switch on arrival. A self-transition re-enters; internal=true requires a zero-duration self-transition and preserves dwell. Values are local transition assignments. Manual fire validates the live source and guard, then bypasses chance and selection.",
    "{id} — the transition's id, `t1`, `t2`, …, which every later op addresses it by");

op!(TransitionEdit, "machine transition edit", 2, TransitionEditArgs {
    pub machine: String,
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
    #[schemars(with = "Option<Vec<Value>>")]
    pub triggers: Option<Vec<Trigger>>,
    #[schemars(with = "Option<Value>")]
    pub duration: Option<Seconds>,
    pub chance: Option<f64>,
    pub weight: Option<f64>,
    pub guard: Option<String>,
    pub internal: Option<bool>,
    pub values: Option<Value>,
    #[schemars(with = "Option<String>")]
    pub curve: Option<Curve>,
},
    "Change a transition: any field from `machine transition add`, the rest kept. Triggers replace the list. An empty guard clears it. Values patch local assignments; null clears one. Endpoint changes remove invalid state-order references.",
    "{id}");

op!(TransitionRemove, "machine transition remove", 2, TransitionRemoveArgs {
    pub machine: String,
    pub id: String,
},
    "Delete a transition.",
    "{removed: true}");

op!(PlayheadAdd, "machine playhead add", 2, PlayheadAddArgs {
    pub machine: String,
    pub name: Option<String>,
    pub color: Option<String>,
    pub start: String,
},
    "Add a playhead, starting in state `start`. Its name is its variable group: `variables.<name>.<attribute>` for every attribute, and the machine's own `variables.<name>.state` (the state it is in, or is moving to), `.prev` (the state it left, empty at rest) and `.progress` (0..1 along the transition, 1 at rest). The group is the machine's: locked whole, written by the machine alone, removed with the playhead. Without a name, use the first free playhead0/playhead1/... name. `color` is what the canvas draws it as.",
    "{name} — the playhead as stored");

op!(PlayheadEdit, "machine playhead edit", 2, PlayheadEditArgs {
    pub machine: String,
    pub name: String,
    pub color: Option<String>,
    pub start: Option<String>,
},
    "Change a playhead's colour and/or start state. The start is where a reset and a load put it.",
    "{name}");

op!(PlayheadRemove, "machine playhead remove", 2, PlayheadRemoveArgs {
    pub machine: String,
    pub name: String,
},
    "Delete a playhead and its variable group.",
    "{removed: true}");

op!(PlayheadRename, "machine playhead rename", 2, PlayheadRenameArgs {
    pub machine: String,
    pub name: String,
    pub to: String,
},
    "Rename a playhead — its variable group — and rewrite every expression that reads it.",
    "{name} — the name as stored");

op!(Fire, "machine fire", 1, FireArgs {
    pub machine: String,
    pub playhead: String,
    pub transition: String,
},
    "Take an explicitly selected transition for a playhead: the transition must leave the state the playhead is in, or is moving to — a manual fire is the one thing that redirects a playhead in flight. Not undoable, and no change to the patch.",
    "{ok: true}");

op!(Event, "machine event", 1, EventArgs {
    pub machine: String,
    pub event: String,
    pub playhead: Option<String>,
},
    "Offer a named event to resident playheads. Without `playhead`, all resident playheads receive it as one batch. Event triggers, guards, chances and the state's selection policy decide each move. An event is consumed once. Not undoable, and no change to the patch.",
    "{ok: true}");

op!(Jump, "machine jump", 1, JumpArgs {
    pub machine: String,
    pub playhead: String,
    pub state: String,
},
    "Put a playhead in a state now: instant, no easing. Not undoable, and no change to the patch.",
    "{ok: true}");

op!(Reset, "machine reset", 1, ResetArgs {
    pub machine: Option<String>,
},
    "Put every playhead of a machine — or of every machine, when none is named — back in its start state, as a load does. Not undoable, and no change to the patch.",
    "{ok: true}");

impl ReadOp for List {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(json!({ "machines": tx.g.machines(), "health": tx.g.machine_health(), "outgoing": tx.g.machine_outgoing(), "runtime_elements": OWN }))
    }
}

/// `v` as an attribute's value: a number, a list, a bool or a string.
fn literal(v: Value) -> Result<Data, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("`{v}` is not a number, a list, a bool or a string: {e}"))
}

fn machine_of(g: &Graph, name: &str) -> Result<Machine, String> {
    g.machines().get(name).cloned().ok_or_else(|| format!("no machine `{name}` — `machine add` creates one"))
}

/// Replace `name` whole with `next`, as one undo step.
fn set(tx: &mut Txn, name: &str, next: Machine) -> Result<(), String> {
    tx.apply(Command::SetMachine { name: name.to_string(), machine: Some(Box::new(next)), at: None })?;
    Ok(())
}

/// The attribute values an op names. A `null`
/// is answered as `None`: the caller decides whether that clears or is refused.
fn values_of(m: &Machine, values: Option<Value>) -> Result<Vec<(String, Option<Data>)>, String> {
    let Some(values) = values else { return Ok(Vec::new()) };
    let map = values.as_object().ok_or(VALUES)?;
    let mut out = Vec::with_capacity(map.len());
    for (attr, v) in map {
        if !m.attributes.contains_key(attr) { return Err(format!("no attribute `{attr}` in the machine")); }
        if v.is_null() {
            out.push((attr.clone(), None));
            continue;
        }
        let value = literal(v.clone())?;
        out.push((attr.clone(), Some(value)));
    }
    Ok(out)
}

fn apply_values(target: &mut goofi_core::indexmap::IndexMap<String, Data>, edits: Vec<(String, Option<Data>)>) {
    for (name, value) in edits {
        match value {
            Some(value) => { target.insert(name, value); }
            None => { target.shift_remove(&name); }
        }
    }
}

impl WriteOp for Add {
    fn run(tx: &mut Txn, a: AddArgs) -> Result<Value, String> {
        let name = a.name.unwrap_or_else(|| goofi_core::fresh_name("machine", 0, |n| tx.g.machines().contains_key(n)));
        if tx.g.machines().contains_key(&name) {
            return Err(format!("machine `{name}` already exists"));
        }
        set(tx, &name, Machine::default())?;
        Ok(json!({ "name": name }))
    }

    fn label(_: &AddArgs, o: &Value) -> String {
        format!("Add machine {}", o["name"].as_str().unwrap_or_default())
    }
}

impl WriteOp for Remove {
    fn run(tx: &mut Txn, a: RemoveArgs) -> Result<Value, String> {
        machine_of(&tx.g, &a.machine)?;
        tx.apply(Command::SetMachine { name: a.machine, machine: None, at: None })?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &RemoveArgs, _: &Value) -> String {
        format!("Remove machine {}", a.machine)
    }
}

impl WriteOp for Rename {
    fn run(tx: &mut Txn, a: RenameArgs) -> Result<Value, String> {
        tx.apply(Command::RenameMachine { from: a.machine, to: a.to.clone() })?;
        Ok(json!({ "name": a.to }))
    }

    fn label(a: &RenameArgs, _: &Value) -> String {
        format!("Rename machine {} → {}", a.machine, a.to)
    }
}

impl WriteOp for Edit {
    fn run(tx: &mut Txn, a: EditArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        m.seed = a.seed.or(m.seed);
        let seed = m.seed;
        set(tx, &a.machine, m)?;
        Ok(json!({ "seed": seed }))
    }

    fn label(a: &EditArgs, _: &Value) -> String {
        format!("Edit machine {}", a.machine)
    }
}

impl WriteOp for AttributeAdd {
    fn run(tx: &mut Txn, a: AttributeAddArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        if m.attributes.contains_key(&a.name) {
            return Err(format!("attribute `{}` already exists — `machine attribute edit` changes it", a.name));
        }
        if OWN.contains(&a.name.as_str()) {
            return Err(format!("`{}` is the machine's own element of a playhead; an attribute cannot take it", a.name));
        }
        let value = literal(a.value.0)?;
        let kind = a.kind.unwrap_or_else(|| AttributeKind::of(&value));
        m.attributes.insert(a.name.clone(), Attribute { identity: Default::default(), default: value.clone(), kind });
        set(tx, &a.machine, m)?;
        Ok(json!({ "name": a.name, "value": value }))
    }

    fn label(a: &AttributeAddArgs, _: &Value) -> String {
        format!("Add attribute {} to {}", a.name, a.machine)
    }
}

impl WriteOp for AttributeEdit {
    fn run(tx: &mut Txn, a: AttributeEditArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        let attr = m.attributes.get_mut(&a.name).ok_or_else(|| format!("no attribute `{}` in machine `{}`", a.name, a.machine))?;
        if a.value.is_none() && a.kind.is_none() {
            return Err("nothing to change — give a value or a kind".into());
        }
        if let Some(v) = a.value {
            attr.default = literal(v.0)?;
        }
        if let Some(k) = a.kind {
            attr.kind = k;
        }
        set(tx, &a.machine, m)?;
        Ok(json!({ "name": a.name }))
    }

    fn label(a: &AttributeEditArgs, _: &Value) -> String {
        format!("Edit attribute {}.{}", a.machine, a.name)
    }
}

impl WriteOp for AttributeRemove {
    fn run(tx: &mut Txn, a: AttributeRemoveArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        m.attributes.shift_remove(&a.name).ok_or_else(|| format!("no attribute `{}` in machine `{}`", a.name, a.machine))?;
        for s in m.states.values_mut() {
            s.values.shift_remove(&a.name);
            s.exit_values.shift_remove(&a.name);
        }
        for t in m.transitions.values_mut() { t.values.shift_remove(&a.name); }
        set(tx, &a.machine, m)?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &AttributeRemoveArgs, _: &Value) -> String {
        format!("Remove attribute {}.{}", a.machine, a.name)
    }
}

impl WriteOp for AttributeRename {
    fn run(tx: &mut Txn, a: AttributeRenameArgs) -> Result<Value, String> {
        if OWN.contains(&a.to.as_str()) {
            return Err(format!("`{}` is the machine's own element of a playhead; an attribute cannot take it", a.to));
        }
        tx.apply(Command::RenameAttribute { machine: a.machine, from: a.name, to: a.to.clone() })?;
        Ok(json!({ "name": a.to }))
    }

    fn label(a: &AttributeRenameArgs, _: &Value) -> String {
        format!("Rename attribute {} → {}", a.name, a.to)
    }
}

impl WriteOp for StateAdd {
    fn run(tx: &mut Txn, a: StateAddArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        let name = a.name.unwrap_or_else(|| goofi_core::fresh_name("state", 0, |n| m.states.contains_key(n)));
        if m.states.contains_key(&name) {
            return Err(format!("state `{name}` already exists — `machine state edit` changes it"));
        }
        let values = values_of(&m, a.values)?.into_iter().filter_map(|(k, v)| Some((k, v?))).collect();
        let exit_values = values_of(&m, a.exit_values)?.into_iter().filter_map(|(k, v)| Some((k, v?))).collect();
        m.states.insert(name.clone(), State { identity: Default::default(), pos: a.pos.unwrap_or_default(), values, exit_values,
            selection: a.selection.unwrap_or_default(), order: a.order.unwrap_or_default() });
        set(tx, &a.machine, m)?;
        Ok(json!({ "name": name }))
    }

    fn label(_: &StateAddArgs, o: &Value) -> String {
        format!("Add state {}", o["name"].as_str().unwrap_or_default())
    }
}

impl WriteOp for StateEdit {
    fn run(tx: &mut Txn, a: StateEditArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        if a.pos.is_none() && a.values.is_none() && a.exit_values.is_none() && a.selection.is_none() && a.order.is_none() {
            return Err("nothing to change — give pos, values, exit_values, selection or order".into());
        }
        let values = values_of(&m, a.values)?;
        let exit_values = values_of(&m, a.exit_values)?;
        let s = m.states.get_mut(&a.name).ok_or_else(|| format!("no state `{}` in machine `{}`", a.name, a.machine))?;
        if let Some(pos) = a.pos {
            s.pos = pos;
        }
        if let Some(selection) = a.selection { s.selection = selection; }
        if let Some(order) = a.order { s.order = order; }
        apply_values(&mut s.exit_values, exit_values);
        for (attr, v) in values {
            match v {
                Some(v) => {
                    s.values.insert(attr, v);
                }
                None => {
                    s.values.shift_remove(&attr);
                }
            }
        }
        set(tx, &a.machine, m)?;
        Ok(json!({ "name": a.name }))
    }

    fn label(a: &StateEditArgs, _: &Value) -> String {
        match (&a.pos, &a.values) {
            (Some(_), None) => format!("Move state {}", a.name),
            _ => format!("Edit state {}", a.name),
        }
    }
}

impl WriteOp for StateRemove {
    fn run(tx: &mut Txn, a: StateRemoveArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        if !m.states.contains_key(&a.name) {
            return Err(format!("no state `{}` in machine `{}`", a.name, a.machine));
        }
        if let Some((ph, _)) = m.playheads.iter().find(|(_, p)| p.start == a.name) {
            return Err(format!("playhead `{ph}` starts in `{}` — move it first", a.name));
        }
        m.states.shift_remove(&a.name);
        m.transitions.retain(|_, t| t.from != a.name && t.to != a.name);
        m.prune_order();
        set(tx, &a.machine, m)?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &StateRemoveArgs, _: &Value) -> String {
        format!("Remove state {}", a.name)
    }
}

impl WriteOp for StateRename {
    fn run(tx: &mut Txn, a: StateRenameArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        if m.states.contains_key(&a.to) {
            return Err(format!("state `{}` already exists", a.to));
        }
        let at = m.states.get_index_of(&a.name).ok_or_else(|| format!("no state `{}` in machine `{}`", a.name, a.machine))?;
        let s = m.states.shift_remove(&a.name).expect("the index answered");
        m.states.shift_insert(at, a.to.clone(), s);
        for t in m.transitions.values_mut() {
            for end in [&mut t.from, &mut t.to] {
                if *end == a.name {
                    *end = a.to.clone();
                }
            }
        }
        for p in m.playheads.values_mut().filter(|p| p.start == a.name) {
            p.start = a.to.clone();
        }
        set(tx, &a.machine, m)?;
        Ok(json!({ "name": a.to }))
    }

    fn label(a: &StateRenameArgs, _: &Value) -> String {
        format!("Rename state {} → {}", a.name, a.to)
    }
}

impl WriteOp for TransitionAdd {
    fn run(tx: &mut Txn, a: TransitionAddArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        let id = goofi_core::fresh_name("t", 1, |n| m.transitions.contains_key(n));
        let values = values_of(&m, a.values)?.into_iter().filter_map(|(k, v)| Some((k, v?))).collect();
        let t = Transition {
            from: a.from,
            to: a.to,
            triggers: a.triggers.unwrap_or_default(),
            duration: a.duration.unwrap_or_default(),
            chance: a.chance.unwrap_or(1.0),
            weight: a.weight.unwrap_or(1.0),
            guard: a.guard.filter(|s| !s.trim().is_empty()),
            internal: a.internal.unwrap_or_default(),
            values,
            curve: a.curve.unwrap_or_default(),
        };
        m.transitions.insert(id.clone(), t);
        set(tx, &a.machine, m)?;
        Ok(json!({ "id": id }))
    }

    fn label(a: &TransitionAddArgs, _: &Value) -> String {
        format!("Add transition {} → {}", a.from, a.to)
    }
}

impl WriteOp for TransitionEdit {
    fn run(tx: &mut Txn, a: TransitionEditArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        let values = values_of(&m, a.values)?;
        let t = m.transitions.get_mut(&a.id).ok_or_else(|| format!("no transition `{}` in machine `{}`", a.id, a.machine))?;
        if let Some(from) = a.from {
            t.from = from;
        }
        if let Some(to) = a.to {
            t.to = to;
        }
        if let Some(triggers) = a.triggers {
            t.triggers = triggers;
        }
        if let Some(duration) = a.duration {
            t.duration = duration;
        }
        if let Some(curve) = a.curve { t.curve = curve; }
        if let Some(chance) = a.chance { t.chance = chance; }
        if let Some(weight) = a.weight { t.weight = weight; }
        if let Some(guard) = a.guard { t.guard = (!guard.trim().is_empty()).then_some(guard); }
        if let Some(internal) = a.internal { t.internal = internal; }
        apply_values(&mut t.values, values);
        m.prune_order();
        set(tx, &a.machine, m)?;
        Ok(json!({ "id": a.id }))
    }

    fn label(a: &TransitionEditArgs, _: &Value) -> String {
        format!("Edit transition {}", a.id)
    }
}

impl WriteOp for TransitionRemove {
    fn run(tx: &mut Txn, a: TransitionRemoveArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        m.transitions.shift_remove(&a.id).ok_or_else(|| format!("no transition `{}` in machine `{}`", a.id, a.machine))?;
        m.prune_order();
        set(tx, &a.machine, m)?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &TransitionRemoveArgs, _: &Value) -> String {
        format!("Remove transition {}", a.id)
    }
}

impl WriteOp for PlayheadAdd {
    fn run(tx: &mut Txn, a: PlayheadAddArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        let name = a.name.unwrap_or_else(|| goofi_core::fresh_name("playhead", 0, |n| tx.g.group_taken(n)));
        if m.playheads.contains_key(&name) {
            return Err(format!("playhead `{name}` already exists — `machine playhead edit` changes it"));
        }
        m.playheads.insert(name.clone(), Playhead { identity: Default::default(), color: a.color.unwrap_or_default(), start: a.start });
        set(tx, &a.machine, m)?;
        Ok(json!({ "name": name }))
    }

    fn label(_: &PlayheadAddArgs, o: &Value) -> String {
        format!("Add playhead {}", o["name"].as_str().unwrap_or_default())
    }
}

impl WriteOp for PlayheadEdit {
    fn run(tx: &mut Txn, a: PlayheadEditArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        let p = m.playheads.get_mut(&a.name).ok_or_else(|| format!("no playhead `{}` in machine `{}`", a.name, a.machine))?;
        if a.color.is_none() && a.start.is_none() {
            return Err("nothing to change — give a color or a start".into());
        }
        if let Some(color) = a.color {
            p.color = color;
        }
        if let Some(start) = a.start {
            p.start = start;
        }
        set(tx, &a.machine, m)?;
        Ok(json!({ "name": a.name }))
    }

    fn label(a: &PlayheadEditArgs, _: &Value) -> String {
        format!("Edit playhead {}", a.name)
    }
}

impl WriteOp for PlayheadRemove {
    fn run(tx: &mut Txn, a: PlayheadRemoveArgs) -> Result<Value, String> {
        let mut m = machine_of(&tx.g, &a.machine)?;
        m.playheads.shift_remove(&a.name).ok_or_else(|| format!("no playhead `{}` in machine `{}`", a.name, a.machine))?;
        set(tx, &a.machine, m)?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &PlayheadRemoveArgs, _: &Value) -> String {
        format!("Remove playhead {}", a.name)
    }
}

impl WriteOp for PlayheadRename {
    fn run(tx: &mut Txn, a: PlayheadRenameArgs) -> Result<Value, String> {
        tx.apply(Command::RenamePlayhead { machine: a.machine, from: a.name, to: a.to.clone() })?;
        Ok(json!({ "name": a.to }))
    }

    fn label(a: &PlayheadRenameArgs, _: &Value) -> String {
        format!("Rename playhead {} → {}", a.name, a.to)
    }
}

impl EffectOp for Fire {
    fn run(state: &AppState, a: FireArgs, _: &Caller) -> Result<Value, String> {
        state.machines.effect(Effect::Fire { machine: a.machine, playhead: a.playhead, transition: a.transition })?;
        Ok(json!({ "ok": true }))
    }
}

impl EffectOp for Jump {
    fn run(state: &AppState, a: JumpArgs, _: &Caller) -> Result<Value, String> {
        state.machines.effect(Effect::Jump { machine: a.machine, playhead: a.playhead, state: a.state })?;
        Ok(json!({ "ok": true }))
    }
}

impl EffectOp for Reset {
    fn run(state: &AppState, a: ResetArgs, _: &Caller) -> Result<Value, String> {
        state.machines.effect(Effect::Reset { machine: a.machine })?;
        Ok(json!({ "ok": true }))
    }
}

impl EffectOp for Event {
    fn run(state: &AppState, a: EventArgs, _: &Caller) -> Result<Value, String> {
        state.machines.effect(Effect::Event { machine: a.machine, name: a.event, playhead: a.playhead })?;
        Ok(json!({ "ok": true }))
    }
}
