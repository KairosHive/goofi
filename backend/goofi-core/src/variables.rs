//! Patch-scoped variables — named frames, an array or a string each, shared across a patch.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{control, Data, Meta, Value};

/// Where a variable's frames go: the data plane the store publishes on. A value write is a
/// publish and nothing else; a removed variable retires its wire.
pub trait Plane: Send + Sync {
    fn publish(&self, name: &str, value: &Data);
    fn retire(&self, name: &str);
}

/// What a control element is drawn as.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ControlKind {
    Knob,
    Slider,
    Number,
    Text,
    Toggle,
    Dropdown,
    Paint,
    Vector,
    Color,
}

impl ControlKind {
    /// Every kind, in the order a palette offers them.
    pub const ALL: [ControlKind; 9] = [
        ControlKind::Knob,
        ControlKind::Slider,
        ControlKind::Number,
        ControlKind::Text,
        ControlKind::Toggle,
        ControlKind::Dropdown,
        ControlKind::Paint,
        ControlKind::Vector,
        ControlKind::Color,
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
            ControlKind::Vector => "vector",
            ControlKind::Color => "color",
        }
    }

    /// What a widget of this kind draws: one `number`, a `text`, an `image` (an `[h, w, 4]` RGBA
    /// array in 0..1), a `vector` (an `[n]` array), a `color` (an `[4]` RGBA in 0..1), or `any` frame's truth.
    pub fn draws(self) -> &'static str {
        match self {
            ControlKind::Knob | ControlKind::Slider | ControlKind::Number => "number",
            ControlKind::Toggle => "any",
            ControlKind::Text | ControlKind::Dropdown => "text",
            ControlKind::Paint => "image",
            ControlKind::Vector => "vector",
            ControlKind::Color => "color",
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
            ControlKind::Vector => (8.0, 2.0),
            ControlKind::Color => (2.0, 2.0),
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
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
    /// A paint pad's side in texels; absent is 128.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<u32>,
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
    /// Whether this widget can draw `value`, by what its kind draws.
    pub fn fits(&self, value: &Data) -> bool {
        match (self.kind.draws(), value.value()) {
            ("number", Value::Array(a)) => a.shape() == [1],
            ("vector", Value::Array(a)) => a.shape().len() == 1,
            ("color", Value::Array(a)) => a.shape() == [4],
            ("image", Value::Array(a)) => matches!(a.shape(), [_, _, 4]),
            ("any", _) | ("text", Value::Str(_)) => true,
            _ => false,
        }
    }

    /// The value a widget of this kind is born holding: a paint pad, a clear `[side, side, 4]` sheet;
    /// a vector three zeros; a colour opaque black.
    pub fn born_value(&self) -> Data {
        match self.kind {
            ControlKind::Text | ControlKind::Dropdown => Data::text(""),
            ControlKind::Vector => Data::array_f32(vec![3], vec![0; 12], Meta::default()).expect("three numbers"),
            ControlKind::Color => Data::array_f32(vec![4], vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 63], Meta::default()).expect("four numbers"),
            ControlKind::Paint => {
                let side = self.resolution.unwrap_or(128).clamp(1, 1024) as usize;
                Data::array_f32(vec![side, side, 4], vec![0; side * side * 16], Meta::default()).expect("a whole number of texels")
            }
            _ => Data::number(0.0),
        }
    }

    /// Why this widget cannot draw `value`, in the words a refusal uses.
    pub fn mismatch(&self, value: &Data) -> String {
        format!("a `{}` cannot draw {}", self.kind.as_str(), control::form(value))
    }
}

/// A lock on a variable or a whole group: `config` freezes the name, the widget and
/// membership; `value` freezes the value alone. A group's lock reaches every member.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
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

/// Where a MIDI group's values come from: a port the host lists. The session's fact, never a
/// patch's: the group exists while the device is open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Midi {
    pub port: String,
}

/// A group's own record: its lock; for a device's group, the device it is read from; for a
/// playhead's, the machine that moves it. An OWNED group is locked whole against every hand but
/// its owner's, and the owner writes it through [`VariableStore::drive`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct Group {
    #[serde(default)]
    pub lock: Lock,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub midi: Option<Midi>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub machine: Option<String>,
}

