//! The patch variables, and the control panels that draw a group of them as widgets.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{op, Any, EffectOp, NoArgs, ReadOp, WriteOp};
use crate::{inspect, AppState, Event, Txn};
use goofi_core::variables::{Control, ControlKind, Lock, VariableSource, VariableValue};
use goofi_graph::{Command, Graph};

// ---- variable list (Read)
op!(List, "variable list", 0, NoArgs, Value,
    "Every patch variable — what an expression can read and the variable writes can set — each with the lock that holds it (its own and its group's together), and every group that carries a lock. The `system` group is goofi's own: config-locked for life, and its EPHEMERAL members — `system.goofi_home` and the `system.audio_*` facts the audio engine publishes — are goofi's own value, never saved into a patch.",
    "{variables: [{name, type, value, lock: {config, value}, control?, source?}], groups: {group: {lock}}}");

// ---- variable entry add (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryAddArgs {
    pub name: Option<String>,
    pub group: Option<String>,
    #[serde(rename = "type")]
    pub ty: Option<String>,
    pub value: Option<Any>,
    /// Absent leaves the widget alone, `null` clears it, an object sets it.
    #[serde(default, deserialize_with = "super::nullable")]
    #[schemars(with = "Option<Value>")]
    pub control: Option<Option<Control>>,
}

op!(EntryAdd, "variable entry add", 1, EntryAddArgs, Value,
    "Create a patch variable. Give `group` alone to add a float entry with value 0 and the first free entry0/entry1/... name. Otherwise `name`, `type` and `value` are required. `name` is `group.element` — every variable is in a group. `type` is one of float/int/bool/string; a name the patch already holds is refused — `variable entry edit` changes one. `control` makes it a control-panel element: {kind, min, max, step, options, x, y, w, h}, where kind is knob/slider/number/text/toggle/dropdown/paint and must be able to draw the type.",
    "{name, value} — the name and value as stored");

// ---- variable entry edit (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryEditArgs {
    pub name: String,
    pub value: Option<Any>,
    #[serde(rename = "type")]
    pub ty: Option<String>,
    /// Absent leaves the widget alone, `null` clears it, an object sets it.
    #[serde(default, deserialize_with = "super::nullable")]
    #[schemars(with = "Option<Value>")]
    pub control: Option<Option<Control>>,
}

op!(EntryEdit, "variable entry edit", 1, EntryEditArgs, Value,
    "Change an existing variable's value, type-coerced to the type it holds. An explicit `type` changes the type, converting the current value when no value is supplied (an unsupported conversion uses an empty value); config-locked entries refuse type changes. A control widget must support the new type. A value-locked variable refuses the edit, and so does an ephemeral one (system.goofi_home, system.audio_*). `control` sets the control-panel widget and its place, `null` clears it, and giving one makes `value` optional — which is what a panel sends when it moves a widget; a config-locked variable refuses it.",
    "{value} — the value as stored, type-coerced");

// ---- variable entry remove (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryRemoveArgs {
    pub name: String,
}

op!(EntryRemove, "variable entry remove", 1, EntryRemoveArgs, Value,
    "Delete a patch variable. A config-locked one refuses, and a system variable always is.",
    "{removed: true}");

// ---- variable entry source (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntrySourceArgs {
    pub name: String,
    pub reference: String,
    pub index: Option<i64>,
}

op!(EntrySource, "variable entry source", 2, EntrySourceArgs, Value,
    "Make a variable FOLLOW one producer output, `node.slot`, at that producer's rate: the manager writes the variable on every frame that changes it, and nobody else may set it until the source is cleared with an empty reference. `index` picks one number out of a frame wider than one — a MIDI controller's `cc` is 128 of them — and a frame that holds one number needs none. The reference follows a node rename exactly as a param's does. A config-locked variable refuses; a value-locked one holds its value and takes nothing.",
    "{source: {reference, index} | null}");

