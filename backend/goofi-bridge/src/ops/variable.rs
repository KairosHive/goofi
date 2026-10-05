//! The patch variables, and the control panels that draw a group of them as widgets.

use serde_json::{json, Value};

use super::{op, Any, NoArgs, ReadOp, WriteOp};
use crate::{inspect, Txn};
use goofi_core::variables::{Control, ControlKind, Lock};
use goofi_core::Data;
use goofi_graph::{Command, Graph};

op!(List, "variable list", 0, NoArgs,
    "Every patch variable — what an expression can read and the variable writes can set — each with the lock that holds it (its own and its group's together), and every group that carries a lock. A value is an array or a string: a number, a list (nested for a wider array) or text. The `system` group is goofi's own: config-locked for life, and its EPHEMERAL members — `system.goofi_home` and the `system.audio_*` facts the audio engine publishes — are goofi's own value, never saved into a patch.",
    "{variables: [{name, value, lock: {config, value}, control?, expression?, error?}], groups: {group: {lock}}}");

op!(EntryAdd, "variable entry add", 1, EntryAddArgs {
    pub name: Option<String>,
    pub group: Option<String>,
    pub value: Option<Any>,
    /// Absent leaves the widget alone, `null` clears it, an object sets it.
    #[serde(default, deserialize_with = "super::nullable")]
    #[schemars(with = "Option<Value>")]
    pub control: Option<Option<Control>>,
},
    "Create a patch variable. Give `group` alone to add an entry holding 0 under the first free entry0/entry1/... name. Otherwise `name` and `value` are required. `name` is `group.element` — every variable is in a group. `value` is a number, a list of numbers (nested for a wider array), a bool or a string; a bool is stored as 1 or 0, and a param reading a variable converts it to its own kind. A name the patch already holds is refused — `variable entry edit` changes one. `control` makes it a control-panel element: {kind, min, max, step, options, x, y, w, h}, where kind is knob/slider/number/text/toggle/dropdown/paint/vector/color and must be able to draw the value.",
    "{name, value} — the name and value as stored");

op!(EntryEdit, "variable entry edit", 1, EntryEditArgs {
    pub name: String,
    pub value: Option<Any>,
    /// Absent leaves the widget alone, `null` clears it, an object sets it.
    #[serde(default, deserialize_with = "super::nullable")]
    #[schemars(with = "Option<Value>")]
    pub control: Option<Option<Control>>,
    pub expression: Option<String>,
},
    "Change an existing variable's value: a number, a list of numbers (nested for a wider array), a bool or a string, as `variable entry add` takes it. A control widget must be able to draw the new value. A value-locked variable refuses the edit, and so does an ephemeral one (system.goofi_home, system.audio_*). `control` sets the control-panel widget and its place, `null` clears it, and giving one makes `value` optional — which is what a panel sends when it moves a widget; a config-locked variable refuses it. `expression` makes the variable COMPUTED, in the one language a param's expression is: bare — `nd('level')`, `nd('midi').out.cc[74]`, `variables.desk.gain` — the manager copies that one frame or element as it comes, at the producer's rate; around any Python it evaluates on its own worker. Nobody else may set a computed variable; an empty expression clears it. A name it spells follows a rename as a param's does.",
    "{value} — the value as stored");

op!(EntryRemove, "variable entry remove", 1, EntryRemoveArgs {
    pub name: String,
},
    "Delete a patch variable. A config-locked one refuses, and a system variable always is.",
    "{removed: true}");


op!(EntryLock, "variable entry lock", 1, EntryLockArgs {
    pub name: String,
    pub config: Option<bool>,
    pub value: Option<bool>,
},
    "Lock or unlock one variable on its own account: `config` freezes its name, its widget and its place in a group, `value` freezes its value alone. An axis not named keeps what it has, and its group's lock holds it besides. The system group's locks are goofi's own.",
    "{lock: {config, value}} — the variable's own lock as stored");

op!(EntryRename, "variable entry rename", 2, EntryRenameArgs {
    pub name: String,
    pub to: String,
},
    "Rename a variable, and rewrite every expression that reads it. `to` is a full `group.element`, so one op both renames an element and moves it to another group.",
    "{name} — the name as stored");

op!(GroupAdd, "variable group add", 1, GroupAddArgs {
    pub group: Option<String>,
},
    "Create an empty variables group. Without a name, use the first free group0/group1/... name.",
    "{group} — the created group name");

op!(GroupRename, "variable group rename", 2, GroupRenameArgs {
    pub from: String,
    pub to: String,
},
    "Rename a group, moving every member with it and rewriting every expression that reads one. A config-locked group refuses, and the system group always does.",
    "{group} — the group as stored");

