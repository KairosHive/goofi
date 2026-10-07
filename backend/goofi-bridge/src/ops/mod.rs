//! The op registry: a phrase TREE, and the single place the op SET is declared. An op is a type;
//! its typed `Args` are the validation on every transport, and its row is built from it once.

pub mod history;
pub mod host;
pub mod layout;
pub mod library;
pub mod machine;
pub mod midi;
pub mod node;
pub mod record;
pub mod session;
pub mod variable;

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{AppState, Caller, Txn};
use goofi_graph::{Graph, Uid};

/// One op's contract: its full phrase, its documentation and its typed arguments. Deserializing
/// the payload into `Args` is the validation on every path, so every `Args` denies unknown fields.
pub trait Op: 'static {
    const NAME: &'static str;
    /// The doc TEMPLATE; read it through [`Row::doc`], which expands the vocabularies.
    const DOC: &'static str;
    /// The result schema, as the shape a caller gets back.
    const RESULT: &'static str;
    /// How many of the LEADING declared args a command line takes as positionals (0..=2). A
    /// list-typed positional is variadic; every positional stays reachable as a flag too.
    const POSITIONAL: usize = 0;
    type Args: DeserializeOwned + JsonSchema + Clone;
}

/// A read: it runs on the transaction, which holds the graph, and changes nothing.
pub trait ReadOp: Op {
    fn run(tx: &mut Txn, a: Self::Args) -> Result<Value, String>;
}

/// A write: it runs on the transaction and routes every mutation through the history.
pub trait WriteOp: Op {
    /// The history entry's label: what an undo button says it takes back.
    const LABEL: &'static str = "";
    fn run(tx: &mut Txn, a: Self::Args) -> Result<Value, String>;
    fn label(_: &Self::Args, _: &Value) -> String {
        Self::LABEL.into()
    }
}

/// An effect: it holds what it needs itself and owns its consequences.
pub trait EffectOp: Op {
    fn run(cx: &AppState, a: Self::Args, caller: &Caller) -> Result<Value, String>;
}

/// A read or a write, erased: a write also names its history entry on the transaction.
pub type TxnFn = fn(&mut Txn, &Value) -> Result<Value, String>;
pub type EffectFn = fn(&AppState, &Value, &Caller) -> Result<Value, String>;

/// An op's handler, which is also its KIND.
#[derive(Clone, Copy)]
pub enum Handler {
    PluginRead,
    PluginEffect,
    /// Reads state, changes nothing: never dirties, never re-mirrors.
    Read(TxnFn),
    /// Routes every mutation through the command history, so it has an exact inverse; the shared
    /// tail in [`AppState::call`] re-mirrors and raises the unsaved dot.
    Write(TxnFn),
    /// Owns its consequences itself, because they are not a graph command's: a save, a process,
    /// a restart, the history ops.
    Effect(EffectFn),
}

impl Handler {
    /// The kind as the word `op list` answers with.
    pub fn name(&self) -> &'static str {
        match self {
            Handler::Read(_) | Handler::PluginRead => "read",
            Handler::Write(_) => "write",
            Handler::Effect(_) | Handler::PluginEffect => "effect",
        }
    }
    pub fn is_write(&self) -> bool {
        matches!(self, Handler::Write(_))
    }
}

/// One declared argument, as the phrase layer, completion and `op list` read it. `ty` is the
/// CLI's type word (`uid`, `float`, `float2`, `any`, `endpoint`, …), with `[]` for a list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ArgDecl {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub required: bool,
}

impl ArgDecl {
    pub fn is_list(&self) -> bool {
        self.ty.ends_with("[]")
    }
    /// The type word of one element: the type itself for a scalar.
    pub fn item(&self) -> &str {
        self.ty.trim_end_matches("[]")
    }
    pub fn is_bool(&self) -> bool {
        self.ty == "bool"
    }

    /// A plugin manifest's `name:type` list, `!` marking a required one — the one place the
    /// space-separated spelling is still read, because the plugin SDK writes it.
    pub fn parse_list(spec: &str) -> Vec<ArgDecl> {
        spec.split_whitespace()
            .filter_map(|a| {
                let (name, ty) = a.split_once(':')?;
                Some(ArgDecl { name: name.into(), ty: ty.trim_end_matches('!').into(), required: ty.ends_with('!') })
            })
            .collect()
    }
}

