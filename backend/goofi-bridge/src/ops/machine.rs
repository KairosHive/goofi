//! The state machines: build one through ops, and drive its playheads. A machine, a state, an
//! attribute and a playhead each have an id the manager mints; an op names one by its id or by
//! its name, and a rename moves the name alone.

use serde_json::{json, Value};

use super::{op, Any, EffectOp, NoArgs, ReadOp, WriteOp};
use crate::machines::Msg;
use crate::{AppState, Caller, Txn};
use goofi_core::ease::Curve;
use goofi_core::Data;
use goofi_graph::machine::{self, Attribute, AttributeKind, Decision, Machine, Named, Playhead, State, Transition};
use goofi_graph::Graph;
use goofi_core::indexmap::IndexMap;

const VALUES: &str = "`values` is `{attribute: literal}`, each a number, a list of numbers (nested for a wider array), a bool or a string; an attribute a state leaves out is kept by a playhead entering it";
const ENVELOPES: &str = "`envelopes` is `{attribute: [attack, decay, sustain, release]}` over number attributes the state sets: on a playhead's arrival the attribute plays the envelope times the value — up from 0 over `attack` seconds, down over `decay` seconds to `sustain` (0..1), then down to 0 over `release` seconds; a release of -1 holds the sustain. A transition out eases the attribute from wherever the envelope stands to the next state's value, or to 0 where that state's own envelope plays on it";

op!(List, "machine list", 0, NoArgs,
    "Every state machine, whole, each record under its id: its name, attributes, states, transitions, playheads and seed. A state's `values` and `envelopes` are keyed by attribute id; a transition's `from` and `to` and a playhead's `start` are state ids. Every other `machine` op names a machine, a state, an attribute or a playhead by its id or by its name. Where a playhead IS right now is its variables: `vars.<playhead>.state` and `.progress`, read through `variable list` like its attributes.",
    "{machines: {id: {name, attributes: {id: {name, default, kind, curve}}, states: {id: {name, pos, values, envelopes, interval, interval_expression?, mode, noop, weights, expression}}, transitions: {id: {from, to, condition?, duration}}, playheads: {id: {name, color, start}}, seed?}}}");

op!(Add, "machine add", 1, AddArgs {
    pub name: Option<String>,
},
    "Create an empty state machine. Without a name, use the first free machine0/machine1/... name. A machine is a set of states, the transitions between them, and the playheads that travel through them; a playhead writes the values its states hold into the variables `vars.<playhead>.<attribute>`, which any param reads as it reads a control panel's.",
    "{id, name} — the machine as stored");

op!(Remove, "machine remove", 1, RemoveArgs {
    pub machine: String,
},
    "Delete a machine, its playheads and their variable groups. A panel showing it shows the first machine left.",
    "{removed: true, name}");

op!(Rename, "machine rename", 2, RenameArgs {
    pub machine: String,
    pub to: String,
},
    "Rename a machine. The name is a label: its id, its playheads and their runs stay as they are.",
    "{id, name, was} — the name as stored, and the name it had");

op!(Edit, "machine edit", 1, EditArgs {
    pub machine: String,
    pub seed: Option<u64>,
},
    "Change a machine's own settings: `seed` starts its random draws — a `random` state's decisions — so a run repeats.",
    "{seed, name}");

op!(AttributeAdd, "machine attribute add", 2, AttributeAddArgs {
    pub machine: String,
    pub name: String,
    pub value: Any,
    #[schemars(with = "Option<Value>")]
    pub kind: Option<AttributeKind>,
    pub curve: Option<Value>,
},
    "Add an attribute: a value every state may set and every playhead carries as `vars.<playhead>.<name>`, or `sm('<playhead>').<name>`. `value` is its default. `kind` is what the attribute is, as a node's param is: `{type: num, vmin, vmax, int?, face?}` holding a number or a list of them (`face` is `plain`, or `color` and `envelope` for four RGBA or ADSR numbers), `{type: bool}` holding 0 or 1, or `{type: string, options?}` holding a text; left out, a number is a `num` in 0..1 and a text a `string`. `curve` is how the attribute eases over a transition's duration: `step`, `linear` (the default), `in`, `out`, `in_out`, `smooth`, or `{bezier: [x1, y1, x2, y2]}`, a cubic from (0, 0) through two handles to (1, 1) as CSS writes one, the handles' x in 0..1. The inspector draws each with that param's widget, and a playhead's variable wears the control-panel widget nearest it. `state` and `progress` are the machine's own and refused.",
    "{id, name, value} — the attribute and its default as stored");