// ---- variable entry lock (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryLockArgs {
    pub name: String,
    pub config: Option<bool>,
    pub value: Option<bool>,
}

op!(EntryLock, "variable entry lock", 1, EntryLockArgs, Value,
    "Lock or unlock one variable on its own account: `config` freezes its name, its widget and its place in a group, `value` freezes its value alone. An axis not named keeps what it has, and its group's lock holds it besides. The system group's locks are goofi's own.",
    "{lock: {config, value}} — the variable's own lock as stored");

// ---- variable entry rename (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryRenameArgs {
    pub name: String,
    pub to: String,
}

op!(EntryRename, "variable entry rename", 2, EntryRenameArgs, Value,
    "Rename a variable, and rewrite every expression that reads it. `to` is a full `group.element`, so one op both renames an element and moves it to another group.",
    "{name} — the name as stored");

// ---- variable group add (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupAddArgs {
    pub group: Option<String>,
}

op!(GroupAdd, "variable group add", 1, GroupAddArgs, Value,
    "Create an empty variables group. Without a name, use the first free group0/group1/... name.",
    "{group} — the created group name");

// ---- variable group rename (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupRenameArgs {
    pub from: String,
    pub to: String,
}

op!(GroupRename, "variable group rename", 2, GroupRenameArgs, Value,
    "Rename a group, moving every member with it and rewriting every expression that reads one. A config-locked group refuses, and the system group always does.",
    "{group} — the group as stored");

// ---- variable group lock (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupLockArgs {
    pub group: String,
    pub config: Option<bool>,
    pub value: Option<bool>,
}

op!(GroupLock, "variable group lock", 1, GroupLockArgs, Value,
    "Lock or unlock a whole group, reaching every member: `config` freezes every name, widget and the membership itself — nothing is added, renamed or removed — and `value` freezes every value. An axis not named keeps what it has. The system group's lock is goofi's own.",
    "{lock: {config, value}} — the group's lock as stored");

// ---- control list (Read)
op!(ControlList, "control list", 0, NoArgs, Value,
    "Every control panel and the group it draws, and every group holding a widget: each element with its value, its widget (`control`), its lock and what it follows (`source`).",
    "{panels: [{panel, group}], groups: {group: {lock, elements: [{name, element, type, value, control, lock, source?}]}}}");

// ---- control add (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlAddArgs {
    pub group: String,
    pub kind: String,
    pub element: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub w: Option<f64>,
    pub h: Option<f64>,
    pub value: Option<Any>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub options: Option<Value>,
}

op!(ControlAdd, "control add", 2, ControlAddArgs, Value,
    "Bear a widget in a control panel's group: a variable of the kind's own type, carrying the widget. `kind` is knob/slider/number/text/toggle/dropdown/paint. `element` is minted `knob0`, `knob1`, … when not given, and the cell is the first free one when `x`/`y` are not. A config-locked group refuses it, as it refuses every other edit to what it holds.",
    "{name, control} — the variable's full name and the widget as stored");

// ---- control edit (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlEditArgs {
    pub group: String,
    pub element: String,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub options: Option<Value>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub w: Option<f64>,
    pub h: Option<f64>,
}

op!(ControlEdit, "control edit", 2, ControlEditArgs, Value,
    "Change a widget: `name` renames the element (every expression reading it follows), and the rest re-shape the widget, its range, its options or its cell. ONE undo step, and refused by a config lock.",
    "{name} — the element's full name after the edit");

// ---- control remove (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlRemoveArgs {
    pub group: String,
    pub element: String,
}

op!(ControlRemove, "control remove", 2, ControlRemoveArgs, Value,
    "Delete a widget and the variable under it. Refused by a config lock.",
    "{removed: true}");

// ---- control paint (Effect)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlPaintArgs {
    pub group: String,
    pub element: String,
    pub steps: String,
}

