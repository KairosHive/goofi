//! The state machines: build one through ops, and drive its playheads.

use serde_json::{json, Value};

use super::{op, Any, EffectOp, NoArgs, ReadOp, WriteOp};
use crate::machines::Msg;
use crate::{AppState, Caller, Txn};
use goofi_core::ease::Curve;
use goofi_core::variables::Control;
use goofi_core::Data;
use goofi_graph::machine::{Attribute, Machine, Playhead, State, Transition, Trigger, OWN};
use goofi_graph::{Command, Graph};

const VALUES: &str = "`values` is `{attribute: literal}`, each a number, a list of numbers (nested for a wider array), a bool or a string; an attribute a state leaves out is kept by a playhead entering it";

op!(List, "machine list", 0, NoArgs,
    "Every state machine, whole: its attributes, states, transitions (by id), playheads and seed. Where a playhead IS right now is its variables: `variables.<playhead>.state`, `.left` and `.progress`, read through `variable list` like its attributes.",
    "{machines: {name: {attributes: {name: {default, control?}}, states: {name: {pos, values}}, transitions: {id: {from, to, triggers, duration, curve, weight}}, playheads: {name: {color, start}}, seed?}}}");

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
    "Change a machine's own settings: `seed` starts its random draws — an `after` trigger's chance and the draw among transitions that fire together — so a run repeats.",
    "{seed}");

op!(AttributeAdd, "machine attribute add", 2, AttributeAddArgs {
    pub machine: String,
    pub name: String,
    pub value: Any,
    #[schemars(with = "Option<Value>")]
    pub control: Option<Control>,
},
    "Add an attribute: a value every state may set and every playhead carries as `variables.<playhead>.<name>`. `value` is its default, a number, a list of numbers (nested for a wider array), a bool or a string. `control` is the widget a state card and a playhead row draw it with, as `variable entry add` takes one; it must be able to draw the value. `state`, `left` and `progress` are the machine's own and refused.",
    "{name, value} — the attribute and its default as stored");

op!(AttributeEdit, "machine attribute edit", 2, AttributeEditArgs {
    pub machine: String,
    pub name: String,
    pub value: Option<Any>,
    /// Absent leaves the widget alone, `null` clears it, an object sets it.
    #[serde(default, deserialize_with = "super::nullable")]
    #[schemars(with = "Option<Value>")]
    pub control: Option<Option<Control>>,
},
    "Change an attribute's default and/or its widget.",
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
},
    "Add a state. Without a name, use the first free state0/state1/... name. `pos` is where its card sits on the canvas. `values` is `{attribute: literal}`, each a number, a list of numbers (nested for a wider array), a bool or a string; an attribute a state leaves out is kept by a playhead entering it.",
    "{name} — the state as stored");

op!(StateEdit, "machine state edit", 2, StateEditArgs {
    pub machine: String,
    pub name: String,
    pub pos: Option<[f64; 2]>,
    pub values: Option<Value>,
},
    "Change a state's place and/or what it sets. `values` keys set an attribute each, and a key set to `null` clears that attribute from the state, so a playhead entering it keeps what it holds. Previewable: a card drag sends `pos` as a preview.",
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
    pub triggers: Option<Value>,
    pub duration: Option<f64>,
    pub curve: Option<String>,
    pub weight: Option<f64>,
},
    "Add a transition from one state to another (`from` may be `*`, any state; `to` may equal `from`, a re-entry that restarts the dwell). It fires when any of its `triggers` fires: `{kind: manual}` — `machine fire`, or a tap in the panel; `{kind: after, seconds, chance?}` — once the playhead has dwelt `seconds` (a number, or an expression over `variables.*` read on entry), rolling `chance` (default 1; a failed roll re-arms the same dwell); `{kind: when, expression}` — the one expression language a param uses, over `variables.*` and `t` alone, firing on the rising edge of its truth; `{kind: meet, policy?}` — when a second playhead arrives in `from`, `fifo` (the longest resident goes, the default), `lifo` (the newest) or `all` (every resident); `{kind: alone}` — for the playhead left behind when the second-to-last leaves `from`. `duration` is seconds (0, instant, is the default); `curve` is step/linear/in/out/in_out/smooth, eased from what the playhead holds now so a redirection is continuous, arrays of one shape elementwise, anything else switching on arrival. When several transitions out of one state fire in one tick, one is drawn by `weight` (default 1; 0 is never drawn) — a random branch is several `after` transitions of one dwell with different weights.",
    "{id} — the transition's id, `t1`, `t2`, …, which every later op addresses it by");