impl Group {
    pub fn owned(&self) -> bool {
        self.midi.is_some() || self.machine.is_some()
    }

    /// Who holds the group, in the words a refusal uses.
    fn owner(&self) -> &'static str {
        match (&self.midi, &self.machine) {
            (Some(_), _) => "a MIDI device's",
            (_, Some(_)) => "a machine's playhead",
            _ => "nobody's",
        }
    }
}

/// The entries a MIDI group holds, over all 16 channels: every controller in 0..1 and every held
/// note's velocity at `(channel - 1) * 128 + number`, the wheel in -1..1 and the pressure in 0..1.
pub const MIDI_ENTRIES: [(&str, usize); 4] = [("cc", 16 * 128), ("notes", 16 * 128), ("bend", 16), ("pressure", 16)];


/// A code-owned system variable: its group is config-locked for life. An EPHEMERAL one is
/// value-locked too — goofi derives its value, it is re-derived at every reassert, and a `.gfi`
/// never carries it.
pub struct VariableDef {
    pub name: &'static str,
    pub value: fn() -> Data,
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
        value: || Data::number(30.0),
        doc: "Default update rate (Hz) for producer nodes that have not overridden it.",
        ephemeral: false,
    },
    VariableDef {
        name: "system.viewer_fps",
        value: || Data::number(30.0),
        doc: "The fastest a viewer is served (frames a second), whatever rate its display declares.",
        ephemeral: false,
    },
    VariableDef {
        name: "system.default_width",
        value: || Data::number(f64::from(DEFAULT_SIZE)),
        doc: "Default texture width (pixels) for graphics nodes that make their own frames.",
        ephemeral: false,
    },
    VariableDef {
        name: "system.default_height",
        value: || Data::number(f64::from(DEFAULT_SIZE)),
        doc: "Default texture height (pixels) for graphics nodes that make their own frames.",
        ephemeral: false,
    },
    VariableDef {
        name: "system.audio_rate",
        value: || Data::number(0.0),
        doc: "The audio clock's sample rate. The audio engine says it; 0 where no engine runs.",
        ephemeral: true,
    },
    VariableDef {
        name: "system.audio_channels",
        value: || Data::number(0.0),
        doc: "How many channels the audio clock carries. The audio engine says it; 0 where no engine runs.",
        ephemeral: true,
    },
    VariableDef {
        name: "system.audio_device",
        value: || Data::text(""),
        doc: "The device driving the audio clock, empty under the external clock or where none is open.",
        ephemeral: true,
    },
    VariableDef {
        name: "system.audio_driver",
        value: || Data::text(""),
        doc: "The ASIO driver holding the process, empty where none does — one loads at a time, so it is the patch's.",
        ephemeral: true,
    },
    VariableDef {
        name: "system.audio_hosts",
        value: || Data::text(""),
        doc: "The audio APIs this build carries, comma separated. Every device name begins with one of them, so this is the whole of what a device can be chosen from.",
        ephemeral: true,
    },
    VariableDef {
        name: "system.goofi_home",
        value: || Data::text(crate::path::to_slash(&goofi_supervisor::layout::home())),
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

/// What an owned group holds: each entry's name, the value it is born with, and its widget.
pub type Entries = Vec<(String, Data, Option<Control>)>;

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

/// A variable's name as a producer's slot name: one leaked copy per distinct name, because a
/// slot is spelled `&'static str` everywhere a wire is, and a patch names few variables.
pub fn slot_name(name: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut held = NAMES.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    match held.get(name) {
        Some(s) => s,
        None => {
            let s: &'static str = Box::leak(name.to_string().into_boxed_str());
            held.insert(s);
            s
        }
    }
}

fn is_ephemeral(name: &str) -> bool {
    SYSTEM_VARIABLES.iter().any(|d| d.ephemeral && d.name == name)
}

fn group_of(name: &str) -> &str {
    split_variable(name).map(|(g, _)| g).unwrap_or(name)
}

/// One variable: its latest frame beside the widget, expression and lock it carries. The frame is
/// the plane's; the rest is the document's (`goofi_graph::doc::VariableRecord`).
#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub value: Data,
    pub control: Option<Control>,
    /// The expression the manager computes it by — bare, one producer's frame copied as it comes;
    /// in Python, evaluated as a param's is. A computed variable is written by nobody else.
    pub expression: Option<String>,
    /// The variable's OWN lock, apart from its group's; absent is the default.
    pub lock: Option<Lock>,
}