op!(ControlPaint, "control paint", 2, ControlPaintArgs, Value,
    "Draw on a `paint` widget with turtle steps — another hand on the pad, not a second painter: the op parses the script and the WIDGET makes the strokes, by the code a mouse reaches. So a pad nobody has open draws nothing, and the reply says how many clients heard it. `steps` is one step per line and the whole block is one submission; `//` to end of line is a comment, and `#` cannot be one because it opens every colour. Coordinates span a 1000 square whatever pixel size the pad is, the origin is the TOP-left with y running down, heading 0 faces +x and `right` turns clockwise. The steps: `forward <d>`, `back <d>`, `left <deg>`, `right <deg>`, `heading <deg>`, `goto <x> <y>`, `home` (the middle, facing +x), `up`, `down`, `curve <c1x> <c1y> <c2x> <c2y> <x> <y>` — a cubic bezier in the turtle's OWN frame, +x along the heading and +y to its right, which it leaves along the curve's exit tangent — `pen <#rgb|#rrggbb|#rrggbbaa|erase>`, `width <w>`, `soft <s>` and `clear`. `pen`, `width` and `soft` are the widget's own colour, size and softness, so a script says what a hand would set.",
    "{steps, marks, clients} — the steps read, the strokes they make, and how many clients were listening");

// ---- control source (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlSourceArgs {
    pub group: String,
    pub element: String,
    pub reference: String,
    pub index: Option<i64>,
}

op!(ControlSource, "control source", 2, ControlSourceArgs, Value,
    "Make a widget FOLLOW one producer output, `node.slot`, with `index` picking one number out of a wide frame — a MIDI controller's `cc` is 128 of them — so a knob on a controller drives the widget. An empty reference clears it. Refused by a config lock.",
    "{source: {reference, index} | null}");

impl ReadOp for List {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(inspect::variables(&tx.g))
    }
}

impl WriteOp for EntryAdd {
    /// Create a typed variable.
    fn run(tx: &mut Txn, a: EntryAddArgs) -> Result<Value, String> {
        let name = match (&a.group, a.name) {
            (Some(_), Some(_)) => return Err("variable entry add: give either name or group".to_string()),
            (Some(group), None) => {
                if !tx.g.variables().has_group(group) {
                    return Err(format!("no variable group `{group}`"));
                }
                (0..).map(|i| format!("{group}.entry{i}"))
                    .find(|name| tx.g.variables().get(name).is_none())
                    .ok_or("no free entry name")?
            }
            (None, Some(name)) => name,
            (None, None) => return Err("variable entry add: missing field `name`".into()),
        };
        if tx.g.variables().get(&name).is_some() {
            return Err(format!("variable entry add: `{name}` already exists — `variable entry edit` changes it"));
        }
        let value = match (a.group.is_some(), a.ty, a.value) {
            (true, None, None) => VariableValue::Float(0.0),
            (_, ty, val) => {
                let ty = ty.ok_or("variable entry add: missing field `type`")?;
                let val = val.map(|v| v.0).filter(|v| !v.is_null()).ok_or("variable entry add: missing value")?;
                goofi_graph::variable_from_json(&json!({ "value": val, "type": ty }))
                    .ok_or_else(|| format!("variable entry add: `{val}` is not a {ty}"))?
            }
        };
        tx.apply(Command::EditVariable { name: name.clone(), value: Some(value.clone()), at: None, control: a.control })?;
        // As STORED: the conversion is type-directed, so a fraction into an int rounds.
        Ok(json!({ "name": name, "value": goofi_graph::variable_to_json(&value)["value"] }))
    }

    fn label(_: &EntryAddArgs, o: &Value) -> String {
        format!("Add variable {}", o["name"].as_str().unwrap_or_default())
    }
}

