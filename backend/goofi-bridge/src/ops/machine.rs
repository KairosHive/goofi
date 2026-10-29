//! The state machines: build one through ops, and drive its playheads.

use serde_json::{json, Value};

use super::{op, Any, EffectOp, NoArgs, ReadOp, WriteOp};
use crate::machines::Msg;
use crate::{AppState, Caller, Txn};
use goofi_core::ease::Curve;
use goofi_core::Data;
use goofi_graph::expr_rewrite;
use goofi_graph::machine::{Attribute, AttributeKind, Machine, Decision, Playhead, State, Transition, OWN};
use goofi_graph::{Graph};

const VALUES: &str = "`values` is `{attribute: literal}`, each a number, a list of numbers (nested for a wider array), a bool or a string; an attribute a state leaves out is kept by a playhead entering it";
const ENVELOPES: &str = "`envelopes` is `{attribute: [attack, decay, sustain, release]}` over number attributes the state sets: on a playhead's arrival the attribute plays the envelope times the value — up from 0 over `attack` seconds, down over `decay` seconds to `sustain` (0..1), then down to 0 over `release` seconds; a release of -1 holds the sustain. A transition out eases the attribute from wherever the envelope stands to the next state's value, or to 0 where that state's own envelope plays on it";

op!(List, "machine list", 0, NoArgs,
    "Every state machine, whole: its attributes, states, transitions (by id), playheads and seed. Where a playhead IS right now is its variables: `variables.<playhead>.state`, `.prev` and `.progress`, read through `variable list` like its attributes.",
    "{machines: {name: {attributes: {name: {default, kind}}, states: {name: {pos, values, envelopes, interval, interval_expression?, mode, noop, weights, expression}}, transitions: {id: {from, to, condition?, duration, curve}}, playheads: {name: {color, start}}, seed?}}}");

op!(Add, "machine add", 1, AddArgs {
    pub name: Option<String>,
},
    "Create an empty state machine. Without a name, use the first free machine0/machine1/... name. A machine is a set of states, the transitions between them, and the playheads that travel through them; a playhead writes the values its states hold into the variables `variables.<playhead>.<attribute>`, which any param reads as it reads a control panel's.",
    "{name} — the machine as stored");

op!(Remove, "machine remove", 1, RemoveArgs {
    pub machine: String,
},
    "Delete a machine, its playheads and their variable groups. A panel showing it shows the first machine left.",
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
    "Change a machine's own settings: `seed` starts its random draws — a `random` state's decisions — so a run repeats.",
    "{seed}");

op!(AttributeAdd, "machine attribute add", 2, AttributeAddArgs {
    pub machine: String,
    pub name: String,
    pub value: Any,
    #[schemars(with = "Option<Value>")]
    pub kind: Option<AttributeKind>,
},
    "Add an attribute: a value every state may set and every playhead carries as `variables.<playhead>.<name>`. `value` is its default. `kind` is what the attribute is, as a node's param is: `{type: num, vmin, vmax, int?, face?}` holding a number or a list of them (`face` is `plain`, or `color` and `envelope` for four RGBA or ADSR numbers), `{type: bool}` holding 0 or 1, or `{type: string, options?}` holding a text; left out, a number is a `num` in 0..1 and a text a `string`. The inspector draws each with that param's widget, and a playhead's variable wears the control-panel widget nearest it. `state`, `prev` and `progress` are the machine's own and refused.",
    "{name, value} — the attribute and its default as stored");

op!(AttributeEdit, "machine attribute edit", 2, AttributeEditArgs {
    pub machine: String,
    pub name: String,
    pub value: Option<Any>,
    #[schemars(with = "Option<Value>")]
    pub kind: Option<AttributeKind>,
},
    "Change an attribute's default and/or its kind; the kind must hold the default. A new kind reads every state's value as a param reads a frame: a text parses to numbers, a number prints as a text, a bool is its truth, a vector fills or cuts to the default's length; a value the kind still cannot hold, such as a text outside the options, becomes the default.",
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
    pub envelopes: Option<Value>,
},
    "Add a state. Without a name, use the first free state0/state1/... name. `pos` is where its card sits on the canvas, in flow px on its 24 px grid; leave it out and the card goes to the clear cells nearest the machine's others. `values` is `{attribute: literal}`, each a number, a list of numbers (nested for a wider array), a bool or a string; an attribute a state leaves out is kept by a playhead entering it. `envelopes` is `{attribute: [attack, decay, sustain, release]}` over number attributes the state sets: on a playhead's arrival the attribute plays the envelope times the value — up from 0 over `attack` seconds, down over `decay` seconds to `sustain` (0..1), then down to 0 over `release` seconds; a release of -1 holds the sustain. A transition out eases the attribute from wherever the envelope stands to the next state's value, or to 0 where that state's own envelope plays on it.",
    "{name} — the state as stored");