/// Where a row's argument declarations come from: a built-in op's `Args` type, or a plugin's list.
#[derive(Clone, Copy)]
pub enum ArgSpec {
    Typed(fn() -> Schema),
    Listed(&'static [ArgDecl]),
}

/// One op's row: the full phrase, the handler and the documentation.
#[derive(Clone, Copy)]
pub struct Row {
    pub name: &'static str,
    pub handler: Handler,
    args: ArgSpec,
    pub positional: usize,
    pub doc: &'static str,
    pub result: &'static str,
}

impl Row {
    /// A plugin's row, from the declarations its manifest carries.
    pub fn plugin(name: &'static str, read: bool, args: &'static [ArgDecl], doc: &'static str, result: &'static str) -> Row {
        let handler = if read { Handler::PluginRead } else { Handler::PluginEffect };
        Row { name, handler, args: ArgSpec::Listed(args), positional: 0, doc, result }
    }

    /// Whether the op steps the history without being a write: a plugin may not hook it.
    pub fn moves_history(&self) -> bool {
        matches!(self.name, "compound" | "undo" | "redo")
    }

    /// The declared arguments, in declaration order.
    pub fn args(&self) -> Cow<'static, [ArgDecl]> {
        match self.args {
            ArgSpec::Typed(schema) => Cow::Owned(decls(schema().as_value())),
            ArgSpec::Listed(list) => Cow::Borrowed(list),
        }
    }

    /// The arguments as JSON Schema — what an MCP client or a form reads.
    pub fn schema(&self) -> Value {
        match self.args {
            ArgSpec::Typed(schema) => schema().to_value(),
            ArgSpec::Listed(list) => {
                let properties: serde_json::Map<String, Value> = list
                    .iter()
                    .map(|a| (a.name.clone(), match a.item() {
                        "json" | "any" => json!({}),
                        item => json!({ "type": json_type(item), "format": item }),
                    }))
                    .collect();
                let required: Vec<&str> = list.iter().filter(|a| a.required).map(|a| a.name.as_str()).collect();
                json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
            }
        }
    }

    /// The doc with its vocabulary placeholders expanded.
    pub fn doc(&self) -> String {
        self.doc
            .replace("{panel_types}", &crate::vocab::panel_types_help())
            .replace("{viewer_kinds}", &crate::vocab::viewer_kinds_help())
            .replace("{boundary_types}", &crate::vocab::boundary_types_help())
    }
}

/// The JSON type a CLI type word names.
fn json_type(word: &str) -> &'static str {
    match word {
        "float" => "number",
        "int" => "integer",
        "bool" => "boolean",
        _ => "string",
    }
}

/// An `Args` schema read back as declarations: each property in declaration order, typed by the
/// CLI word its schema spells, required when the schema says so.
fn decls(schema: &Value) -> Vec<ArgDecl> {
    let required: Vec<&str> =
        schema["required"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let Some(props) = schema["properties"].as_object() else { return Vec::new() };
    props
        .iter()
        .map(|(name, s)| ArgDecl { name: name.clone(), ty: type_word(s, &schema["$defs"]), required: required.contains(&name.as_str()) })
        .collect()
}

/// The CLI type word of one property schema: a `format` names a goofi vocabulary, a JSON type
/// its scalar, and a list its element; an `Option` is read through its `null` half.
fn type_word(s: &Value, defs: &Value) -> String {
    if let Some(r) = s["$ref"].as_str() {
        return type_word(&defs[r.rsplit('/').next().unwrap_or(r)], defs);
    }
    if let Some(any) = s["anyOf"].as_array() {
        if let Some(inner) = any.iter().find(|v| v["type"] != "null") {
            return type_word(inner, defs);
        }
    }
    if let Some(f) = s["format"].as_str() {
        if matches!(f, "uid" | "endpoint" | "param_addr" | "panel_type" | "any") {
            return f.to_string();
        }
    }
    let ty = match &s["type"] {
        Value::String(t) => t.as_str(),
        Value::Array(ts) => ts.iter().filter_map(Value::as_str).find(|t| *t != "null").unwrap_or("json"),
        _ => "json",
    };
    match ty {
        "string" => "string".into(),
        "number" => "float".into(),
        "integer" => "int".into(),
        "boolean" => "bool".into(),
        "array" if s["minItems"] == 2 && s["maxItems"] == 2 && s["items"]["type"] == "number" => "float2".into(),
        "array" => format!("{}[]", type_word(&s["items"], defs)),
        _ => "json".into(),
    }
}

fn schema_of<T: Op>() -> Schema {
    schemars::generate::SchemaSettings::default().into_generator().into_root_schema_for::<T::Args>()
}

fn args_of<T: Op>(v: &Value) -> Result<T::Args, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("{}: {e}", T::NAME))
}