impl WriteOp for EntryEdit {
    fn run(tx: &mut Txn, a: EntryEditArgs) -> Result<Value, String> {
        let name = a.name;
        let held = tx.g.variables().get(&name).map(goofi_graph::variable_to_json);
        let Some(held) = held else {
            return Err(format!("variable entry edit: no variable `{name}` — `variable entry add` creates one"));
        };
        let ty = a.ty.clone().unwrap_or_else(|| held["type"].as_str().unwrap_or_default().to_string());
        // A control-only edit is what the panel sends when it moves a widget, so the value is optional
        // once a `control` is given — and the entry keeps the one it holds, followed or locked as it may be.
        let value = match a.value.map(|v| v.0).filter(|v| !v.is_null()) {
            Some(val) => Some(
                goofi_graph::variable_from_json(&json!({ "value": val, "type": ty }))
                    .ok_or_else(|| format!("variable entry edit: `{val}` is not a {ty}"))?,
            ),
            None if a.ty.is_some() => Some(
                tx.g.variables().get(&name).and_then(|value| value.converted_to(&ty))
                    .ok_or_else(|| format!("variable entry edit: unknown type `{ty}`"))?,
            ),
            None if a.control.is_some() => None,
            None => return Err("variable entry edit: missing value".to_string()),
        };
        let stored = match &value {
            Some(v) => goofi_graph::variable_to_json(v)["value"].clone(),
            None => held["value"].clone(),
        };
        tx.apply(Command::EditVariable { name, value, at: None, control: a.control })?;
        Ok(json!({ "value": stored }))
    }

    fn label(a: &EntryEditArgs, _: &Value) -> String {
        match (&a.ty, &a.value) {
            (Some(_), None) => format!("Change variable {} type", a.name),
            (_, None) => format!("Edit control {}", a.name),
            _ => format!("Set variable {}", a.name),
        }
    }
}

impl WriteOp for EntryRemove {
    fn run(tx: &mut Txn, a: EntryRemoveArgs) -> Result<Value, String> {
        if tx.g.variables().get(&a.name).is_none() {
            return Err(format!("variable entry remove: no variable `{}`", a.name));
        }
        tx.apply(Command::RemoveVariable { name: a.name })?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &EntryRemoveArgs, _: &Value) -> String {
        format!("Remove variable {}", a.name)
    }
}

/// `index` picks one number out of a frame wider than one; a whole number or nothing.
fn source_of(op: &str, reference: &str, index: Option<i64>) -> Result<Option<VariableSource>, String> {
    let index = index
        .map(|i| usize::try_from(i).map_err(|_| format!("{op}: `index` is a whole number, not `{i}`")))
        .transpose()?;
    let reference = reference.trim().to_string();
    Ok((!reference.is_empty()).then_some(VariableSource { reference, index }))
}

impl WriteOp for EntrySource {
    fn run(tx: &mut Txn, a: EntrySourceArgs) -> Result<Value, String> {
        if tx.g.variables().get(&a.name).is_none() {
            return Err(format!("variable entry source: no variable `{}`", a.name));
        }
        let source = source_of("variable entry source", &a.reference, a.index)?;
        tx.apply(Command::SourceVariable { name: a.name, source: source.clone() })?;
        Ok(json!({ "source": source }))
    }

    fn label(a: &EntrySourceArgs, _: &Value) -> String {
        format!("Source variable {}", a.name)
    }
}

/// The lock an edit asks for over `held`: an axis it does not name keeps what it has.
fn lock_over(held: Lock, config: Option<bool>, value: Option<bool>) -> Lock {
    Lock { config: config.unwrap_or(held.config), value: value.unwrap_or(held.value) }
}

impl WriteOp for EntryLock {
    fn run(tx: &mut Txn, a: EntryLockArgs) -> Result<Value, String> {
        if tx.g.variables().get(&a.name).is_none() {
            return Err(format!("variable entry lock: no variable `{}`", a.name));
        }
        let lock = lock_over(tx.g.variables().own_lock(&a.name), a.config, a.value);
        tx.apply(Command::LockVariable { name: a.name, lock })?;
        Ok(json!({ "lock": lock }))
    }