impl Variable {
    pub fn of(value: Data) -> Variable {
        Variable { value, control: None, expression: None, lock: None }
    }

    pub fn own_lock(&self) -> Lock {
        self.lock.unwrap_or_default()
    }
}

/// The authoritative variables map. Locks decide what a caller may change, and the insertion order
/// is observable (the panel, the `.gfi` and the mirror all read it). Every value write goes out on
/// the plane from here, under the store's own lock, so the order the plane sees is the store's.
#[derive(Clone)]
pub struct VariableStore {
    entries: IndexMap<String, Variable>,
    groups: IndexMap<String, Group>,
    plane: Option<std::sync::Arc<dyn Plane>>,
}

impl Default for VariableStore {
    fn default() -> VariableStore {
        VariableStore::new()
    }
}

impl VariableStore {
    pub fn new() -> VariableStore {
        let mut s = VariableStore { entries: IndexMap::new(), groups: IndexMap::new(), plane: None };
        s.reassert_system();
        s
    }

    /// Put the store on a plane: every frame it holds goes out now, and every write from here on.
    pub fn set_plane(&mut self, plane: std::sync::Arc<dyn Plane>) {
        self.plane = Some(plane);
        for (name, v) in &self.entries {
            self.emit(name, &v.value);
        }
    }

    fn emit(&self, name: &str, value: &Data) {
        if let Some(p) = &self.plane {
            p.publish(name, value);
        }
    }

    fn retire(&self, name: &str) {
        if let Some(p) = &self.plane {
            p.retire(name);
        }
    }

    /// Back to the seeded store a load starts from, every wire of the old content retired.
    pub fn reset(&mut self) {
        // The system wires stay open: every node's default expression reads one, and a wire
        // reopened under a live reader is a second service. Their values start over in place.
        // A device's group is the session's as well, and stays as the device does.
        let gone: Vec<String> = self.entries.keys().filter(|n| !n.starts_with("system.") && self.midi(group_of(n)).is_none()).cloned().collect();
        for name in &gone {
            self.retire(name);
            self.entries.shift_remove(name);
        }
        self.groups.retain(|_, rec| rec.midi.is_some());
        self.seed_system(true);
    }

    /// Back-fill any missing system variable with its default — on construction and after a load —
    /// and re-lock the system group. An EPHEMERAL one is overwritten instead: goofi says what it
    /// holds, never a file.
    pub fn reassert_system(&mut self) {
        self.seed_system(false);
    }

    fn seed_system(&mut self, all: bool) {
        for def in SYSTEM_VARIABLES {
            if all || def.ephemeral || !self.entries.contains_key(def.name) {
                let mut v = Variable::of((def.value)());
                v.lock = def.ephemeral.then_some(Lock { config: false, value: true });
                self.emit(def.name, &v.value);
                self.entries.insert(def.name.to_string(), v);
            }
        }
        self.groups.insert(SYSTEM_GROUP.to_string(), Group { lock: Lock { config: true, value: false }, ..Group::default() });
    }