op!(GroupLock, "variable group lock", 1, GroupLockArgs {
    pub group: String,
    pub config: Option<bool>,
    pub value: Option<bool>,
},
    "Lock or unlock a whole group, reaching every member: `config` freezes every name, widget and the membership itself — nothing is added, renamed or removed — and `value` freezes every value. An axis not named keeps what it has. The system group's lock is goofi's own.",
    "{lock: {config, value}} — the group's lock as stored");

op!(ControlList, "control list", 0, NoArgs,
    "Every control panel and the group it draws, and every group holding a widget: each element with its value, its widget (`control`), its lock and the expression that computes it.",
    "{panels: [{panel, group}], groups: {group: {lock, elements: [{name, element, value, control, lock, expression?, error?}]}}}");

op!(ControlAdd, "control add", 2, ControlAddArgs {
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
    pub resolution: Option<u32>,
},
    "Bear a widget in a control panel's group: a variable holding what the kind draws, carrying the widget. `kind` is knob/slider/number/text/toggle/dropdown/paint/vector/color. `element` is minted `knob0`, `knob1`, … when not given, and the cell is the first free one when `x`/`y` are not. A `paint` pad holds an `[h, w, 4]` RGBA array in 0..1, `resolution` texels a side (128 when not given); a `vector` an `[n]` array, born `[0, 0, 0]`; a `color` an `[4]` RGBA in 0..1. A config-locked group refuses it, as it refuses every other edit to what it holds.",
    "{name, control} — the variable's full name and the widget as stored");

op!(ControlEdit, "control edit", 2, ControlEditArgs {
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
    pub resolution: Option<u32>,
    pub expression: Option<String>,
},
    "Change a widget: `name` renames the element (every expression reading it follows), `expression` makes it computed as `variable entry edit` does — `nd('midi').out.cc[74]` puts a controller's knob on it, an empty one hands it back to the hand — and the rest re-shape the widget, its range, its options or its cell. `resolution` re-sizes a paint pad, which starts it clear. ONE undo step, and refused by a config lock.",
    "{name} — the element's full name after the edit");

op!(ControlRemove, "control remove", 2, ControlRemoveArgs {
    pub group: String,
    pub element: String,
},
    "Delete a widget and the variable under it. Refused by a config lock.",
    "{removed: true}");

op!(ControlPaint, "control paint", 2, ControlPaintArgs {
    pub group: String,
    pub element: String,
    pub ops: String,
},
    "Draw onto a `paint` widget's array, as ONE undoable edit of its variable — the pad sends each finished hand stroke through this same op. The variable holds the sheet as an `[h, w, 4]` RGBA array in 0..1; `node snapshot variables/<group>.<element> --raw` reads it back. `ops` is one op per line or `;`, `//` to end of line a comment: `stroke [ink] [width w] [soft s] [cap round|butt|square] [dash solid|dash|dot] : <path>`, `fill [ink] : <path>` and `clear`, which drops what came before. A path is `M x y` (move), `L x y` (line), `C x1 y1 x2 y2 x y` (cubic bezier) and `Z` (close), and must start with `M`. Ink is `#rgb`, `#rrggbb`, `#rrggbbaa` or `erase`; the stroke defaults are black, width 10, soft 0, cap round, dash solid. Coordinates, width and soft span 0..1000 whatever pixel size the pad is, the origin is the TOP-left with y running down.",
    "{ops, shape} — the ops drawn, and the sheet's shape");


impl ReadOp for List {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(inspect::variables(&tx.g))
    }
}

/// `v` as a variable's value: a number, a list, a bool or a string.
fn literal(v: Value) -> Result<Data, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("`{v}` is not a number, a list, a bool or a string: {e}"))
}

impl WriteOp for EntryAdd {
    fn run(tx: &mut Txn, a: EntryAddArgs) -> Result<Value, String> {
        let name = match (&a.group, a.name) {
            (Some(_), Some(_)) => return Err("give either name or group".to_string()),
            (Some(group), None) => {
                if !tx.g.variables().has_group(group) {
                    return Err(format!("no variable group `{group}`"));
                }
                goofi_core::fresh_name(&format!("{group}.entry"), 0, |n| tx.g.variables().get(n).is_some())
            }
            (None, Some(name)) => name,
            (None, None) => return Err("missing field `name`".into()),
        };
        if tx.g.variables().get(&name).is_some() {
            return Err(format!("`{name}` already exists — `variable entry edit` changes it"));
        }
        let value = match (a.group.is_some(), a.value.map(|v| v.0).filter(|v| !v.is_null())) {
            (true, None) => Data::number(0.0),
            (_, val) => literal(val.ok_or("missing value")?)?,
        };
        tx.apply(Command::EditVariable { name: name.clone(), value: Some(value.clone()), at: None, control: a.control })?;
        Ok(json!({ "name": name, "value": value }))
    }