    fn label(a: &EntryLockArgs, _: &Value) -> String {
        format!("Lock variable {}", a.name)
    }
}

impl WriteOp for EntryRename {
    fn run(tx: &mut Txn, a: EntryRenameArgs) -> Result<Value, String> {
        tx.apply(Command::RenameVariable { from: a.name, to: a.to.clone() })?;
        Ok(json!({ "name": a.to }))
    }

    fn label(a: &EntryRenameArgs, _: &Value) -> String {
        format!("Rename variable {} → {}", a.name, a.to)
    }
}

impl WriteOp for GroupAdd {
    fn run(tx: &mut Txn, a: GroupAddArgs) -> Result<Value, String> {
        let group = match a.group {
            Some(group) => group,
            None => {
                let panels = tx.g.arrangement().control_panels();
                (0..).map(|i| format!("group{i}"))
                    .find(|name| !tx.g.variables().has_group(name) && !panels.iter().any(|(_, group)| group == name))
                    .ok_or("no free group name")?
            }
        };
        tx.apply(Command::AddVariableGroup { group: group.clone(), at: None })?;
        Ok(json!({ "group": group }))
    }

    fn label(_: &GroupAddArgs, o: &Value) -> String {
        format!("Add variable group {}", o["group"].as_str().unwrap_or_default())
    }
}

impl WriteOp for GroupRename {
    fn run(tx: &mut Txn, a: GroupRenameArgs) -> Result<Value, String> {
        tx.apply(Command::RenameVariableGroup { from: a.from, to: a.to.clone(), members: None })?;
        Ok(json!({ "group": a.to }))
    }

    fn label(a: &GroupRenameArgs, _: &Value) -> String {
        format!("Rename variable group {} → {}", a.from, a.to)
    }
}

impl WriteOp for GroupLock {
    fn run(tx: &mut Txn, a: GroupLockArgs) -> Result<Value, String> {
        let lock = lock_over(tx.g.variables().group_lock(&a.group), a.config, a.value);
        tx.apply(Command::LockVariableGroup { group: a.group, lock: Some(lock) })?;
        Ok(json!({ "lock": lock }))
    }

    fn label(a: &GroupLockArgs, _: &Value) -> String {
        format!("Lock variable group {}", a.group)
    }
}

fn parse_kind(v: &str) -> Result<ControlKind, String> {
    serde_json::from_value(json!(v)).map_err(|_| {
        let kinds = ControlKind::ALL.iter().map(|k| k.as_str()).collect::<Vec<_>>().join("/");
        format!("`kind` is one of {kinds}, not `{v}`")
    })
}

/// A control panel's element, addressed `group` + `element`, as the `control` ops read it.
fn element_of(g: &Graph, op: &str, group: &str, element: &str) -> Result<String, String> {
    let name = format!("{group}.{element}");
    if g.variables().control(&name).is_none() {
        return Err(format!("{op}: no element `{element}` in control panel `{group}`"));
    }
    Ok(name)
}

fn element_json(g: &Graph, name: &str, value: &VariableValue) -> Value {
    let mut e = goofi_graph::variable_to_json(value);
    e["name"] = json!(name);
    e["element"] = json!(name.split_once('.').map(|(_, el)| el).unwrap_or(name));
    e["lock"] = serde_json::to_value(g.variables().lock_of(name)).expect("a plain record");
    e["control"] = serde_json::to_value(g.variables().control(name)).expect("a plain record");
    if let Some(s) = g.variables().source(name) {
        e["source"] = serde_json::to_value(s).expect("a plain record");
    }
    e
}