op!(StateEdit, "machine state edit", 2, StateEditArgs {
    pub machine: String,
    pub name: String,
    pub pos: Option<[f64; 2]>,
    pub values: Option<Value>,
    pub envelopes: Option<Value>,
    pub interval: Option<f64>,
    pub interval_expression: Option<String>,
    #[schemars(with = "Option<String>")]
    pub mode: Option<Decision>,
    pub noop: Option<f64>,
    pub weights: Option<std::collections::BTreeMap<String, f64>>,
    pub expression: Option<String>,
},
    "Change a state's place, what it sets, and how it decides. `values` keys set an attribute each, and a key set to `null` clears that attribute from the state, so a playhead entering it keeps what it holds; an attribute no state sets any more leaves the machine. `envelopes` keys give an attribute an envelope each (see `machine state add`), and a key set to `null` takes it away; clearing a value clears its envelope. A playhead resting in a state decides every `interval` seconds from its arrival (0: once, on arrival; -1: never) — or every `interval_expression` seconds, an expression read for the playhead as each beat is armed, a negative value being never; empty, `interval` holds — which transition out of it to take, by `mode`: `sequence` takes the next transition out in authored order, each playhead remembering its place per state across visits; `random` draws by weight among `noop` (wait one more interval) and `weights` `{transition id: weight}`, which replaces the whole map and in which a transition left out weighs 1; `expression` evaluates `expression` to the index of the transition out to take, a negative index waiting (see `machine transition add` for what an expression reads). A new state draws evenly among its transitions out: `random` with `noop` 0 and no weights, every second. A state with no transition out lands its values again and plays its envelopes every `interval` seconds instead. Previewable: a card drag sends `pos` as a preview.",
    "{name}");

op!(Arrange, "machine arrange", 1, ArrangeArgs {
    pub machine: String,
},
    "Put every state's card on a ring, in the order a playhead reaches them from its start, on the canvas's 24 px grid. A machine is all-to-all, so a ring keeps every transition short and readable. The ◇ button on the canvas is this op. One undo step.",
    "{moved: n} — how many states changed place");

op!(StateRemove, "machine state remove", 2, StateRemoveArgs {
    pub machine: String,
    pub name: String,
},
    "Delete a state and every transition into or out of it, and every attribute no other state sets. A state a playhead starts in refuses: move the playhead first.",
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
    pub condition: Option<String>,
    pub duration: Option<f64>,
    pub curve: Option<String>,
},
    "Add a transition from one state to another (`from` may be `*`, any state; `to` may equal `from`, a re-entry that restarts the state's decisions). A playhead takes it when its `from` state's decision picks it (see `machine state edit`), when `machine fire` or the inspector's fire button fires it, or the instant its `condition` turns true: the condition is read for each playhead resting in `from` as it arrives and at every instant the machine settles after — a write to a variable it reads, a playhead's landing, a decision's due — and fires on the turn from false to true, so one already true on arrival waits for the next turn. Several conditions turning true at one instant take the first in authored order; a condition goes before a decision due at the same instant. `st('A').residents >= 2` fires every resident as a second playhead arrives, `… and me.name == st('A').playheads[0]` the longest resident alone, `… [-1]` the newest; `st('A').residents == 1 and st('A').departures > 0` fires the one left behind as the other leaves. `duration` is seconds (0, instant, is the default); `curve` is step/linear/in/out/in_out/smooth, eased from what the playhead holds now so a redirection is continuous, arrays of one shape elementwise, anything else switching on arrival; an attribute the entering state shapes with an envelope, or one the leaving state's envelope has at rest, moves by that envelope instead (see `machine state add`). An expression is the one Python a param's expression is, over `variables.<group>.<element>`, `t` (patch seconds), `time()`, `np`, `math` and `lfo()`, `nse()` and `qtz()`; a machine's also reads `me` — the playhead it is read for: `me.name`, `me.state`, `me.arrived` and `me.<attribute>` — and `st('<state>')`: `.residents` (how many playheads rest there), `.playheads` (their names, longest resident first), `.arrivals` and `.departures` (how many arrived there and left it at this instant). A node's output is not readable; follow it in a variable",
    "{id} — the transition's id, `t1`, `t2`, …, which every later op addresses it by");

