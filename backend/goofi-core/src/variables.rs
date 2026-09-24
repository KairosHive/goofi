//! Patch-scoped variables — named typed scalars shared across a patch.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// A patch variable's value — a typed scalar. The serde shape is the `{type, value}` of the `.gfi`
/// and the doc: the tag is what preserves float-vs-int through JSON's whole-float normalization.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum VariableValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    #[serde(rename = "string")]
    Str(String),
}

impl VariableValue {
    /// The type's name, as the doc and every op spell it.
    pub fn type_name(&self) -> &'static str {
        match self {
            VariableValue::Float(_) => "float",
            VariableValue::Int(_) => "int",
            VariableValue::Bool(_) => "bool",
            VariableValue::Str(_) => "string",
        }
    }

    /// Convert to a named type, using an empty value when conversion is not possible.
    pub fn converted_to(&self, ty: &str) -> Option<VariableValue> {
        let template = match ty {
            "float" => VariableValue::Float(0.0),
            "int" => VariableValue::Int(0),
            "bool" => VariableValue::Bool(false),
            "string" => VariableValue::Str(String::new()),
            _ => return None,
        };
        Some(self.clone().coerced_like(&template))
    }

    /// Coerce to `template`'s variant, so an existing variable's declared type stays stable on set.
    fn coerced_like(self, template: &VariableValue) -> VariableValue {
        use VariableValue as G;
        match (template, self) {
            (G::Float(_), G::Int(v)) => G::Float(v as f64),
            (G::Float(_), G::Bool(v)) => G::Float(if v { 1.0 } else { 0.0 }),
            (G::Float(_), G::Str(_)) => G::Float(0.0),
            (G::Int(_), G::Float(v)) => G::Int(v.round() as i64),
            (G::Int(_), G::Bool(v)) => G::Int(v.into()),
            (G::Int(_), G::Str(_)) => G::Int(0),
            (G::Bool(_), G::Float(_) | G::Int(_) | G::Str(_)) => G::Bool(false),
            (G::Str(_), G::Float(v)) => G::Str(v.to_string()),
            (G::Str(_), G::Int(v)) => G::Str(v.to_string()),
            (G::Str(_), G::Bool(v)) => G::Str(v.to_string()),
            (_, same) => same,
        }
    }
}

/// What a control element is drawn as.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlKind {
    Knob,
    Slider,
    Number,
    Text,
    Toggle,
    Dropdown,
    Paint,
}

impl ControlKind {
    /// Every kind, in the order a palette offers them.
    pub const ALL: [ControlKind; 7] = [
        ControlKind::Knob,
        ControlKind::Slider,
        ControlKind::Number,
        ControlKind::Text,
        ControlKind::Toggle,
        ControlKind::Dropdown,
        ControlKind::Paint,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ControlKind::Knob => "knob",
            ControlKind::Slider => "slider",
            ControlKind::Number => "number",
            ControlKind::Text => "text",
            ControlKind::Toggle => "toggle",
            ControlKind::Dropdown => "dropdown",
            ControlKind::Paint => "paint",
        }
    }

    /// The value a widget of this kind is born holding — which is also its type.
    pub fn born_value(self) -> VariableValue {
        match self {
            ControlKind::Knob | ControlKind::Slider | ControlKind::Number => VariableValue::Float(0.0),
            ControlKind::Toggle => VariableValue::Bool(false),
            // A drawing is a `data:image/png;base64,…` URL, which is a STRING like any other: the
            // widget draws it, an expression reads it, and nothing new crosses the wire for it.
            ControlKind::Text | ControlKind::Dropdown | ControlKind::Paint => VariableValue::Str(String::new()),
        }
    }

    /// The box it is born in, in grid units.
    pub fn born_box(self) -> (f64, f64) {
        match self {
            ControlKind::Knob => (4.0, 4.0),
            ControlKind::Slider => (8.0, 2.0),
            ControlKind::Number => (4.0, 2.0),
            // A text widget is born THREE rows tall because it is a text area, not a line: a poem is
            // what people put in one, and a one-line box says the opposite.
            ControlKind::Text => (6.0, 3.0),
            ControlKind::Dropdown => (6.0, 2.0),
            ControlKind::Toggle => (2.0, 2.0),
            ControlKind::Paint => (8.0, 8.0),
        }
    }
}

/// How many columns a control panel's grid is, whatever its pixel width.
pub const CONTROL_COLUMNS: f64 = 16.0;