impl ReadOp for ControlList {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        let panels: Vec<Value> =
            tx.g.arrangement().control_panels().into_iter().map(|(panel, group)| json!({ "panel": panel, "group": group })).collect();
        let mut groups = serde_json::Map::new();
        let mut order: Vec<String> = tx.g.arrangement().control_panels().into_iter().map(|(_, group)| group).collect();
        for (name, v) in tx.g.variables().entries() {
            if let (Some(_), Some((group, _))) = (&v.control, name.split_once('.')) {
                if !order.iter().any(|o| o == group) {
                    order.push(group.to_string());
                }
            }
        }
        for group in order {
            let elements: Vec<Value> = tx.g
                .variables()
                .entries()
                .filter(|(name, v)| v.control.is_some() && name.split_once('.').is_some_and(|(gr, _)| gr == group))
                .map(|(name, v)| element_json(&tx.g, name, &v.value))
                .collect();
            groups.insert(group.clone(), json!({ "lock": tx.g.variables().group_lock(&group), "elements": elements }));
        }
        Ok(json!({ "panels": panels, "groups": groups }))
    }
}

impl WriteOp for ControlAdd {
    /// Bear a widget: the manager mints its name and its cell where none is given.
    fn run(tx: &mut Txn, a: ControlAddArgs) -> Result<Value, String> {
        use goofi_core::variables::{free_cell, is_valid_identifier};
        let group = a.group;
        if !is_valid_identifier(&group) {
            return Err(format!("control add: invalid group `{group}`: {}", goofi_core::variables::VARIABLE_NAME_RULE));
        }
        let kind = parse_kind(&a.kind).map_err(|e| format!("control add: {e}"))?;
        let element = match a.element {
            Some(e) => e,
            None => (0..)
                .map(|n| format!("{}{n}", kind.as_str()))
                .find(|e| tx.g.variables().get(&format!("{group}.{e}")).is_none())
                .expect("the integers do not run out"),
        };
        let name = format!("{group}.{element}");
        if tx.g.variables().get(&name).is_some() {
            return Err(format!("control add: `{name}` already exists — `control edit` changes it"));
        }
        let born = kind.born_value();
        let value = match a.value.map(|v| v.0).filter(|v| !v.is_null()) {
            Some(v) => goofi_graph::variable_from_json(&json!({ "value": v, "type": born.type_name() }))
                .ok_or_else(|| format!("control add: `{v}` is not a {}", born.type_name()))?,
            None => born,
        };
        let (bw, bh) = kind.born_box();
        let (w, h) = (a.w.unwrap_or(bw), a.h.unwrap_or(bh));
        let (x, y) = match (a.x, a.y) {
            (Some(x), Some(y)) => (x, y),
            _ => {
                let taken: Vec<(f64, f64, f64, f64)> = tx.g
                    .variables()
                    .entries()
                    .filter(|(n, ..)| n.split_once('.').is_some_and(|(gr, _)| gr == group))
                    .filter_map(|(_, v)| v.control.as_ref().map(|c| (c.x, c.y, c.w, c.h)))
                    .collect();
                free_cell(&taken, w, h)
            }
        };
        let mut record = json!({ "kind": kind, "x": x, "y": y, "w": w, "h": h });
        for (key, v) in [("min", json!(a.min)), ("max", json!(a.max)), ("step", json!(a.step)), ("options", json!(a.options))] {
            if !v.is_null() {
                record[key] = v;
            }
        }
        if matches!(value, VariableValue::Float(_)) {
            for (key, or) in [("min", json!(0.0)), ("max", json!(1.0)), ("step", json!(0.01))] {
                record.as_object_mut().unwrap().entry(key).or_insert(or);
            }
        }
        let control: Control = serde_json::from_value(record.clone()).map_err(|e| format!("control add: {e}"))?;
        tx.apply(Command::EditVariable { name: name.clone(), value: Some(value), at: None, control: Some(Some(control)) })?;
        Ok(json!({ "name": name, "control": record }))
    }

    fn label(a: &ControlAddArgs, _: &Value) -> String {
        format!("Add {} to {}", a.kind, a.group)
    }
}