op!(TransitionEdit, "machine transition edit", 2, TransitionEditArgs {
    pub machine: String,
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub condition: Option<String>,
    pub duration: Option<f64>,
    pub curve: Option<String>,
},
    "Change a transition: any of the fields `machine transition add` takes, the rest kept; an empty `condition` leaves it to its state's decision and the fire button.",
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

/// Replace `name` whole with `next`, as one undo step; a weight on a transition that no longer
/// leaves its state goes with it.
fn set(tx: &mut Txn, name: &str, mut next: Machine) -> Result<(), String> {
    next.prune();
    tx.key(format!("machine {name}"));
    tx.g.set_machine(name, Some(next), None)?;
    Ok(())
}

/// The envelopes an op names, each four numbers; a `null` clears, as `values_of` answers one.
fn envelopes_of(values: Option<Value>) -> Result<Edits<[f64; 4]>, String> {
    let Some(values) = values else { return Ok(Vec::new()) };
    let map = values.as_object().ok_or(ENVELOPES)?;
    map.iter()
        .map(|(attr, v)| match v {
            Value::Null => Ok((attr.clone(), None)),
            v => serde_json::from_value::<[f64; 4]>(v.clone()).map(|e| (attr.clone(), Some(e))).map_err(|_| format!("attribute `{attr}`: {ENVELOPES}")),
        })
        .collect()
}

/// An attribute no state sets any more leaves the machine, and every playhead with it.
fn drop_unset(m: &mut Machine, attrs: impl IntoIterator<Item = String>) {
    for attr in attrs {
        if !m.states.values().any(|s| s.values.contains_key(&attr)) {
            m.attributes.shift_remove(&attr);
        }
    }
}

/// A kind must hold every value it will be handed.
fn fits(kind: &AttributeKind, value: &Data) -> Result<(), String> {
    if kind.fits(value) { Ok(()) } else { Err(kind.mismatch(value)) }
}

/// Per attribute an op names, what it sets or, for a `null`, clears.
type Edits<T> = Vec<(String, Option<T>)>;

/// The attribute values an op names, each checked against the attribute and its widget. A `null`
/// is answered as `None`: the caller decides whether that clears or is refused.
fn values_of(m: &Machine, values: Option<Value>) -> Result<Edits<Data>, String> {
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
        fits(&a.kind, &value).map_err(|why| format!("attribute `{attr}`: {why}"))?;
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
        tx.g.set_machine(&a.machine, None, None)?;
        // A panel over the removed machine shows the next one the patch has.
        if let Some(next) = tx.g.machines().keys().next().cloned() {
            let writes = tx.g.arrangement().retarget("machine", "machine", &a.machine, &next);
            if !writes.is_empty() {
                tx.g.arrangement_mut().set_contents(&writes);
            }
        }
        Ok(json!({ "removed": true }))
    }

    fn label(a: &RemoveArgs, _: &Value) -> String {
        format!("Remove machine {}", a.machine)
    }
}