op!(AttributeEdit, "machine attribute edit", 2, AttributeEditArgs {
    pub machine: String,
    pub name: String,
    pub value: Option<Any>,
    #[schemars(with = "Option<Value>")]
    pub kind: Option<AttributeKind>,
    pub curve: Option<Value>,
},
    "Change an attribute's default, its kind and/or its curve (see `machine attribute add`); the kind must hold the default. A new kind reads every state's value as a param reads a frame: a text parses to numbers, a number prints as a text, a bool is its truth, a vector fills or cuts to the default's length; a value the kind still cannot hold, such as a text outside the options, becomes the default.",
    "{name}");

op!(AttributeRemove, "machine attribute remove", 2, AttributeRemoveArgs {
    pub machine: String,
    pub name: String,
},
    "Delete an attribute, from every state and every playhead with it.",
    "{removed: true, name}");

op!(AttributeRename, "machine attribute rename", 2, AttributeRenameArgs {
    pub machine: String,
    pub name: String,
    pub to: String,
},
    "Rename an attribute, and rewrite every expression that reads it on a playhead. Every playhead keeps the value it holds.",
    "{id, name, was} — the name as stored, and the name it had");

op!(StateAdd, "machine state add", 2, StateAddArgs {
    pub machine: String,
    pub name: Option<String>,
    pub pos: Option<[f64; 2]>,
    pub values: Option<Value>,
    pub envelopes: Option<Value>,
},
    "Add a state. Without a name, use the first free state0/state1/... name. `pos` is where its card sits on the canvas, in flow px on its 24 px grid; leave it out and the card goes to the clear cells nearest the machine's others. `values` is `{attribute: literal}`, each a number, a list of numbers (nested for a wider array), a bool or a string; an attribute a state leaves out is kept by a playhead entering it. `envelopes` is `{attribute: [attack, decay, sustain, release]}` over number attributes the state sets: on a playhead's arrival the attribute plays the envelope times the value — up from 0 over `attack` seconds, down over `decay` seconds to `sustain` (0..1), then down to 0 over `release` seconds; a release of -1 holds the sustain. A transition out eases the attribute from wherever the envelope stands to the next state's value, or to 0 where that state's own envelope plays on it.",
    "{id, name} — the state as stored");

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
    "{removed: true, name}");

op!(StateRename, "machine state rename", 2, StateRenameArgs {
    pub machine: String,
    pub name: String,
    pub to: String,
},
    "Rename a state, and rewrite every `sm('<name>')` of the machine that reads it. A playhead resting in it or flying to it goes on as it was.",
    "{id, name, was} — the name as stored, and the name it had");

op!(TransitionAdd, "machine transition add", 1, TransitionAddArgs {
    pub machine: String,
    pub from: String,
    pub to: String,
    pub condition: Option<String>,
    pub duration: Option<f64>,
},
    "Add a transition from one state to another (`from` may be `*`, any state; `to` may equal `from`, a re-entry that restarts the state's decisions). A playhead takes it when its `from` state's decision picks it (see `machine state edit`), when `machine fire` or the inspector's fire button fires it, or the instant its `condition` turns true: the condition is read for each playhead resting in `from` as it arrives and at every instant the machine settles after — a write to a variable it reads, a playhead's landing, a decision's due — and fires on the turn from false to true, so one already true on arrival waits for the next turn. Several conditions turning true at one instant take the first in authored order; a condition goes before a decision due at the same instant. `sm('A').residents >= 2` fires every resident as a second playhead arrives, `… and me.name == sm('A').playheads[0]` the longest resident alone, `… [-1]` the newest; `sm('A').residents == 1 and sm('A').departures > 0` fires the one left behind as the other leaves. `duration` is seconds (0, instant, is the default): each attribute the entering state sets eases over it by the attribute's own `curve` (see `machine attribute add`), from what the playhead holds now so a redirection is continuous, arrays of one shape elementwise, anything else switching on arrival; an attribute the entering state shapes with an envelope, or one the leaving state's envelope has at rest, moves by that envelope instead (see `machine state add`). An expression is the one Python a param's expression is, over `vars.<group>.<element>`, `nd('<node>')` (its output, `.out.<slot>` or `.params.<group>.<param>`, read as a param reads it; a frame it emits settles the machine), `t` (patch seconds), `np`, `math` and `lfo()`, `nse()` and `qtz()`; a machine's also reads `me` — the playhead it is read for: `me.name`, `me.state`, `me.arrived` and `me.<attribute>` — and `sm('<name>')`: a state's `.residents` (how many playheads rest there), `.playheads` (their names, longest resident first), `.arrivals` and `.departures` (how many arrived there and left it at this instant), or another playhead's record, as `me` is",
    "{id} — the transition's id, `t1`, `t2`, …, which every later op addresses it by");