/// Every refusal an op's run answers is prefixed with its phrase here, once.
fn refusal<T: Op>(e: String) -> String {
    format!("{}: {e}", T::NAME)
}

fn erased_read<T: ReadOp>(tx: &mut Txn, v: &Value) -> Result<Value, String> {
    T::run(tx, args_of::<T>(v)?).map_err(refusal::<T>)
}

fn erased_write<T: WriteOp>(tx: &mut Txn, v: &Value) -> Result<Value, String> {
    let a = args_of::<T>(v)?;
    let out = T::run(tx, a.clone()).map_err(refusal::<T>)?;
    tx.label(T::label(&a, &out));
    Ok(out)
}

fn erased_effect<T: EffectOp>(cx: &AppState, v: &Value, caller: &Caller) -> Result<Value, String> {
    T::run(cx, args_of::<T>(v)?, caller).map_err(refusal::<T>)
}

const fn row<T: Op>(handler: Handler) -> Row {
    Row { name: T::NAME, handler, args: ArgSpec::Typed(schema_of::<T>), positional: T::POSITIONAL, doc: T::DOC, result: T::RESULT }
}

pub const fn read<T: ReadOp>() -> Row {
    row::<T>(Handler::Read(erased_read::<T>))
}

pub const fn write<T: WriteOp>() -> Row {
    row::<T>(Handler::Write(erased_write::<T>))
}

pub const fn effect<T: EffectOp>() -> Row {
    row::<T>(Handler::Effect(erased_effect::<T>))
}

/// One op's contract in one line: the type, its phrase, its positionals, its args and result types.
/// An inline `Args { .. }` is declared here, so every one denies unknown fields.
macro_rules! op {
    ($t:ident, $name:literal, $positional:literal, $args:ident { $($body:tt)* }, $doc:expr, $result:expr) => {
        #[derive(Clone, Debug, ::serde::Deserialize, ::schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $args { $($body)* }
        $crate::ops::op!($t, $name, $positional, $args, $doc, $result);
    };
    ($t:ident, $name:literal, $positional:literal, $args:ty, $doc:expr, $result:expr) => {
        pub struct $t;
        impl $crate::ops::Op for $t {
            const NAME: &'static str = $name;
            const DOC: &'static str = $doc;
            const RESULT: &'static str = $result;
            const POSITIONAL: usize = $positional;
            type Args = $args;
        }
    };
}
pub(crate) use op;

/// The arguments of an op that takes none.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

/// A `format` word: the vocabulary the CLI completes and the phrase layer types by. Each
/// newtype below is one such word.
macro_rules! word {
    ($t:ident, $inner:ty, $schema:tt, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct $t(pub $inner);
        impl JsonSchema for $t {
            fn schema_name() -> Cow<'static, str> {
                stringify!($t).into()
            }
            fn inline_schema() -> bool {
                true
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                schemars::json_schema!($schema)
            }
        }
    };
}

word!(NodeRef, String, { "type": "string", "format": "uid" }, "A node, by its display name or its uid.");
word!(Endpoint, String, { "type": "string", "format": "endpoint" }, "`node/slot`, split on the FIRST `/`, the node half a uid or a name.");
word!(ParamAddr, String, { "type": "string", "format": "param_addr" }, "`group/param`, split on the FIRST `/`.");
word!(PanelType, String, { "type": "string", "format": "panel_type" }, "A panel type id, one of the vocabulary's.");
word!(Any, Value, { "format": "any" }, "A value of any JSON shape; on a command line, JSON when it parses and the bare string otherwise.");