/// Where a `w × h` box lands among `taken` boxes `(x, y, w, h)`: the first free cell in reading
/// order, never off the right edge.
pub fn free_cell(taken: &[(f64, f64, f64, f64)], w: f64, h: f64) -> (f64, f64) {
    let w = w.clamp(1.0, CONTROL_COLUMNS);
    let h = h.max(1.0);
    let overlaps = |x: f64, y: f64| {
        taken.iter().any(|(tx, ty, tw, th)| x < tx + tw && *tx < x + w && y < ty + th && *ty < y + h)
    };
    let mut y = 0.0;
    loop {
        let mut x = 0.0;
        while x + w <= CONTROL_COLUMNS {
            if !overlaps(x, y) {
                return (x, y);
            }
            x += 1.0;
        }
        y += 1.0;
    }
}

/// A variable drawn in a control panel: the widget, its range, and its place in the grid. Carrying
/// one is what makes a variable an ELEMENT — there is no second list of what a panel holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Control {
    pub kind: ControlKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub w: f64,
    #[serde(default)]
    pub h: f64,
}

impl Control {
    /// Whether this widget can draw `value`'s type.
    pub fn fits(&self, value: &VariableValue) -> bool {
        use ControlKind as K;
        use VariableValue as G;
        match self.kind {
            K::Knob | K::Slider | K::Number => matches!(value, G::Float(_) | G::Int(_)),
            K::Toggle => matches!(value, G::Bool(_)),
            K::Text | K::Dropdown | K::Paint => matches!(value, G::Str(_)),
        }
    }

    /// Why this widget cannot draw `value`, in the words a refusal uses.
    pub fn mismatch(&self, value: &VariableValue) -> String {
        format!("a `{}` cannot draw a {}", self.kind.as_str(), value.type_name())
    }
}

/// A lock on a variable or a whole group: `config` freezes the name, the type, the widget and
/// membership; `value` freezes the value alone. A group's lock reaches every member.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lock {
    #[serde(default)]
    pub config: bool,
    #[serde(default)]
    pub value: bool,
}

impl Lock {
    pub fn is_default(self) -> bool {
        self == Lock::default()
    }
    /// This lock and `other` together: an axis is locked when either locks it.
    pub fn or(self, other: Lock) -> Lock {
        Lock { config: self.config || other.config, value: self.value || other.value }
    }
}

/// What a variable follows: one producer output, `node.slot`, and for a frame wider than one
/// number the index it reads. A followed variable is written by the manager on every frame and by
/// nobody else — a MIDI knob bound to a widget is one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VariableSource {
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

/// A code-owned system variable: its group is config-locked for life. An EPHEMERAL one is
/// value-locked too — goofi derives its value, it is re-derived at every reassert, and a `.gfi`
/// never carries it.
pub struct VariableDef {
    pub name: &'static str,
    pub value: fn() -> VariableValue,
    pub doc: &'static str,
    /// Whether goofi owns the value outright: nobody may set it, and no patch carries it.
    pub ephemeral: bool,
}

/// What a texture is when nothing says otherwise: the two default-size variables start here, and a
/// graphics chain with nothing to follow falls back to it.
pub const DEFAULT_SIZE: u32 = 1024;

pub static SYSTEM_VARIABLES: &[VariableDef] = &[
    VariableDef {
        name: "system.default_ufreq",
        value: || VariableValue::Float(30.0),
        doc: "Default update rate (Hz) for producer nodes that have not overridden it.",
        ephemeral: false,
    },
    VariableDef {
        name: "system.viewer_fps",
        value: || VariableValue::Float(30.0),
        doc: "The fastest a viewer is served (frames a second), whatever rate its display declares.",
        ephemeral: false,
    },
    VariableDef {
        name: "system.default_width",
        value: || VariableValue::Int(DEFAULT_SIZE as i64),
        doc: "Default texture width (pixels) for graphics nodes that make their own frames.",
        ephemeral: false,
    },
    VariableDef {
        name: "system.default_height",
        value: || VariableValue::Int(DEFAULT_SIZE as i64),
        doc: "Default texture height (pixels) for graphics nodes that make their own frames.",
        ephemeral: false,
    },
    VariableDef {
        name: "system.audio_rate",
        value: || VariableValue::Float(0.0),
        doc: "The audio clock's sample rate. The audio engine says it; 0 where no engine runs.",
        ephemeral: true,
    },
    VariableDef {
        name: "system.audio_channels",
        value: || VariableValue::Int(0),
        doc: "How many channels the audio clock carries. The audio engine says it; 0 where no engine runs.",
        ephemeral: true,
    },
    VariableDef {
        name: "system.audio_device",
        value: || VariableValue::Str(String::new()),
        doc: "The device driving the audio clock, empty under the external clock or where none is open.",
        ephemeral: true,
    },
    VariableDef {
        name: "system.audio_driver",
        value: || VariableValue::Str(String::new()),
        doc: "The ASIO driver holding the process, empty where none does — one loads at a time, so it is the patch's.",
        ephemeral: true,
    },
    VariableDef {
        name: "system.audio_hosts",
        value: || VariableValue::Str(String::new()),
        doc: "The audio APIs this build carries, comma separated. Every device name begins with one of them, so this is the whole of what a device can be chosen from.",
        ephemeral: true,
    },
    VariableDef {
        name: "system.goofi_home",
        value: || VariableValue::Str(crate::path::to_slash(&crate::home::dir())),
        doc: "The .goofi folder, where goofi keeps its own files. The machine says where it is.",
        ephemeral: true,
    },
];