impl WriteOp for Rename {
    fn run(tx: &mut Txn, a: RenameArgs) -> Result<Value, String> {
        tx.g.rename_machine(&a.machine, &a.to)?;
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
        fits(&kind, &value)?;
        m.attributes.insert(a.name.clone(), Attribute { default: value.clone(), kind });
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
        let rekind = a.kind.is_some();
        if let Some(k) = a.kind {
            attr.kind = k;
        }
        fits(&attr.kind, &attr.default)?;
        let (kind, default) = (attr.kind.clone(), attr.default.clone());
        if rekind {
            for s in m.states.values_mut() {
                if let Some(v) = s.values.get_mut(&a.name) {
                    *v = kind.read(v, &default);
                }
                if !matches!(kind, AttributeKind::Num { .. }) {
                    s.envelopes.shift_remove(&a.name);
                }
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
            s.envelopes.shift_remove(&a.name);
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
        for r in tx.g.rename_attribute(&a.machine, &a.name, &a.to)? {
            tx.echo(r);
        }
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
        let envelopes = envelopes_of(a.envelopes)?.into_iter().filter_map(|(k, e)| Some((k, e?))).collect();
        let pos = a.pos.unwrap_or_else(|| goofi_graph::canvas::place_state(&m));
        m.states.insert(name.clone(), State { pos, values, envelopes, ..State::default() });
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
        if a.pos.is_none() && a.values.is_none() && a.envelopes.is_none() && a.interval.is_none() && a.interval_expression.is_none() && a.mode.is_none() && a.noop.is_none() && a.weights.is_none() && a.expression.is_none() {
            return Err("nothing to change — give a pos, values, envelopes, interval, interval_expression, mode, noop, weights or expression".into());
        }
        let values = values_of(&m, a.values)?;
        let envelopes = envelopes_of(a.envelopes)?;
        if let Some(id) = a.weights.iter().flat_map(|w| w.keys()).find(|id| !m.outgoing(&a.name).any(|o| o == *id)) {
            return Err(format!("state `{}` weighs `{id}`, which is no transition out of it", a.name));
        }
        let s = m.states.get_mut(&a.name).ok_or_else(|| format!("no state `{}` in machine `{}`", a.name, a.machine))?;
        if let Some(pos) = a.pos {
            s.pos = pos;
        }
        s.interval = a.interval.unwrap_or(s.interval);
        s.interval_expression = a.interval_expression.unwrap_or(std::mem::take(&mut s.interval_expression));
        s.mode = a.mode.unwrap_or(s.mode);
        s.noop = a.noop.unwrap_or(s.noop);
        if let Some(w) = a.weights {
            s.weights = w.into_iter().collect();
        }
        s.expression = a.expression.unwrap_or(std::mem::take(&mut s.expression));
        let mut cleared = Vec::new();
        for (attr, v) in values {
            match v {
                Some(v) => {
                    s.values.insert(attr, v);
                }
                None => {
                    s.values.shift_remove(&attr);
                    s.envelopes.shift_remove(&attr);
                    cleared.push(attr);
                }
            }
        }
        for (attr, e) in envelopes {
            match e {
                Some(e) => {
                    s.envelopes.insert(attr, e);
                }
                None => {
                    s.envelopes.shift_remove(&attr);
                }
            }
        }
        drop_unset(&mut m, cleared);
        set(tx, &a.machine, m)?;
        Ok(json!({ "name": a.name }))
    }

    fn label(a: &StateEditArgs, _: &Value) -> String {
        match (&a.pos, &a.values) {
            (Some(_), None) if a.envelopes.is_none() && a.interval.is_none() && a.interval_expression.is_none() && a.mode.is_none() && a.noop.is_none() && a.weights.is_none() && a.expression.is_none() => format!("Move state {}", a.name),
            _ => format!("Edit state {}", a.name),
        }
    }
}

impl WriteOp for Arrange {
    const LABEL: &str = "Arrange";
    fn run(tx: &mut Txn, a: ArrangeArgs) -> Result<Value, String> {
        let m = machine_of(&tx.g, &a.machine)?;
        let next = goofi_graph::canvas::arrange_machine(&m);
        let moved = next.states.iter().filter(|(k, s)| m.states.get(*k).map(|o| o.pos) != Some(s.pos)).count();
        if moved > 0 {
            set(tx, &a.machine, next)?;
        }
        Ok(json!({ "moved": moved }))
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
        let set_here: Vec<String> = m.states.shift_remove(&a.name).map(|s| s.values.into_keys().collect()).unwrap_or_default();
        m.transitions.retain(|_, t| t.from != a.name && t.to != a.name);
        drop_unset(&mut m, set_here);
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
        m.rewrite_expressions(|e| expr_rewrite::rename_states(e, |s| (s == a.name).then(|| a.to.clone())));
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
            condition: a.condition.unwrap_or_default(),
            duration: a.duration.unwrap_or(0.0),
            curve: a.curve.as_deref().map(parse_curve).transpose()?.unwrap_or_default(),
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
        if let Some(condition) = a.condition {
            t.condition = condition;
        }
        if let Some(duration) = a.duration {
            t.duration = duration;
        }
        if let Some(curve) = &a.curve {
            t.curve = parse_curve(curve)?;
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
        for r in tx.g.rename_playhead(&a.machine, &a.name, &a.to)? {
            tx.echo(r);
        }
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
        let at = state.graph.lock().time().now();
        state.machines.send(Msg::Fire { at, machine: a.machine, playhead: a.playhead, transition: a.transition });
        Ok(json!({ "ok": true }))
    }
}

impl EffectOp for Jump {
    fn run(state: &AppState, a: JumpArgs, _: &Caller) -> Result<Value, String> {
        addressed(state, &a.machine, &a.playhead, None, Some(&a.state))?;
        let at = state.graph.lock().time().now();
        state.machines.send(Msg::Jump { at, machine: a.machine, playhead: a.playhead, state: a.state });
        Ok(json!({ "ok": true }))
    }
}

impl EffectOp for Reset {
    fn run(state: &AppState, a: ResetArgs, _: &Caller) -> Result<Value, String> {
        let at = {
            let g = state.graph.lock();
            if let Some(m) = &a.machine {
                machine_of(&g, m)?;
            }
            g.time().now()
        };
        state.machines.send(Msg::Reset { at, machine: a.machine });
        Ok(json!({ "ok": true }))
    }
}