    fn label(_: &EntryAddArgs, o: &Value) -> String {
        format!("Add variable {}", o["name"].as_str().unwrap_or_default())
    }
}

impl WriteOp for EntryEdit {
    fn run(tx: &mut Txn, a: EntryEditArgs) -> Result<Value, String> {
        let name = a.name;
        let Some(held) = tx.g.variables().get(&name).cloned() else {
            return Err(format!("no variable `{name}` — `variable entry add` creates one"));
        };
        // A control-only edit is what the panel sends when it moves a widget, so the value is optional
        // once a `control` or an `expression` is given — and the entry keeps the one it holds.
        let value = match a.value.map(|v| v.0).filter(|v| !v.is_null()) {
            Some(val) => Some(literal(val)?),
            None if a.control.is_some() || a.expression.is_some() => None,
            None => return Err("missing value".to_string()),
        };
        let stored = value.clone().unwrap_or(held);
        let mut cmds = Vec::new();
        if value.is_some() || a.control.is_some() {
            cmds.push(Command::EditVariable { name: name.clone(), value, at: None, control: a.control });
        }
        if let Some(expression) = a.expression {
            cmds.push(Command::SourceVariable { name, expression: Some(expression) });
        }
        tx.apply(compound(cmds))?;
        Ok(json!({ "value": stored }))
    }

    fn label(a: &EntryEditArgs, _: &Value) -> String {
        match (&a.value, &a.expression) {
            (None, Some(_)) => format!("Compute variable {}", a.name),
            (None, None) => format!("Edit control {}", a.name),
            _ => format!("Set variable {}", a.name),
        }
    }
}

/// One command or several as one undo step.
fn compound(mut cmds: Vec<Command>) -> Command {
    match cmds.len() {
        1 => cmds.pop().expect("one"),
        _ => Command::Compound(cmds),
    }
}

impl WriteOp for EntryRemove {
    fn run(tx: &mut Txn, a: EntryRemoveArgs) -> Result<Value, String> {
        tx.apply(Command::RemoveVariable { name: a.name })?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &EntryRemoveArgs, _: &Value) -> String {
        format!("Remove variable {}", a.name)
    }
}


/// The lock an edit asks for over `held`: an axis it does not name keeps what it has.
fn lock_over(held: Lock, config: Option<bool>, value: Option<bool>) -> Lock {
    Lock { config: config.unwrap_or(held.config), value: value.unwrap_or(held.value) }
}

impl WriteOp for EntryLock {
    fn run(tx: &mut Txn, a: EntryLockArgs) -> Result<Value, String> {
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
        let group = a.group.unwrap_or_else(|| goofi_core::fresh_name("group", 0, |n| tx.g.group_taken(n)));
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
fn element_of(g: &Graph, group: &str, element: &str) -> Result<String, String> {
    let name = format!("{group}.{element}");
    if g.variables().control(&name).is_none() {
        return Err(format!("no element `{element}` in control panel `{group}`"));
    }
    Ok(name)
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
        let store = tx.g.variables();
        for group in order {
            let elements: Vec<Value> = store
                .entries()
                .filter(|(name, v)| v.control.is_some() && name.split_once('.').is_some_and(|(gr, _)| gr == group))
                .map(|(name, v)| {
                    let mut e = inspect::variable_json(&tx.g, &store, name, v);
                    e["element"] = json!(name.split_once('.').map_or(name, |(_, el)| el));
                    e
                })
                .collect();
            groups.insert(group.clone(), json!({ "lock": store.group_lock(&group), "elements": elements }));
        }
        Ok(json!({ "panels": panels, "groups": groups }))
    }
}

impl WriteOp for ControlAdd {
    fn run(tx: &mut Txn, a: ControlAddArgs) -> Result<Value, String> {
        use goofi_core::variables::{free_cell, is_valid_identifier};
        let group = a.group;
        if !is_valid_identifier(&group) {
            return Err(format!("invalid group `{group}`: {}", goofi_core::variables::VARIABLE_NAME_RULE));
        }
        let kind = parse_kind(&a.kind)?;
        let element = match a.element {
            Some(e) => e,
            None => goofi_core::fresh_name(kind.as_str(), 0, |e| tx.g.variables().get(&format!("{group}.{e}")).is_some()),
        };
        let name = format!("{group}.{element}");
        if tx.g.variables().get(&name).is_some() {
            return Err(format!("`{name}` already exists — `control edit` changes it"));
        }
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
        for (key, v) in [("min", json!(a.min)), ("max", json!(a.max)), ("step", json!(a.step)), ("options", json!(a.options)), ("resolution", json!(a.resolution))] {
            if !v.is_null() {
                record[key] = v;
            }
        }
        if matches!(kind, ControlKind::Knob | ControlKind::Slider | ControlKind::Number | ControlKind::Vector) {
            for (key, or) in [("min", json!(0.0)), ("max", json!(1.0)), ("step", json!(0.01))] {
                record.as_object_mut().unwrap().entry(key).or_insert(or);
            }
        }
        let control: Control = serde_json::from_value(record.clone()).map_err(|e| e.to_string())?;
        let value = match a.value.map(|v| v.0).filter(|v| !v.is_null()) {
            Some(v) => literal(v)?,
            None => control.born_value(),
        };
        tx.apply(Command::EditVariable { name: name.clone(), value: Some(value), at: None, control: Some(Some(control)) })?;
        Ok(json!({ "name": name, "control": record }))
    }