/// Python's keywords, plus goofi's own namespace token `variables`. A regex reads each as an
/// identifier and a parser does not, so a name that is one cannot be an attribute — which is the
/// position every name here is read in.
const RESERVED: &[&str] = &[
    "variables", "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
    "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "variable", "if",
    "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try",
    "while", "with", "yield",
];

/// A legal name in the ONE expression namespace: `[A-Za-z_][A-Za-z0-9_]*` and not reserved.
///
/// Every name an expression can spell is held to this, because an expression reads one as an
/// ATTRIBUTE — `variables.gain`, and a sub-patch's slot in `nd('chain').drain`. A name Python cannot
/// parse there breaks every reference to it and takes the rewrite with it: the next rename has no
/// `nd('<old>')` left to follow, so the damage cannot be undone by renaming back.
pub fn is_valid_identifier(name: &str) -> bool {
    if RESERVED.contains(&name) {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// What a node or slot name has to be, said once — it is the tail of every refusal about one.
pub const NAME_RULE: &str =
    "a letter then letters or digits, and not a Python keyword — an expression reads a name as an attribute, and a reference spells `node.slot`";

/// The group and the element of a variable's name, or `None` when it is not `group.element`.
pub fn split_variable(name: &str) -> Option<(&str, &str)> {
    let (group, element) = name.split_once('.')?;
    match is_valid_identifier(group) && is_valid_identifier(element) {
        true => Some((group, element)),
        false => None,
    }
}

/// A legal variable name: a group and an element, each an identifier. Every variable is in a group.
pub fn is_valid_variable_name(name: &str) -> bool {
    split_variable(name).is_some()
}

/// What a variable's name has to be, said once — it is the tail of every refusal about one.
pub const VARIABLE_NAME_RULE: &str =
    "a group and an element, `group.element`, each a letter or underscore then letters, digits or underscores, and neither a Python keyword";

/// A legal node or slot name: `[A-Za-z][A-Za-z0-9]*` and not reserved. Narrower than a variable's
/// identifier so that `node.slot` needs no quoting anywhere it is spelled.
pub fn is_valid_name(name: &str) -> bool {
    if RESERVED.contains(&name) {
        return false;
    }
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric())
}

/// The group `system`, which is goofi's own: born config-locked, and no lock of its is a caller's
/// to set.
pub const SYSTEM_GROUP: &str = "system";

fn is_ephemeral(name: &str) -> bool {
    SYSTEM_VARIABLES.iter().any(|d| d.ephemeral && d.name == name)
}

fn group_of(name: &str) -> &str {
    split_variable(name).map(|(g, _)| g).unwrap_or(name)
}

/// The authoritative variables map. Locks decide what a caller may change, and the insertion order
/// is observable (the panel, the `.gfi` and the mirror all read it).
#[derive(Clone)]
pub struct VariableStore {
    values: IndexMap<String, VariableValue>,
    controls: IndexMap<String, Control>,
    sources: IndexMap<String, VariableSource>,
    locks: IndexMap<String, Lock>,
    groups: IndexMap<String, Lock>,
}

impl Default for VariableStore {
    fn default() -> VariableStore {
        VariableStore::new()
    }
}

impl VariableStore {
    pub fn new() -> VariableStore {
        let mut s = VariableStore {
            values: IndexMap::new(),
            controls: IndexMap::new(),
            sources: IndexMap::new(),
            locks: IndexMap::new(),
            groups: IndexMap::new(),
        };
        s.reassert_system();
        s
    }

    /// Back-fill any missing system variable with its default — on construction and after a load —
    /// and re-lock the system group. An EPHEMERAL one is overwritten instead: goofi says what it
    /// holds, never a file.
    pub fn reassert_system(&mut self) {
        for def in SYSTEM_VARIABLES {
            if def.ephemeral {
                self.values.insert(def.name.to_string(), (def.value)());
                self.locks.insert(def.name.to_string(), Lock { config: false, value: true });
            } else {
                self.values.entry(def.name.to_string()).or_insert_with(def.value);
            }
        }
        self.groups.insert(SYSTEM_GROUP.to_string(), Lock { config: true, value: false });
    }

    pub fn get(&self, name: &str) -> Option<&VariableValue> {
        self.values.get(name)
    }
    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// Every variable in order, with its OWN lock, the control record that makes it an element, and
    /// the source it follows.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &VariableValue, Lock, Option<&Control>, Option<&VariableSource>)> {
        self.values.iter().map(|(k, v)| {
            (k.as_str(), v, self.locks.get(k).copied().unwrap_or_default(), self.controls.get(k), self.sources.get(k))
        })
    }

    pub fn source(&self, name: &str) -> Option<&VariableSource> {
        self.sources.get(name)
    }

    /// Set or clear what a variable follows, answering what it followed. A source is config.
    pub fn set_source(&mut self, name: &str, source: Option<VariableSource>) -> Result<Option<VariableSource>, String> {
        if !self.values.contains_key(name) {
            return Err(format!("no such variable `{name}`"));
        }
        self.config_locked(name)?;
        Ok(match source {
            Some(s) => self.sources.insert(name.to_string(), s),
            None => self.sources.shift_remove(name),
        })
    }

    /// The follower's own write: what the source delivered, coerced to the type held. It answers
    /// whether the value CHANGED, and a value-locked variable takes nothing, silently.
    pub fn follow(&mut self, name: &str, value: VariableValue) -> bool {
        // A variable with no source has no follower: a pick already in flight when one is cleared
        // would otherwise land after, and overwrite the value the clearing author then typed.
        if is_ephemeral(name) || self.lock_of(name).value || !self.sources.contains_key(name) {
            return false;
        }
        let Some(existing) = self.values.get(name) else { return false };
        let coerced = value.coerced_like(existing);
        if *existing == coerced {
            return false;
        }
        self.values.insert(name.to_string(), coerced);
        true
    }

    /// An ENGINE's own published fact, which is why it passes the value lock: the lock exists to
    /// keep every other writer out, and the engine is the one it is held for. Only an ephemeral
    /// name takes one. Answers whether the value MOVED, which is what a rebind is worth doing for.
    pub fn publish(&mut self, name: &str, value: VariableValue) -> bool {
        if !is_ephemeral(name) {
            return false;
        }
        let Some(existing) = self.values.get(name) else { return false };
        let coerced = value.coerced_like(existing);
        if *existing == coerced {
            return false;
        }
        self.values.insert(name.to_string(), coerced);
        true
    }

    /// Explicit groups and their built-in flags, in creation order.
    pub fn groups(&self) -> impl Iterator<Item = (&str, Lock)> {
        self.groups.iter().map(|(g, l)| (g.as_str(), *l))
    }

    /// Whether a `.gfi` must leave `name` out: an ephemeral variable's value is goofi's own.
    pub fn is_ephemeral(&self, name: &str) -> bool {
        is_ephemeral(name)
    }

    /// A variable's OWN lock, apart from its group's.
    pub fn own_lock(&self, name: &str) -> Lock {
        self.locks.get(name).copied().unwrap_or_default()
    }

    pub fn group_lock(&self, group: &str) -> Lock {
        self.groups.get(group).copied().unwrap_or_default()
    }

    /// What holds `name` right now: its own lock and its group's together.
    pub fn lock_of(&self, name: &str) -> Lock {
        self.locks.get(name).copied().unwrap_or_default().or(self.group_lock(group_of(name)))
    }

    /// Set a variable's own lock, answering the one it held. The system group's are not a caller's.
    pub fn set_lock(&mut self, name: &str, lock: Lock) -> Result<Lock, String> {
        if group_of(name) == SYSTEM_GROUP {
            return Err(format!("`{name}` is goofi's own; its lock is not yours to set"));
        }
        if !self.values.contains_key(name) {
            return Err(format!("no such variable `{name}`"));
        }
        let old = self.locks.get(name).copied().unwrap_or_default();
        match lock.is_default() {
            true => drop(self.locks.shift_remove(name)),
            false => drop(self.locks.insert(name.to_string(), lock)),
        }
        Ok(old)
    }

    /// Set or remove a group record, answering the previous record for undo.
    pub fn set_group_lock(&mut self, group: &str, lock: Option<Lock>) -> Result<Option<Lock>, String> {
        if group == SYSTEM_GROUP {
            return Err(format!("`{SYSTEM_GROUP}` is goofi's own; its lock is not yours to set"));
        }
        if !is_valid_identifier(group) {
            return Err(format!("invalid group name `{group}`: {VARIABLE_NAME_RULE}"));
        }
        Ok(match lock {
            Some(lock) => self.groups.insert(group.to_string(), lock),
            None => self.groups.shift_remove(group),
        })
    }

    /// Create an empty group at its saved position.
    pub fn add_group(&mut self, group: &str, at: Option<usize>) -> Result<(), String> {
        if !is_valid_identifier(group) {
            return Err(format!("invalid group name `{group}`: {VARIABLE_NAME_RULE}"));
        }
        if self.has_group(group) {
            return Err(format!("variable group `{group}` already exists"));
        }
        let at = at.unwrap_or(self.groups.len()).min(self.groups.len());
        self.groups.shift_insert(at, group.to_string(), Lock::default());
        Ok(())
    }

    pub fn group_index(&self, group: &str) -> Option<usize> {
        self.groups.get_index_of(group)
    }

    /// Remove an empty, unlocked group.
    pub fn remove_group(&mut self, group: &str) -> Result<(), String> {
        if self.group_lock(group).config || self.group_lock(group).value {
            return Err(format!("variable group `{group}` is locked"));
        }
        if self.values.keys().any(|name| group_of(name) == group) {
            return Err(format!("variable group `{group}` is not empty"));
        }
        self.groups.shift_remove(group).ok_or_else(|| format!("no variable group `{group}`"))?;
        Ok(())
    }

    fn config_locked(&self, name: &str) -> Result<(), String> {
        match self.lock_of(name).config {
            true if group_of(name) == SYSTEM_GROUP => Err(format!("`{name}` is a system variable; its name is goofi's")),
            true => Err(format!("variable `{name}` is config-locked")),
            false => Ok(()),
        }
    }

    pub fn control(&self, name: &str) -> Option<&Control> {
        self.controls.get(name)
    }

    pub fn check_control(&self, name: &str, value: &VariableValue, control: Option<&Control>) -> Result<(), String> {
        self.config_locked(name)?;
        match control {
            Some(c) if !c.fits(value) => Err(c.mismatch(value)),
            _ => Ok(()),
        }
    }

    pub fn set_control(&mut self, name: &str, control: Option<Control>) -> Result<(), String> {
        let value = self.values.get(name).ok_or_else(|| format!("no such variable `{name}`"))?;
        self.check_control(name, value, control.as_ref())?;
        match control {
            Some(c) => { self.controls.insert(name.to_string(), c); }
            None => { self.controls.shift_remove(name); }
        }
        Ok(())
    }

    /// Set an existing variable. A type change also requires an unlocked configuration.
    pub fn set(&mut self, name: &str, value: VariableValue) -> Result<(), String> {
        if is_ephemeral(name) {
            return Err(format!("variable `{name}` is read-only: it is ephemeral, and goofi says what it holds"));
        }
        if self.lock_of(name).value {
            return Err(format!("variable `{name}` is value-locked"));
        }
        if let Some(s) = self.sources.get(name) {
            return Err(format!("variable `{name}` follows `{}`; clear its source to set it", s.reference));
        }
        match self.values.get(name) {
            Some(existing) => {
                if existing.type_name() != value.type_name() {
                    self.config_locked(name)?;
                }
                self.values.insert(name.to_string(), value);
                Ok(())
            }
            None => Err(format!("no such variable `{name}`")),
        }
    }

    /// Add a NEW user variable, at ordered position `at` (clamped) when given — the re-add a
    /// delete/rename undo needs. Errors on an invalid name or a collision.
    pub fn add(&mut self, name: &str, value: VariableValue, at: Option<usize>) -> Result<(), String> {
        if !is_valid_variable_name(name) {
            return Err(format!("invalid variable name `{name}`: {VARIABLE_NAME_RULE}"));
        }
        if self.values.contains_key(name) {
            return Err(format!("variable `{name}` already exists"));
        }
        if self.group_lock(group_of(name)).config {
            return Err(format!("group `{}` is config-locked", group_of(name)));
        }
        let at = at.unwrap_or(usize::MAX).min(self.values.len());
        self.values.shift_insert(at, name.to_string(), value);
        Ok(())
    }

    /// Ordered position of `name` — a delete's inverse captures it to re-add at the original slot.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.values.get_index_of(name)
    }

    /// Remove a variable; errors when it is config-locked or absent.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        if !self.values.contains_key(name) {
            return Err(format!("no such variable `{name}`"));
        }
        self.config_locked(name)?;
        self.values.shift_remove(name);
        self.controls.shift_remove(name);
        self.sources.shift_remove(name);
        self.locks.shift_remove(name);
        Ok(())
    }

    /// Rename a variable, keeping its ordered position; its own lock travels with it.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        if !self.values.contains_key(from) {
            return Err(format!("no such variable `{from}`"));
        }
        self.config_locked(from)?;
        if !is_valid_variable_name(to) {
            return Err(format!("invalid variable name `{to}`: {VARIABLE_NAME_RULE}"));
        }
        if self.values.contains_key(to) {
            return Err(format!("variable `{to}` already exists"));
        }
        if self.group_lock(group_of(to)).config && group_of(to) != group_of(from) {
            return Err(format!("group `{}` is config-locked", group_of(to)));
        }
        let at = self.values.get_index_of(from).expect("checked above");
        let value = self.values.shift_remove(from).expect("the index answered");
        self.values.shift_insert(at, to.to_string(), value);
        if let Some(c) = self.controls.shift_remove(from) {
            self.controls.insert(to.to_string(), c);
        }
        if let Some(l) = self.locks.shift_remove(from) {
            self.locks.insert(to.to_string(), l);
        }
        if let Some(s) = self.sources.shift_remove(from) {
            self.sources.insert(to.to_string(), s);
        }
        Ok(())
    }

    /// Rename a group, answering every member's old and new name in order. A group with no member
    /// is not refused here: a control panel naming it is what makes it a group, and only the graph
    /// sees panels.
    pub fn rename_group(&mut self, from: &str, to: &str) -> Result<Vec<(String, String)>, String> {
        if !is_valid_identifier(to) {
            return Err(format!("invalid group name `{to}`: {VARIABLE_NAME_RULE}"));
        }
        if from == SYSTEM_GROUP {
            return Err(format!("`{SYSTEM_GROUP}` is goofi's own; it keeps its name"));
        }
        if self.group_lock(from).config {
            return Err(format!("group `{from}` is config-locked"));
        }
        if self.has_group(to) {
            return Err(format!("variable group `{to}` already exists"));
        }
        let moved: Vec<(String, String)> = self
            .values
            .keys()
            .filter_map(|k| split_variable(k).filter(|(g, _)| *g == from).map(|(_, e)| (k.clone(), format!("{to}.{e}"))))
            .collect();
        for (old, new) in &moved {
            if self.values.contains_key(new.as_str()) {
                return Err(format!("variable `{new}` already exists"));
            }
            self.config_locked(old)?;
        }
        for (old, new) in &moved {
            self.rename(old, new)?;
        }
        if let Some(at) = self.groups.get_index_of(from) {
            let lock = self.groups.shift_remove(from).unwrap_or_default();
            self.groups.shift_insert(at, to.to_string(), lock);
        }
        Ok(moved)
    }

    /// Whether a group has an explicit record or an entry.
    pub fn has_group(&self, group: &str) -> bool {
        self.groups.contains_key(group) || self.values.keys().any(|k| group_of(k) == group)
    }

    /// Apply one change: `Some(v)` sets or adds (a NEW variable lands at `at`); `None` leaves the
    /// value alone, which is what an edit to the widget beside it means.
    pub fn apply_change(
        &mut self,
        name: &str,
        value: Option<VariableValue>,
        at: Option<usize>,
    ) -> Result<(), String> {
        match value {
            Some(v) if self.values.contains_key(name) => self.set(name, v),
            Some(v) => self.add(name, v, at),
            None if self.values.contains_key(name) => Ok(()),
            None => Err(format!("no such variable `{name}`")),
        }
    }
}