op!(TransitionEdit, "machine transition edit", 2, TransitionEditArgs {
    pub machine: String,
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub condition: Option<String>,
    pub duration: Option<f64>,
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
    "Add a playhead, starting in state `start`. Its name is its variable group: `vars.<name>.<attribute>`, or `sm('<name>').<attribute>` for every attribute, and the machine's own `vars.<name>.state` (the state it is in, or is moving to) and `.progress` (0..1 along the transition, 1 at rest); the state a flight left and the second it arrived ride those frames as metadata, for the canvas, not as variables. The group is the machine's: locked whole, written by the machine alone, removed with the playhead. Without a name, use the first free playhead0/playhead1/... name. `color` is what the canvas draws it as.",
    "{id, name} — the playhead as stored");

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
    "{removed: true, name}");

op!(PlayheadRename, "machine playhead rename", 2, PlayheadRenameArgs {
    pub machine: String,
    pub name: String,
    pub to: String,
},
    "Rename a playhead — its variable group, which moves with the values it holds — and rewrite every expression that reads it. Its run goes on as it was.",
    "{id, name, was} — the name as stored, and the name it had");

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

/// The machine `given` names, with its id.
fn machine_of(g: &Graph, given: &str) -> Result<(String, Machine), String> {
    let id = id_of("machine", g.machines(), given).map_err(|why| format!("{why} — `machine add` creates one"))?;
    Ok((id.clone(), g.machines()[&id].clone()))
}

/// The id of the `kind` that `given` names in `map`.
fn id_of<V: Named>(kind: &str, map: &IndexMap<String, V>, given: &str) -> Result<String, String> {
    machine::find(map, given).map(str::to_string).ok_or_else(|| format!("no {kind} `{given}`"))
}

/// Replace the machine `id` whole with `next`, as one undo step; a weight on a transition that
/// no longer leaves its state goes with it. The graph follows every name that moved.
fn set(tx: &mut Txn, id: &str, mut next: Machine) -> Result<(), String> {
    next.prune();
    tx.key(format!("machine {id}"));
    for touched in tx.g.set_machine(id, Some(next), None)? {
        tx.echo(touched);
    }
    Ok(())
}

/// The name an op answered with: what its history label spells, whatever the caller named it by.
fn named(o: &Value) -> &str {
    o["name"].as_str().unwrap_or_default()
}

/// A rename's history label, off what the op answered: the name it had and the name it has.
fn renamed(kind: &str, o: &Value) -> String {
    format!("Rename {kind} {} → {}", o["was"].as_str().unwrap_or_default(), named(o))
}

/// Per attribute id an op names, what it sets or, for a `null`, clears.
type Edits<T> = Vec<(String, Option<T>)>;