op!(TransitionEdit, "machine transition edit", 2, TransitionEditArgs {
    pub machine: String,
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub triggers: Option<Value>,
    pub duration: Option<f64>,
    pub curve: Option<String>,
    pub weight: Option<f64>,
},
    "Change a transition: any of the fields `machine transition add` takes, the rest kept. `triggers` replaces the whole list.",
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
    "Add a playhead, starting in state `start`. Its name is its variable group: `variables.<name>.<attribute>` for every attribute, and the machine's own `variables.<name>.state` (the state it is in, or is moving to), `.left` (the state it left, empty at rest) and `.progress` (0..1 along the transition, 1 at rest). The group is the machine's: locked whole, written by the machine alone, removed with the playhead. Without a name, use the first free playhead0/playhead1/... name. `color` is what the canvas draws it as.",
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
    "Take a transition for a playhead, as a tap on it does: the transition must leave the state the playhead is in, or is moving to — a manual fire is the one thing that redirects a playhead in flight. Not undoable, and no change to the patch.",
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
        Ok(json!({ "machines": tx.g.machines() }))
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

/// A widget must be able to draw every value it will be handed.
fn fits(control: Option<&Control>, value: &Data) -> Result<(), String> {
    match control.filter(|c| !c.fits(value)) {
        Some(c) => Err(c.mismatch(value)),
        None => Ok(()),
    }
}

/// The attribute values an op names, each checked against the attribute and its widget. A `null`
/// is answered as `None`: the caller decides whether that clears or is refused.
fn values_of(m: &Machine, values: Option<Value>) -> Result<Vec<(String, Option<Data>)>, String> {
    let Some(values) = values else { return Ok(Vec::new()) };
    let map = values.as_object().ok_or(VALUES)?;
    let mut out = Vec::with_capacity(map.len());
    for (attr, v) in map {
        let a = m.attributes.get(attr).ok_or_else(|| format!("no attribute `{attr}` in the machine"))?;
        if v.is_null() {
            out.push((attr.clone(), None));
            continue;
        }
        let value = literal(v.clone())?;
        fits(a.control.as_ref(), &value).map_err(|why| format!("attribute `{attr}`: {why}"))?;
        out.push((attr.clone(), Some(value)));
    }
    Ok(out)
}

fn parse_curve(v: &str) -> Result<Curve, String> {
    serde_json::from_value(json!(v)).map_err(|_| {
        let curves = Curve::ALL.iter().map(|c| c.as_str()).collect::<Vec<_>>().join("/");
        format!("`curve` is one of {curves}, not `{v}`")
    })
}

fn parse_triggers(v: Value) -> Result<Vec<Trigger>, String> {
    serde_json::from_value(v).map_err(|e| format!("`triggers` is a list of {{kind: manual|after|when|meet|alone, …}}: {e}"))
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
        fits(a.control.as_ref(), &value)?;
        m.attributes.insert(a.name.clone(), Attribute { default: value.clone(), control: a.control });
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
        if a.value.is_none() && a.control.is_none() {
            return Err("nothing to change — give a value or a control".into());
        }
        if let Some(v) = a.value {
            attr.default = literal(v.0)?;
        }
        if let Some(c) = a.control {
            attr.control = c;
        }
        fits(attr.control.as_ref(), &attr.default)?;
        let widget = attr.control.clone();
        for (state, s) in &m.states {
            if let Some(v) = s.values.get(&a.name) {
                fits(widget.as_ref(), v).map_err(|why| format!("state `{state}`: {why}"))?;
            }
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
        }
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
        m.states.insert(name.clone(), State { pos: a.pos.unwrap_or_default(), values });
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
        if a.pos.is_none() && a.values.is_none() {
            return Err("nothing to change — give a pos or values".into());
        }
        let values = values_of(&m, a.values)?;
        let s = m.states.get_mut(&a.name).ok_or_else(|| format!("no state `{}` in machine `{}`", a.name, a.machine))?;
        if let Some(pos) = a.pos {
            s.pos = pos;
        }
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
        let t = Transition {
            from: a.from,
            to: a.to,
            triggers: a.triggers.map(parse_triggers).transpose()?.unwrap_or_default(),
            duration: a.duration.unwrap_or(0.0),
            curve: a.curve.as_deref().map(parse_curve).transpose()?.unwrap_or_default(),
            weight: a.weight.unwrap_or(1.0),
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
        let t = m.transitions.get_mut(&a.id).ok_or_else(|| format!("no transition `{}` in machine `{}`", a.id, a.machine))?;
        if let Some(from) = a.from {
            t.from = from;
        }
        if let Some(to) = a.to {
            t.to = to;
        }
        if let Some(triggers) = a.triggers {
            t.triggers = parse_triggers(triggers)?;
        }
        if let Some(duration) = a.duration {
            t.duration = duration;
        }
        if let Some(curve) = &a.curve {
            t.curve = parse_curve(curve)?;
        }
        if let Some(weight) = a.weight {
            t.weight = weight;
        }
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
        m.playheads.insert(name.clone(), Playhead { color: a.color.unwrap_or_default(), start: a.start });
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

/// The machine, playhead and (when named) transition or state an effect addresses, checked
/// against the settled model before the thread is told.
fn addressed(state: &AppState, machine: &str, playhead: &str, transition: Option<&str>, target: Option<&str>) -> Result<(), String> {
    let g = state.graph.lock();
    let m = machine_of(&g, machine)?;
    if !m.playheads.contains_key(playhead) {
        return Err(format!("no playhead `{playhead}` in machine `{machine}`"));
    }
    if let Some(id) = transition.filter(|id| !m.transitions.contains_key(*id)) {
        return Err(format!("no transition `{id}` in machine `{machine}`"));
    }
    if let Some(s) = target.filter(|s| !m.states.contains_key(*s)) {
        return Err(format!("no state `{s}` in machine `{machine}`"));
    }
    Ok(())
}

impl EffectOp for Fire {
    fn run(state: &AppState, a: FireArgs, _: &Caller) -> Result<Value, String> {
        addressed(state, &a.machine, &a.playhead, Some(&a.transition), None)?;
        state.machines.send(Msg::Fire { machine: a.machine, playhead: a.playhead, transition: a.transition });
        Ok(json!({ "ok": true }))
    }
}

impl EffectOp for Jump {
    fn run(state: &AppState, a: JumpArgs, _: &Caller) -> Result<Value, String> {
        addressed(state, &a.machine, &a.playhead, None, Some(&a.state))?;
        state.machines.send(Msg::Jump { machine: a.machine, playhead: a.playhead, state: a.state });
        Ok(json!({ "ok": true }))
    }
}

impl EffectOp for Reset {
    fn run(state: &AppState, a: ResetArgs, _: &Caller) -> Result<Value, String> {
        if let Some(m) = &a.machine {
            machine_of(&state.graph.lock(), m)?;
        }
        state.machines.send(Msg::Reset { machine: a.machine });
        Ok(json!({ "ok": true }))
    }
}