impl WriteOp for ControlEdit {
    fn run(tx: &mut Txn, a: ControlEditArgs) -> Result<Value, String> {
        let name = element_of(&tx.g, "control edit", &a.group, &a.element)?;
        let mut cmds = Vec::new();
        let mut target = name.clone();
        if let Some(to) = &a.name {
            target = format!("{}.{to}", a.group);
            if target != name {
                cmds.push(Command::RenameVariable { from: name.clone(), to: target.clone() });
            }
        }
        let mut record = serde_json::to_value(tx.g.variables().control(&name)).expect("a plain record");
        let mut touched = false;
        if let Some(kind) = &a.kind {
            parse_kind(kind).map_err(|e| format!("control edit: {e}"))?;
        }
        let fields = [
            ("kind", json!(a.kind)), ("min", json!(a.min)), ("max", json!(a.max)), ("step", json!(a.step)),
            ("options", json!(a.options)), ("x", json!(a.x)), ("y", json!(a.y)), ("w", json!(a.w)), ("h", json!(a.h)),
        ];
        for (key, v) in fields {
            if !v.is_null() {
                record[key] = v;
                touched = true;
            }
        }
        if touched {
            let control: Control = serde_json::from_value(record).map_err(|e| format!("control edit: {e}"))?;
            cmds.push(Command::EditVariable { name: target.clone(), value: None, at: None, control: Some(Some(control)) });
        }
        if cmds.is_empty() {
            return Err("control edit: nothing to change — give a name, a kind, a range, options or a cell".into());
        }
        tx.apply(Command::Compound(cmds))?;
        Ok(json!({ "name": target }))
    }

    fn label(a: &ControlEditArgs, _: &Value) -> String {
        format!("Edit {}.{}", a.group, a.element)
    }
}

impl WriteOp for ControlRemove {
    fn run(tx: &mut Txn, a: ControlRemoveArgs) -> Result<Value, String> {
        let name = element_of(&tx.g, "control remove", &a.group, &a.element)?;
        tx.apply(Command::RemoveVariable { name })?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &ControlRemoveArgs, _: &Value) -> String {
        format!("Remove {}.{}", a.group, a.element)
    }
}

impl EffectOp for ControlPaint {
    /// Turtle steps as the strokes a pad makes. The op PARSES — so a refusal names the line — and
    /// the widget draws, through the very code a hand at the pad reaches: the CLI is another hand
    /// on the same canvas, never a second painter. What the widget then commits is the one write.
    fn run(state: &AppState, a: ControlPaintArgs, _: &str) -> Result<Value, String> {
        let name = {
            let g = state.graph.lock();
            let name = element_of(&g, "control paint", &a.group, &a.element)?;
            match g.variables().control(&name).map(|c| c.kind) {
                Some(ControlKind::Paint) => name,
                Some(other) => {
                    return Err(format!("control paint: `{name}` is a {} widget; only a `paint` one takes steps", other.as_str()))
                }
                None => return Err(format!("control paint: `{name}` is not a control element")),
            }
        };
        let steps = goofi_core::turtle::parse(&a.steps).map_err(|e| format!("control paint: {e}"))?;
        let marks = goofi_core::turtle::marks(&steps);
        state.events.send(Event::ControlPaint { name, marks: json!(marks) });
        // A pad is drawn on by whoever has it OPEN, so what the caller needs to know is whether
        // anyone was listening. Zero clients is a script that went nowhere.
        Ok(json!({ "steps": steps.len(), "marks": marks.len(), "clients": state.events.listeners() }))
    }
}

impl WriteOp for ControlSource {
    fn run(tx: &mut Txn, a: ControlSourceArgs) -> Result<Value, String> {
        let name = element_of(&tx.g, "control source", &a.group, &a.element)?;
        let source = source_of("control source", &a.reference, a.index)?;
        tx.apply(Command::SourceVariable { name, source: source.clone() })?;
        Ok(json!({ "source": source }))
    }

    fn label(a: &ControlSourceArgs, _: &Value) -> String {
        format!("Source {}.{}", a.group, a.element)
    }
}