impl NodeRef {
    pub fn resolve(&self, g: &Graph) -> Result<Uid, String> {
        g.resolve_ref(&self.0).ok_or_else(|| format!("`{}` names no node — give a uid or a node's name", self.0))
    }

    /// A list resolved whole or refused whole: a caller that named one bad node asked for a batch
    /// that is not the one it would get.
    pub fn resolve_all(list: &[NodeRef], g: &Graph) -> Result<Vec<Uid>, String> {
        list.iter().map(|n| n.resolve(g)).collect()
    }
}

impl Endpoint {
    /// The slot half may itself be a port uid (wiring a facade from outside), so it is never
    /// validated here.
    pub fn resolve(&self, g: &Graph, key: &str) -> Result<(Uid, String), String> {
        let (node, slot) = self.0.split_once('/').ok_or_else(|| format!("`{key}` is `node/slot`, not `{}`", self.0))?;
        let uid = g.resolve_ref(node).ok_or_else(|| format!("`{node}` in `{key}` names no node"))?;
        Ok((uid, slot.to_string()))
    }
}

impl ParamAddr {
    pub fn split(&self) -> Result<(String, String), String> {
        let (group, name) = self.0.split_once('/').ok_or_else(|| format!("`{}` is not `group/param`", self.0))?;
        Ok((group.to_string(), name.to_string()))
    }
}

/// One node of the phrase tree: a group word with its own one-line doc, or a leaf op.
pub enum Entry {
    Group(&'static str, &'static str, &'static [Entry]),
    Leaf(Row),
}

impl Entry {
    /// The word this entry answers to at its level: a group's word, or an op's last word.
    pub fn word(&self) -> &'static str {
        match self {
            Entry::Group(word, _, _) => word,
            Entry::Leaf(op) => op.name.rsplit(' ').next().unwrap_or(op.name),
        }
    }
}

use Entry::{Group, Leaf};