    pub fn get(&self, name: &str) -> Option<&Data> {
        self.entries.get(name).map(|v| &v.value)
    }
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Every variable in order, whole.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Variable)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn expression(&self, name: &str) -> Option<&str> {
        self.entries.get(name)?.expression.as_deref()
    }

    /// Set or clear the expression a variable is computed by, answering the one it replaced. An
    /// expression is config.
    pub fn set_expression(&mut self, name: &str, expression: Option<String>) -> Result<Option<String>, String> {
        if !self.entries.contains_key(name) {
            return Err(format!("no such variable `{name}`"));
        }
        self.config_locked(name)?;
        Ok(std::mem::replace(&mut self.entries[name].expression, expression.filter(|e| !e.trim().is_empty())))
    }

    /// The follower's own write: what the expression delivered. A value-locked variable takes
    /// nothing, silently.
    pub fn follow(&mut self, name: &str, value: Data) {
        // A variable with no expression has no follower: a pick already in flight when one is
        // cleared would otherwise land after, and overwrite the value the clearing author then typed.
        if !is_ephemeral(name) && !self.lock_of(name).value && self.expression(name).is_some() {
            self.move_value(name, value);
        }
    }

    /// An ENGINE's own published fact, which is why it passes the value lock: the lock exists to
    /// keep every other writer out, and the engine is the one it is held for. Only an ephemeral
    /// name takes one.
    pub fn publish(&mut self, name: &str, value: Data) {
        if is_ephemeral(name) {
            self.move_value(name, value);
        }
    }

    fn move_value(&mut self, name: &str, value: Data) -> bool {
        let Some(existing) = self.entries.get_mut(name) else { return false };
        if existing.value == value {
            return false;
        }
        existing.value = value;
        self.emit(name, &self.entries[name].value);
        true
    }

    /// Explicit groups and their records, in creation order.
    pub fn groups(&self) -> impl Iterator<Item = (&str, &Group)> {
        self.groups.iter().map(|(g, rec)| (g.as_str(), rec))
    }

    /// The device a group reads, if it is a MIDI group.
    pub fn midi(&self, group: &str) -> Option<&Midi> {
        self.groups.get(group)?.midi.as_ref()
    }

    /// The machine whose playhead a group is, if it is one.
    pub fn machine(&self, group: &str) -> Option<&str> {
        self.groups.get(group)?.machine.as_deref()
    }

    fn owned(&self, group: &str) -> bool {
        self.groups.get(group).is_some_and(Group::owned)
    }

    /// Open a device as a group: the device's flag, which locks the group whole, and its entries
    /// at rest. Not a command — the session owns it, as it owns the system group.
    pub fn grab_midi(&mut self, group: &str, port: &str) -> Result<(), String> {
        if self.has_group(group) {
            return Err(format!("variable group `{group}` already exists"));
        }
        let record = Group { midi: Some(Midi { port: port.to_string() }), ..Group::default() };
        let entries = MIDI_ENTRIES.map(|(entry, width)| (format!("{group}.{entry}"), Data::numbers(std::iter::repeat_n(0.0, width)), None));
        self.claim(group, record, entries.into())
    }

    /// Hold `group` whole for its owner, with exactly `entries`: one already held keeps its value
    /// and takes the widget, a new one is born at its value, one not named is retired. The owner's
    /// write, never a command; a group that is somebody else's is refused.
    pub fn claim(&mut self, group: &str, record: Group, entries: Entries) -> Result<(), String> {
        if !is_valid_identifier(group) || group == SYSTEM_GROUP {
            return Err(format!("invalid group name `{group}`: {VARIABLE_NAME_RULE}"));
        }
        if self.has_group(group) && self.groups.get(group).map(|r| (&r.midi, &r.machine)) != Some((&record.midi, &record.machine)) {
            return Err(format!("variable group `{group}` already exists"));
        }
        if let Some((name, ..)) = entries.iter().find(|(n, ..)| split_variable(n).is_none_or(|(g, _)| g != group)) {
            return Err(format!("invalid variable name `{name}`: {VARIABLE_NAME_RULE}"));
        }
        let gone: Vec<String> =
            self.entries.keys().filter(|n| group_of(n) == group && !entries.iter().any(|(e, ..)| e == *n)).cloned().collect();
        for name in &gone {
            self.retire(name);
            self.entries.shift_remove(name);
        }
        for (name, value, control) in entries {
            match self.entries.get_mut(&name) {
                Some(held) => held.control = control,
                None => {
                    self.emit(&name, &value);
                    self.entries.insert(name, Variable { control, ..Variable::of(value) });
                }
            }
        }
        self.groups.insert(group.to_string(), record);
        Ok(())
    }

    /// Close an owned group: its entries retired, its record gone. Nothing if `group` is nobody's.
    pub fn release(&mut self, group: &str) {
        if !self.owned(group) {
            return;
        }
        let gone: Vec<String> = self.entries.keys().filter(|n| group_of(n) == group).cloned().collect();
        for name in &gone {
            self.retire(name);
            self.entries.shift_remove(name);
        }
        self.groups.shift_remove(group);
    }

    /// An owner's write: lands only on an owned group's entry, under no lock, with no undo.
    pub fn drive(&mut self, name: &str, value: Data) -> bool {
        self.owned(group_of(name)) && self.move_value(name, value)
    }

    /// Whether a `.gfi` must leave `name` out: an ephemeral value is goofi's own, a device's is
    /// the session's, a playhead's is the machine's to re-derive.
    pub fn is_ephemeral(&self, name: &str) -> bool {
        is_ephemeral(name) || self.owned(group_of(name))
    }

    /// A variable's OWN lock, apart from its group's.
    pub fn own_lock(&self, name: &str) -> Lock {
        self.entries.get(name).map(Variable::own_lock).unwrap_or_default()
    }

    /// A group's lock as it holds its members: an owned group is its owner's on both axes.
    pub fn group_lock(&self, group: &str) -> Lock {
        let Some(rec) = self.groups.get(group) else { return Lock::default() };
        if rec.owned() { Lock { config: true, value: true } } else { rec.lock }
    }

    /// What holds `name` right now: its own lock and its group's together.
    pub fn lock_of(&self, name: &str) -> Lock {
        self.own_lock(name).or(self.group_lock(group_of(name)))
    }

    /// Set a variable's own lock, answering the one it held. The system group's are not a caller's.
    pub fn set_lock(&mut self, name: &str, lock: Lock) -> Result<Lock, String> {
        if group_of(name) == SYSTEM_GROUP {
            return Err(format!("`{name}` is goofi's own; its lock is not yours to set"));
        }
        let Some(v) = self.entries.get_mut(name) else {
            return Err(format!("no such variable `{name}`"));
        };
        let old = v.own_lock();
        v.lock = (!lock.is_default()).then_some(lock);
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
        if let Some(rec) = self.groups.get(group).filter(|r| r.owned()) {
            return Err(format!("group `{group}` is {}; it is locked whole", rec.owner()));
        }
        let old = self.groups.get(group).map(|rec| rec.lock);
        match lock {
            Some(lock) => self.groups.entry(group.to_string()).or_default().lock = lock,
            None => {
                self.groups.shift_remove(group);
            }
        }
        Ok(old)
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
        self.groups.shift_insert(at, group.to_string(), Group::default());
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
        if self.entries.keys().any(|name| group_of(name) == group) {
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
        self.entries.get(name)?.control.as_ref()
    }

    /// Set an existing variable. A change of FORM, an array for a string or back, is what every
    /// expression reading it depends on, so it also needs an unlocked configuration.
    pub fn set(&mut self, name: &str, value: Data) -> Result<(), String> {
        if is_ephemeral(name) {
            return Err(format!("variable `{name}` is read-only: it is ephemeral, and goofi says what it holds"));
        }
        if self.lock_of(name).value {
            return Err(format!("variable `{name}` is value-locked"));
        }
        if let Some(e) = self.expression(name) {
            return Err(format!("variable `{name}` is computed by `{e}`; clear its expression to set it"));
        }
        let Some(existing) = self.get(name) else { return Err(format!("no such variable `{name}`")) };
        if existing.dtype_tag() != value.dtype_tag() {
            self.config_locked(name).map_err(|why| {
                format!("{why}, and it holds {}: {} is {}", control::form(existing), control::text(&value), control::form(&value))
            })?;
        }
        self.emit(name, &value);
        self.entries[name].value = value;
        Ok(())
    }

    /// Add a NEW user variable, at ordered position `at` (clamped) when given — the re-add a
    /// delete/rename undo needs. Errors on an invalid name or a collision.
    pub fn add(&mut self, name: &str, value: Data, at: Option<usize>) -> Result<(), String> {
        if !is_valid_variable_name(name) {
            return Err(format!("invalid variable name `{name}`: {VARIABLE_NAME_RULE}"));
        }
        if self.entries.contains_key(name) {
            return Err(format!("variable `{name}` already exists"));
        }
        if self.group_lock(group_of(name)).config {
            return Err(format!("group `{}` is config-locked", group_of(name)));
        }
        let at = at.unwrap_or(usize::MAX).min(self.entries.len());
        self.emit(name, &value);
        self.entries.shift_insert(at, name.to_string(), Variable::of(value));
        Ok(())
    }

    /// Ordered position of `name` — a delete's inverse captures it to re-add at the original slot.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.entries.get_index_of(name)
    }

    /// Remove a variable; errors when it is config-locked or absent.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        if !self.entries.contains_key(name) {
            return Err(format!("no such variable `{name}`"));
        }
        self.config_locked(name)?;
        self.entries.shift_remove(name);
        self.retire(name);
        Ok(())
    }

    /// Rename a variable, keeping its ordered position; its own lock travels with it.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        if !self.entries.contains_key(from) {
            return Err(format!("no such variable `{from}`"));
        }
        self.config_locked(from)?;
        if !is_valid_variable_name(to) {
            return Err(format!("invalid variable name `{to}`: {VARIABLE_NAME_RULE}"));
        }
        if self.entries.contains_key(to) {
            return Err(format!("variable `{to}` already exists"));
        }
        if self.group_lock(group_of(to)).config && group_of(to) != group_of(from) {
            return Err(format!("group `{}` is config-locked", group_of(to)));
        }
        self.move_entry(from, to);
        Ok(())
    }

    /// The rename itself: the entry keeps its position, its wire is retired and reopened.
    fn move_entry(&mut self, from: &str, to: &str) {
        let at = self.entries.get_index_of(from).expect("checked by the caller");
        let variable = self.entries.shift_remove(from).expect("the index answered");
        self.retire(from);
        self.emit(to, &variable.value);
        self.entries.shift_insert(at, to.to_string(), variable);
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
            .entries
            .keys()
            .filter_map(|k| split_variable(k).filter(|(g, _)| *g == from).map(|(_, e)| (k.clone(), format!("{to}.{e}"))))
            .collect();
        for (old, new) in &moved {
            if self.entries.contains_key(new.as_str()) {
                return Err(format!("variable `{new}` already exists"));
            }
            self.config_locked(old)?;
        }
        for (old, new) in &moved {
            self.move_entry(old, new);
        }
        if let Some(at) = self.groups.get_index_of(from) {
            let rec = self.groups.shift_remove(from).unwrap_or_default();
            self.groups.shift_insert(at, to.to_string(), rec);
        }
        Ok(moved)
    }

    /// Whether a group has an explicit record or an entry.
    pub fn has_group(&self, group: &str) -> bool {
        self.groups.contains_key(group) || self.entries.keys().any(|k| group_of(k) == group)
    }

    /// Apply one change: `Some(v)` sets or adds (a NEW variable lands at `at`); `None` leaves the
    /// value alone, which is what an edit to the widget beside it means.
    pub fn apply_change(
        &mut self,
        name: &str,
        value: Option<Data>,
        at: Option<usize>,
        control: Option<Option<Control>>,
    ) -> Result<(), String> {
        // The widget that will be held must fit the value that will be held, checked before any write.
        let held = value.as_ref().or_else(|| self.get(name)).ok_or_else(|| format!("no such variable `{name}`"))?;
        let widget = match &control {
            Some(c) => {
                self.config_locked(name)?;
                c.as_ref()
            }
            None if value.is_some() => self.control(name),
            None => None,
        };
        if let Some(c) = widget.filter(|c| !c.fits(held)) {
            return Err(c.mismatch(held));
        }
        match value {
            Some(v) if self.entries.contains_key(name) => self.set(name, v)?,
            Some(v) => self.add(name, v, at)?,
            None => {}
        }
        if let Some(c) = control {
            self.entries[name].control = c;
        }
        Ok(())
    }
}