/// The envelopes an op names, each four numbers; a `null` clears, as `values_of` answers one.
fn envelopes_of(m: &Machine, values: Option<Value>) -> Result<Edits<[f64; 4]>, String> {
    let Some(values) = values else { return Ok(Vec::new()) };
    let map = values.as_object().ok_or(ENVELOPES)?;
    map.iter()
        .map(|(attr, v)| {
            let id = id_of("attribute", &m.attributes, attr)?;
            match v {
                Value::Null => Ok((id, None)),
                v => serde_json::from_value::<[f64; 4]>(v.clone()).map(|e| (id, Some(e))).map_err(|_| format!("attribute `{attr}`: {ENVELOPES}")),
            }
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

/// The attribute values an op names, each checked against the attribute and its widget. A `null`
/// is answered as `None`: the caller decides whether that clears or is refused.
fn values_of(m: &Machine, values: Option<Value>) -> Result<Edits<Data>, String> {
    let Some(values) = values else { return Ok(Vec::new()) };
    let map = values.as_object().ok_or(VALUES)?;
    let mut out = Vec::with_capacity(map.len());
    for (attr, v) in map {
        let id = id_of("attribute", &m.attributes, attr)?;
        if v.is_null() {
            out.push((id, None));
            continue;
        }
        let value = literal(v.clone())?;
        fits(&m.attributes[&id].kind, &value).map_err(|why| format!("attribute `{attr}`: {why}"))?;
        out.push((id, Some(value)));
    }
    Ok(out)
}

fn parse_curve(v: Value) -> Result<Curve, String> {
    let curve: Curve = serde_json::from_value(v.clone()).map_err(|_| {
        let presets = Curve::PRESETS.iter().map(|c| c.as_str()).collect::<Vec<_>>().join("/");
        format!("`curve` is one of {presets} or `{{bezier: [x1, y1, x2, y2]}}`, not `{v}`")
    })?;
    curve.check()?;
    Ok(curve)
}

impl WriteOp for Add {
    fn run(tx: &mut Txn, a: AddArgs) -> Result<Value, String> {
        let name = a.name.unwrap_or_else(|| goofi_core::fresh_name("machine", 0, |n| tx.g.machines().values().any(|m| m.name == n)));
        let id = tx.g.mint_id("m", |_| false);
        set(tx, &id, Machine { name: name.clone(), ..Machine::default() })?;
        Ok(json!({ "id": id, "name": name }))
    }

    fn label(_: &AddArgs, o: &Value) -> String {
        format!("Add machine {}", named(o))
    }
}

impl WriteOp for Remove {
    fn run(tx: &mut Txn, a: RemoveArgs) -> Result<Value, String> {
        let (id, m) = machine_of(&tx.g, &a.machine)?;
        tx.g.set_machine(&id, None, None)?;
        // A panel over the removed machine shows the next one the patch has.
        if let Some(next) = tx.g.machines().keys().next().cloned() {
            let writes = tx.g.arrangement().retarget("machine", "machine", &id, &next);
            if !writes.is_empty() {
                tx.g.arrangement_mut().set_contents(&writes);
            }
        }
        Ok(json!({ "removed": true, "name": m.name }))
    }

    fn label(_: &RemoveArgs, o: &Value) -> String {
        format!("Remove machine {}", named(o))
    }
}

impl WriteOp for Rename {
    fn run(tx: &mut Txn, a: RenameArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let was = std::mem::replace(&mut m.name, a.to.clone());
        set(tx, &id, m)?;
        Ok(json!({ "id": id, "name": a.to, "was": was }))
    }

    fn label(_: &RenameArgs, o: &Value) -> String {
        renamed("machine", o)
    }
}

impl WriteOp for Edit {
    fn run(tx: &mut Txn, a: EditArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        m.seed = a.seed.or(m.seed);
        let out = json!({ "seed": m.seed, "name": m.name });
        set(tx, &id, m)?;
        Ok(out)
    }

    fn label(_: &EditArgs, o: &Value) -> String {
        format!("Edit machine {}", named(o))
    }
}

impl WriteOp for AttributeAdd {
    fn run(tx: &mut Txn, a: AttributeAddArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let value = literal(a.value.0)?;
        let kind = a.kind.unwrap_or_else(|| AttributeKind::of(&value));
        fits(&kind, &value)?;
        let curve = a.curve.map(parse_curve).transpose()?.unwrap_or_default();
        let attr = tx.g.mint_id("a", |id| m.attributes.contains_key(id));
        m.attributes.insert(attr.clone(), Attribute { name: a.name.clone(), default: value.clone(), kind, curve });
        set(tx, &id, m)?;
        Ok(json!({ "id": attr, "name": a.name, "value": value }))
    }

    fn label(a: &AttributeAddArgs, _: &Value) -> String {
        format!("Add attribute {}", a.name)
    }
}

impl WriteOp for AttributeEdit {
    fn run(tx: &mut Txn, a: AttributeEditArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let aid = id_of("attribute", &m.attributes, &a.name)?;
        if a.value.is_none() && a.kind.is_none() && a.curve.is_none() {
            return Err("nothing to change — give a value, a kind or a curve".into());
        }
        let attr = &mut m.attributes[&aid];
        if let Some(v) = a.value {
            attr.default = literal(v.0)?;
        }
        if let Some(c) = a.curve {
            attr.curve = parse_curve(c)?;
        }
        let rekind = a.kind.is_some();
        if let Some(k) = a.kind {
            attr.kind = k;
        }
        fits(&attr.kind, &attr.default)?;
        let (kind, default) = (attr.kind.clone(), attr.default.clone());
        if rekind {
            for s in m.states.values_mut() {
                if let Some(v) = s.values.get_mut(&aid) {
                    *v = kind.read(v, &default);
                }
                if !matches!(kind, AttributeKind::Num { .. }) {
                    s.envelopes.shift_remove(&aid);
                }
            }
        }
        let out = json!({ "name": m.attributes[&aid].name });
        set(tx, &id, m)?;
        Ok(out)
    }

    fn label(_: &AttributeEditArgs, o: &Value) -> String {
        format!("Edit attribute {}", named(o))
    }
}

impl WriteOp for AttributeRemove {
    fn run(tx: &mut Txn, a: AttributeRemoveArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let aid = id_of("attribute", &m.attributes, &a.name)?;
        let gone = m.attributes.shift_remove(&aid).expect("resolved");
        for s in m.states.values_mut() {
            s.values.shift_remove(&aid);
            s.envelopes.shift_remove(&aid);
        }
        set(tx, &id, m)?;
        Ok(json!({ "removed": true, "name": gone.name }))
    }

    fn label(_: &AttributeRemoveArgs, o: &Value) -> String {
        format!("Remove attribute {}", named(o))
    }
}

impl WriteOp for AttributeRename {
    fn run(tx: &mut Txn, a: AttributeRenameArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let aid = id_of("attribute", &m.attributes, &a.name)?;
        let was = std::mem::replace(&mut m.attributes[&aid].name, a.to.clone());
        set(tx, &id, m)?;
        Ok(json!({ "id": aid, "name": a.to, "was": was }))
    }

    fn label(_: &AttributeRenameArgs, o: &Value) -> String {
        renamed("attribute", o)
    }
}

impl WriteOp for StateAdd {
    fn run(tx: &mut Txn, a: StateAddArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let name = a.name.unwrap_or_else(|| goofi_core::fresh_name("state", 0, |n| m.states.values().any(|s| s.name == n)));
        let values = values_of(&m, a.values)?.into_iter().filter_map(|(k, v)| Some((k, v?))).collect();
        let envelopes = envelopes_of(&m, a.envelopes)?.into_iter().filter_map(|(k, e)| Some((k, e?))).collect();
        let pos = a.pos.unwrap_or_else(|| goofi_graph::canvas::place_state(&m));
        let state = tx.g.mint_id("s", |id| m.states.contains_key(id));
        m.states.insert(state.clone(), State { name: name.clone(), pos, values, envelopes, ..State::default() });
        set(tx, &id, m)?;
        Ok(json!({ "id": state, "name": name }))
    }

    fn label(_: &StateAddArgs, o: &Value) -> String {
        format!("Add state {}", named(o))
    }
}

impl WriteOp for StateEdit {
    fn run(tx: &mut Txn, a: StateEditArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let sid = id_of("state", &m.states, &a.name)?;
        if a.pos.is_none() && a.values.is_none() && a.envelopes.is_none() && a.interval.is_none() && a.interval_expression.is_none() && a.mode.is_none() && a.noop.is_none() && a.weights.is_none() && a.expression.is_none() {
            return Err("nothing to change — give a pos, values, envelopes, interval, interval_expression, mode, noop, weights or expression".into());
        }
        let values = values_of(&m, a.values)?;
        let envelopes = envelopes_of(&m, a.envelopes)?;
        if let Some(t) = a.weights.iter().flat_map(|w| w.keys()).find(|t| !m.outgoing(&sid).any(|o| o == *t)) {
            return Err(format!("state `{}` weighs `{t}`, which is no transition out of it", m.states[&sid].name));
        }
        let s = &mut m.states[&sid];
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
        let out = json!({ "name": m.states[&sid].name });
        set(tx, &id, m)?;
        Ok(out)
    }

    fn label(a: &StateEditArgs, o: &Value) -> String {
        match (&a.pos, &a.values) {
            (Some(_), None) if a.envelopes.is_none() && a.interval.is_none() && a.interval_expression.is_none() && a.mode.is_none() && a.noop.is_none() && a.weights.is_none() && a.expression.is_none() => format!("Move state {}", named(o)),
            _ => format!("Edit state {}", named(o)),
        }
    }
}

impl WriteOp for Arrange {
    const LABEL: &str = "Arrange";
    fn run(tx: &mut Txn, a: ArrangeArgs) -> Result<Value, String> {
        let (id, m) = machine_of(&tx.g, &a.machine)?;
        let next = goofi_graph::canvas::arrange_machine(&m);
        let moved = next.states.iter().filter(|(k, s)| m.states.get(*k).map(|o| o.pos) != Some(s.pos)).count();
        if moved > 0 {
            set(tx, &id, next)?;
        }
        Ok(json!({ "moved": moved }))
    }
}

impl WriteOp for StateRemove {
    fn run(tx: &mut Txn, a: StateRemoveArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let sid = id_of("state", &m.states, &a.name)?;
        if let Some(p) = m.playheads.values().find(|p| p.start == sid) {
            return Err(format!("playhead `{}` starts in `{}` — move it first", p.name, m.states[&sid].name));
        }
        let gone = m.states.shift_remove(&sid).expect("resolved");
        m.transitions.retain(|_, t| t.from != sid && t.to != sid);
        drop_unset(&mut m, gone.values.into_keys());
        set(tx, &id, m)?;
        Ok(json!({ "removed": true, "name": gone.name }))
    }

    fn label(_: &StateRemoveArgs, o: &Value) -> String {
        format!("Remove state {}", named(o))
    }
}

impl WriteOp for StateRename {
    fn run(tx: &mut Txn, a: StateRenameArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let sid = id_of("state", &m.states, &a.name)?;
        let was = std::mem::replace(&mut m.states[&sid].name, a.to.clone());
        set(tx, &id, m)?;
        Ok(json!({ "id": sid, "name": a.to, "was": was }))
    }

    fn label(_: &StateRenameArgs, o: &Value) -> String {
        renamed("state", o)
    }
}

/// The state a transition's end names: `*`, any state, where `any` allows it.
fn end_of(m: &Machine, given: &str, any: bool) -> Result<String, String> {
    if any && given == "*" { Ok(given.to_string()) } else { id_of("state", &m.states, given) }
}

impl WriteOp for TransitionAdd {
    fn run(tx: &mut Txn, a: TransitionAddArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let transition = goofi_core::fresh_name("t", 1, |n| m.transitions.contains_key(n));
        let t = Transition {
            from: end_of(&m, &a.from, true)?,
            to: end_of(&m, &a.to, false)?,
            condition: a.condition.unwrap_or_default(),
            duration: a.duration.unwrap_or(0.0),
        };
        m.transitions.insert(transition.clone(), t);
        set(tx, &id, m)?;
        Ok(json!({ "id": transition }))
    }

    fn label(_: &TransitionAddArgs, o: &Value) -> String {
        format!("Add transition {}", o["id"].as_str().unwrap_or_default())
    }
}

impl WriteOp for TransitionEdit {
    fn run(tx: &mut Txn, a: TransitionEditArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let from = a.from.map(|s| end_of(&m, &s, true)).transpose()?;
        let to = a.to.map(|s| end_of(&m, &s, false)).transpose()?;
        let t = m.transitions.get_mut(&a.id).ok_or_else(|| format!("no transition `{}`", a.id))?;
        if let Some(from) = from {
            t.from = from;
        }
        if let Some(to) = to {
            t.to = to;
        }
        if let Some(condition) = a.condition {
            t.condition = condition;
        }
        if let Some(duration) = a.duration {
            t.duration = duration;
        }
        set(tx, &id, m)?;
        Ok(json!({ "id": a.id }))
    }

    fn label(a: &TransitionEditArgs, _: &Value) -> String {
        format!("Edit transition {}", a.id)
    }
}

impl WriteOp for TransitionRemove {
    fn run(tx: &mut Txn, a: TransitionRemoveArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        m.transitions.shift_remove(&a.id).ok_or_else(|| format!("no transition `{}`", a.id))?;
        set(tx, &id, m)?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &TransitionRemoveArgs, _: &Value) -> String {
        format!("Remove transition {}", a.id)
    }
}

impl WriteOp for PlayheadAdd {
    fn run(tx: &mut Txn, a: PlayheadAddArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let name = a.name.unwrap_or_else(|| goofi_core::fresh_name("playhead", 0, |n| tx.g.group_taken(n)));
        let start = id_of("state", &m.states, &a.start)?;
        let playhead = tx.g.mint_id("p", |id| m.playheads.contains_key(id));
        m.playheads.insert(playhead.clone(), Playhead { name: name.clone(), color: a.color.unwrap_or_default(), start });
        set(tx, &id, m)?;
        Ok(json!({ "id": playhead, "name": name }))
    }

    fn label(_: &PlayheadAddArgs, o: &Value) -> String {
        format!("Add playhead {}", named(o))
    }
}

impl WriteOp for PlayheadEdit {
    fn run(tx: &mut Txn, a: PlayheadEditArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let pid = id_of("playhead", &m.playheads, &a.name)?;
        if a.color.is_none() && a.start.is_none() {
            return Err("nothing to change — give a color or a start".into());
        }
        let start = a.start.map(|s| id_of("state", &m.states, &s)).transpose()?;
        let p = &mut m.playheads[&pid];
        if let Some(color) = a.color {
            p.color = color;
        }
        if let Some(start) = start {
            p.start = start;
        }
        let out = json!({ "name": m.playheads[&pid].name });
        set(tx, &id, m)?;
        Ok(out)
    }

    fn label(_: &PlayheadEditArgs, o: &Value) -> String {
        format!("Edit playhead {}", named(o))
    }
}

impl WriteOp for PlayheadRemove {
    fn run(tx: &mut Txn, a: PlayheadRemoveArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let pid = id_of("playhead", &m.playheads, &a.name)?;
        let gone = m.playheads.shift_remove(&pid).expect("resolved");
        set(tx, &id, m)?;
        Ok(json!({ "removed": true, "name": gone.name }))
    }

    fn label(_: &PlayheadRemoveArgs, o: &Value) -> String {
        format!("Remove playhead {}", named(o))
    }
}

impl WriteOp for PlayheadRename {
    fn run(tx: &mut Txn, a: PlayheadRenameArgs) -> Result<Value, String> {
        let (id, mut m) = machine_of(&tx.g, &a.machine)?;
        let pid = id_of("playhead", &m.playheads, &a.name)?;
        let was = std::mem::replace(&mut m.playheads[&pid].name, a.to.clone());
        set(tx, &id, m)?;
        Ok(json!({ "id": pid, "name": a.to, "was": was }))
    }

    fn label(_: &PlayheadRenameArgs, o: &Value) -> String {
        renamed("playhead", o)
    }
}

/// The ids of the machine and the playhead an effect addresses, and the patch second it is
/// stamped with, read off the settled model before the thread is told.
fn addressed(state: &AppState, machine: &str, playhead: &str) -> Result<(String, Machine, String, f64), String> {
    let g = state.graph.lock();
    let (id, m) = machine_of(&g, machine)?;
    let pid = id_of("playhead", &m.playheads, playhead)?;
    Ok((id, m, pid, g.time().now()))
}

impl EffectOp for Fire {
    fn run(state: &AppState, a: FireArgs, _: &Caller) -> Result<Value, String> {
        let (machine, m, playhead, at) = addressed(state, &a.machine, &a.playhead)?;
        if !m.transitions.contains_key(&a.transition) {
            return Err(format!("no transition `{}`", a.transition));
        }
        state.machines.send(Msg::Fire { at, machine, playhead, transition: a.transition });
        Ok(json!({ "ok": true }))
    }
}

impl EffectOp for Jump {
    fn run(state: &AppState, a: JumpArgs, _: &Caller) -> Result<Value, String> {
        let (machine, m, playhead, at) = addressed(state, &a.machine, &a.playhead)?;
        let target = id_of("state", &m.states, &a.state)?;
        state.machines.send(Msg::Jump { at, machine, playhead, state: target });
        Ok(json!({ "ok": true }))
    }
}

impl EffectOp for Reset {
    fn run(state: &AppState, a: ResetArgs, _: &Caller) -> Result<Value, String> {
        let (machine, at) = {
            let g = state.graph.lock();
            (a.machine.map(|m| machine_of(&g, &m).map(|(id, _)| id)).transpose()?, g.time().now())
        };
        state.machines.send(Msg::Reset { at, machine });
        Ok(json!({ "ok": true }))
    }
}