pub static TREE: &[Entry] = &[
    Group("plugin", "installed plugin services and UI", &[
        Leaf(read::<host::PluginList>()),
    ]),
    Group("session", "the goofi instance as a whole — identity, the open patch, save and load", &[
        Leaf(read::<session::Status>()),
        Leaf(read::<session::State>()),
        Leaf(read::<session::Manifest>()),
        Leaf(effect::<session::Save>()),
        Leaf(effect::<session::Load>()),
        Leaf(effect::<session::New>()),
        Leaf(read::<session::Recoverable>()),
        Leaf(effect::<session::Recover>()),
        Leaf(effect::<session::Discard>()),
    ]),
    Group("update", "a newer goofi: whether one is out, and installing it", &[
        Leaf(effect::<session::UpdateCheck>()),
        Leaf(effect::<session::UpdateStart>()),
    ]),
    Group("node", "one node instance — read it, build it, tune it, remove it", &[
        Leaf(read::<node::State>()),
        Leaf(read::<node::Snapshot>()),
        Leaf(write::<node::Add>()),
        Leaf(write::<node::Edit>()),
        Group("param", "one param of a node, addressed `group/param`", &[
            Leaf(write::<node::ParamEdit>()),
            Leaf(effect::<node::ParamRequest>()),
        ]),
        Leaf(write::<node::Remove>()),
        Leaf(write::<node::Baseline>()),
        Leaf(effect::<node::Restart>()),
        Leaf(effect::<node::Editor>()),
    ]),
    Group("nodes", "the graph of several nodes — inspect, copy, paste, group", &[
        Leaf(read::<node::NodesInspect>()),
        Leaf(read::<node::NodesCopy>()),
        Leaf(write::<node::NodesPaste>()),
        Leaf(write::<node::NodesGroup>()),
        Leaf(write::<node::NodesUngroup>()),
        Leaf(write::<node::NodesArrange>()),
    ]),
    Group("link", "one wire between an output and an input", &[
        Leaf(write::<node::LinkAdd>()),
        Leaf(write::<node::LinkRemove>()),
    ]),
    Group("variable", "the patch variables — what an expression reads as `variables.group.element`", &[
        Leaf(read::<variable::List>()),
        Group("entry", "one variable, addressed `group.element`", &[
            Leaf(write::<variable::EntryAdd>()),
            Leaf(write::<variable::EntryEdit>()),
            Leaf(write::<variable::EntryRemove>()),
            Leaf(write::<variable::EntryLock>()),
            Leaf(write::<variable::EntryRename>()),
        ]),
        Group("group", "a whole group of variables — a control panel is one", &[
            Leaf(write::<variable::GroupAdd>()),
            Leaf(write::<variable::GroupRename>()),
            Leaf(write::<variable::GroupRemove>()),
            Leaf(write::<variable::GroupLock>()),
        ]),
    ]),
    Group("machine", "a state machine: states, the transitions between them, and the playheads whose variables they write", &[
        Leaf(read::<machine::List>()),
        Leaf(write::<machine::Add>()),
        Leaf(write::<machine::Remove>()),
        Leaf(write::<machine::Rename>()),
        Leaf(write::<machine::Edit>()),
        Leaf(write::<machine::Arrange>()),
        Group("attribute", "one attribute — a value every state may set, carried by every playhead as `variables.<playhead>.<attribute>`", &[
            Leaf(write::<machine::AttributeAdd>()),
            Leaf(write::<machine::AttributeEdit>()),
            Leaf(write::<machine::AttributeRemove>()),
            Leaf(write::<machine::AttributeRename>()),
        ]),
        Group("state", "one state — a card on the canvas and the attributes it sets", &[
            Leaf(write::<machine::StateAdd>()),
            Leaf(write::<machine::StateEdit>()),
            Leaf(write::<machine::StateRemove>()),
            Leaf(write::<machine::StateRename>()),
        ]),
        Group("transition", "one transition, addressed by the id the manager minted", &[
            Leaf(write::<machine::TransitionAdd>()),
            Leaf(write::<machine::TransitionEdit>()),
            Leaf(write::<machine::TransitionRemove>()),
        ]),
        Group("playhead", "one playhead — a dot on the canvas, and the variable group it writes", &[
            Leaf(write::<machine::PlayheadAdd>()),
            Leaf(write::<machine::PlayheadEdit>()),
            Leaf(write::<machine::PlayheadRemove>()),
            Leaf(write::<machine::PlayheadRename>()),
        ]),
        Leaf(effect::<machine::Fire>()),
        Leaf(effect::<machine::Jump>()),
        Leaf(effect::<machine::Reset>()),
    ]),
    Group("midi", "the MIDI devices on the variable bus — a port the patch reads is a group, open while it is read", &[
        Leaf(read::<midi::List>()),
        Leaf(effect::<midi::Learn>()),
    ]),
    Group("control", "a control panel: one group of variables drawn as widgets, and the door that edits them", &[
        Leaf(read::<variable::ControlList>()),
        Leaf(write::<variable::ControlAdd>()),
        Leaf(write::<variable::ControlEdit>()),
        Leaf(write::<variable::ControlRemove>()),
        Leaf(write::<variable::ControlPaint>()),
    ]),
    Group("library", "the node types — what `node add` can build", &[
        Leaf(read::<library::List>()),
        Leaf(read::<library::Get>()),
        Leaf(effect::<library::Save>()),
        Leaf(effect::<library::Refresh>()),
    ]),
    Group("dir", "the goofi host's filesystem", &[
        Leaf(read::<host::DirStat>()),
        Leaf(read::<host::DirList>()),
    ]),
    Group("log", "application messages", &[
        Leaf(read::<host::LogList>()),
        Leaf(effect::<host::LogWrite>()),
    ]),
    Group("op", "the vocabulary itself", &[
        Leaf(read::<history::OpList>()),
        Leaf(read::<history::OpComplete>()),
    ]),
    Group("agent", "the agent harnesses goofi launches", &[
        Leaf(read::<host::AgentList>()),
        Leaf(effect::<host::AgentStart>()),
        Leaf(effect::<host::AgentStop>()),
    ]),
    Group("record", "capture any node's output to disk, on one clock", &[
        Leaf(read::<record::Status>()),
        Leaf(write::<record::Arm>()),
        Leaf(write::<record::Quality>()),
        Leaf(write::<record::Disarm>()),
        Leaf(effect::<record::Start>()),
        Leaf(effect::<record::Stop>()),
    ]),
    Leaf(effect::<history::Undo>()),
    Leaf(effect::<history::Redo>()),
    Leaf(effect::<history::Compound>()),
    Group("layout", "panels, tabs and splits — absent under headless", &[
        Leaf(read::<layout::Inspect>()),
        Group("panel", "one panel — the unit a viewer or tool lives in", &[
            Leaf(write::<layout::PanelAdd>()),
            Leaf(write::<layout::PanelEdit>()),
        ]),
        Leaf(write::<layout::Move>()),
        Leaf(write::<layout::Remove>()),
        Group("tab", "one tab in the strip", &[
            Leaf(write::<layout::TabEdit>()),
        ]),
        Group("split", "one split's children and their shares", &[
            Leaf(write::<layout::SplitEdit>()),
        ]),
        Group("viewpoint", "where this client is looking", &[
            Leaf(effect::<layout::ViewpointEdit>()),
        ]),
    ]),
];