    fn label(a: &ControlAddArgs, _: &Value) -> String {
        format!("Add {} to {}", a.kind, a.group)
    }
}

impl WriteOp for ControlEdit {
    fn run(tx: &mut Txn, a: ControlEditArgs) -> Result<Value, String> {
        let name = element_of(&tx.g, &a.group, &a.element)?;
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
            parse_kind(kind)?;
        }
        let fields = [
            ("kind", json!(a.kind)), ("min", json!(a.min)), ("max", json!(a.max)), ("step", json!(a.step)),
            ("options", json!(a.options)), ("x", json!(a.x)), ("y", json!(a.y)), ("w", json!(a.w)), ("h", json!(a.h)),
            ("resolution", json!(a.resolution)),
        ];
        for (key, v) in fields {
            if !v.is_null() {
                record[key] = v;
                touched = true;
            }
        }
        if touched {
            let control: Control = serde_json::from_value(record).map_err(|e| e.to_string())?;
            // A pad re-sized starts clear: its array is the one shape the widget draws.
            let value = (a.resolution.is_some() && control.kind == ControlKind::Paint).then(|| control.born_value());
            cmds.push(Command::EditVariable { name: target.clone(), value, at: None, control: Some(Some(control)) });
        }
        if let Some(expression) = a.expression {
            cmds.push(Command::SourceVariable { name: target.clone(), expression: Some(expression) });
        }
        if cmds.is_empty() {
            return Err("nothing to change — give a name, a kind, a range, options, a cell or an expression".into());
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
        let name = element_of(&tx.g, &a.group, &a.element)?;
        tx.apply(Command::RemoveVariable { name })?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &ControlRemoveArgs, _: &Value) -> String {
        format!("Remove {}.{}", a.group, a.element)
    }
}

/// The `paint` widget `group.element`, and the sheet it holds.
fn sheet_of(g: &Graph, group: &str, element: &str) -> Result<(String, Data), String> {
    let name = element_of(g, group, element)?;
    let variables = g.variables();
    match (variables.control(&name).map(|c| c.kind), variables.get(&name)) {
        (Some(ControlKind::Paint), Some(sheet)) => Ok((name, sheet.clone())),
        (kind, _) => {
            let kind = kind.map_or("", ControlKind::as_str);
            Err(format!("`{name}` is a {kind} widget; only a `paint` one holds a sheet"))
        }
    }
}

impl WriteOp for ControlPaint {
    fn run(tx: &mut Txn, a: ControlPaintArgs) -> Result<Value, String> {
        let (name, sheet) = sheet_of(&tx.g, &a.group, &a.element)?;
        let ops = goofi_core::drawing::parse(&a.ops)?;
        if ops.is_empty() {
            return Err("no ops to draw".into());
        }
        let goofi_core::Value::Array(held) = sheet.value() else { return Err(format!("`{name}` holds no sheet")) };
        let [h, w, 4] = held.shape() else { return Err(format!("`{name}` holds {:?}, not a sheet", held.shape())) };
        let texels: Vec<f32> = held.values().collect();
        let drawn = goofi_core::drawing::raster(&texels, *w as u32, *h as u32, &ops)?;
        let bytes = drawn.iter().flat_map(|v| v.to_le_bytes()).collect();
        let value = Data::array_f32(vec![*h, *w, 4], bytes, goofi_core::Meta::default()).map_err(|e| e.to_string())?;
        tx.apply(Command::EditVariable { name, value: Some(value), at: None, control: None })?;
        Ok(json!({ "ops": ops.len(), "shape": [h, w, 4] }))
    }

    fn label(a: &ControlPaintArgs, _: &Value) -> String {
        format!("Paint {}.{}", a.group, a.element)
    }
}