/// The phrases the CLIENT owns — the door words, and the future `plugin` prefix. Never
/// registrable, and prefix-free with the registry: the contracts invariant checks both together.
pub static RESERVED: &[&str] = &["help", "session list", "agent term", "completions"];

/// The flat rows the tree spells, collected once per process — what the socket, `op list` and
/// the generated `OpName` union read. A leaf filed under the wrong group is refused here.
pub fn registry() -> &'static [Row] {
    use std::sync::OnceLock;
    static FLAT: OnceLock<Vec<Row>> = OnceLock::new();
    fn walk(prefix: &str, entries: &[Entry], out: &mut Vec<Row>) {
        for e in entries {
            match e {
                Entry::Group(word, _, children) => walk(&format!("{prefix}{word} "), children, out),
                Entry::Leaf(op) => {
                    let rest = op.name.strip_prefix(prefix).unwrap_or_default();
                    assert!(!rest.is_empty() && !rest.contains(' '), "`{}` is filed under `{prefix}`", op.name);
                    out.push(*op);
                }
            }
        }
    }
    FLAT.get_or_init(|| {
        let mut out = Vec::new();
        walk("", TREE, &mut out);
        out
    })
}

/// The row for `name`, if the op exists.
pub fn find(name: &str) -> Option<&'static Row> {
    registry().iter().find(|o| o.name == name)
}

/// The rows one server serves. A mode does not REGISTER what it withholds — the one spelling of
/// each mode, so `op list`, the phrase resolver and the MCP all shrink with it.
pub fn table(mode: crate::Mode) -> Vec<Row> {
    // What a demo drops: what reaches the host's files or spawns on it, since every visitor
    // shares one process. `session new` stays: it is the visitor's reset.
    const DEMO_DROPS: [&str; 9] =
        ["dir", "agent", "session save", "session load", "library save",
         "session recoverable", "session recover", "session discard", "update"];
    let dropped = |name: &str, group: &str| {
        name == group || name.strip_prefix(group).is_some_and(|rest| rest.starts_with(' '))
    };
    registry()
        .iter()
        .filter(|o| !mode.headless || !dropped(o.name, "layout"))
        .filter(|o| !mode.demo || !DEMO_DROPS.iter().any(|d| dropped(o.name, d)))
        .copied()
        .collect()
}

/// The frontend's `OpName` union and each op's kind, generated from the registry and checked
/// into the tree.
pub fn typescript() -> String {
    let names: Vec<String> =
        registry().iter().map(|o| format!("\t| '{}'", o.name)).collect();
    let kinds: Vec<String> =
        registry().iter().map(|o| format!("\t'{}': '{}'", o.name, o.handler.name())).collect();
    format!(
        "// GENERATED from backend/goofi-bridge/src/ops/mod.rs — do not edit by hand.\n\
         // The manager's op registry is the only place an op name is declared: naming one that is\n\
         // not in it is a type error here and an `unknown op` refusal there. Regenerate by running\n\
         // `cargo test -p goofi-tests contracts::`, which rewrites this file when it drifts.\n\
         export type OpName =\n\t| `plugin ${{string}}`\n{};\n\n\
         /** A write is a history step; a read touches nothing; an effect owns its consequences. */\n\
         export const OP_KINDS: Record<string, 'read' | 'write' | 'effect'> = {{\n{}\n}};\n",
        names.join("\n"),
        kinds.join(",\n")
    )
}

/// An argument that tells `null` from absent: absent is `None`, `null` is `Some(None)`.
pub(crate) fn nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(d).map(Some)
}
