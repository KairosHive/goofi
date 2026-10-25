//! The graph, and the nodes that schedule themselves.

use goofi_core::record::RecordedOutput;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use goofi_core::{control, Data, Param};
use goofi_node::{
    Edit, EditorAction,
    BindingView, BoundVar, DrainWaker, Edge, Engine, EventId, ExprMode, GraphView, Isolation,
    IsolationCell, LibraryEntry, NodeManifest, NodeView, ParamDecl, ParamGroups, ParamKey,
    Status,
};
use indexmap::IndexMap;

pub mod archive;
pub mod canvas;
pub mod doc;
pub use doc::MANIFEST_VERSION;

pub mod subpatch;
pub mod layout;

pub mod history;
pub use history::{Flip, History, Snapshot};

pub mod expr_rewrite;
pub mod machine;

pub use goofi_node::Uid;

use expr_rewrite::Target;
use goofi_core::variables::NAME_RULE;
use subpatch::Dir;

/// What a node IS. The thin distinction the backend keeps and the frontend never sees: a leaf runs,
/// so it carries a thread and params; a facade and a port do not, so they carry neither.
#[derive(Clone)]
enum Kind {
    /// Boxed: a leaf's runtime state dwarfs the other two, and one map holds all three.
    Leaf(Box<Leaf>),
    /// A sub-patch facade. Its members are whatever `scope_of` places inside it.
    Facade,
    /// A boundary port. It relays rather than produces, so its dtype is fixed by its type at birth.
    Port(subpatch::Port),
}

/// The state only a RUNNING node has: the RECORD the ops write, and the health its instance
/// reports.
#[derive(Clone)]
struct Leaf {
    manifest: &'static NodeManifest,
    /// The type's cell, captured at birth — shared per type, so a runtime demotion of a Python
    /// type reads through here too.
    isolation: &'static IsolationCell,
    /// The id of the engine whose library resolved this node's type — its runtime authority.
    engine: &'static str,
    /// The param RECORD — the literals `serialize` writes, every param of the class. An evaluated
    /// value must never reach it; the typed params with bounds derive from the class on read.
    values: Record,
    /// The AUTHORED source per param: the mode and the text the document holds. What it derives
    /// to against the graph is the runtime's ([`Runtime::bindings`]), never the record's.
    sources: HashMap<ParamKey, SourceState>,
}

/// A leaf's literals, with the typed params derived from them memoized until the next write:
/// the record's one reader derives through [`Graph::typed`], and a write clears the memo.
#[derive(Clone)]
struct Record {
    values: doc::Values,
    typed: std::sync::OnceLock<Arc<ParamGroups>>,
}

impl Record {
    fn new(values: doc::Values) -> Record {
        Record { values, typed: std::sync::OnceLock::new() }
    }

    fn get(&self) -> &doc::Values {
        &self.values
    }

    /// The literals to write; the typed memo goes with the borrow.
    fn get_mut(&mut self) -> &mut doc::Values {
        self.typed.take();
        &mut self.values
    }
}

/// One running node: the engine it was born into and what it reports. Born and removed by
/// `settle` alone, so a batch's adds and removes net before any engine hears of them.
struct Instance {
    engine: &'static str,
    health: Health,
}

/// What a batch changed between two snapshots, as [`Graph::settle`] reads it.
#[derive(Default)]
struct Diff {
    /// Nodes whose record is not the one the last settle saw, born ones included.
    changed: Vec<Uid>,
    removed: Vec<Uid>,
    /// Leaves whose class or engine changed under them: a rebirth.
    reclassed: Vec<Uid>,
    /// Display names that moved, old and new, and the nodes wearing them.
    renamed: Vec<(String, String)>,
    renamed_uids: Vec<Uid>,
    /// Display names that came with a node born, or went with one removed: what a binding
    /// spelling one re-derives on.
    named: Vec<String>,
    /// Nodes whose armed outputs moved.
    rearmed: Vec<Uid>,
    /// Params whose literal or authored source moved.
    edited: Vec<(Uid, ParamKey)>,
    /// Resolved edges one snapshot has and the other has not.
    rewired: Vec<Edge>,
    /// Whether the links root moved at all: what a stream read through a port follows.
    relinked: bool,
    /// Whether the machines root, or the arrangement, moved: what their followers read.
    machines: bool,
    arrangement: bool,
}

impl Diff {
    fn is_empty(&self) -> bool {
        self.changed.is_empty()
            && self.removed.is_empty()
            && self.rewired.is_empty()
            && !self.relinked
            && !self.machines
            && !self.arrangement
    }
}

/// What the settles since the last take moved that a follower of the graph reads: each follower
/// runs on its mark alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    /// A binding was derived anew, or a node was born, reborn or removed: the variable
    /// producer's ringers, the reducers' taps and the follower's expressions may have moved.
    pub ringers: bool,
    /// The machines root moved.
    pub machines: bool,
    /// An armed output moved, or a node whose arming names a generation was born or removed.
    pub armed: bool,
    /// A source, a feed or a variable moved: what the MIDI watch reads for the groups it opens.
    pub groups: bool,
}

/// What one drain of the engines' reports landed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drained {
    pub reports: usize,
    /// Whether a node's stage moved: what a reader waiting on a node's start re-reads for.
    pub staged: bool,
}

/// What the running instance reports about itself — a one-way projection with two writers by
/// construction: a BIRTH replaces the whole struct, and the status drain mutates it after.
struct Health {
    /// `Some` when INITIALIZATION failed — the param replay and `setup()` together, which are one
    /// unit. Not `last_error`, which a later process failure would overwrite.
    setup_error: Option<String>,
    last_error: Option<String>,
    /// The derived error and WHEN it first read that way — re-stamped only when the message
    /// changes, so the instant is its onset: a settling pipeline reads differently from a broken one.
    error_since: Option<(String, Instant)>,
    /// The stage the node last reported; `creating` until it reports anything. The `error` stage
    /// is DERIVED from the fault, never stored here.
    stage: &'static str,
    /// The same number the node stamps as `meta["ufreq"]`. `None` until it has emitted twice.
    ufreq: Option<f64>,
    /// What the node last reported evaluating its bindings to. Kept apart from `params` so a
    /// broken binding still has the authored literal to fall back to.
    evaluated: IndexMap<ParamKey, Param>,
    /// Every error THIS INSTANCE reported, by param. It dies with the instance, where
    /// `Binding::bind_error` survives because the source does.
    param_errors: IndexMap<ParamKey, String>,
    /// A refreshable `Str` param's re-enumerated options — the overlay a projection reads over
    /// the record's declared options. The answer to a ⟳ lands here, never in the record.
    options: IndexMap<ParamKey, Vec<String>>,
}

impl Health {
    /// A birth's whole health: fresh, so a reborn node cannot show its predecessor's numbers,
    /// carrying the one fact only the birth knows — whether its services came up.
    fn born(boot_error: Option<String>) -> Health {
        Health {
            setup_error: boot_error,
            last_error: None,
            error_since: None,
            stage: "creating",
            ufreq: None,
            evaluated: IndexMap::new(),
            param_errors: IndexMap::new(),
            options: IndexMap::new(),
        }
    }
}

/// One entry in the ONE node map: a leaf, a sub-patch facade or a boundary port. Everything an op
/// can address about a node — its name, where it sits, what it shows — is here for all three.
#[derive(Clone)]
struct NodeEntry {
    kind: Kind,
    name: String,
    pos: [f64; 2],
    /// The scope this record is a member of; `None` is ROOT. The ONE source of truth for
    /// parentage and membership.
    scope: Option<Uid>,
    /// Opaque: persisted and round-tripped, never interpreted.
    viewers: serde_json::Value,
    /// The values the touched-only filter counts FROM, `group/name` to value, opaque like
    /// `viewers`. Clearing records the current values as the new zero, so a preset load can be reset.
    baseline: serde_json::Value,
    /// The output slots armed for recording, in the order they were armed.
    record: Vec<RecordedOutput>,
}

impl NodeEntry {
    /// A record at ROOT with nothing shown, counted from or armed; a restore sets those after.
    fn new(kind: Kind, name: String, pos: [f64; 2]) -> NodeEntry {
        let empty = serde_json::json!({});
        NodeEntry { kind, name, pos, scope: None, viewers: empty.clone(), baseline: empty, record: Vec::new() }
    }
    fn leaf(&self) -> Option<&Leaf> {
        match &self.kind {
            Kind::Leaf(l) => Some(l),
            Kind::Facade | Kind::Port(_) => None,
        }
    }
    fn leaf_mut(&mut self) -> Option<&mut Leaf> {
        match &mut self.kind {
            Kind::Leaf(l) => Some(l),
            Kind::Facade | Kind::Port(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub node_out: Uid,
    pub slot_out: &'static str,
    pub node_in: Uid,
    pub slot_in: &'static str,
}

impl Link {
    fn key(&self) -> String {
        doc::link_key(self.node_out, self.slot_out, self.node_in, self.slot_in)
    }
}

/// A param's value as JSON, and the one definition of it — the inverse of [`param_from_json`].
pub fn param_value_json(p: &Param) -> serde_json::Value {
    serde_json::to_value(control::data_of(p)).unwrap_or_default()
}

/// A JSON literal as a `Param` of `existing`'s kind, keeping its bounds; what is no literal
/// leaves `existing` as it is.
pub fn param_from_json(existing: &Param, v: &serde_json::Value) -> Param {
    match serde_json::from_value::<Data>(v.clone()) {
        Ok(d) => control::read(&d, existing),
        Err(_) => existing.clone(),
    }
}

impl Graph {
    /// Apply the one params bag, `{group: {param: value | {value, expression, mode}}}`, typed
    /// against the params the node holds NOW. A literal lands before a source on the same param:
    /// a literal write switches the mode to constant, which the source then says over it.
    pub fn apply_params(&mut self, uid: Uid, params: &serde_json::Value) -> Result<(), String> {
        let groups = params.as_object().ok_or("params is {group: {param: …}}")?;
        if !self.is_leaf(uid) {
            return Err(format!("no node {}", uid.to_hex()));
        }
        let declared = self.params(uid);
        for (group, entries) in groups {
            let entries =
                entries.as_object().ok_or_else(|| format!("params.{group} is {{param: …}}"))?;
            for (name, spec) in entries {
                // `name[i]` is one dimension of a vector: it edits that element's literal or source.
                let existing = declared
                    .as_ref()
                    .and_then(|p| goofi_node::param_dim(p, group, name))
                    .ok_or_else(|| format!("no param {group}.{name}"))?;
                let cur = self.param_source(uid, group, name).map(|s| s.state);
                let (value, source) = param_change(&existing, cur, spec)
                    .map_err(|e| format!("params.{group}.{name}: {e}"))?;
                // A LIST of expressions on a vector is one expression per element: each element
                // takes its own, and the whole param steps back to its literal with the list retained.
                if let Some(items) = element_expressions(&existing, name, spec) {
                    let mut element_spec = spec.clone();
                    element_spec.as_object_mut().expect("a list expression spec").shift_remove("value");
                    for (k, item) in items.iter().enumerate() {
                        let element = format!("{name}[{k}]");
                        let cur = self.param_source(uid, group, &element).map(|s| s.state);
                        element_spec["expression"] = serde_json::json!(item);
                        let (_, source) = param_change(&existing.dim(k), cur, &element_spec)
                            .map_err(|e| format!("params.{group}.{element}: {e}"))?;
                        if let Some(s) = source {
                            self.set_source(uid, group, &element, s)?;
                        }
                    }
                    let whole = SourceState { mode: Mode::Constant, ..source.expect("a list expression source") };
                    if let Some(v) = value {
                        self.update_param(uid, group, name, v)?;
                    }
                    self.set_source(uid, group, name, whole)?;
                    continue;
                }
                if value.is_none() && source.is_none() {
                    return Err(format!("params.{group}.{name} sets neither a value nor a source"));
                }
                if let Some(v) = value {
                    self.update_param(uid, group, name, v)?;
                }
                if let Some(s) = source {
                    self.set_source(uid, group, name, s)?;
                }
            }
        }
        Ok(())
    }
}

/// A CLI `--value` arrives as its raw string; for a param that is no string it is read as JSON
/// first, so `true` and `[1, 0]` mean what they say, and bare `1 0 0 1` reads as its numbers.
fn coerced_value(existing: &Param, v: &serde_json::Value) -> Param {
    let parsed = match (existing, v.as_str()) {
        (Param::Str { .. }, _) | (_, None) => None,
        (_, Some(s)) => serde_json::from_str::<serde_json::Value>(s).ok(),
    };
    param_from_json(existing, parsed.as_ref().unwrap_or(v))
}

/// The expression a whole VECTOR param is given as a Python list of exactly its dimensions, one
/// item per element; anything else — a scalar, an element, a list of another length or an
/// expression that merely evaluates to a list — is none of this.
fn element_expressions(existing: &Param, name: &str, spec: &serde_json::Value) -> Option<Vec<String>> {
    if goofi_node::element(name).1.is_some() || existing.dims() < 2 {
        return None;
    }
    let text = spec.get("expression")?.as_str()?.trim();
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    let items = split_top_level(inner);
    (items.len() == existing.dims()).then_some(items)
}

/// `text` split on the commas outside every bracket, parenthesis, brace and string.
fn split_top_level(text: &str) -> Vec<String> {
    use goofi_node::expr::{tokens, Kind};
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for token in tokens(text) {
        match token.kind {
            Kind::Punct(b'[' | b'(' | b'{') => depth += 1,
            Kind::Punct(b']' | b')' | b'}') => depth -= 1,
            Kind::Punct(b',') if depth == 0 => {
                items.push(text[start..token.start].trim().to_string());
                start = token.end;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() || !items.is_empty() {
        items.push(last.to_string());
    }
    items
}

/// What every writer hears when it offers a pulse a value, whichever shape it came in.
const PULSE_HOLDS_NO_VALUE: &str =
    "this param is a pulse and holds no value; fire it with `node param request`";

/// One `params.<group>.<name>` entry: a bare literal, or an object (no param type is one). A text
/// given names its mode unless one is said; a mode or a trigger alone edits what is retained.
fn param_change(
    existing: &Param,
    cur: Option<SourceState>,
    spec: &serde_json::Value,
) -> Result<(Option<Param>, Option<SourceState>), String> {
    let pulse = matches!(existing, Param::Pulse);
    let Some(o) = spec.as_object() else {
        if pulse {
            return Err(PULSE_HOLDS_NO_VALUE.to_string());
        }
        return Ok((Some(coerced_value(existing, spec)), None));
    };
    if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "value" | "expression" | "mode")) {
        return Err(format!("unknown field `{k}` — value, expression, mode"));
    }
    let field = |k: &str| o.get(k).filter(|v| !v.is_null());
    if pulse && field("value").is_some() {
        return Err(PULSE_HOLDS_NO_VALUE.to_string());
    }
    let value = field("value").map(|v| coerced_value(existing, v));
    let text = |k: &str| {
        field(k)
            .map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("{k} is a string, not {v}")))
            .transpose()
    };
    let expression = text("expression")?;
    let mode = field("mode")
        .map(|v| {
            serde_json::from_value::<Mode>(v.clone())
                .ok()
                .ok_or_else(|| format!("mode is `constant` or `expression`, not {v}"))
        })
        .transpose()?;
    if expression.is_none() && mode.is_none() {
        return Ok((value, None));
    }
    let cur = cur.unwrap_or_default();
    let mode = match (mode, expression.as_deref()) {
        (Some(m), _) => m,
        (None, Some("")) => Mode::Constant,
        (None, Some(_)) => Mode::Expression,
        (None, None) => cur.mode,
    };
    let state = SourceState { mode, expression: expression.unwrap_or(cur.expression) };
    if state.mode == Mode::Expression && state.expression.is_empty() {
        return Err("mode `expression` with no expression to evaluate".to_string());
    }
    Ok((value, Some(state)))
}

/// The active source of a param's value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Constant,
    Expression,
}

/// A param's authored source: the mode, and the expression it retains whatever the mode. Nothing
/// retained and a constant mode is no record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceState {
    pub mode: Mode,
    pub expression: String,
}

impl SourceState {
    pub fn is_empty(&self) -> bool {
        self.mode == Mode::Constant && self.expression.is_empty()
    }
}

/// What a param's authored source derives to against the graph, at settle: the state it was
/// derived from, and everything below it. The runtime's, keyed by node and param.
struct Binding {
    state: SourceState,
    /// Compiled from [`Self::rewritten`], never from the authored text: the evaluator is handed
    /// variables, not names. `None` when the compile failed, or there is no evaluator.
    id: Option<goofi_node::BindingId>,
    /// Derived: the text with every term replaced by a generated variable.
    rewritten: String,
    /// Derived: one entry per variable `rewritten` names, resolved against the graph.
    vars: Vec<BoundVar>,
    /// What each of `vars` resolves, in step with it: a re-derivation tells a param read apart.
    refs: Vec<expr_rewrite::VarRef>,
    /// Why the GRAPH could not bind this source. It describes the SOURCE, so it outlives any one
    /// instance.
    bind_error: Option<String>,
}

/// What a source derives to, before the compile.
struct Derived {
    rewritten: String,
    vars: Vec<BoundVar>,
    refs: Vec<expr_rewrite::VarRef>,
    error: Option<String>,
}

impl Binding {
    fn fresh(state: SourceState) -> Binding {
        Binding { state, id: None, rewritten: String::new(), vars: Vec::new(), refs: Vec::new(), bind_error: None }
    }

    /// Whether the graph SHIPS this source. A constant mode and a source the graph could not bind
    /// both leave the param on its literal, and the node is TOLD so.
    fn live(&self) -> bool {
        self.state.mode != Mode::Constant && self.bind_error.is_none()
    }
}

/// One variable's source as the follower reads it: the rewritten text, the evaluator's handle —
/// none for a bare source — and every variable it names, resolved.
pub struct VariableBinding<'a> {
    pub name: &'a str,
    pub rewritten: &'a str,
    pub id: Option<goofi_node::BindingId>,
    pub vars: &'a [BoundVar],
}

/// A source projected for the bridge and the `.gfi`: the authored state, and why it does not
/// hold — the graph could not bind it, or the node could not evaluate it.
pub struct SourceInfo {
    pub state: SourceState,
    pub error: Option<String>,
}

/// One end of a wire, resolved once: the slot's `&'static` name, its dtype, and whether it takes
/// many wires. A leaf reads it off its manifest; a port answers for itself.
#[derive(Clone, Copy)]
struct SlotFace {
    name: &'static str,
    kind: goofi_core::SlotType,
    multi: bool,
}

/// What a source address really produces. A port relays rather than runs, so its stream is
/// whatever is wired into it — and "nothing yet" is an answer, not a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// A real leaf output slot — the only thing the transport can subscribe to.
    At(Uid, &'static str),
    /// The port the walk stopped at, because nothing feeds it yet.
    Open(Uid),
}

/// A type the palette greys: why, and what it last resolved to. The manifest is kept because
/// instances born from it still run, and a row with no slots would erase them from every canvas.
struct Greyed {
    reason: String,
    last: Option<(&'static str, &'static NodeManifest)>,
}

/// Where a scanned type came from: the open patch's workspace, the user's own private library, a
/// node root by directory name, or an engine's own find — a plugin, which belongs to no tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Patch,
    Custom,
    Root(String),
    Plugin,
}

/// The document: what the archive, the replica and a paste fragment project. A VALUE: every root
/// a write can touch sits behind an `Arc`, a write copies the one node or root it touches, and a
/// clone is pointers — so the settle reads what a batch changed off two snapshots, pointer by
/// pointer, and a history entry is a snapshot.
#[derive(Clone)]
struct Patch {
    nodes: IndexMap<Uid, Arc<NodeEntry>>,
    /// Keyed by [`Link::key`], in connection order — which IS a multi input's wire order.
    links: Arc<IndexMap<String, Link>>,
    /// The panel arrangement, held FLAT — the fifth doc root. Every mutation is an ordinary
    /// write, so the layout has exactly one projection, as nodes and links do.
    arrangement: Arc<layout::Layout>,
    /// Patch-scoped variables, system ones seeded and re-asserted by every `clear`/load. Under a
    /// lock of its own: a value write is the store's and the plane's, never the graph's.
    variables: Arc<goofi_supervisor::sync::Mutex<goofi_core::variables::VariableStore>>,
    /// The state machines, the sixth doc root. Each playhead is a group of variables the machine
    /// owns, held in step with this model by [`Graph::sync_playheads`].
    machines: Arc<IndexMap<String, machine::Machine>>,
}

/// What runs the patch: the engines, the catalog, the mint and the live instances.
struct Runtime {
    /// The registered engines, signal first, reached only through the trait. Registered at the
    /// composition root, so an empty set is a bare MODEL — it serializes, and runs nothing.
    engines: Vec<Box<dyn Engine>>,
    /// Shared with every engine and the drain worker: a report notifies, the worker parks on it.
    waker: Arc<DrainWaker>,
    /// What service names are scoped by. Random, not the bridge's instance id: a service name has
    /// to be unique on the MACHINE, across this process's own graphs and every stale record.
    instance: String,
    /// Bumped by every settle that delivers and every birth: what a listener compares to know
    /// the graph it resolved against is gone, without taking the lock to look.
    epoch: Arc<std::sync::atomic::AtomicU64>,
    /// The patch's time, shared with every engine — one object, never a copy of what it says.
    time: Arc<goofi_supervisor::timing::Time>,
    /// `None` ⇒ bindings are stored and round-trip but never evaluate; the literal stands.
    evaluator: Option<Arc<dyn goofi_node::ExprEvaluator>>,
    /// Every variable with an expression, derived at settle as a param's source is.
    variable_binds: IndexMap<String, Binding>,
    /// Every leaf's authored sources derived against the graph, at the last settle and at the
    /// write that set them: the one place a compiled handle lives.
    bindings: HashMap<Uid, HashMap<ParamKey, Binding>>,
    /// The snapshot the last settle delivered. The next settle reads what changed off this and
    /// the patch, pointer by pointer, and then holds the patch here.
    settled: Patch,
    /// The resolved edges of that snapshot: what a wire that moved is told apart against.
    settled_edges: Vec<Edge>,
    /// The snapshot the document last projected, and the variable store's version then: what
    /// [`Graph::projection`] reads the changed records off.
    projected: Patch,
    projected_vars: u64,
    /// A variable's binding error moved since the last projection: the one runtime fact the
    /// document carries beside a variable.
    binds_moved: bool,
    /// What the settles since the last take moved, for the followers.
    marks: Marks,
    /// Bumped by every settle that delivered and every viewpoint write: what autosave compares to
    /// know the patch moved since it last wrote.
    revision: u64,
    /// The variable store's configuration version at the last settle: a variable added, removed,
    /// renamed or re-sourced re-derives every binding and re-sends every node.
    settled_vars: u64,
    /// Types that exist on disk but cannot load here → why. Greyed in the palette, so a node
    /// needing an uninstalled dependency explains itself instead of silently not existing.
    unavailable: std::collections::BTreeMap<String, Greyed>,
    /// Where each scanned type came from — the one thing about a type that only the scan can
    /// know. Re-derived wholesale by each scan.
    origins: std::collections::HashMap<String, Origin>,
    /// A process-lifetime counter: undo restores deleted uids, so none is ever handed out twice.
    next_uid: u64,
    /// Every node is re-sent at the next settle, whatever else moved: a `system.*` value an
    /// engine reads off the view moved, or the library rescanned.
    redeliver: bool,
    /// Nodes whose instance is respawned at the next settle, in place, by request.
    restarts: Vec<Uid>,
    /// Nodes a reducer started or stopped watching since the last settle: an input of a settle no
    /// snapshot holds.
    rewatched: Vec<Uid>,
    /// Every uid's birth generation, bumped on EVERY birth and never reset, so a reborn node's
    /// service names stay clear of its predecessor's. Never enters the archive.
    generations: HashMap<Uid, u64>,
    /// The last arming serial minted; every arming of a slot gets the next one.
    arm_serial: u64,
    /// Params whose options a node has re-enumerated since anyone looked. Options are the one
    /// thing a node reports that the doc has no field for, so the worker must be TOLD to echo them.
    refreshed: Vec<(Uid, ParamKey)>,
    /// The output slots a reducer watches; each producer rings the slot's view door once its
    /// frame is out, so the reducer wakes on the frame rather than on a clock.
    watched: HashSet<(Uid, String)>,
    /// What each watched slot's viewers asked the producer to make; offered only while no wire
    /// reads the slot, since a consumer takes the frame itself, never a viewer's preview.
    view_wants: HashMap<(Uid, String), Option<goofi_view::ViewWant>>,
    /// Every leaf an engine runs, by uid. A leaf absent here is born at the next settle.
    instances: HashMap<Uid, Instance>,
}

pub struct Graph {
    patch: Patch,
    /// Where a client is LOOKING. Not a doc root — converging it would drag peers and dirty the
    /// patch on mere navigation — but persistence is the other axis, so it rides the `.gfi`.
    viewpoint: serde_json::Value,
    runtime: Runtime,
}

impl Drop for Graph {
    /// A node's transport is owned by its own thread and releases its segments when it DROPS, so
    /// raising the halt flags and returning leaves every one allocated if the process exits first.
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Whether an array is written beside the manifest as a native file rather than into it.
fn is_wide(value: &Data) -> bool {
    matches!(value.value(), goofi_core::Value::Array(a) if a.as_bytes().len() > 64 * 4)
}

/// Where a wide variable's array lives in a workspace: `variables/<name>.npy`.
fn variable_file(workspace: &std::path::Path, name: &str) -> std::path::PathBuf {
    workspace.join("variables").join(format!("{name}.npy"))
}

/// The array a workspace holds for `name`.
fn read_variable_file(workspace: &std::path::Path, name: &str) -> Result<Data, String> {
    let path = variable_file(workspace, name);
    let file = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (shape, samples) = goofi_core::replay::read_npy(&file).map_err(|e| format!("{}: {e}", path.display()))?;
    Data::array_f32(shape, samples, goofi_core::Meta::default()).map_err(|e| e.to_string())
}

/// The lowest free doorbell id in the expression range, or `None` when a node has spent all 64.
fn next_event_id(taken: &[EventId]) -> Option<EventId> {
    (65..=128).find(|id| !taken.contains(id))
}

impl Graph {
    /// `instance` is the service-name scope this graph mints under: fresh per graph and never a
    /// pid, which is reused, since a recycled scope would JOIN stale services.
    pub fn new(instance: String) -> Graph {
        let patch = Patch {
            nodes: IndexMap::new(),
            links: Arc::new(IndexMap::new()),
            arrangement: Arc::new(layout::Layout::default()),
            variables: Arc::new(goofi_supervisor::sync::Mutex::new(goofi_core::variables::VariableStore::new())),
            machines: Arc::new(IndexMap::new()),
        };
        Graph {
            patch: patch.clone(),
            viewpoint: serde_json::Value::Null,
            runtime: Runtime {
                engines: Vec::new(),
                waker: Arc::new(DrainWaker::default()),
                instance,
                epoch: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                time: Arc::new(goofi_supervisor::timing::Time::new()),
                evaluator: None,
                variable_binds: IndexMap::new(),
                bindings: HashMap::new(),
                settled: patch.clone(),
                settled_edges: Vec::new(),
                settled_vars: 0,
                // Roots of its own, so the first projection carries every root, empty or not.
                projected: Patch {
                    nodes: IndexMap::new(),
                    links: Arc::new(IndexMap::new()),
                    arrangement: Arc::new(layout::Layout::default()),
                    variables: patch.variables.clone(),
                    machines: Arc::new(IndexMap::new()),
                },
                projected_vars: 0,
                binds_moved: false,
                marks: Marks::default(),
                revision: 0,
                unavailable: std::collections::BTreeMap::new(),
                origins: std::collections::HashMap::new(),
                next_uid: 1,
                redeliver: false,
                restarts: Vec::new(),
                rewatched: Vec::new(),
                generations: HashMap::new(),
                arm_serial: 0,
                refreshed: Vec::new(),
                watched: HashSet::new(),
                view_wants: HashMap::new(),
                instances: HashMap::new(),
            },
        }
    }

    /// A node's record to write. The one door every write to a node goes through: the snapshot
    /// keeps the record as it was, and the write copies it once.
    fn entry_mut(&mut self, uid: Uid) -> Option<&mut NodeEntry> {
        self.patch.nodes.get_mut(&uid).map(Arc::make_mut)
    }

    fn links_mut(&mut self) -> &mut IndexMap<String, Link> {
        Arc::make_mut(&mut self.patch.links)
    }

    fn machines_mut(&mut self) -> &mut IndexMap<String, machine::Machine> {
        Arc::make_mut(&mut self.patch.machines)
    }

    /// Stop every node and WAIT for each to release its shared memory. The waiting is why this is
    /// not what `clear()` does: only a process about to EXIT has no "a moment later".
    pub fn shutdown(&mut self) {
        for e in self.engines_mut() {
            e.shutdown();
        }
        self.patch.nodes.clear();
        self.runtime.instances.clear();
        self.runtime.bindings.clear();
        self.runtime.restarts.clear();
        self.runtime.rewatched.clear();
    }

    /// The authoritative variables store, taken for one statement: a guard held across a call
    /// that takes it again would deadlock, and the graph lock already orders every caller.
    pub fn variables(&self) -> std::sync::MutexGuard<'_, goofi_core::variables::VariableStore> {
        self.patch.variables.lock()
    }

    /// The store itself, for the writers that take no graph lock: the follower and the machines.
    pub fn variable_store(&self) -> Arc<goofi_supervisor::sync::Mutex<goofi_core::variables::VariableStore>> {
        self.patch.variables.clone()
    }

    /// Every door the variables producer rings: per variable, the consumer and the id a live
    /// binding on it streams by, with the consumer's generation to name its door.
    pub fn variable_ringers(&self) -> Vec<(String, Uid, u64, EventId)> {
        let mut out = Vec::new();
        for (uid, e) in &self.patch.nodes {
            let Some(leaf) = e.leaf() else { continue };
            for (_, b) in self.bindings_of(*uid).filter(|(_, b)| b.live()) {
                for v in &b.vars {
                    if let BoundVar::Stream { producer: Uid::VARIABLES, slot, event_id, .. } = v {
                        out.push((slot.to_string(), *uid, self.node_generation(*uid), *event_id));
                    }
                }
            }
            // A feed: the variable it names enters an input slot, rung by that slot's id.
            let typed = self.typed(leaf);
            for (slot, group, d) in goofi_node::feed_decls(leaf.manifest) {
                let Some(at) = leaf.manifest.inputs.iter().position(|s| s.name == slot) else { continue };
                if let Some(name) = goofi_node::feed_name(&typed, group, d) {
                    out.push((name, *uid, self.node_generation(*uid), (at + 1).min(64) as EventId));
                }
            }
        }
        out
    }

    /// Every variable group the patch refers to: in a source's text, a feed, or a variable's
    /// expression. What holds a MIDI device open — a group a device's name resolves to is read.
    pub fn referenced_groups(&self) -> HashSet<String> {
        let group_of = |name: &str| goofi_core::variables::split_variable(name).map(|(g, _)| g.to_string());
        let mut out = HashSet::new();
        let node_refs = self.runtime.bindings.values().flat_map(|m| m.values()).flat_map(|b| &b.refs);
        for r in node_refs.chain(self.runtime.variable_binds.values().flat_map(|b| &b.refs)) {
            if let expr_rewrite::Target::Variable { key } = &r.target {
                out.extend(group_of(key));
            }
        }
        for (_, leaf) in self.leaves() {
            let typed = self.typed(leaf);
            for (_, group, d) in goofi_node::feed_decls(leaf.manifest) {
                if let Some(goofi_core::Param::Str { value, .. }) = goofi_node::param(&typed, d.group, group) {
                    if !value.is_empty() {
                        out.insert(value.clone());
                    }
                }
            }
        }
        out
    }

    /// Apply one variable change; a NEW variable lands at position `at`, and `None` leaves the value
    /// alone. The next settle re-sends every binding that reads it.
    pub fn apply_variable_change(
        &mut self,
        name: &str,
        value: Option<Data>,
        at: Option<usize>,
        control: Option<Option<goofi_core::variables::Control>>,
    ) -> Result<(), String> {
        self.variables().apply_change(name, value, at, control)?;
        // An engine reads goofi's own groups off the settled view, so a value there is delivered by
        // a settle.
        if goofi_core::variables::split_variable(name).is_some_and(|(g, _)| goofi_core::variables::is_own_group(g)) {
            self.runtime.redeliver = true;
        }
        Ok(())
    }

    /// Set or clear the expression a variable is computed by, answering the one it replaced. The
    /// text is held to the spelling a param's is; what it names is resolved at every settle.
    pub fn set_variable_expression(&mut self, name: &str, expression: Option<String>) -> Result<Option<String>, String> {
        if let Some(e) = &expression {
            expr_rewrite::rewrite(e).map_err(|e| e.0)?;
        }
        self.variables().set_expression(name, expression)
    }

    /// Why a variable's expression cannot bind, from the last settle.
    pub fn variable_binding_error(&self, name: &str) -> Option<String> {
        self.runtime.variable_binds.get(name).and_then(|b| b.bind_error.clone())
    }

    /// Every variable with a live expression, as the follower reads it.
    pub fn variable_bindings(&self) -> Vec<VariableBinding<'_>> {
        self.runtime
            .variable_binds
            .iter()
            .filter(|(_, b)| b.live())
            .map(|(name, b)| VariableBinding { name, rewritten: &b.rewritten, id: b.id, vars: &b.vars })
            .collect()
    }

    /// Derive every variable's expression against the graph, as a param's source is: a handle
    /// compiled once per text, and the reason a text cannot bind kept beside it.
    fn derive_variables(&mut self) {
        let wanted: Vec<(String, String)> =
            self.variables().entries().filter_map(|(n, v)| Some((n.to_string(), v.expression.clone()?))).collect();
        let gone: Vec<String> = self.runtime.variable_binds.keys().filter(|k| !wanted.iter().any(|(n, _)| n == *k)).cloned().collect();
        for name in gone {
            if let Some(b) = self.runtime.variable_binds.shift_remove(&name) {
                self.runtime.binds_moved |= b.bind_error.is_some();
                if let (Some(ev), Some(id)) = (&self.runtime.evaluator, b.id) {
                    ev.release(id);
                }
            }
        }
        for (name, expression) in wanted {
            let state = SourceState { mode: Mode::Expression, expression };
            let prior = self.runtime.variable_binds.shift_remove(&name).unwrap_or_else(|| Binding::fresh(state.clone()));
            let derived = self.derive(Uid::VARIABLES, &ParamKey::new("variables", &name), &state);
            let (id, error) = self.compiled(&prior, &derived);
            self.runtime.binds_moved |= prior.bind_error != error;
            let record = Binding { state, id, rewritten: derived.rewritten, vars: derived.vars, refs: derived.refs, bind_error: error };
            self.runtime.variable_binds.insert(name, record);
        }
    }

    /// Whether a group name is held: by the variables, or by a control panel that names it.
    pub fn group_taken(&self, group: &str) -> bool {
        self.variables().has_group(group) || self.patch.arrangement.control_panels().iter().any(|(_, held)| held == group)
    }

    /// Add an empty group.
    pub fn add_variable_group(&mut self, group: &str, at: Option<usize>) -> Result<(), String> {
        if self.group_taken(group) {
            return Err(format!("variable group `{group}` already exists"));
        }
        self.variables().add_group(group, at)
    }

    /// Remove an empty group's record. A control panel naming it keeps the name, over nothing.
    pub fn remove_variable_group(&mut self, group: &str) -> Result<(), String> {
        self.variables().remove_group(group)
    }

    /// Rename one variable, and rewrite every expression that reads it.
    pub fn rename_variable(&mut self, from: &str, to: &str) -> Result<Vec<Uid>, String> {
        self.variables().rename(from, to)?;
        let touched = self.rewrite_variable_reads(&[(from.to_string(), to.to_string())]);
        Ok(touched)
    }

    /// Rename a group, and rewrite every expression that reads any member.
    pub fn rename_variable_group(&mut self, from: &str, to: &str) -> Result<Vec<Uid>, String> {
        if self.patch.arrangement.control_panels().iter().any(|(_, group)| group == to) {
            return Err(format!("variable group `{to}` already exists"));
        }
        let writes = self.patch.arrangement.retarget("control", "group", from, to);
        if writes.is_empty() && !self.variables().has_group(from) {
            return Err(format!("no variable group `{from}`"));
        }
        let moved = self.variables().rename_group(from, to)?;
        let touched = self.rewrite_variable_reads(&moved);
        self.arrangement_mut().set_contents(&writes);
        Ok(touched)
    }

    pub fn machines(&self) -> &IndexMap<String, machine::Machine> {
        &self.patch.machines
    }

    /// Set, replace or remove (`None`) a machine whole, answering the record it replaced. A new one
    /// lands at `at`. Every playhead's group follows.
    pub fn set_machine(&mut self, name: &str, record: Option<machine::Machine>, at: Option<usize>) -> Result<Option<machine::Machine>, String> {
        if !goofi_core::variables::is_valid_identifier(name) {
            return Err(format!("invalid machine name `{name}`: {}", machine::NAME_RULE));
        }
        let old = match record {
            None => self.machines_mut().shift_remove(name),
            Some(record) => {
                record.check()?;
                for ph in record.playheads.keys() {
                    let mine = self.variables().machine(ph) == Some(name);
                    if !mine && self.group_taken(ph) {
                        return Err(format!("`{ph}` is a variable group already; a playhead's name is its group"));
                    }
                }
                match self.patch.machines.get_index_of(name) {
                    Some(_) => self.machines_mut().insert(name.to_string(), record),
                    None => {
                        let at = at.unwrap_or(usize::MAX).min(self.patch.machines.len());
                        self.machines_mut().shift_insert(at, name.to_string(), record);
                        None
                    }
                }
            }
        };
        self.sync_playheads();
        Ok(old)
    }

    /// Rename a machine, keeping its place. Its playheads' groups are the playheads' own names.
    pub fn rename_machine(&mut self, from: &str, to: &str) -> Result<(), String> {
        if !goofi_core::variables::is_valid_identifier(to) {
            return Err(format!("invalid machine name `{to}`: {}", machine::NAME_RULE));
        }
        if self.patch.machines.contains_key(to) {
            return Err(format!("machine `{to}` already exists"));
        }
        let at = self.patch.machines.get_index_of(from).ok_or_else(|| format!("no machine `{from}`"))?;
        let record = self.machines_mut().shift_remove(from).expect("the index answered");
        self.machines_mut().shift_insert(at, to.to_string(), record);
        let writes = self.patch.arrangement.retarget("machine", "machine", from, to);
        self.arrangement_mut().set_contents(&writes);
        self.sync_playheads();
        Ok(())
    }

    /// Rename a playhead, and rewrite every expression that reads its variables.
    pub fn rename_playhead(&mut self, machine: &str, from: &str, to: &str) -> Result<Vec<Uid>, String> {
        let m = self.patch.machines.get(machine).ok_or_else(|| format!("no machine `{machine}`"))?;
        if !m.playheads.contains_key(from) {
            return Err(format!("no playhead `{from}` in machine `{machine}`"));
        }
        let mut next = m.clone();
        let at = next.playheads.get_index_of(from).expect("checked");
        let record = next.playheads.shift_remove(from).expect("checked");
        next.playheads.shift_insert(at, to.to_string(), record);
        let moved: Vec<(String, String)> =
            m.playhead_entries(from).into_iter().map(|(name, ..)| (name.clone(), name.replacen(from, to, 1))).collect();
        self.set_machine(machine, Some(next), None)?;
        Ok(self.rewrite_variable_reads(&moved))
    }

    /// Rename an attribute in every state and playhead, and rewrite every expression that reads it.
    pub fn rename_attribute(&mut self, machine: &str, from: &str, to: &str) -> Result<Vec<Uid>, String> {
        let m = self.patch.machines.get(machine).ok_or_else(|| format!("no machine `{machine}`"))?;
        if !m.attributes.contains_key(from) {
            return Err(format!("no attribute `{from}` in machine `{machine}`"));
        }
        if m.attributes.contains_key(to) {
            return Err(format!("attribute `{to}` already exists in machine `{machine}`"));
        }
        let mut next = m.clone();
        fn rekey<V>(map: &mut IndexMap<String, V>, from: &str, to: &str) {
            if let Some(at) = map.get_index_of(from) {
                let v = map.shift_remove(from).expect("the index answered");
                map.shift_insert(at, to.to_string(), v);
            }
        }
        rekey(&mut next.attributes, from, to);
        for s in next.states.values_mut() {
            rekey(&mut s.values, from, to);
            rekey(&mut s.envelopes, from, to);
        }
        next.rewrite_expressions(|e| expr_rewrite::rename_me_fields(e, |f| (f == from).then(|| to.to_string())));
        let moved: Vec<(String, String)> = m.playheads.keys().map(|ph| (format!("{ph}.{from}"), format!("{ph}.{to}"))).collect();
        self.set_machine(machine, Some(next), None)?;
        Ok(self.rewrite_variable_reads(&moved))
    }

    /// Hold every playhead's group in step with the machines: one group per playhead, owned by its
    /// machine, with the machine's three elements and one entry per attribute; nothing else.
    fn sync_playheads(&mut self) {
        let mut store = self.variables();
        let wanted: Vec<(String, String, goofi_core::variables::Entries)> = self
            .patch
            .machines
            .iter()
            .flat_map(|(m, record)| record.playheads.keys().map(move |ph| (ph.clone(), m.clone(), record.playhead_entries(ph))))
            .collect();
        let gone: Vec<String> =
            store.groups().filter(|(g, rec)| rec.machine.is_some() && !wanted.iter().any(|(ph, m, _)| ph == g && Some(m.as_str()) == rec.machine.as_deref())).map(|(g, _)| g.to_string()).collect();
        for group in gone {
            store.release(&group);
        }
        for (ph, m, entries) in wanted {
            let record = goofi_core::variables::Group { machine: Some(m), ..Default::default() };
            let _ = store.claim(&ph, record, entries);
        }
    }

    /// Follow a set of variable renames into every expression that spells one — a variable's, a
    /// source's, a machine's — answering the nodes whose source text changed.
    fn rewrite_variable_reads(&mut self, moved: &[(String, String)]) -> Vec<Uid> {
        let rename = |name: &str| moved.iter().find(|(old, _)| old == name).map(|(_, new)| new.clone());
        self.rewrite_variable_expressions(|e| expr_rewrite::rename_variables(e, rename));
        for m in self.machines_mut().values_mut() {
            m.rewrite_expressions(|e| expr_rewrite::rename_variables(e, rename));
        }
        self.rewrite_sources(|s| {
            Some(SourceState { expression: expr_rewrite::rename_variables(&s.expression, rename)?, ..s.clone() })
        })
    }

    /// Re-set every variable expression `edit` rewrites.
    fn rewrite_variable_expressions(&mut self, edit: impl Fn(&str) -> Option<String>) {
        let edits: Vec<(String, String)> =
            self.variables().entries().filter_map(|(n, v)| Some((n.to_string(), edit(v.expression.as_deref()?)?))).collect();
        for (name, expression) in edits {
            let _ = self.variables().set_expression(&name, Some(expression));
        }
    }

    /// Re-set every source `edit` rewrites, answering the distinct nodes whose source changed.
    fn rewrite_sources(&mut self, edit: impl Fn(&SourceState) -> Option<SourceState>) -> Vec<Uid> {
        let edit = &edit;
        let edits: Vec<(Uid, ParamKey, SourceState)> = self
            .leaves()
            .flat_map(|(uid, l)| l.sources.iter().filter_map(move |(k, state)| Some((uid, k.clone(), edit(state)?))))
            .collect();
        let mut referrers: Vec<Uid> = Vec::new();
        for (uid, key, state) in edits {
            if self.set_source(uid, &key.group, &key.name, state).is_ok() && !referrers.contains(&uid) {
                referrers.push(uid);
            }
        }
        referrers
    }

    /// A leaf's derived bindings, as the last derivation left them.
    fn bindings_of(&self, uid: Uid) -> impl Iterator<Item = (&ParamKey, &Binding)> {
        self.runtime.bindings.get(&uid).into_iter().flat_map(|m| m.iter())
    }

    fn binding(&self, uid: Uid, key: &ParamKey) -> Option<&Binding> {
        self.runtime.bindings.get(&uid)?.get(key)
    }

    /// Derive the sources of the leaves `uids` names against the graph as it stands — at settle,
    /// for the nodes the batch changed and the referrers of what it moved, and for every node
    /// when a variable or the library moved. A node gone releases what it held. Answers the nodes
    /// whose binding moved, which their engine is re-sent.
    fn derive_nodes(&mut self, uids: &[Uid], edited: &[(Uid, ParamKey)]) -> Vec<Uid> {
        let mut moved = Vec::new();
        for &uid in uids {
            let Some(leaf) = self.leaf(uid) else {
                self.release_bindings(uid);
                continue;
            };
            let mut keys: Vec<ParamKey> = leaf.sources.keys().cloned().collect();
            keys.sort();
            let stale: Vec<ParamKey> =
                self.bindings_of(uid).map(|(k, _)| k).filter(|k| !leaf.sources.contains_key(k)).cloned().collect();
            let mut node_moved = !stale.is_empty();
            for key in stale {
                self.release_binding(uid, &key);
            }
            for key in keys {
                node_moved |= self.derive_one(uid, &key, edited);
            }
            if node_moved {
                moved.push(uid);
            }
        }
        moved
    }

    /// The leaves whose bindings spell one of `names` as a node, or as a slot — a port's name is
    /// a slot label on its facade — resolved or not: a node born, gone or renamed re-derives each.
    fn referrers(&self, names: &[&str]) -> Vec<Uid> {
        let spells = |t: &Target| match t {
            Target::Node { name, slot } => {
                names.contains(&name.as_str()) || slot.as_deref().is_some_and(|s| names.contains(&s))
            }
            Target::NodeParam { name, .. } => names.contains(&name.as_str()),
            _ => false,
        };
        self.runtime
            .bindings
            .iter()
            .filter(|(_, m)| m.values().any(|b| b.refs.iter().any(|r| spells(&r.target))))
            .map(|(uid, _)| *uid)
            .collect()
    }

    /// The leaves whose bindings read a producer's stream: what a wire made or cut re-derives.
    fn stream_readers(&self) -> Vec<Uid> {
        self.runtime
            .bindings
            .iter()
            .filter(|(_, m)| m.values().any(|b| b.refs.iter().any(|r| matches!(r.target, Target::Node { .. }))))
            .map(|(uid, _)| *uid)
            .collect()
    }

    /// The leaves whose bindings read one of the params `edited` names: a param read follows an
    /// authored edit of that param, so each re-derives.
    fn param_readers(&self, edited: &[(Uid, ParamKey)]) -> Vec<Uid> {
        if edited.is_empty() {
            return Vec::new();
        }
        let named: Vec<(&str, &ParamKey)> =
            edited.iter().filter_map(|(u, k)| Some((self.name(*u)?, k))).collect();
        let reads = |consumer: Uid, t: &Target| match t {
            Target::NodeParam { name, group, param } => {
                named.iter().any(|(n, k)| *n == name && k.group == *group && k.name == *param)
            }
            Target::MeParam { group, param } => {
                edited.iter().any(|(u, k)| *u == consumer && k.group == *group && k.name == *param)
            }
            _ => false,
        };
        self.runtime
            .bindings
            .iter()
            .filter(|(uid, m)| m.values().any(|b| b.refs.iter().any(|r| reads(**uid, &r.target))))
            .map(|(uid, _)| *uid)
            .collect()
    }

    /// Derive one source against the graph, and keep the result when it says something the held
    /// binding does not. Answers whether it moved.
    fn derive_one(&mut self, uid: Uid, key: &ParamKey, edited: &[(Uid, ParamKey)]) -> bool {
        let Some(state) = self.leaf(uid).and_then(|e| e.sources.get(key)).cloned() else { return false };
        let derived = self.derive(uid, key, &state);
        let fresh;
        let prior = match self.binding(uid, key) {
            Some(b) => b,
            None => {
                fresh = Binding::fresh(state.clone());
                &fresh
            }
        };
        if self.binding(uid, key).is_some() && prior.state == state && !self.moved(uid, prior, &derived, edited) {
            return false;
        }
        let (id, error) = self.compiled(prior, &derived);
        let record = Binding {
            state,
            id,
            rewritten: derived.rewritten,
            vars: derived.vars,
            refs: derived.refs,
            bind_error: error,
        };
        self.runtime.bindings.entry(uid).or_default().insert(key.clone(), record);
        true
    }

    /// Whether a derivation says something the record does not. A param read follows an authored
    /// edit of that param, never its evaluation, or driven params would cascade through every settle.
    fn moved(&self, consumer: Uid, prior: &Binding, next: &Derived, edited: &[(Uid, ParamKey)]) -> bool {
        if prior.rewritten != next.rewritten || prior.bind_error != next.error || prior.refs != next.refs {
            return true;
        }
        prior.vars.iter().zip(&next.vars).zip(&next.refs).any(|((was, now), r)| match &r.target {
            Target::NodeParam { name, group, param } => {
                self.uid_by_name(name).is_some_and(|uid| edited.iter().any(|(u, k)| *u == uid && k.group == *group && k.name == *param))
            }
            Target::MeParam { group, param } => {
                edited.iter().any(|(u, k)| *u == consumer && k.group == *group && k.name == *param)
            }
            _ => was != now,
        })
    }

    /// The handle for a derivation: the prior one where the rewritten text is the text it
    /// compiled, else a fresh compile, and the prior released. A refused compile is the error. A
    /// BARE text compiles to nothing: the engine reads its one target in Rust.
    fn compiled(&self, prior: &Binding, next: &Derived) -> (Option<goofi_node::BindingId>, Option<String>) {
        let evaluator = self.runtime.evaluator.as_ref();
        if prior.state.mode == Mode::Expression && next.error.is_none() && !expr_rewrite::is_bare(&next.rewritten) {
            if prior.id.is_some() && prior.rewritten == next.rewritten {
                return (prior.id, None);
            }
            let Some(ev) = evaluator else {
                return (None, Some("no expression evaluator available".to_string()));
            };
            if let Some(id) = prior.id {
                ev.release(id);
            }
            return match ev.compile(&next.rewritten) {
                Ok(c) => (Some(c.id), None),
                Err(e) => (None, Some(e.0)),
            };
        }
        if let (Some(ev), Some(id)) = (evaluator, prior.id) {
            ev.release(id);
        }
        (None, next.error.clone())
    }

    fn source_state(&self, uid: Uid, key: &ParamKey) -> Option<SourceState> {
        self.leaf(uid).and_then(|e| e.sources.get(key)).cloned()
    }

    /// The evaluator the graph compiles against, for the manager's own worker.
    pub fn evaluator(&self) -> Option<Arc<dyn goofi_node::ExprEvaluator>> {
        self.runtime.evaluator.clone()
    }

    /// Inject the param-expression evaluator (pyo3, from goofi-python). Wired by the CLI at
    /// startup; without it, expression bindings are stored but not evaluated.
    pub fn set_evaluator(&mut self, evaluator: Arc<dyn goofi_node::ExprEvaluator>) {
        self.runtime.evaluator = Some(evaluator.clone());
        for e in self.engines_mut() {
            e.set_evaluator(evaluator.clone());
        }
        // Every binding reads through the new evaluator: the next settle re-derives and re-sends.
        self.runtime.redeliver = true;
    }

    /// Register an engine, signal first by convention. Its library joins the merged view, and its
    /// nodes ride every generic path — the trait is the whole integration. The id must be free.
    pub fn register_engine(&mut self, engine: Box<dyn Engine>) {
        // The id roots every type id it advertises, so a second holder makes `id:Name` name two.
        assert!(
            self.runtime.engines.iter().all(|e| e.id() != engine.id()),
            "engine id `{}` is already registered",
            engine.id()
        );
        self.runtime.engines.push(engine);
        self.forget_typed();
    }

    /// What every engine and the drain worker share: a node report notifies it, the worker parks
    /// on it between paced duties — the alternative to a poll-to-discover.
    pub fn drain_waker(&self) -> Arc<DrainWaker> {
        self.runtime.waker.clone()
    }

    /// The change epoch, readable without the lock; see the field.
    pub fn epoch(&self) -> Arc<std::sync::atomic::AtomicU64> {
        self.runtime.epoch.clone()
    }

    fn engines(&self) -> impl Iterator<Item = &dyn Engine> {
        self.runtime.engines.iter().map(|e| e.as_ref() as &dyn Engine)
    }

    fn engines_mut(&mut self) -> impl Iterator<Item = &mut dyn Engine> {
        self.runtime.engines.iter_mut().map(|e| e.as_mut() as &mut dyn Engine)
    }

    fn engine(&self, id: &str) -> Option<&dyn Engine> {
        self.engines().find(|e| e.id() == id)
    }

    /// Tell whichever engine owns `uid` what its readers want of `slot`. Offered to every engine
    /// rather than routed: an engine that does not hold the uid, or cannot render to size, no-ops.
    pub fn set_view_demand(&mut self, uid: Uid, slot: &str, want: Option<goofi_view::ViewWant>) {
        if uid == Uid::VARIABLES {
            return;
        }
        self.runtime.view_wants.insert((uid, slot.to_string()), want);
        let edges = self.resolved_edges();
        self.offer_view_wants(&edges);
    }

    /// Offer every engine its viewers' demand, or the full frame where a wire reads the slot.
    fn offer_view_wants(&mut self, edges: &[Edge]) {
        let Runtime { view_wants, engines, .. } = &mut self.runtime;
        for ((uid, slot), want) in view_wants.iter() {
            let wired = edges.iter().any(|e| e.producer.0 == *uid && e.producer.1 == slot);
            for e in engines.iter_mut() {
                e.view_demand(*uid, slot, if wired { None } else { *want });
            }
        }
    }

    /// Start or stop watching a producer's output: while watched, its every frame rings the
    /// slot's view door. Answers whether that moved, which the settle then delivers.
    pub fn set_view_watch(&mut self, uid: Uid, slot: &str, on: bool) -> bool {
        let key = (uid, slot.to_string());
        // A node removed since the caller looked is never marked watched: its removal ran past.
        if !self.leaf(uid).is_some_and(|l| l.manifest.outputs.iter().any(|o| o.name == slot)) {
            return !on && self.runtime.watched.remove(&key);
        }
        let changed = if on { self.runtime.watched.insert(key) } else { self.runtime.watched.remove(&key) };
        if changed {
            self.runtime.rewatched.push(uid);
        }
        changed
    }

    /// Whether a viewer's feed watches this output now (test/diagnostic).
    pub fn view_watched(&self, uid: Uid, slot: &str) -> bool {
        self.runtime.watched.contains(&(uid, slot.to_string()))
    }

    pub fn engine_mut(&mut self, id: &str) -> Option<&mut dyn Engine> {
        self.engines_mut().find(|e| e.id() == id)
    }

    /// Whether a node of `engine`'s `type_name` has an editor window of its own.
    pub fn type_has_editor(&self, engine: &str, type_name: &str) -> bool {
        self.engine(engine).is_some_and(|e| e.has_editor(type_name))
    }

    /// Show or hide a node's own editor: the action, for the caller to run off the lock.
    pub fn node_editor(&mut self, uid: Uid, show: bool) -> Result<EditorAction, String> {
        if self.is_facade(uid) {
            return Err("a sub-patch has no editor".into());
        }
        if self.stub(uid).is_some() {
            return Err("a port has no editor".into());
        }
        let (engine, type_name) =
            self.leaf(uid).map(|e| (e.engine, e.manifest.type_name)).ok_or_else(|| format!("no such node {uid}"))?;
        let e = self.engine_mut(engine).expect("a leaf's engine is registered");
        if !e.has_editor(type_name) {
            return Err(format!("`{}` has no editor", goofi_node::qualify(engine, type_name)));
        }
        e.editor(uid, show)
    }

    /// Every value a node's own editor wrote since the last call, for the worker to put through
    /// the param op — the one door a value enters the document by.
    pub fn take_edits(&mut self) -> Vec<Edit> {
        self.engines_mut().flat_map(|e| e.take_edits()).collect()
    }

    /// Every advertised entry with the id of the engine that advertises it, in registration order.
    pub fn library_entries(&self) -> Vec<(&'static str, LibraryEntry)> {
        self.engines().flat_map(|e| e.library().into_iter().map(move |l| (e.id(), l))).collect()
    }

    /// The one entry a type reference names. A qualified id names its engine; a bare name resolves
    /// only when exactly one engine offers it.
    pub fn resolve_type(&self, type_ref: &str) -> Result<(&'static str, LibraryEntry), String> {
        let (engine, name) = goofi_node::split_type_id(type_ref);
        let mut hits = self
            .library_entries()
            .into_iter()
            .filter(|(e, l)| l.manifest.type_name == name && engine.is_none_or(|want| want == *e));
        match (hits.next(), hits.next()) {
            (Some(hit), None) => Ok(hit),
            (None, _) => Err(self.reject_type(type_ref)),
            (Some(a), Some(b)) => {
                let mut all = vec![goofi_node::qualify(a.0, name), goofi_node::qualify(b.0, name)];
                all.extend(hits.map(|(e, _)| goofi_node::qualify(e, name)));
                Err(format!("`{name}` is offered by more than one engine: {}", all.join(", ")))
            }
        }
    }

    /// The entry a type reference resolves to, and the id of the engine that advertised it —
    /// which IS the engine the type belongs to.
    pub fn library_entry(&self, type_ref: &str) -> Option<(&'static str, LibraryEntry)> {
        self.resolve_type(type_ref).ok()
    }

    /// The universal param group an engine adds to every one of its nodes. By ENGINE and MANIFEST,
    /// because a name would have to be resolved, and two engines may offer one.
    pub fn universal_decls(&self, engine: &str, manifest: &'static NodeManifest) -> Vec<ParamDecl> {
        self.engine(engine).map(|e| e.universal_decls(manifest)).unwrap_or_default()
    }

    /// Forget the unavailable row for a type that now resolves — a registration's caller clears
    /// it, or the greyed row would give one name two palette rows.
    pub fn forget_unavailable(&mut self, type_name: &str) -> bool {
        self.runtime.unavailable.remove(type_name).is_some()
    }

    /// Scan `root` for every engine, each taking the files that name it, and keep the greyed
    /// overlay in step with what each registered: a name holds a registration or a reason, never both.
    pub fn scan_root(&mut self, root: &std::path::Path) -> Vec<goofi_node::ScannedType> {
        let held = self.held_manifests();
        let mut out = Vec::new();
        if root.is_dir() {
            goofi_supervisor::progress::scanning(root, goofi_node::node_file_count(root));
            for engine in &mut self.runtime.engines {
                out.extend(qualified(engine.id(), engine.scan(root)));
            }
            self.forget_typed();
            let unavailable = out.iter().filter(|t| matches!(t.outcome, goofi_node::Scanned::Unavailable(_))).count();
            goofi_supervisor::progress::indexed(root, out.len() - unavailable, unavailable);
        }
        self.note_scanned(&out, &held);
        out
    }

    /// Every engine's off-lock work for a scan of `dir` (see `Engine::prepare`).
    pub fn prepare(&self, dir: &std::path::Path) -> Vec<Box<dyn FnOnce() + Send>> {
        self.runtime.engines.iter().filter_map(|e| e.prepare(dir)).collect()
    }

    pub fn engine_ids(&self) -> Vec<&'static str> {
        self.runtime.engines.iter().map(|e| e.id()).collect()
    }

    /// What the engines find on their own account, after every root.
    pub fn scan_own(&mut self) -> Vec<goofi_node::ScannedType> {
        let held = self.held_manifests();
        let out: Vec<_> =
            self.runtime.engines.iter_mut().flat_map(|e| qualified(e.id(), e.scan_own())).collect();
        self.forget_typed();
        self.note_scanned(&out, &held);
        out
    }

    /// What every name resolved to before a scan displaced it — the last good manifest a greyed
    /// row keeps, so a node still running on it keeps its slots and its params.
    fn held_manifests(&self) -> HashMap<String, (&'static str, &'static NodeManifest)> {
        self.library_entries()
            .into_iter()
            .map(|(id, l)| (goofi_node::qualify(id, l.manifest.type_name), (id, l.manifest)))
            .collect()
    }

    fn note_scanned(&mut self, out: &[goofi_node::ScannedType], held: &HashMap<String, (&'static str, &'static NodeManifest)>) {
        for t in out {
            match &t.outcome {
                goofi_node::Scanned::Registered { .. } => {
                    self.runtime.unavailable.remove(&t.type_name);
                }
                // A name a library still answers is never greyed: one name, one row.
                goofi_node::Scanned::Unavailable(reason) if self.library_entry(&t.type_name).is_none() => {
                    // A file that broke keeps the manifest it last loaded: its instances are still
                    // running on it, still wired, and a row with no slots would erase them.
                    let last = held.get(&t.type_name).copied().or_else(|| self.last_owner(&t.type_name));
                    self.runtime.unavailable.insert(t.type_name.clone(), Greyed { reason: reason.clone(), last });
                }
                goofi_node::Scanned::Unavailable(_) => {}
            }
        }
    }

    /// The engine and manifest a greyed name last resolved to, if it ever did.
    fn last_owner(&self, type_name: &str) -> Option<(&'static str, &'static NodeManifest)> {
        self.runtime.unavailable.get(type_name).and_then(|g| g.last)
    }

    /// The manifest a greyed name last resolved to — the shape its live instances still hold.
    pub fn last_manifest(&self, type_name: &str) -> Option<&'static NodeManifest> {
        self.last_owner(type_name).map(|(_, m)| m)
    }

    /// Forget a scanned type from every registry — the engine that held it and the greyed
    /// overlay. Whether anything was forgotten.
    pub fn remove_type(&mut self, type_name: &str) -> bool {
        let (engine, name) = goofi_node::split_type_id(type_name);
        let mut had = false;
        for e in self.runtime.engines.iter_mut().filter(|e| engine.is_none_or(|want| want == e.id())) {
            had |= e.remove_type(name);
        }
        self.forget_typed();
        self.runtime.unavailable.remove(type_name).is_some() || had
    }

    /// The open patch's workspace, told to every engine: what a node's opaque state is kept in.
    pub fn set_workspace(&mut self, dir: &std::path::Path) {
        for engine in &mut self.runtime.engines {
            engine.set_workspace(dir);
        }
    }

    /// Tell every engine the boot scan is over.
    pub fn boot_done(&mut self) {
        for engine in &mut self.runtime.engines {
            engine.boot_done();
        }
    }

    /// Every live node's opaque state written into the workspace, so a pack carries it as it is.
    pub fn persist(&mut self) {
        for engine in &mut self.runtime.engines {
            engine.persist();
        }
    }

    /// Every wide variable's array written under `workspace/variables/` as a native file, the
    /// recorder's format, and every file there that names no wide variable removed.
    pub fn persist_variables(&self, workspace: &std::path::Path) -> Result<(), String> {
        write_variables(workspace, &self.wide_variables())
    }

    /// Every wide variable's value, by name: what [`write_variables`] puts in the workspace.
    pub fn wide_variables(&self) -> Vec<(String, Data)> {
        self.variables().entries().filter(|(_, v)| is_wide(&v.value)).map(|(n, v)| (n.to_string(), v.value.clone())).collect()
    }
}

/// Write `wide` into `workspace` as one `.npy` per variable, and remove the files of variables that
/// are no longer wide. Needs no graph: a caller takes the values under the lock and writes after.
pub fn write_variables(workspace: &std::path::Path, wide: &[(String, Data)]) -> Result<(), String> {
    {
        let dir = workspace.join("variables");
        if let Ok(held) = std::fs::read_dir(&dir) {
            for entry in held.flatten() {
                let keep = wide.iter().any(|(n, _)| entry.path() == variable_file(workspace, n));
                if !keep {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        if wide.is_empty() {
            let _ = std::fs::remove_dir(&dir);
            return Ok(());
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for (name, value) in wide {
            let goofi_core::Value::Array(a) = value.value() else { continue };
            let (path, bytes) = (variable_file(workspace, name), goofi_record::npy::bytes(a.shape(), a.as_bytes()));
            // An unchanged file is left alone: a write is an edit to the watcher and the baseline.
            if std::fs::read(&path).is_ok_and(|held| held == bytes) {
                continue;
            }
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(())
    }
}

impl Graph {

    /// Every engine that builds `.rs` node files, with the SDK crate it builds them against.
    pub fn rust_sdks(&self) -> Vec<(&'static str, &'static str)> {
        self.runtime.engines.iter().filter_map(|e| e.rust_sdk().map(|sdk| (e.id(), sdk))).collect()
    }

    /// The engine whose library resolves `type_name`.
    pub fn type_engine(&self, type_name: &str) -> Option<&'static str> {
        self.library_entry(type_name).map(|(id, _)| id)
    }

    /// The ONE phrasing for a rejected type, shared by `build_node` and the load gate. An
    /// unavailable type names its missing dependency; anything else reads as the typo it is.
    fn reject_type(&self, type_name: &str) -> String {
        // The overlay is keyed by the qualified name the scan registered; a bare reference to a
        // greyed type must read as unavailable rather than unknown.
        let greyed = self.runtime.unavailable.get(type_name).or_else(|| {
            let bare = goofi_node::bare(type_name);
            self.runtime.unavailable.iter().find(|(k, _)| goofi_node::bare(k) == bare).map(|(_, g)| g)
        });
        match greyed {
            Some(Greyed { reason, .. }) => format!("node type `{type_name}` is unavailable: {reason}"),
            None => format!("unknown node type `{type_name}`"),
        }
    }

    /// `creating` / `setup` / `ready` / `error`. Only `creating` is the graph's own — a node whose
    /// thread has not reported in yet. `error` means there is NO instance running.
    pub fn node_stage(&self, uid: Uid) -> &'static str {
        // A facade and a port never run, so they reach no stage — `ready` is what "nothing is
        // starting up here" means for something that is simply present.
        if self.leaf(uid).is_none() {
            return match self.patch.nodes.contains_key(&uid) {
                true => "ready",
                false => "error",
            };
        }
        // A `process()` raise is deliberately NOT folded in: the stage says whether the node has an
        // instance behind it, and what its last run did is the ERROR, which rides its own field.
        let Some(health) = self.health(uid) else { return "creating" };
        if health.setup_error.is_some() {
            return "error";
        }
        health.stage
    }

    /// What a leaf's instance reports; `None` until settle has born it.
    fn health(&self, uid: Uid) -> Option<&Health> {
        self.runtime.instances.get(&uid).map(|i| &i.health)
    }

    /// The node's current measured update frequency (Hz), as it last reported it. `None` until it
    /// has been measured (≥2 emits).
    pub fn node_ufreq(&self, uid: Uid) -> Option<f64> {
        self.health(uid).and_then(|h| h.ufreq)
    }

    /// Which node INSTANCE this uid holds: bumped on every birth, so a report from the node born at
    /// a uid is distinguishable from its predecessor's last one.
    pub fn node_generation(&self, uid: Uid) -> u64 {
        self.runtime.generations.get(&uid).copied().unwrap_or(0)
    }

    /// The flat arrangement — pages, splits and panels. Reads plan against this; writes go through
    /// a transaction, so undo and the replica come for free.
    pub fn arrangement(&self) -> &layout::Layout {
        &self.patch.arrangement
    }

    /// The one write door, held by the layout ops and by a load.
    pub fn arrangement_mut(&mut self) -> &mut layout::Layout {
        Arc::make_mut(&mut self.patch.arrangement)
    }

    /// The client-local viewpoint blob (see the field).
    pub fn viewpoint(&self) -> &serde_json::Value {
        &self.viewpoint
    }

    pub fn set_viewpoint(&mut self, viewpoint: serde_json::Value) {
        self.viewpoint = viewpoint;
        self.runtime.revision += 1;
    }

    /// Record a type that could not be loaded, and why. Refused while any engine's library still
    /// advertises the name — the scanner displaces a stale runtime type BEFORE recording it.
    pub fn register_unavailable(&mut self, type_name: String, reason: String) -> bool {
        if self.library_entry(&type_name).is_some() {
            return false;
        }
        // A name greyed twice keeps the manifest it had the first time.
        let last = self.last_owner(&type_name);
        self.runtime.unavailable.insert(type_name, Greyed { reason, last });
        true
    }

    /// The unloadable types, `(type_name, reason)`, sorted by name.
    pub fn unavailable_types(&self) -> impl Iterator<Item = (&str, &str)> {
        self.runtime.unavailable.iter().map(|(k, v)| (k.as_str(), v.reason.as_str()))
    }

    /// Declare where every scanned type came from — the palette's provenance badge and bundle.
    /// Written WHOLESALE, because only the scan knows the answer.
    pub fn set_type_origins(&mut self, origins: std::collections::HashMap<String, Origin>) {
        self.runtime.origins = origins;
    }

    /// One more type of the patch's own — what a type registered in place of a scan of it is.
    pub fn add_patch_type(&mut self, name: &str) {
        self.runtime.origins.insert(name.to_string(), Origin::Patch);
    }

    /// Where `type_name` was scanned from (see [`Graph::set_type_origins`]).
    pub fn origin(&self, type_name: &str) -> Option<&Origin> {
        self.runtime.origins.get(type_name)
    }

    pub fn node_count(&self) -> usize {
        self.patch.nodes.len()
    }

    /// The RUNNING node at `uid`, or `None` for a facade, a port, or nothing at all — the one seam
    /// every reader of a leaf-only field goes through.
    fn leaf(&self, uid: Uid) -> Option<&Leaf> {
        self.patch.nodes.get(&uid)?.leaf()
    }

    fn leaf_mut(&mut self, uid: Uid) -> Option<&mut Leaf> {
        self.entry_mut(uid)?.leaf_mut()
    }

    /// Every running node, in insertion order.
    fn leaves(&self) -> impl Iterator<Item = (Uid, &Leaf)> {
        self.patch.nodes.iter().filter_map(|(u, e)| e.leaf().map(|l| (*u, l)))
    }

    /// Is `uid` a RUNNING node — a leaf, not a facade or a port?
    pub fn is_leaf(&self, uid: Uid) -> bool {
        self.leaf(uid).is_some()
    }

    /// Is `uid` a live endpoint a wire may name — a leaf or a boundary port? A facade is not: an
    /// address naming one is folded onto its port before any link is stored.
    pub fn wirable(&self, uid: Uid) -> bool {
        self.is_leaf(uid) || self.stub(uid).is_some()
    }

    /// Node uids in insertion order.
    pub fn node_uids(&self) -> Vec<Uid> {
        self.leaves().map(|(u, _)| u).collect()
    }

    /// Is there a node of ANY kind at `uid`?
    pub fn exists(&self, uid: Uid) -> bool {
        self.patch.nodes.contains_key(&uid)
    }

    /// Every uid in the patch — leaves, facades and ports alike.
    pub fn all_uids(&self) -> Vec<Uid> {
        self.patch.nodes.keys().copied().collect()
    }

    pub fn manifest(&self, uid: Uid) -> Option<&'static NodeManifest> {
        self.leaf(uid).map(|e| e.manifest)
    }

    /// The tier `uid`'s instance runs on — a leaf alone wears one.
    pub fn node_tier(&self, uid: Uid) -> Option<Isolation> {
        self.leaf(uid).map(|e| e.isolation.get())
    }

    /// A TYPE's tier, from whichever engine's library advertises it.
    pub fn type_tier(&self, type_name: &str) -> Option<Isolation> {
        self.library_entry(type_name).map(|(_, l)| l.isolation.get())
    }

    /// Derived fresh on read, so a binding that recovers on a node which never runs again still
    /// clears. Initialization failure wins, then a process error, then the smallest errored key.
    pub fn last_error(&self, uid: Uid) -> Option<&str> {
        // A facade runs nothing, so its health is its members': the first errored descendant.
        self.subtree_of(&[uid]).into_iter().filter(|u| self.is_leaf(*u)).find_map(|u| entry_error(self.runtime.bindings.get(&u), self.health(u)))
    }

    /// How long this node's CURRENT error has been standing, or `None` when it is healthy. The
    /// clock restarts when the message changes and at every rebirth, so it never outlives an instance.
    pub fn error_age(&self, uid: Uid) -> Option<Duration> {
        let (_, since) = self.health(uid)?.error_since.as_ref()?;
        Some(since.elapsed())
    }

    pub(crate) fn mint(&mut self) -> Uid {
        let u = Uid(self.runtime.next_uid);
        self.runtime.next_uid += 1;
        u
    }

    /// The uid a loaded record restores at — the one the archive named, unless it is unreadable or
    /// already `claimed`. Restoring rather than reminting is what makes a load restore IDENTITY.
    fn restore_uid(&mut self, key: &str, claimed: &HashSet<Uid>) -> Uid {
        match Uid::from_hex(key).filter(|u| !claimed.contains(u)) {
            Some(u) => {
                // `from_hex` admits only the 48-bit canonical domain, so `+ 1` cannot overflow.
                self.runtime.next_uid = self.runtime.next_uid.max(u.0 + 1);
                u
            }
            None => self.mint(),
        }
    }
    /// The record a fresh instance of `type_name` starts from — the owning engine's own
    /// normalization, resolved without constructing the node. Also what the palette renders.
    pub fn default_params_of(&self, type_name: &str, supplied: Option<ParamGroups>) -> Result<ParamGroups, String> {
        let (id, manifest) = self.owner_of(type_name).ok_or_else(|| self.reject_type(type_name))?;
        Ok(self.engine(id).expect("the owner named it").normalize_params(manifest, supplied))
    }

    /// The engine and manifest a name resolves to — the library's, or the one a greyed name last
    /// had, whose instances are still running on it.
    fn owner_of(&self, type_name: &str) -> Option<(&'static str, &'static NodeManifest)> {
        match self.library_entry(type_name) {
            Some((id, entry)) => Some((id, entry.manifest)),
            None => self.last_owner(type_name),
        }
    }

    /// Instantiate a node by type name. `params` defaults to the type's defaults.
    pub fn add_node(&mut self, type_name: &str, params: Option<ParamGroups>) -> Result<Uid, String> {
        self.create_node(type_name, None, "", params.as_ref().map(values_of), None)
    }

    /// Create a leaf, a sub-patch facade or a boundary port at `uid`, or at a fresh mint. A port
    /// needs the `scope` it is a port OF; an empty or taken `name` is minted fresh.
    pub fn create_node(
        &mut self,
        type_name: &str,
        uid: Option<Uid>,
        name: &str,
        params: Option<doc::Values>,
        scope: Option<Uid>,
    ) -> Result<Uid, String> {
        if let Some(u) = uid.filter(|u| self.patch.nodes.contains_key(u)) {
            return Err(format!("node add: uid {} already in use", u.to_hex()));
        }
        if let Some(s) = scope.filter(|s| !self.is_facade(*s)) {
            return Err(format!("node add: no such scope {s}"));
        }
        let (kind, base) = match subpatch::boundary_type(type_name) {
            Some((dir, dtype)) => {
                if scope.is_none() {
                    return Err("node add: a boundary port needs a scope — it is a port OF a sub-patch".into());
                }
                (Kind::Port(subpatch::Port { dir, dtype }), dir.name().to_string())
            }
            None if type_name == subpatch::SCOPE_TYPE => (Kind::Facade, "subpatch".to_string()),
            None => {
                // A leaf is the only kind with a manifest, so it is the only one that can seed the
                // default expressions its type declares.
                let seed = params.is_none();
                let (engine, entry) = self.resolve_type(type_name)?;
                let values = values_of(&self.folded(type_name, &params.unwrap_or_default())?);
                let uid = self.claim(uid);
                let born = self.pick_name(name, &name_base(goofi_node::bare(type_name)), None);
                self.insert_node_at(uid, born.clone(), engine, entry, values);
                let manifest = entry.manifest;
                if seed {
                    self.seed_default_expressions(uid, engine, manifest);
                }
                self.set_member_scope(uid, scope);
                return Ok(uid);
            }
        };
        let uid = self.claim(uid);
        let born = self.pick_name(name, &base, None);
        self.patch.nodes.insert(uid, Arc::new(NodeEntry::new(kind, born, [0.0, 0.0])));
        self.set_member_scope(uid, scope);
        Ok(uid)
    }

    /// The uid a create will use, with the mint counter kept past it so a restored uid is never
    /// handed out a second time.
    fn claim(&mut self, uid: Option<Uid>) -> Uid {
        let uid = uid.unwrap_or_else(|| self.mint());
        self.runtime.next_uid = self.runtime.next_uid.max(uid.0 + 1);
        uid
    }

    /// The name asked for, or a fresh `base<N>` when it is empty, worn or illegal. A create degrades
    /// where a rename refuses: a hand-edited archive must cost one name, not the patch.
    fn pick_name(&self, want: &str, base: &str, except: Option<Uid>) -> String {
        match self.name_taken(want, except) || !goofi_core::variables::is_valid_name(want) {
            true => goofi_core::fresh_name(base, 0, |name| self.name_taken(name, None)),
            false => want.to_string(),
        }
    }

    fn seed_default_expressions(&mut self, uid: Uid, engine: &'static str, manifest: &'static NodeManifest) {
        if self.runtime.evaluator.is_none() {
            return;
        }
        // The manifest's own declarations win over the engine's universal group, as they do on
        // the value side.
        let declared = manifest.params.iter().map(|d| (d.group, d.name, d.expression));
        let universal = self
            .universal_decls(engine, manifest)
            .into_iter()
            .filter(|d| !manifest.params.iter().any(|o| o.group == d.group && o.name == d.name))
            .map(|d| (d.group, d.name, d.expression))
            .collect::<Vec<_>>();
        for (group, name, expression) in declared.chain(universal) {
            if let Some(e) = expression {
                let enabled = matches!(e.mode, ExprMode::On);
                let state = SourceState {
                    mode: if enabled { Mode::Expression } else { Mode::Constant },
                    expression: e.source.to_string(),
                };
                let _ = self.set_source(uid, group, name, state);
            }
        }
    }

    /// What a restore re-applies after the create: the place, the sources and the blobs it carried.
    pub(crate) fn restore_extras(
        &mut self,
        uid: Uid,
        pos: [f64; 2],
        sources: &[(String, String, SourceState)],
        viewers: Option<serde_json::Value>,
        baseline: Option<serde_json::Value>,
        record: Option<Vec<RecordedOutput>>,
    ) {
        let _ = self.set_node_pos(uid, pos);
        for (group, name, s) in sources {
            let _ = self.set_source(uid, group, name, s.clone());
        }
        if let Some(v) = baseline {
            let _ = self.set_node_baseline(uid, v);
        }
        if let Some(v) = viewers {
            let _ = self.set_node_viewers(uid, v);
        }
        if let Some(r) = record {
            let _ = self.set_recorded(uid, r);
        }
    }

    /// Where a leaf's record lands, whichever door it came through — a fresh add, an undo of a
    /// delete, a load. Its instance is born at the next settle, so a rolled-back add runs nothing.
    fn insert_node_at(
        &mut self,
        uid: Uid,
        name: String,
        engine: &'static str,
        entry: LibraryEntry,
        values: doc::Values,
    ) {
        let leaf = Leaf { manifest: entry.manifest, isolation: entry.isolation, engine, values: Record::new(values), sources: HashMap::new() };
        self.patch.nodes.insert(uid, Arc::new(NodeEntry::new(Kind::Leaf(Box::new(leaf)), name, [0.0, 0.0])));
    }

    /// Every display name in the patch with the uid wearing it — leaves, sub-patch facades and
    /// boundary ports share ONE namespace, because `nd('name')` addresses any of them.
    fn named(&self) -> impl Iterator<Item = (Uid, &str)> {
        self.patch.nodes
            .iter()
            .map(|(u, e)| (*u, e.name.as_str()))
    }

    /// Is `name` already worn by something other than `except`? The commands tolerate a collision
    /// as a no-op, so the user-facing error is raised at the RPC boundary.
    pub fn name_taken(&self, name: &str, except: Option<Uid>) -> bool {
        self.named().any(|(u, n)| Some(u) != except && n == name)
    }

    /// Display name of anything a uid can name — one map, so one lookup for all three kinds.
    pub fn name(&self, uid: Uid) -> Option<&str> {
        self.patch.nodes.get(&uid).map(|e| e.name.as_str())
    }

    /// Where anything a uid can name sits on the canvas.
    pub fn pos(&self, uid: Uid) -> Option<[f64; 2]> {
        self.patch.nodes.get(&uid).map(|e| e.pos)
    }

    /// The boundary port a uid names, with the scope holding it.
    pub fn stub(&self, uid: Uid) -> Option<(Uid, subpatch::Port)> {
        let e = self.patch.nodes.get(&uid)?;
        match e.kind {
            Kind::Port(p) => Some((e.scope?, p)),
            Kind::Leaf(_) | Kind::Facade => None,
        }
    }

    /// Is `uid` a sub-patch facade?
    pub fn is_facade(&self, uid: Uid) -> bool {
        matches!(self.patch.nodes.get(&uid).map(|e| &e.kind), Some(Kind::Facade))
    }

    /// A scope's boundary ports, in the map's insertion order — which is the order they were
    /// authored in, and the order a facade lists its slots.
    pub fn ports_of(&self, scope: Uid) -> Vec<Uid> {
        self.patch.nodes
            .iter()
            .filter(|(u, e)| matches!(e.kind, Kind::Port(_)) && self.scope_of(**u) == Some(scope))
            .map(|(u, _)| *u)
            .collect()
    }

    /// A node's params as of now. An owned snapshot rather than a borrow: cloning the `Arc` is
    /// cheap, and a `&` would borrow the whole graph for as long as the caller held it.
    pub fn params(&self, uid: Uid) -> Option<Arc<ParamGroups>> {
        self.leaf(uid).map(|l| self.typed(l))
    }

    /// The literals a leaf holds, as the document spells them.
    pub fn values(&self, uid: Uid) -> Option<doc::Values> {
        self.leaf(uid).map(|l| l.values.get().clone())
    }

    /// A leaf's params as its class declares them NOW, with the record's literals folded in:
    /// the one derivation of a typed param, so no record can carry a stale bound or option.
    /// Memoized on the record until it is written or the library moves.
    fn typed(&self, leaf: &Leaf) -> Arc<ParamGroups> {
        leaf.values
            .typed
            .get_or_init(|| Arc::new(self.folded(&goofi_node::qualify(leaf.engine, leaf.manifest.type_name), leaf.values.get()).unwrap_or_default()))
            .clone()
    }

    /// The library moved: every typed memo derives again on its next read.
    fn forget_typed(&mut self) {
        for leaf in self.patch.nodes.values_mut().filter_map(|e| Arc::make_mut(e).leaf_mut()) {
            leaf.values.typed.take();
        }
        self.runtime.redeliver = true;
    }

    /// The class's params with `values` folded in, each coerced to its declared type; a value
    /// the class does not declare is ignored.
    fn folded(&self, type_ref: &str, values: &doc::Values) -> Result<ParamGroups, String> {
        let mut params = self.default_params_of(type_ref, None)?;
        for (group, names) in values {
            let Some(g) = params.get_mut(group) else { continue };
            for (name, value) in names {
                if let Some(existing) = g.get_mut(name) {
                    *existing = control::read(value, existing);
                }
            }
        }
        Ok(params)
    }

    /// `p` as a projection shows it: an instance that answered a refresh shows its live options
    /// over the declared ones. Never persisted; dies with the instance.
    pub fn shown_param(&self, uid: Uid, group: &str, name: &str, p: &Param) -> Param {
        let mut p = p.clone();
        let live = self.health(uid).and_then(|h| h.options.get(&ParamKey::new(group, name)));
        if let (Param::Str { options, .. }, Some(live)) = (&mut p, live) {
            *options = Some(live.clone());
        }
        p
    }

    /// Rename a node. Every `nd('old')` in the patch follows to `nd('new')`, and the referrer uids
    /// come back so the bridge can rebroadcast them. The rewrite happens only on success.
    pub fn rename_node(&mut self, uid: Uid, name: &str) -> Result<Vec<Uid>, String> {
        if !goofi_core::variables::is_valid_name(name) {
            return Err(format!("`{name}` is not a legal name: {NAME_RULE}"));
        }
        if self.name_taken(name, None) {
            return Err(format!("display name `{name}` already in use"));
        }
        // A facade, a boundary port and a leaf all wear a name in the ONE namespace `nd()` reads,
        // and now in one map — so the rename is one write and the rewrite below is shared.
        let e = self.entry_mut(uid).ok_or_else(|| format!("no such node {uid}"))?;
        let old_name = std::mem::replace(&mut e.name, name.to_string());
        // `name_taken` guarantees `name != old_name`, so the rename moved the
        // display name — propagate it into every expression that referenced it.
        let touched = self.rewrite_nd_refs_for_rename(uid, &old_name, name);
        Ok(touched)
    }

    /// Rewrite `nd('old')` -> `nd('new')` across every param expression, re-binding each changed
    /// source. Returns the distinct referrer uids whose source changed.
    fn rewrite_nd_refs_for_rename(&mut self, uid: Uid, old: &str, new: &str) -> Vec<Uid> {
        // A port's name is ALSO a slot label — on the facade that holds it, and nowhere else — so
        // the one rename reaches an expression in both positions through the one rewrite.
        let facade = self.stub(uid).and_then(|(scope, _)| self.name(scope)).map(str::to_string);
        let rename = |n: &str, slot: Option<&str>| {
            (
                (n == old).then(|| new.to_string()),
                (slot == Some(old) && Some(n) == facade.as_deref()).then(|| new.to_string()),
            )
        };
        // A variable's expression spells a producer as a param's does, so the one rename reaches it.
        self.rewrite_variable_expressions(|e| expr_rewrite::rename_refs(e, rename));
        self.rewrite_sources(|s| {
            Some(SourceState { expression: expr_rewrite::rename_refs(&s.expression, rename)?, ..s.clone() })
        })
    }

    pub fn set_node_pos(&mut self, uid: Uid, pos: [f64; 2]) -> Result<(), String> {
        let e = self.entry_mut(uid).ok_or_else(|| format!("no such node {uid}"))?;
        e.pos = pos;
        Ok(())
    }

    /// Replace a node's opaque viewer view-state blob (persisted to `.gfi`, echoed in node
    /// info). The backend never interprets it — it is the editor's per-slot kind/settings.
    pub fn set_node_viewers(&mut self, uid: Uid, viewers: serde_json::Value) -> Result<(), String> {
        let e = self.entry_mut(uid).ok_or_else(|| format!("no such node {uid}"))?;
        e.viewers = viewers;
        Ok(())
    }

    /// The viewer view-state blob of anything a uid can name (empty object if never set).
    pub fn viewers(&self, uid: Uid) -> Option<&serde_json::Value> {
        self.patch.nodes.get(&uid).map(|e| &e.viewers)
    }

    /// Replace the values the touched filter counts from, WHOLE.
    pub fn set_node_baseline(&mut self, uid: Uid, baseline: serde_json::Value) -> Result<(), String> {
        let e = self.entry_mut(uid).ok_or_else(|| format!("no such node {uid}"))?;
        e.baseline = baseline;
        Ok(())
    }

    /// What the node holds now, as a baseline: per `group/name`, the value and the source, since a
    /// move from a constant to an expression is a change. A pulse holds no value and is left out.
    pub fn touched_baseline(&self, uid: Uid) -> serde_json::Value {
        let Some(leaf) = self.patch.nodes.get(&uid).and_then(|e| e.leaf()) else {
            return serde_json::json!({});
        };
        let mut out = serde_json::Map::new();
        for (group, names) in self.typed(leaf).iter() {
            for (name, p) in names {
                if matches!(p, Param::Pulse) {
                    continue;
                }
                let source = leaf.sources.get(&ParamKey::new(group, name));
                out.insert(
                    format!("{group}/{name}"),
                    serde_json::json!({
                        "value": param_value_json(p),
                        "mode": source.map(|s| s.mode).unwrap_or_default(),
                        "expression": source.map(|s| s.expression.clone()).unwrap_or_default(),
                    }),
                );
            }
        }
        serde_json::Value::Object(out)
    }

    /// The touched-filter baseline of anything a uid can name (empty object if never cleared).
    pub fn baseline(&self, uid: Uid) -> Option<&serde_json::Value> {
        self.patch.nodes.get(&uid).map(|e| &e.baseline)
    }

    /// Replace the output slots armed for recording, whole.
    pub fn set_recorded(&mut self, uid: Uid, mut record: Vec<RecordedOutput>) -> Result<(), String> {
        let e = self.entry_mut(uid).ok_or_else(|| format!("no such node {uid}"))?;
        // A slot still armed keeps its serial; every other arming is a new one, minted at settle,
        // an undo's re-arm included: its old service name is one the recorder may have let go of.
        for output in &mut record {
            output.serial = e.record.iter().find(|held| held.slot == output.slot).map_or(0, |held| held.serial);
        }
        e.record = record;
        Ok(())
    }

    /// Every arming the batch left unnumbered gets the next serial: one per slot however often
    /// the batch armed it.
    fn mint_serials(&mut self, uid: Uid) {
        let mut serial = self.runtime.arm_serial;
        if let Some(e) = self.entry_mut(uid) {
            for output in e.record.iter_mut().filter(|o| o.serial == 0) {
                serial += 1;
                output.serial = serial;
            }
        }
        self.runtime.arm_serial = serial;
    }

    /// The output slots armed for recording on anything a uid can name.
    pub fn recorded(&self, uid: Uid) -> Option<&[RecordedOutput]> {
        self.patch.nodes.get(&uid).map(|e| e.record.as_slice())
    }

    /// The parent scope of a node/scope (`None` = ROOT). Absent ⇒ ROOT, so a plain flat graph
    /// needs no entries.
    pub fn scope_of(&self, uid: Uid) -> Option<Uid> {
        self.patch.nodes.get(&uid)?.scope
    }

    /// Everything `scope_of` places inside `scope`, ports included — they are members like any
    /// other node, which is the whole of what a scope holds.
    pub fn scope_members(&self, scope: Uid) -> Vec<Uid> {
        self.patch.nodes.keys().copied().filter(|u| self.scope_of(*u) == Some(scope)).collect()
    }


    /// The leaf `(uid, slot)` a port exposes, walking nested ports; `None` if unwired.
    fn resolve_stub(&self, port: Uid) -> Option<(Uid, &'static str)> {
        let mut at = port;
        // A hand-edited `.gfi` can persist a cyclic chain; walking it must stop, not recurse.
        let mut seen: Vec<Uid> = Vec::new();
        loop {
            if seen.contains(&at) {
                return None;
            }
            seen.push(at);
            let (node, slot) = self.port_inner(at)?;
            match self.stub(node).is_some() {
                true => at = node,
                false => return Some((node, slot)),
            }
        }
    }

    /// The slots of anything a uid names on one side, as `(key, label, dtype)`: a leaf's
    /// declarations, a facade's ports keyed by uid and labelled by name, and a port's one `value`.
    pub fn slots(&self, uid: Uid, dir: subpatch::Dir) -> Vec<(String, String, goofi_core::SlotType)> {
        if self.is_facade(uid) {
            return self
                .ports_of(uid)
                .into_iter()
                .filter_map(|id| self.stub(id).map(|(_, p)| (id, p)))
                .filter(|(_, p)| p.dir == dir)
                .map(|(id, p)| (id.to_hex(), self.name(id).unwrap_or("").to_string(), p.dtype))
                .collect();
        }
        if let Some((_, st)) = self.stub(uid) {
            let slot = subpatch::BOUNDARY_SLOT.to_string();
            return vec![(slot.clone(), slot, st.dtype)];
        }
        let Some(m) = self.manifest(uid) else { return Vec::new() };
        let own = |name: &str, kind| (name.to_string(), name.to_string(), kind);
        match dir {
            Dir::Out => m.outputs.iter().map(|o| own(o.name, o.kind)).collect(),
            Dir::In => m.inputs.iter().map(|s| own(s.name, s.kind)).collect(),
        }
    }

    /// The type id of anything a uid names — a leaf's `engine:Name`, or the bare structural name
    /// a facade and a boundary port wear, which belong to the model rather than to an engine.
    pub fn node_type(&self, uid: Uid) -> Option<String> {
        if self.is_facade(uid) {
            return Some(subpatch::SCOPE_TYPE.to_string());
        }
        if let Some((_, st)) = self.stub(uid) {
            return Some(subpatch::boundary_type_name(st.dir, st.dtype).to_string());
        }
        self.leaf(uid).map(|e| goofi_node::qualify(e.engine, e.manifest.type_name))
    }

    /// A facade address, folded one level onto the port it names. Everything else is itself. What
    /// is BEHIND that port is a separate question ([`Graph::stream`]), asked at plan time.
    pub fn normalise(&self, uid: Uid, slot: &str) -> (Uid, String) {
        match self.is_facade(uid) {
            true => match Uid::from_hex(slot) {
                Some(port) if self.stub(port).is_some() => (port, subpatch::BOUNDARY_SLOT.to_string()),
                _ => (uid, slot.to_string()),
            },
            false => (uid, slot.to_string()),
        }
    }

    /// What real leaf slot an address stands for, one hop per port. `Open` is the port where nothing
    /// feeds the walk yet; `None` means no output slot. `nd()` and a cable both resolve here.
    pub fn stream(&self, uid: Uid, slot: &str) -> Option<Stream> {
        if uid == Uid::VARIABLES {
            return self.variables().contains(slot).then(|| Stream::At(uid, goofi_core::variables::slot_name(slot)));
        }
        let (mut at, mut slot) = self.normalise(uid, slot);
        let mut seen: Vec<Uid> = Vec::new();
        loop {
            if self.leaf(at).is_some() {
                return Some(Stream::At(at, self.slot_face(at, &slot, Dir::Out)?.name));
            }
            // A hand-edited `.gfi` can persist a cyclic chain; walking it must stop, not recurse.
            if seen.contains(&at) {
                return Some(Stream::Open(at));
            }
            seen.push(at);
            self.stub(at)?;
            let Some((next, next_slot)) = self.stub_feed(at) else {
                return Some(Stream::Open(at));
            };
            (at, slot) = (next, next_slot.to_string());
        }
    }

    /// The wire on a port's INSIDE — the member it feeds (an IN port) or drains (an OUT port). An
    /// ordinary link, so this is a lookup rather than a field beside `links` to keep in step.
    fn port_inner(&self, port: Uid) -> Option<(Uid, &'static str)> {
        match self.stub(port)?.1.dir {
            Dir::In => self.patch.links.values().find(|l| l.node_out == port).map(|l| (l.node_in, l.slot_in)),
            Dir::Out => self.stub_feed(port),
        }
    }

    /// One hop back from a port to whatever feeds it: the wire arriving AT it, whichever side of
    /// the wall that is.
    fn stub_feed(&self, port: Uid) -> Option<(Uid, &'static str)> {
        self.patch.links.values().find(|l| l.node_in == port).map(|l| (l.node_out, l.slot_out))
    }

    /// The scope an end of a wire FACES: a leaf its own, a port its scope on one side and the parent
    /// on the other. Two ends may be linked exactly when their faces agree.
    fn face(&self, uid: Uid, producer: bool) -> Option<Option<Uid>> {
        if let Some((scope, st)) = self.stub(uid) {
            let inward = st.dir == Dir::In;
            return Some(match inward == producer {
                true => Some(scope),
                false => self.scope_of(scope),
            });
        }
        self.patch.nodes.contains_key(&uid).then(|| self.scope_of(uid))
    }

    /// One end of a wire: `Out` for the producer end, `In` for the consumer end. A port wears the
    /// one `value` slot on both sides, because it relays.
    fn slot_face(&self, uid: Uid, slot: &str, dir: subpatch::Dir) -> Option<SlotFace> {
        if let Some((_, st)) = self.stub(uid) {
            return (slot == subpatch::BOUNDARY_SLOT)
                .then_some(SlotFace { name: subpatch::BOUNDARY_SLOT, kind: st.dtype, multi: false });
        }
        let m = self.manifest(uid)?;
        match dir {
            Dir::Out => {
                m.outputs.iter().find(|o| o.name == slot).map(|o| SlotFace { name: o.name, kind: o.kind, multi: false })
            }
            Dir::In => {
                m.inputs.iter().find(|s| s.name == slot).map(|s| SlotFace { name: s.name, kind: s.kind, multi: s.multi })
            }
        }
    }

    /// Move a node or scope into `scope` (`None` = ROOT), returning its prior membership — the one
    /// validated re-parent seam. Errors on an unknown uid or a `scope` that is not a live scope.
    pub fn reparent(&mut self, uid: Uid, scope: Option<Uid>) -> Result<Option<Uid>, String> {
        if !self.patch.nodes.contains_key(&uid) {
            return Err(format!("reparent: no such node/scope {uid}"));
        }
        if let Some(s) = scope {
            if !self.is_facade(s) {
                return Err(format!("reparent: no such scope {s}"));
            }
        }
        let old = self.scope_of(uid);
        self.set_member_scope(uid, scope);
        Ok(old)
    }

    /// Re-tag a member's scope: the one place membership changes. `None` = ROOT scope.
    fn set_member_scope(&mut self, member: Uid, scope: Option<Uid>) {
        if let Some(e) = self.entry_mut(member) {
            e.scope = scope;
        }
    }

    /// `uid` and every scope above it, innermost first.
    fn ancestors(&self, uid: Uid) -> impl Iterator<Item = Uid> + '_ {
        std::iter::successors(Some(uid), |u| self.scope_of(*u))
    }

    /// The stub on nested scope `scope` whose chain-to-leaf resolution is exactly `(leaf, slot)`
    /// in direction `dir` — the interior endpoint of a link crossing into a nested member.
    fn stub_exposing(&self, scope: Uid, leaf: Uid, slot: &str, dir: subpatch::Dir) -> Option<Uid> {
        self.ports_of(scope)
            .into_iter()
            .filter(|id| self.stub(*id).is_some_and(|(_, p)| p.dir == dir))
            .find(|id| self.resolve_stub(*id).is_some_and(|(u, sl)| u == leaf && sl == slot))
    }

    /// The inner-slot key a group boundary stub should reference for a crossing link: the real slot,
    /// a nested scope's existing stub, or a freshly MINTED chain of ports.
    fn expose_in_nested_member(
        &mut self,
        member: Uid,
        leaf: Uid,
        slot: &str,
        dir: subpatch::Dir,
    ) -> String {
        if member == leaf {
            return slot.to_string();
        }
        // The thing crossing is ALREADY a port of this member — one leaf slot sits behind exactly
        // one chain of ports, so the outer port names this one rather than minting a rival.
        if self.stub(leaf).is_some_and(|(s, _)| s == member) {
            return leaf.to_hex();
        }
        if let Some(id) = self.stub_exposing(member, leaf, slot, dir) {
            return id.to_hex();
        }
        // No port exposes the leaf — mint one. Its inner is the leaf directly when the leaf is a
        // direct member, else the (recursively ensured) port on the intermediate nested scope.
        let child = self.ancestors(leaf).find(|u| self.scope_of(*u) == Some(member)).unwrap_or(leaf);
        let inner = if child == leaf {
            (leaf, slot.to_string())
        } else {
            let child_stub = self.expose_in_nested_member(child, leaf, slot, dir);
            (child, child_stub)
        };
        let at = self.pos(member).unwrap_or([0.0, 0.0]);
        let Ok(id) = self.mint_port(member, dir, leaf, slot, at) else {
            return slot.to_string();
        };
        let (node, slot) = inner;
        let _ = match dir {
            Dir::In => self.add_link(id, subpatch::BOUNDARY_SLOT, node, &slot),
            Dir::Out => self.add_link(node, &slot, id, subpatch::BOUNDARY_SLOT),
        };
        id.to_hex()
    }

    /// Mint a port of `scope` for `end.slot`, placed beside `at` on the side `dir` faces.
    fn mint_port(&mut self, scope: Uid, dir: Dir, end: Uid, slot: &str, at: [f64; 2]) -> Result<Uid, String> {
        let dtype = self.slot_face(end, slot, dir).map_or(goofi_core::SlotType::Array, |f| f.kind);
        let id = self.create_node(subpatch::boundary_type_name(dir, dtype), None, "", None, Some(scope))?;
        let dx = if dir == Dir::Out { 220.0 } else { -40.0 };
        let _ = self.set_node_pos(id, [at[0] + dx, at[1]]);
        Ok(id)
    }

    /// The single common parent scope of `members`, or an error if the set is empty or spans several
    /// scopes.
    fn common_parent(&self, members: &[Uid]) -> Result<Option<Uid>, String> {
        if members.is_empty() {
            return Err("group: empty selection".into());
        }
        let mut parent: Option<Option<Uid>> = None;
        for &m in members {
            if !self.patch.nodes.contains_key(&m) {
                return Err(format!("group: no such member {m}"));
            }
            let s = self.scope_of(m);
            match parent {
                None => parent = Some(s),
                Some(prev) if prev != s => return Err("group: members span multiple scopes".into()),
                _ => {}
            }
        }
        Ok(parent.unwrap())
    }

    /// Group `members` into a new scope at `pos`: the facade, a port for every wire that crosses
    /// its boundary, and the members moved inside. Answers the scope's uid.
    pub fn group_nodes(&mut self, members: &[Uid], pos: [f64; 2]) -> Result<Uid, String> {
        // 1. Validate BEFORE any mutation: each exists, and all share one parent scope.
        let parent = self.common_parent(members)?;
        let member_set: std::collections::HashSet<Uid> = members.iter().copied().collect();
        let scope_uid = self.mint();
        // Registered BEFORE its ports are minted, so each port's fresh name sees the ones before it
        // and `nd()` can tell two ports apart.
        let disp = goofi_core::fresh_name("subpatch", 0, |name| self.name_taken(name, None));
        self.patch.nodes.insert(scope_uid, Arc::new(NodeEntry::new(Kind::Facade, disp, pos)));
        self.set_member_scope(scope_uid, parent);

        // 2. A link with exactly one end inside (transitively) is SPLIT at a port minted for the
        //    slot it crosses; several cables leaving one slot share one port.
        let mut ports: std::collections::HashMap<(Uid, &'static str, bool), Uid> =
            std::collections::HashMap::new();
        let mut wires: Vec<(Uid, String, Uid, String)> = Vec::new();
        let mut cut: Vec<Link> = Vec::new();
        let (mut in_n, mut out_n) = (0usize, 0usize);
        // Snapshot the links: `expose_in_nested_member` may MINT an intermediate stub and needs
        // `&mut self`, so the classification cannot hold a borrow on `self.patch.links`.
        let links = self.links_view();
        for l in &links {
            let out_m = self.ancestors(l.node_out).find(|u| member_set.contains(u));
            let in_m = self.ancestors(l.node_in).find(|u| member_set.contains(u));
            let (member, at_slot, outward) = match (out_m, in_m) {
                (Some(om), None) => (om, l.slot_out, true),
                (None, Some(im)) => (im, l.slot_in, false),
                _ => continue,
            };
            let end = if outward { l.node_out } else { l.node_in };
            let key = (end, at_slot, outward);
            let port = match ports.get(&key) {
                Some(id) => *id,
                None => {
                    let (dir, n) = if outward { (Dir::Out, &mut out_n) } else { (Dir::In, &mut in_n) };
                    let inner_slot = self.expose_in_nested_member(member, end, at_slot, dir);
                    let id = self.mint_port(scope_uid, dir, end, at_slot, [pos[0], pos[1] + 40.0 * *n as f64])?;
                    *n += 1;
                    let b = subpatch::BOUNDARY_SLOT.to_string();
                    wires.push(if outward { (member, inner_slot, id, b) } else { (id, b, member, inner_slot) });
                    ports.insert(key, id);
                    id
                }
            };
            cut.push(*l);
            wires.push(match outward {
                true => (
                    port,
                    subpatch::BOUNDARY_SLOT.to_string(),
                    l.node_in,
                    l.slot_in.to_string(),
                ),
                false => (
                    l.node_out,
                    l.slot_out.to_string(),
                    port,
                    subpatch::BOUNDARY_SLOT.to_string(),
                ),
            });
        }
        // The whole cable goes, so the two halves replace it rather than racing its single-input
        // eviction — a port wired to a member's input would otherwise evict the very cable it carries.
        self.links_mut().retain(|_, l| !cut.contains(l));

        // 3. Re-tag membership. Members stay live; only `scope_of` changes.
        for &m in members {
            self.set_member_scope(m, Some(scope_uid));
        }
        self.set_member_scope(scope_uid, parent);
        // 4. …and only NOW wire the minted ports: a cable's two ends must face the same scope, and
        //    until the re-tag above there was no scope for them to face.
        for (a, so, b, si) in wires {
            self.add_link(a, &so, b, &si)?;
        }
        Ok(scope_uid)
    }

    /// Dissolve a scope, answering the cables its removal JOINED — each pair of halves that met at
    /// a port, now one link. Uid-stable.
    pub fn expand_instance(
        &mut self,
        scope: Uid,
    ) -> Result<Vec<(Uid, &'static str, Uid, &'static str)>, String> {
        if !self.is_facade(scope) {
            return Err(format!("expand_instance: no such scope {scope}"));
        }
        let restored = self.scope_members(scope);
        let parent = self.scope_of(scope); // the grandparent scope members fall back to

        // Every port is about to go, and each carried half a cable on either side of it. Capture the
        // JOIN of those halves — the wall is what the two halves existed for, and the wall is going.
        let ports: Vec<Uid> = self.ports_of(scope);
        let mut splices: Vec<(Uid, &'static str, Uid, &'static str)> = Vec::new();
        for &port in &ports {
            let Some(feed) = self.stub_feed(port) else { continue };
            for l in self.patch.links.values().filter(|l| l.node_out == port) {
                splices.push((feed.0, feed.1, l.node_in, l.slot_in));
            }
        }
        self.links_mut().retain(|_, l| !ports.contains(&l.node_in) && !ports.contains(&l.node_out));

        for &m in &restored {
            self.set_member_scope(m, parent);
        }
        for p in &ports {
            self.patch.nodes.shift_remove(p);
        }
        self.patch.nodes.shift_remove(&scope);
        // …and only now, with the members up one level and the wall gone, do the two halves of each
        // cable become one link that both ends can face.
        let mut joined = Vec::new();
        for (a, so, b, si) in splices {
            if self.add_link(a, so, b, si).is_ok() {
                joined.push((a, so, b, si));
            }
        }
        Ok(joined)
    }

    /// Delete a whole sub-patch scope: tear down every member, recursing into nested scopes, then
    /// drop the scope.
    pub fn remove_instance(&mut self, scope: Uid) -> Result<(), String> {
        if !self.is_facade(scope) {
            return Err(format!("remove_instance: no such scope {scope}"));
        }
        for m in self.scope_members(scope) {
            if self.is_facade(m) {
                self.remove_instance(m)?; // nested scope subtree
            } else {
                let _ = self.remove_node(m); // leaf (tolerate an already-gone member)
            }
        }
        self.patch.nodes.shift_remove(&scope);
        Ok(())
    }

    /// Take every cable that touches `uid` off the graph, through the door `remove_link` is — so
    /// the consumers it fed are re-planned rather than left holding a feed that has gone.
    fn cut_cables(&mut self, uid: Uid) {
        for l in self.links_view() {
            if l.node_in == uid || l.node_out == uid {
                let _ = self.remove_link(l.node_out, l.slot_out, l.node_in, l.slot_in);
            }
        }
    }

    /// Take a boundary port off a scope, answering the port it removed. Every `nd()` naming it goes
    /// unresolvable here rather than at the next edit that happens to touch it.
    pub fn remove_stub(&mut self, scope: Uid, port: Uid) -> Option<(subpatch::Port, String, [f64; 2])> {
        self.stub(port).filter(|(s, _)| *s == scope)?;
        // Its wires go with it, both halves — through the ONE door that removes a link, and BEFORE
        // the port does, so what the port relayed to is re-planned rather than left subscribed.
        self.cut_cables(port);
        let e = self.patch.nodes.shift_remove(&port)?;
        let Kind::Port(p) = e.kind else { return None };
        Some((p, e.name.clone(), e.pos))
    }

    /// All links as resolved views (snapshot projection).
    pub fn links_view(&self) -> Vec<Link> {
        self.patch.links.values().copied().collect()
    }

    /// Drop one derived binding and release its compiled handle.
    fn release_binding(&mut self, uid: Uid, key: &ParamKey) {
        let Some(b) = self.runtime.bindings.get_mut(&uid).and_then(|m| m.remove(key)) else { return };
        if let (Some(ev), Some(id)) = (&self.runtime.evaluator, b.id) {
            ev.release(id);
        }
    }

    /// Drop every derived binding of a node and release each compiled handle, so the evaluator's
    /// registry does not leak across a node or graph teardown.
    fn release_bindings(&mut self, uid: Uid) {
        let Some(held) = self.runtime.bindings.remove(&uid) else { return };
        if let Some(ev) = &self.runtime.evaluator {
            for id in held.values().filter_map(|b| b.id) {
                ev.release(id);
            }
        }
    }

    /// Remove anything a uid names, with what its going takes along: a leaf hands each consumer of
    /// its outputs the first wired input of its own whose kind feeds that consumer's slot, so a
    /// chain stays one; a panel bound to a uid the removal takes stops naming it; a port comes off
    /// its scope, a facade tears its subtree down. Nothing live at the uid is the no-op, answered.
    pub fn remove_entry(&mut self, uid: Uid) -> Result<bool, String> {
        if !self.exists(uid) {
            return Ok(false);
        }
        let bridges = if self.is_leaf(uid) { self.bridges_around(uid) } else { Vec::new() };
        let gone: HashSet<Uid> = self.subtree_of(&[uid]).into_iter().collect();
        let unbind = self.arrangement().unbind(&gone);
        if let Some((scope, _)) = self.stub(uid) {
            self.remove_stub(scope, uid);
        } else if self.is_facade(uid) {
            self.remove_instance(uid)?;
        } else {
            self.remove_node(uid)?;
        }
        for (node_out, slot_out, node_in, slot_in) in bridges {
            let _ = self.add_link(node_out, slot_out, node_in, slot_in);
        }
        if !unbind.is_empty() {
            self.arrangement_mut().set_contents(&unbind);
        }
        Ok(true)
    }

    /// The wires a leaf about to go leaves behind: each consumer of one of its outputs is fed by
    /// the first wired input of the leaf, in declaration order, whose source feeds the consumer's
    /// slot.
    fn bridges_around(&self, uid: Uid) -> Vec<(Uid, &'static str, Uid, &'static str)> {
        let links = self.links_view();
        let kind_of = |slots: Vec<(String, String, goofi_core::SlotType)>, name: &str| {
            slots.into_iter().find(|(n, _, _)| n == name).map(|(_, _, kind)| kind)
        };
        let sources: Vec<(Uid, &'static str, goofi_core::SlotType)> = self
            .slots(uid, Dir::In)
            .iter()
            .filter_map(|(name, _, _)| links.iter().find(|l| l.node_in == uid && l.slot_in == name.as_str()))
            .filter_map(|l| kind_of(self.slots(l.node_out, Dir::Out), l.slot_out).map(|kind| (l.node_out, l.slot_out, kind)))
            .collect();
        links
            .iter()
            .filter(|l| l.node_out == uid && l.node_in != uid)
            .filter_map(|l| {
                let into = kind_of(self.slots(l.node_in, Dir::In), l.slot_in)?;
                let &(node_out, slot_out, _) = sources.iter().find(|(src, _, kind)| *src != l.node_in && kind.feeds(into))?;
                Some((node_out, slot_out, l.node_in, l.slot_in))
            })
            .collect()
    }

    pub fn remove_node(&mut self, uid: Uid) -> Result<(), String> {
        if self.patch.nodes.shift_remove(&uid).is_none() {
            return Err(format!("no such node {uid}"));
        }
        self.runtime.watched.retain(|(u, _)| *u != uid);
        self.runtime.view_wants.retain(|(u, _), _| *u != uid);
        self.release_bindings(uid);
        // The links touching the node go with it: the settle re-plans every consumer slot one
        // of them fed, and its own thread is halted with its services.
        if self.patch.links.values().any(|l| l.node_out == uid || l.node_in == uid) {
            self.links_mut().retain(|_, l| l.node_out != uid && l.node_in != uid);
        }
        Ok(())
    }

    /// Respawn a node's instance IN PLACE. Everything that identifies it in the patch survives, so
    /// remove+add is no substitute. A Python node re-runs the source CAPTURED AT DISCOVERY.
    pub fn restart_node(&mut self, uid: Uid) -> Result<(), String> {
        // A facade has no thread of its own, so restarting it is restarting what is inside it — to
        // any depth. A port has neither thread nor members, so it is a restart of nothing.
        if self.is_facade(uid) {
            for m in self.scope_members(uid) {
                self.restart_node(m)?;
            }
            return Ok(());
        }
        if self.stub(uid).is_some() {
            return Ok(());
        }
        let entry = self.leaf(uid).ok_or_else(|| format!("no such node {uid}"))?;
        // Qualified, because the node's own engine is the one to re-resolve it in.
        let type_ref = &goofi_node::qualify(entry.engine, entry.manifest.type_name);
        // Resolve BEFORE touching the entry: a type that no longer resolves leaves the old
        // instance running rather than half-killing the node.
        let (engine, lib) = self.resolve_type(type_ref)?;
        // Only the saved VALUES carry over — bounds, options and variant are the edited file's
        // to state — and a value the class no longer declares goes.
        let declared = self.default_params_of(type_ref, None)?;

        // A restart is a rebirth at this uid, which settle does: the corpse is halted and the
        // reborn node takes a fresh generation, so it never re-opens names the corpse still holds.
        self.runtime.restarts.push(uid);
        let entry = self.leaf_mut(uid).expect("looked up above");
        // The MANIFEST goes with the instance: keeping the old one leaves the graph describing a
        // node not running.
        entry.manifest = lib.manifest;
        entry.isolation = lib.isolation;
        entry.engine = engine;
        let values = entry.values.get_mut();
        values.retain(|group, names| {
            names.retain(|name, _| declared.get(group).is_some_and(|g| g.contains_key(name)));
            !names.is_empty()
        });
        for (group, names) in &declared {
            let held = values.entry(group.clone()).or_default();
            for (name, p) in names {
                if let (false, Some(v)) = (held.contains_key(name), control::data_of(p)) {
                    held.insert(name.clone(), v);
                }
            }
        }
        // A source on a param the class no longer declares goes with it.
        let retired: Vec<ParamKey> =
            entry.sources.keys().filter(|key| goofi_node::param_dim(&declared, &key.group, &key.name).is_none()).cloned().collect();
        for key in retired {
            self.unbind(uid, &key);
        }
        // A wire onto a slot the reshape retired can never propagate and cannot be repaired — the
        // slot is gone from the palette. Keeping it draws a cable the runtime ignores.
        let orphaned: Vec<(Uid, &'static str, Uid, &'static str)> = self
            .patch
            .links
            .values()
            .filter(|l| {
                (l.node_in == uid && self.slot_face(uid, l.slot_in, Dir::In).is_none())
                    || (l.node_out == uid && self.slot_face(uid, l.slot_out, Dir::Out).is_none())
            })
            .map(|l| (l.node_out, l.slot_out, l.node_in, l.slot_in))
            .collect();
        for (out, so, into, si) in orphaned {
            let _ = self.remove_link(out, so, into, si);
        }
        Ok(())
    }

    /// A value the node's own state took, written as the param's literal: no transaction, so no
    /// undo step, and a param a source drives keeps its source and its literal. Whether the literal
    /// moved.
    pub fn project_param(&mut self, uid: Uid, key: &ParamKey, value: Param) -> Result<bool, String> {
        if self.source_state(uid, key).is_some_and(|s| s.mode != Mode::Constant) {
            return Ok(false);
        }
        let literal = |g: &Self| g.values(uid).and_then(|v| v.get(&key.group)?.get(&key.name).cloned());
        let before = literal(self);
        self.update_param(uid, &key.group, &key.name, value)?;
        Ok(literal(self) != before)
    }

    pub fn update_param(
        &mut self,
        uid: Uid,
        group: &str,
        name: &str,
        value: Param,
    ) -> Result<(), String> {
        let leaf = self.leaf(uid).ok_or_else(|| format!("no such node {uid}"))?;
        let declared = self.typed(leaf);
        let (base, element) = goofi_node::element(name);
        let Some(existing) = goofi_node::param_dim(&declared, group, name) else {
            return Err(format!("no such param `{group}/{name}`"));
        };
        // Read into the DECLARED kind: a literal is only ever a value of the class's param. An
        // element's literal lands in its dimension of the whole param's list.
        let coerced = control::data_of(&value).map_or(existing.clone(), |d| control::read(&d, &existing));
        let whole = match element {
            Some(k) => {
                let whole = goofi_node::param(&declared, group, base).expect("the element's param");
                let mut values = whole.as_vec().unwrap_or_default().to_vec();
                values[k] = coerced.as_f64().unwrap_or(0.0);
                control::read(&Data::numbers(values), whole)
            }
            None => coerced,
        };
        if let Some(v) = control::data_of(&whole) {
            let leaf = self.leaf_mut(uid).expect("looked up above");
            leaf.values.get_mut().entry(group.to_string()).or_default().insert(base.to_string(), v);
        }
        // A LITERAL on a driven param switches it to constant, which is what the node does with
        // this write's `SetParam`; what the record retained stays retained.
        let key = ParamKey::new(group, name);
        if let Some(mut state) = self.source_state(uid, &key).filter(|s| s.mode != Mode::Constant) {
            state.mode = Mode::Constant;
            let _ = self.set_source(uid, group, name, state);
        }
        // The record has moved; the settle reads it off the snapshot and delivers it.
        // `on_param_changed` runs on the node's own thread, so its failure arrives as a fault.
        // A feed's group chosen: the elements offered beside it are that group's now.
        let manifest = self.leaf(uid).expect("looked up above").manifest;
        let elements: Vec<ParamKey> = goofi_node::feed_decls(manifest)
            .filter(|(_, g, d)| d.group == group && *g == name)
            .map(|(_, _, d)| ParamKey::new(d.group, d.name))
            .collect();
        if !elements.is_empty() {
            self.offer_variables(uid);
            self.runtime.refreshed.extend(elements.into_iter().map(|k| (uid, k)));
        }
        Ok(())
    }

    /// One request to a node's own thread: a refresh re-enumerates a `Str`'s options, a pulse fires.
    /// Not a write: a request holds no state, so there is nothing to undo.
    pub fn request(&mut self, uid: Uid, group: &str, name: &str, kind: goofi_node::RequestKind) -> Result<(), String> {
        let entry = self.leaf(uid).ok_or_else(|| format!("no such node {uid}"))?;
        let typed = self.typed(entry);
        let param = goofi_node::param(&typed, group, name)
            .ok_or_else(|| format!("no such param `{group}.{name}`"))?;
        let feed = goofi_node::feed_decls(entry.manifest).any(|(_, g, d)| d.group == group && (d.name == name || g == name));
        match kind {
            goofi_node::RequestKind::Refresh if !matches!(param, Param::Str { refresh: true, .. }) => {
                return Err(format!("param `{group}.{name}` is not refreshable"));
            }
            // A feed names a variable: the graph holds the list, so it answers in the node's place.
            goofi_node::RequestKind::Refresh if feed => {
                self.offer_variables(uid);
                self.runtime.refreshed.push((uid, ParamKey::new(group, name)));
                return Ok(());
            }
            goofi_node::RequestKind::Pulse if !matches!(param, Param::Pulse) => {
                return Err(format!("param `{group}.{name}` is not a pulse"));
            }
            _ => {}
        }
        let engine = entry.engine;
        if let Some(e) = self.engine_mut(engine) {
            e.request(uid, goofi_node::Request { kind, key: ParamKey::new(group, name) });
        }
        Ok(())
    }

    /// Set a param's source record; an empty one is removed. It is derived at once, so a bind error
    /// rides the record rather than refusing, and again at every settle.
    pub fn set_source(
        &mut self,
        uid: Uid,
        group: &str,
        name: &str,
        state: SourceState,
    ) -> Result<(), String> {
        let Some(leaf) = self.leaf(uid) else {
            return Err(format!("no such node {uid}"));
        };
        let key = ParamKey::new(group, name);
        // Only an empty record is a true unbind, and `unbind` owns the release on that path.
        if state.is_empty() {
            self.unbind(uid, &key);
            return Ok(());
        }
        // A record binds a real param: a dangling one is invisible in the descriptor and
        // unclearable from the UI.
        if goofi_node::param_dim(&self.typed(leaf), group, name).is_none() {
            return Err(format!("no such param `{group}/{name}`"));
        }
        if self.leaf(uid).and_then(|e| e.sources.get(&key)) != Some(&state) {
            if let Some(e) = self.leaf_mut(uid) {
                e.sources.insert(key.clone(), state);
            }
        }
        // Derived at once, so a bind error rides the record rather than refusing; the settle
        // derives it again against what the batch left.
        self.derive_one(uid, &key, &[]);
        Ok(())
    }

    /// What a source's active text derives to against the graph: the rewrite, its variables
    /// resolved, and why it cannot bind.
    fn derive(&self, uid: Uid, key: &ParamKey, state: &SourceState) -> Derived {
        let param = self.leaf(uid).and_then(|e| goofi_node::param_dim(&self.typed(e), &key.group, &key.name));
        let scanned = (!state.expression.is_empty()).then(|| expr_rewrite::rewrite(&state.expression));
        let missing = |vars: &[BoundVar]| {
            vars.iter().find_map(|v| match v {
                BoundVar::Missing { reason, .. } => Some(reason.clone()),
                _ => None,
            })
        };
        let none = |error: Option<String>| Derived { rewritten: String::new(), vars: Vec::new(), refs: Vec::new(), error };
        // A variable's expression has no param behind it; a node's does, or the record dangles.
        if uid != Uid::VARIABLES && param.is_none() {
            return none(Some(format!("no such param `{}/{}`", key.group, key.name)));
        }
        match state.mode {
            Mode::Constant => none(None),
            Mode::Expression => match scanned {
                Some(Ok((rewritten, refs))) => {
                    let vars = self.resolve_vars(uid, key, &refs);
                    // A bare read is a cable: it crosses a plane only where a cable may.
                    let bare = expr_rewrite::is_bare(&rewritten);
                    let error = missing(&vars).or_else(|| bare.then(|| self.bare_kind_error(uid, &refs[0], param.as_ref())).flatten());
                    Derived { rewritten, vars, refs, error }
                }
                Some(Err(e)) => Derived { rewritten: state.expression.clone(), vars: Vec::new(), refs: Vec::new(), error: Some(e.0) },
                None => none(Some("no expression to evaluate".to_string())),
            },
        }
    }

    /// Why a bare read of a producer cannot feed `param` on `reader`: an engine-local output never
    /// leaves its plane, so it drives a numeric param of its own engine as a plan edge and nothing
    /// else. `None` when the read is fine, or when the producer was not found (already reported).
    fn bare_kind_error(&self, reader: Uid, r: &expr_rewrite::VarRef, param: Option<&Param>) -> Option<String> {
        let Target::Node { name, slot } = &r.target else { return None };
        let uid = self.uid_by_name(name)?;
        let outputs = self.slots(uid, Dir::Out);
        let (_, slot, kind) = match (slot, outputs.len()) {
            (Some(slot), _) => outputs.into_iter().find(|(_, label, _)| label == slot)?,
            (None, 1) => outputs.into_iter().next()?,
            (None, _) => return None,
        };
        kind.engine_local()?;
        let wants = match param {
            Some(Param::Str { .. }) => goofi_core::SlotType::String,
            _ => goofi_core::SlotType::Array,
        };
        let same_plane = self.leaf(reader).map(|e| e.engine) == self.leaf(uid).map(|e| e.engine);
        (wants != goofi_core::SlotType::Array || !same_plane).then(|| {
            format!("`{name}.{slot}` is an {} output, read on its own plane alone; this param reads {}", kind.name(), wants.name())
        })
    }

    /// Drop a source and release its compiled handle — the shared tail of an empty `set_source`
    /// and of a literal write over a driven param. It does NOT re-plan: its callers do, exactly once.
    fn unbind(&mut self, uid: Uid, key: &ParamKey) {
        if self.leaf(uid).is_some_and(|e| e.sources.contains_key(key)) {
            if let Some(e) = self.leaf_mut(uid) {
                e.sources.remove(key);
            }
        }
        self.release_binding(uid, key);
    }

    /// Resolve a rewrite's variables against the graph: a producer output, a variable's value, or the
    /// reason neither was found. Event ids come from §3.2's `65..=128` budget, lowest free first.
    fn resolve_vars(&self, consumer: Uid, key: &ParamKey, refs: &[expr_rewrite::VarRef]) -> Vec<BoundVar> {
        let mut taken: Vec<EventId> = self
            .bindings_of(consumer)
            .filter(|(k, _)| *k != key)
            .flat_map(|(_, b)| &b.vars)
            .filter_map(|v| match v {
                BoundVar::Stream { event_id, .. } => Some(*event_id),
                _ => None,
            })
            .collect();
        // One shape for every stream reference, `me.out` included: a resolved wire plus an event id.
        let stream = |var: &str, taken: &mut Vec<EventId>, resolved: Result<(Uid, &'static str), String>| match resolved {
            Err(reason) => BoundVar::Missing { var: var.to_string(), reason },
            Ok((producer, slot)) => match next_event_id(taken) {
                None => BoundVar::Missing {
                    var: var.to_string(),
                    reason: "too many expression references on this node".to_string(),
                },
                Some(event_id) => {
                    taken.push(event_id);
                    BoundVar::Stream { var: var.to_string(), producer, slot, event_id }
                }
            },
        };
        let value = |var: &str, resolved: Result<Param, String>| match resolved {
            Ok(value) => BoundVar::Value { var: var.to_string(), value },
            Err(reason) => BoundVar::Missing { var: var.to_string(), reason },
        };
        refs.iter()
            .map(|r| (r.var.as_str(), &r.target))
            .map(|(var, t)| match t {
                // A variable is a slot of the patch's own producer, read as any stream is.
                Target::Variable { key } => stream(var, &mut taken, match self.variables().contains(key) {
                    true => Ok((Uid::VARIABLES, goofi_core::variables::slot_name(key))),
                    false => Err(format!("variable `{key}` is not defined")),
                }),
                Target::Node { name, slot } => {
                    stream(var, &mut taken, self.resolve_stream(name, slot.as_deref()))
                }
                Target::MeOut { slot } => {
                    stream(var, &mut taken, self.resolve_own_stream(consumer, slot.as_deref()))
                }
                Target::NodeParam { name, group, param } => {
                    value(var, self.uid_by_name(name)
                        .ok_or_else(|| format!("no node named `{name}`"))
                        .and_then(|uid| self.param_value_of(uid, name, group, param)))
                }
                Target::MeParam { group, param } => {
                    value(var, self.param_value_of(consumer, "me", group, param))
                }
                // A node's rewrite never mints these; they are a machine's.
                Target::Playhead | Target::State { .. } => BoundVar::Missing { var: var.to_string(), reason: "a machine's reference".to_string() },
            })
            .collect()
    }

    /// This node's own output, for `me.out.slot` and bare `me` — a leaf's manifest is the slot
    /// vocabulary.
    fn resolve_own_stream(&self, uid: Uid, slot: Option<&str>) -> Result<(Uid, &'static str), String> {
        let outputs = self.leaf(uid).ok_or("`me` reads a running node")?.manifest.outputs;
        let bare = || "`me` is ambiguous: this node has multiple outputs; use `me.out.<slot>`".to_string();
        pick_output(outputs, |o| o.name, slot, "this node", bare).map(|o| (uid, o.name))
    }

    /// A param's EFFECTIVE value for an expression: the evaluated report when the param is
    /// driven, else the authored record. `who` names the node in the error as the source spelled it.
    fn param_value_of(&self, uid: Uid, who: &str, group: &str, name: &str) -> Result<Param, String> {
        let entry = self
            .leaf(uid)
            .ok_or_else(|| format!("`{who}` holds no params: a port relays and a facade fronts"))?;
        self.health(uid)
            .and_then(|h| h.evaluated.get(&ParamKey::new(group, name)).cloned())
            .or_else(|| goofi_node::param(&self.typed(entry), group, name).cloned())
            .ok_or_else(|| format!("`{who}` has no param `{group}/{name}`"))
    }

    /// The producer output a `nd('name')` term names, or why it names none. A bare reference to a
    /// multi-output node is refused HERE — the graph is what knows how many outputs a node has.
    fn resolve_stream(&self, name: &str, slot: Option<&str>) -> Result<(Uid, &'static str), String> {
        let uid = self.uid_by_name(name).ok_or_else(|| format!("no node named `{name}`"))?;
        // The slot vocabulary is one question and the stream behind it another, and every node kind
        // answers both — a leaf, a facade whose outputs are its ports, and a port itself.
        let outputs = self.slots(uid, Dir::Out);
        // By NAME, because `nd()` reads the one display namespace: a facade's slot is its port's name.
        let bare = || format!("nd('{name}') is ambiguous: it has multiple outputs; use nd('{name}').out.<slot>");
        let key = &pick_output(&outputs, |(_, label, _)| label, slot, &format!("node `{name}`"), bare)?.0;
        match self.stream(uid, key) {
            Some(Stream::At(leaf, slot)) => Ok((leaf, slot)),
            Some(Stream::Open(port)) => Err(format!(
                "port `{}` has nothing wired to it yet",
                self.name(port).unwrap_or(name)
            )),
            None => Err(format!("node `{name}` has no output `{key}`")),
        }
    }

    /// The source record on a param, for the bridge descriptor + `.gfi` (or `None` if the param is
    /// a plain literal with nothing retained).
    pub fn param_source(&self, uid: Uid, group: &str, name: &str) -> Option<SourceInfo> {
        let key = ParamKey::new(group, name);
        let state = self.leaf(uid)?.sources.get(&key)?;
        Some(SourceInfo { state: state.clone(), error: self.source_error(uid, &key) })
    }

    /// One param's error: the bind the graph refused, or the node's own last evaluation failure.
    /// The one derivation, so a descriptor and the live sweep cannot answer differently.
    fn source_error(&self, uid: Uid, key: &ParamKey) -> Option<String> {
        self.leaf(uid)?.sources.get(key)?;
        self.binding(uid, key).and_then(|b| b.bind_error.clone()).or_else(|| self.health(uid)?.param_errors.get(key).cloned())
    }

    /// Every param error on `uid` as `(group, name, message)` — what the live sweep broadcasts, so
    /// a failure that arrives or clears with no op behind it still reaches a client.
    pub fn param_errors(&self, uid: Uid) -> Vec<(&str, &str, String)> {
        let Some(entry) = self.leaf(uid) else { return Vec::new() };
        entry
            .sources
            .keys()
            .filter_map(|key| self.source_error(uid, key).map(|m| (key.group.as_str(), key.name.as_str(), m)))
            .collect()
    }

    /// Whether any param of `uid` has a source other than its literal.
    pub fn driven(&self, uid: Uid) -> bool {
        self.leaf(uid).is_some_and(|e| e.sources.values().any(|s| s.mode != Mode::Constant))
    }

    /// What the params with a live mode currently evaluate to — the inspector's preview. A constant
    /// is excluded: its value is the literal, already on the descriptor.
    pub fn driven_values(&self, uid: Uid) -> Vec<(&str, &str, &Param)> {
        let (Some(entry), Some(health)) = (self.leaf(uid), self.health(uid)) else {
            return Vec::new();
        };
        health
            .evaluated
            .iter()
            .filter(|(key, _)| entry.sources.get(key).is_some_and(|s| s.mode != Mode::Constant))
            .map(|(key, p)| (key.group.as_str(), key.name.as_str(), p))
            .collect()
    }

    /// Resolve a node display name to its uid — `nd('name')` and the CLI's name spelling alike.
    pub fn uid_by_name(&self, name: &str) -> Option<Uid> {
        self.named().find(|(_, n)| *n == name).map(|(u, _)| u)
    }

    /// A node reference as a uid or the display name. An existing uid wins a hex-looking name; a
    /// well-formed uid that names nothing stays a uid, so an idempotent remove keeps its meaning.
    pub fn resolve_ref(&self, raw: &str) -> Option<Uid> {
        match Uid::from_hex(raw) {
            Some(uid) if self.exists(uid) => Some(uid),
            hex => self.uid_by_name(raw).or(hex),
        }
    }

    /// Does this exact (resolved) wire already exist? What makes a `link add` of it the no-op.
    pub fn has_link(&self, node_out: Uid, slot_out: &str, node_in: Uid, slot_in: &str) -> bool {
        let (Some(out), Some(inp)) = (self.slot_face(node_out, slot_out, Dir::Out), self.slot_face(node_in, slot_in, Dir::In))
        else {
            return false;
        };
        let (slot_out, slot_in) = (out.name, inp.name);
        self.patch.links.contains_key(&Link { node_out, slot_out, node_in, slot_in }.key())
    }

    pub fn add_link(
        &mut self,
        node_out: Uid,
        slot_out: &str,
        node_in: Uid,
        slot_in: &str,
    ) -> Result<(), String> {
        // A facade address IS its port, folded here so no stored link ever names a scope — one
        // normalisation, at the one door every link authoring path goes through.
        let (node_out, so) = self.normalise(node_out, slot_out);
        let (node_in, si) = self.normalise(node_in, slot_in);
        let (slot_out, slot_in) = (so.as_str(), si.as_str());
        // Each slot's face, taken once: it carries both the `&'static` name a link is keyed by and
        // the dtype the check below needs, so there is no second lookup that could fail on its own.
        let out = self
            .slot_face(node_out, slot_out, Dir::Out)
            .ok_or_else(|| format!("no output slot `{slot_out}` on {node_out}"))?;
        let inp = self
            .slot_face(node_in, slot_in, Dir::In)
            .ok_or_else(|| format!("no input slot `{slot_in}` on {node_in}"))?;
        let (slot_out, slot_in) = (out.name, inp.name);
        // Both ends must face the same scope, or the cable crosses a wall without a port to carry
        // it — which is also what refuses an IN port wired from inside, its consumer side being out.
        if self.face(node_out, true) != self.face(node_in, false) {
            let label = |uid: Uid| self.name(uid).unwrap_or("?").to_string();
            return Err(format!(
                "cannot link {} to {}: they are not in the same sub-patch — wire it to a boundary port",
                label(node_out),
                label(node_in),
            ));
        }
        if !out.kind.feeds(inp.kind) {
            let label = |uid: Uid, slot: &str| {
                format!("{}.{slot}", self.name(uid).unwrap_or("?"))
            };
            return Err(format!(
                "cannot link {} ({}) to {} ({}): the slots carry different data types",
                label(node_out, slot_out),
                out.kind.name(),
                label(node_in, slot_in),
                inp.kind.name(),
            ));
        }

        let new = Link {
            node_out,
            slot_out,
            node_in,
            slot_in,
        };
        if self.patch.links.contains_key(&new.key()) {
            return Ok(()); // idempotent
        }
        // A multi slot keeps its wires in connection order, which IS `links`' own order; a single
        // input takes one, so a second wire EVICTS the first. The node hears one declarative set.
        if !inp.multi {
            self.links_mut().retain(|_, l| !(l.node_in == node_in && l.slot_in == slot_in));
        }
        self.links_mut().insert(new.key(), new);
        Ok(())
    }

    pub fn remove_link(
        &mut self,
        node_out: Uid,
        slot_out: &str,
        node_in: Uid,
        slot_in: &str,
    ) -> Result<(), String> {
        if self.links_mut().shift_remove(&doc::link_key(node_out, slot_out, node_in, slot_in)).is_none() {
            return Err("no such link".into());
        }
        Ok(())
    }

    /// The resolver input every service name is scoped by. The graph MINTS it and carries it;
    /// deriving a name from it is `goofi-transport`'s, which the graph never links.
    pub fn instance(&self) -> &str {
        &self.runtime.instance
    }

    /// The patch's time. An engine holds this handle; there is no second origin anywhere.
    pub fn time(&self) -> Arc<goofi_supervisor::timing::Time> {
        self.runtime.time.clone()
    }
    /// The generation of the node about to be born at `uid`: 0 for a first birth, one more than
    /// the last for every rebirth.
    fn bump_generation(&mut self, uid: Uid) -> u64 {
        let next = self.runtime.generations.get(&uid).map_or(0, |g| g + 1);
        self.runtime.generations.insert(uid, next);
        self.runtime.epoch.fetch_add(1, std::sync::atomic::Ordering::Release);
        next
    }

    /// Deliver what the batch changed, read off the two snapshots: the patch as it stands and the
    /// one the last settle delivered, pointer by pointer. One decision per changed node, from
    /// settled state, however often the batch touched it. Free when nothing moved.
    pub fn settle(&mut self) {
        let vars_version = self.variables().version();
        let vars_moved = vars_version != self.runtime.settled_vars;
        let redeliver = std::mem::take(&mut self.runtime.redeliver) || vars_moved;
        let restarts = std::mem::take(&mut self.runtime.restarts);
        let rewatched = std::mem::take(&mut self.runtime.rewatched);
        let diff = self.diff();
        let replan = self.engines().any(|e| e.dirty());
        if diff.is_empty() && restarts.is_empty() && rewatched.is_empty() && !redeliver && !replan {
            return;
        }
        self.runtime.settled_vars = vars_version;
        self.runtime.epoch.fetch_add(1, std::sync::atomic::Ordering::Release);
        self.runtime.revision += 1;
        self.runtime.marks.machines |= diff.machines;
        // Every arming the batch left unnumbered gets its serial, before the snapshot is kept.
        for uid in &diff.rearmed {
            self.mint_serials(*uid);
        }
        self.runtime.settled = self.patch.clone();

        // The instances level with the leaves: removals, births, then rebirths — a node whose
        // class or engine changed under it, or one asked to restart.
        let was = |g: &Graph, u: Uid| g.runtime.instances.contains_key(&u);
        let gone: Vec<Uid> = diff.removed.iter().chain(&diff.changed).filter(|u| was(self, **u) && !self.is_leaf(**u)).copied().collect();
        let born: Vec<Uid> = diff.changed.iter().filter(|u| !was(self, **u) && self.is_leaf(**u)).copied().collect();
        let reborn: Vec<Uid> = diff
            .reclassed
            .iter()
            .chain(&restarts)
            .filter(|u| was(self, **u) && self.is_leaf(**u))
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        for uid in &diff.removed {
            self.runtime.watched.retain(|(u, _)| u != uid);
            self.runtime.view_wants.retain(|(u, _), _| u != uid);
        }
        for uid in &gone {
            self.halt(*uid);
        }
        for uid in &born {
            self.birth(*uid);
        }
        for uid in &reborn {
            self.halt(*uid);
            self.birth(*uid);
        }

        // The bindings: of every node when a variable or the library moved, else of the changed
        // nodes, the referrers of a name that came, went or moved, and the readers of an edited
        // param. A variable's expression spells nodes as a param's does.
        if redeliver || !diff.is_empty() {
            self.derive_variables();
        }
        let derive: Vec<Uid> = if redeliver {
            self.leaves().map(|(u, _)| u).collect()
        } else {
            let names: Vec<&str> =
                diff.renamed.iter().flat_map(|(a, b)| [a.as_str(), b.as_str()]).chain(diff.named.iter().map(String::as_str)).collect();
            let mut derive: Vec<Uid> = diff.changed.iter().chain(&diff.removed).copied().collect();
            derive.extend(self.referrers(&names));
            derive.extend(self.param_readers(&diff.edited));
            // A stream read resolves through the wires: one made or cut behind a port moves it.
            if diff.relinked {
                derive.extend(self.stream_readers());
            }
            derive
        };
        let derive: Vec<Uid> = derive.into_iter().collect::<HashSet<_>>().into_iter().collect();
        let rebound = self.derive_nodes(&derive, &diff.edited);
        let lives_moved = !born.is_empty() || !reborn.is_empty() || !gone.is_empty();
        self.runtime.marks.ringers |= !rebound.is_empty() || lives_moved || self.runtime.binds_moved || redeliver;
        self.runtime.marks.groups |= !derive.is_empty() || redeliver;
        self.runtime.marks.armed |= !diff.rearmed.is_empty() || lives_moved;

        // What each engine is re-sent: the changed leaves, both ends of every wire that moved,
        // the neighbours of a node born or reborn (its service names moved), a consumer whose
        // producer was renamed (a multi slot labels its senders), a binding that moved, a watch
        // that moved — or every leaf, when a fact every node reads moved.
        let edges = self.resolved_edges();
        let changed: Vec<Uid> = if redeliver {
            self.leaves().map(|(u, _)| u).collect()
        } else {
            let mut set: HashSet<Uid> = diff.changed.iter().copied().collect();
            set.extend(rewatched);
            set.extend(rebound);
            for e in diff.rewired.iter() {
                set.insert(e.producer.0);
                set.insert(e.consumer.0);
            }
            let renamed: HashSet<Uid> = diff.renamed_uids.iter().copied().collect();
            let moved: HashSet<Uid> = born.iter().chain(&reborn).copied().collect();
            for e in &edges {
                if moved.contains(&e.producer.0) || moved.contains(&e.consumer.0) {
                    set.insert(e.producer.0);
                    set.insert(e.consumer.0);
                }
                if renamed.contains(&e.producer.0) {
                    set.insert(e.consumer.0);
                }
            }
            set.into_iter().filter(|u| self.is_leaf(*u)).collect()
        };
        let typed: HashMap<Uid, Arc<ParamGroups>> = self.leaves().map(|(u, l)| (u, self.typed(l))).collect();
        let published = {
            let Runtime { generations, instance, engines, watched, bindings, .. } = &mut self.runtime;
            let variables = self.patch.variables.lock();
            let settled = Settled { bindings, generations, watched, instance };
            let view = build_view(&self.patch.nodes, &settled, &typed, &edges, &variables);
            for e in engines.iter_mut() {
                e.settle(&view, &changed);
            }
            engines.iter().flat_map(|e| e.published()).collect::<Vec<_>>()
        };
        // A wire made or cut, or a node reborn, changes what its producer owes its viewers.
        self.offer_view_wants(&edges);
        // The engines' own facts into `system.*`, from the state this settle reached. Not a
        // write of the user's: the undoable act is the param they moved.
        for (name, value) in published {
            self.variables().publish(name, value);
        }
        self.runtime.settled_edges = edges;
    }

    /// What the batch changed, read off the patch and the snapshot the last settle delivered.
    fn diff(&self) -> Diff {
        let prev = &self.runtime.settled;
        let next = &self.patch;
        let mut d = Diff::default();
        for (uid, e) in &next.nodes {
            let Some(p) = prev.nodes.get(uid) else {
                d.changed.push(*uid);
                d.named.push(e.name.clone());
                if !e.record.is_empty() {
                    d.rearmed.push(*uid);
                }
                continue;
            };
            if Arc::ptr_eq(p, e) {
                continue;
            }
            d.changed.push(*uid);
            if p.name != e.name {
                d.renamed.push((p.name.clone(), e.name.clone()));
                d.renamed_uids.push(*uid);
            }
            if p.record != e.record {
                d.rearmed.push(*uid);
            }
            if let (Some(a), Some(b)) = (p.leaf(), e.leaf()) {
                if !std::ptr::eq(a.manifest, b.manifest) || a.engine != b.engine {
                    d.reclassed.push(*uid);
                }
                // An authored edit of a param: its literal or its source moved.
                let keys = b.values.get().iter().flat_map(|(g, names)| names.keys().map(move |n| ParamKey::new(g, n)));
                let old = a.values.get();
                for key in keys.chain(b.sources.keys().cloned()).chain(a.sources.keys().cloned()) {
                    fn lit<'v>(v: &'v doc::Values, key: &ParamKey) -> Option<&'v Data> {
                        v.get(&key.group).and_then(|g| g.get(&key.name))
                    }
                    let moved = lit(old, &key) != lit(b.values.get(), &key) || a.sources.get(&key) != b.sources.get(&key);
                    if moved && !d.edited.contains(&(*uid, key.clone())) {
                        d.edited.push((*uid, key));
                    }
                }
            }
        }
        for (uid, p) in &prev.nodes {
            if !next.nodes.contains_key(uid) {
                d.removed.push(*uid);
                d.named.push(p.name.clone());
            }
        }
        d.relinked = !Arc::ptr_eq(&prev.links, &next.links);
        d.machines = !Arc::ptr_eq(&prev.machines, &next.machines);
        d.arrangement = !Arc::ptr_eq(&prev.arrangement, &next.arrangement);
        if d.relinked || !d.changed.is_empty() || !d.removed.is_empty() {
            // A port's wires resolve to leaves, so a wire that moved shows as a resolved edge
            // that one settle's walk found and the other's did not.
            let before: HashSet<Edge> = self.runtime.settled_edges.iter().copied().collect();
            let after: HashSet<Edge> = self.resolved_edges().into_iter().collect();
            d.rewired = before.symmetric_difference(&after).copied().collect();
        }
        d
    }

    /// Halt every instance whose leaf is gone from the patch — what a `clear` on the way to a
    /// load does at once, into the workspace being replaced, before the settle would.
    fn halt_gone(&mut self) {
        let gone: Vec<Uid> = self.runtime.instances.keys().filter(|u| !self.is_leaf(**u)).copied().collect();
        for uid in gone {
            self.halt(uid);
        }
    }

    /// Hand a leaf to its engine. The generation bump keeps the newborn's service names clear
    /// of any predecessor's, whose teardown does not block.
    fn birth(&mut self, uid: Uid) {
        let generation = self.bump_generation(uid);
        let (engine, type_name, params) = {
            let leaf = self.leaf(uid).expect("a leaf is what is born");
            (leaf.engine, leaf.manifest.type_name, self.typed(leaf))
        };
        let boot_error = self
            .engine_mut(engine)
            .expect("the library entry named it")
            .insert(uid, type_name, generation, &params);
        self.runtime.instances.insert(uid, Instance { engine, health: Health::born(boot_error) });
        self.offer_variables(uid);
    }

    /// The patch's groups as the options of every feed's group param, and the chosen group's
    /// elements as its element param's, shown over the declared (empty) lists; a refresh asks again.
    fn offer_variables(&mut self, uid: Uid) {
        let Some(leaf) = self.leaf(uid) else { return };
        let typed = self.typed(leaf);
        let feeds: Vec<(ParamKey, ParamKey, Option<String>)> = goofi_node::feed_decls(leaf.manifest)
            .map(|(_, group, d)| {
                let chosen = match goofi_node::param(&typed, d.group, group) {
                    Some(goofi_core::Param::Str { value, .. }) => Some(value.clone()),
                    _ => None,
                };
                (ParamKey::new(d.group, group), ParamKey::new(d.group, d.name), chosen)
            })
            .collect();
        if feeds.is_empty() {
            return;
        }
        let variables = self.patch.variables.lock();
        let mut groups: Vec<String> = Vec::new();
        let mut elements: HashMap<String, Vec<String>> = HashMap::new();
        for (name, _) in variables.entries() {
            if let Some((g, e)) = goofi_core::variables::split_variable(name) {
                if !groups.iter().any(|h| h == g) {
                    groups.push(g.to_string());
                }
                elements.entry(g.to_string()).or_default().push(e.to_string());
            }
        }
        drop(variables);
        let Some(inst) = self.runtime.instances.get_mut(&uid) else { return };
        for (group_key, element_key, chosen) in feeds {
            inst.health.options.insert(group_key, groups.clone());
            let offered = chosen.and_then(|g| elements.get(&g).cloned()).unwrap_or_default();
            inst.health.options.insert(element_key, offered);
        }
    }

    /// Take a node's instance back from its engine. The engine holds its OWN handle on the node's
    /// channel; `remove` rather than detach, since nothing queued for this instance applies.
    fn halt(&mut self, uid: Uid) {
        let Some(instance) = self.runtime.instances.remove(&uid) else { return };
        if let Some(e) = self.engine_mut(instance.engine) {
            e.remove(uid);
        }
    }

    /// Every leaf-to-leaf wire, ports resolved away, in link order — which IS a multi input's
    /// wire order. Computed once per settle, so no engine re-implements the relay walk.
    fn resolved_edges(&self) -> Vec<Edge> {
        self.patch
            .links
            .values()
            .filter(|l| self.leaf(l.node_in).is_some())
            .filter_map(|l| match self.stream(l.node_out, l.slot_out)? {
                Stream::At(u, s) => Some(Edge { producer: (u, s), consumer: (l.node_in, l.slot_in) }),
                Stream::Open(_) => None,
            })
            .collect()
    }

    /// Take every engine's waiting reports and apply the health plane. Answers how many landed,
    /// so a caller can tell a quiet graph from one it stopped hearing, and whether a node's
    /// stage moved — what a reader of the health plane re-reads for.
    pub fn drain_status(&mut self) -> Drained {
        let mut drained = Drained::default();
        {
            let Runtime { instances, refreshed, engines, bindings, .. } = &mut self.runtime;
            let nodes = &self.patch.nodes;
            let staged = &mut drained.staged;
            let mut apply = |uid: Uid, status: Status| {
                *staged |= matches!(status, Status::Stage { .. });
                apply_status_to(nodes, bindings, instances, refreshed, uid, status)
            };
            for e in engines.iter_mut() {
                drained.reports += e.drain(&mut apply);
            }
        }
        // A direct driver has no bridge to settle for it, and the drain-side settle lands what
        // the drain marked pending.
        self.settle();
        drained
    }

    /// The params whose options were re-enumerated since the last call — the worker's cue to echo
    /// them. A QUEUE, because options are the one part of a node the doc has no field for.
    pub fn take_refreshed(&mut self) -> Vec<(Uid, ParamKey)> {
        std::mem::take(&mut self.runtime.refreshed)
    }
    /// Remove all nodes and links.
    pub fn clear(&mut self) {
        // Release each node's compiled expression handles before dropping them (load_doc
        // goes through here, so a File→Open cycle can't leak the evaluator's registry).
        let held: Vec<Uid> = self.runtime.bindings.keys().copied().collect();
        for uid in held {
            self.release_bindings(uid);
        }
        // The nodes go wholesale; the settle reads each removal off the snapshot and halts it.
        // The generations stay, keeping whatever is born at those uids next clear of what died.
        self.patch.nodes.clear();
        self.links_mut().clear();
        self.runtime.restarts.clear();
        self.runtime.rewatched.clear();
        // An un-echoed refresh names a node the patch no longer holds — and a load restores uids,
        // so that number can come back and the echo be read as an answer nobody asked for.
        self.runtime.refreshed.clear();
        // Variables and machines are patch CONTENT, so a load starts from a fresh seeded store.
        self.machines_mut().clear();
        self.variables().reset();
        // Time belongs to the PATCH: one loaded an hour in must read what it would at boot. Every
        // engine holds this same object, so there is nothing to push.
        self.runtime.time.restart();
    }

    /// Every uid `roots` reaches: the roots, whatever their scopes hold to any depth, and those
    /// scopes' ports. The subtree a copy, a delete and an export all mean by "these nodes".
    pub fn subtree_of(&self, roots: &[Uid]) -> Vec<Uid> {
        let mut out: Vec<Uid> = Vec::new();
        let mut stack: Vec<Uid> = roots.iter().rev().copied().collect();
        while let Some(u) = stack.pop() {
            if !self.patch.nodes.contains_key(&u) || out.contains(&u) {
                continue;
            }
            out.push(u);
            if self.is_facade(u) {
                stack.extend(self.scope_members(u));
            }
        }
        out
    }

    /// The records of `uids` and the links with BOTH ends among them, in the document's own shape:
    /// a clipboard and a patch are ONE format.
    pub fn fragment(&self, uids: &[Uid]) -> doc::PatchDoc {
        let want: HashSet<Uid> = uids.iter().copied().collect();
        // ONE loop over ONE map: a leaf, a facade and a port are all node records, and membership
        // rides each record rather than a member list beside what `scope_of` owns.
        let nodes = self
            .patch
            .nodes
            .iter()
            .filter(|(u, _)| want.contains(u))
            .map(|(uid, e)| (uid.to_hex(), self.node_record(*uid, e, &|p| want.contains(&p))))
            .collect();
        // A port's inner wire is a link like any other — the same one `add_link` writes — so a
        // fragment has one relation kind as well as one entity kind.
        let links = self
            .patch
            .links
            .values()
            .filter(|l| want.contains(&l.node_out) && want.contains(&l.node_in))
            .map(link_record)
            .collect();
        doc::PatchDoc { nodes, links, ..Default::default() }
    }

    /// One node's record as the document carries it. `kept` says whether a scope rides: a
    /// fragment drops a parent it does not carry, a projection carries every parent.
    fn node_record(&self, uid: Uid, e: &NodeEntry, kept: &dyn Fn(Uid) -> bool) -> doc::NodeRecord {
        let mut params = IndexMap::new();
        if let Some(leaf) = e.leaf() {
            for (group, names) in self.typed(leaf).iter() {
                let entries: IndexMap<String, doc::ParamEntry> = names
                    .iter()
                    .filter_map(|(name, p)| {
                        let entry = doc::ParamEntry { value: control::data_of(p), ..Default::default() };
                        let entry = match leaf.sources.get(&ParamKey::new(group, name)) {
                            Some(s) => entry.with_source(s),
                            None => entry,
                        };
                        // A pulse has no literal, so it is written only for the source it carries.
                        (entry.value.is_some() || entry.mode.is_some()).then(|| (name.clone(), entry))
                    })
                    .chain(element_sources(leaf, group, names))
                    .collect();
                params.insert(group.clone(), entries);
            }
        }
        let blob = |v: &serde_json::Value| v.as_object().is_some_and(|m| !m.is_empty()).then(|| v.clone());
        doc::NodeRecord {
            type_id: self.node_type(uid).unwrap_or_default(),
            name: e.name.clone(),
            pos: e.pos,
            scope: self.scope_of(uid).filter(|p| kept(*p)).map(|p| p.to_hex()),
            params,
            viewers: blob(&e.viewers),
            baseline: blob(&e.baseline),
            record: e.record.clone(),
        }
    }

    /// The variables and their groups as the document carries them: the configuration, never
    /// a value, which is a frame on the data plane.
    fn variable_records(&self) -> (IndexMap<String, doc::VariableRecord>, IndexMap<String, goofi_core::variables::Group>) {
        let variables = self.variables();
        let records = variables
            .entries()
            .map(|(name, v)| (name.to_string(), doc::VariableRecord { value: None, control: v.control.clone(), expression: v.expression.clone(), lock: v.lock }))
            .collect();
        let groups = variables.groups().map(|(g, rec)| (g.to_string(), rec.clone())).collect();
        (records, groups)
    }

    /// The variables root a replica holds: beside a variable's expression, why it cannot bind —
    /// the one runtime fact the panel shows inline.
    fn variables_root(&self, records: &IndexMap<String, doc::VariableRecord>) -> serde_json::Value {
        let mut root = serde_json::to_value(records).expect("a plain record");
        for (name, b) in &self.runtime.variable_binds {
            if let (Some(error), Some(record)) = (&b.bind_error, root.get_mut(name)) {
                record["error"] = serde_json::Value::String(error.clone());
            }
        }
        root
    }

    /// What the settles since the last take moved, for the followers; taken once.
    pub fn take_marks(&mut self) -> Marks {
        std::mem::take(&mut self.runtime.marks)
    }

    /// Moved by every settle that delivered and every viewpoint write.
    pub fn revision(&self) -> u64 {
        self.runtime.revision
    }

    /// The patch and the variable store as they stand: what a history entry keeps.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { patch: self.patch.clone(), variables: self.variables().snapshot() }
    }

    /// Put a snapshot back whole. The settle that follows reads every consequence off the diff:
    /// births, removals, re-derivations, the document's ops.
    pub fn restore(&mut self, s: &Snapshot) {
        self.patch = s.patch.clone();
        self.variables().restore(&s.variables);
    }

    /// Whether the patch or the store's configuration moved since `s` was taken: a write that
    /// changed nothing records no step.
    pub fn differs(&self, s: &Snapshot) -> bool {
        let p = &s.patch;
        self.patch.nodes.len() != p.nodes.len()
            || !Arc::ptr_eq(&self.patch.links, &p.links)
            || !Arc::ptr_eq(&self.patch.machines, &p.machines)
            || !Arc::ptr_eq(&self.patch.arrangement, &p.arrangement)
            || self.patch.nodes.iter().any(|(u, e)| !p.nodes.get(u).is_some_and(|q| Arc::ptr_eq(q, e)))
            || self.variables().version() != s.variables.version
    }

    /// What the document must be told since the last projection, read off the patch and the
    /// snapshot last projected: each changed node's record, each removed node, and each root that
    /// moved, whole. The first call after a boot carries every root. Serialized here, under the
    /// graph lock, so the diff against the document runs off it.
    pub fn projection(&mut self) -> Projection {
        let prev = &self.runtime.projected;
        let mut out = Projection::default();
        for (uid, e) in &self.patch.nodes {
            if prev.nodes.get(uid).is_some_and(|p| Arc::ptr_eq(p, e)) {
                continue;
            }
            let record = self.node_record(*uid, e, &|_| true);
            out.nodes.push((uid.to_hex(), Some(serde_json::to_value(record).expect("a plain record"))));
        }
        for uid in prev.nodes.keys() {
            if !self.patch.nodes.contains_key(uid) {
                out.nodes.push((uid.to_hex(), None));
            }
        }
        if !Arc::ptr_eq(&prev.links, &self.patch.links) {
            let links: IndexMap<String, doc::Link> = self.patch.links.values().map(link_record).collect();
            out.roots.push(("links", serde_json::to_value(links).expect("a plain record")));
        }
        let vars_version = self.variables().version();
        if vars_version != self.runtime.projected_vars || self.runtime.binds_moved {
            let (records, groups) = self.variable_records();
            out.roots.push(("variables", self.variables_root(&records)));
            out.roots.push(("variable_groups", serde_json::to_value(groups).expect("a plain record")));
        }
        if !Arc::ptr_eq(&prev.machines, &self.patch.machines) {
            out.roots.push(("machines", serde_json::to_value(&*self.patch.machines).expect("a plain record")));
        }
        if !Arc::ptr_eq(&prev.arrangement, &self.patch.arrangement) {
            out.roots.push(("arrangement", self.patch.arrangement.to_json()));
        }
        self.runtime.projected = self.patch.clone();
        self.runtime.projected_vars = vars_version;
        self.runtime.binds_moved = false;
        out
    }

    /// The literals a record asks for, folded over the type's defaults. NON-seeding, because a
    /// restore must not re-synthesize a binding the user had unbound.
    fn record_params(&self, rec: &doc::NodeRecord) -> Result<doc::Values, String> {
        let asked: doc::Values = rec
            .params
            .iter()
            .map(|(g, names)| (g.clone(), names.iter().filter_map(|(n, e)| Some((n.clone(), e.value.clone()?))).collect()))
            .collect();
        Ok(values_of(&self.folded(&rec.type_id, &asked)?))
    }

    /// The one gate a load and a paste pass. Every leaf's type resolves or the document is refused;
    /// what cannot land is dropped and said.
    pub fn admit(&self, mut doc: doc::PatchDoc) -> Result<(doc::PatchDoc, Vec<String>), String> {
        let mut warnings = Vec::new();
        for (key, rec) in doc.nodes.iter_mut() {
            // A facade and a boundary port are the model's own types, not the palette's: they have
            // no module to be missing, so the availability gate is not theirs to pass.
            if structural(&rec.type_id) {
                rec.params.clear();
                continue;
            }
            self.resolve_type(&rec.type_id)?;
            let declared = self.default_params_of(&rec.type_id, None)?;
            rec.params.retain(|group, names| {
                names.retain(|name, _| {
                    let kept = declared.get(group).is_some_and(|g| g.contains_key(name));
                    if !kept {
                        warnings.push(format!("{key}: `{group}/{name}` is not a param of {}, dropped", rec.type_id));
                    }
                    kept
                });
                !names.is_empty()
            });
        }
        let nodes = &doc.nodes;
        doc.links.retain(|_, l| {
            let kept = nodes.contains_key(&l.node_out) && nodes.contains_key(&l.node_in);
            if !kept {
                warnings.push(format!("link {}.{} > {}.{} names a record the document does not hold, dropped", l.node_out, l.slot_out, l.node_in, l.slot_in));
            }
            kept
        });
        for name in doc.variables.keys() {
            if !goofi_core::variables::is_valid_variable_name(name) {
                return Err(format!(
                    "this patch holds the variable `{name}`, which is not `group.element`: {}",
                    goofi_core::variables::VARIABLE_NAME_RULE
                ));
            }
        }
        Ok((doc, warnings))
    }

    /// Paste a fragment into `scope` at `offset`: every record under a fresh uid and a fresh
    /// name, with its sources respelled to the copies, then the memberships, then the links.
    /// Answers `old uid -> new uid`, for the viewers that key by it.
    pub fn import_fragment(
        &mut self,
        doc: &doc::PatchDoc,
        scope: Option<Uid>,
        offset: [f64; 2],
    ) -> Result<HashMap<String, String>, String> {
        let nodes = &doc.nodes;
        if let Some(s) = scope.filter(|s| !self.is_facade(*s)) {
            return Err(format!("paste: no such scope {s}"));
        }
        // `uid -> fresh uid` for every record, so a link and a `scope` are remapped before
        // anything is created.
        let idmap: HashMap<String, Uid> = nodes.keys().map(|old| (old.clone(), self.mint())).collect();
        // The names the copy will wear, picked BEFORE anything is built: an expression spells a
        // NAME, and one left naming the original would bind the copy to it.
        let mut taken: std::collections::HashSet<String> =
            self.patch.nodes.values().map(|e| e.name.clone()).collect();
        let mut renamed: HashMap<String, String> = HashMap::new();
        for rec in nodes.values() {
            let base = name_base(goofi_node::bare(&rec.type_id));
            let fresh = goofi_core::fresh_name(&base, 0, |c| taken.contains(c));
            taken.insert(fresh.clone());
            renamed.insert(rec.name.clone(), fresh);
        }
        let by_old = |old: &str| renamed.get(old).cloned();
        // A copied PORT's name is also a slot label — on the copied facade that holds it, and nowhere
        // else — so a slot is renamed only there, never where a node merely shares its name.
        let port_labels: HashMap<&str, Vec<&str>> = nodes
            .values()
            .filter(|rec| subpatch::boundary_type(&rec.type_id).is_some())
            .filter_map(|rec| Some((nodes.get(rec.scope.as_deref()?)?.name.as_str(), rec.name.as_str())))
            .fold(HashMap::new(), |mut m: HashMap<&str, Vec<&str>>, (facade, port)| {
                m.entry(facade).or_default().push(port);
                m
            });
        let at = |p: [f64; 2]| [p[0] + offset[0], p[1] + offset[1]];
        let mut order: Vec<&String> = nodes.keys().collect();
        order.sort_by_key(|old| kind_order(&nodes[*old].type_id));
        for old in &order {
            let rec = &nodes[*old];
            let ty = rec.type_id.as_str();
            let inner = rec.scope.as_deref().and_then(|s| idmap.get(s)).copied();
            let params = (!structural(ty)).then(|| self.record_params(rec)).transpose()?;
            let sources: Vec<(String, String, SourceState)> = sources_of(rec)
                .into_iter()
                .map(|(group, name, mut s)| {
                    let remap = |named: &str, slot: Option<&str>| {
                        let label = slot
                            .filter(|s| port_labels.get(named).is_some_and(|ports| ports.contains(s)))
                            .and_then(by_old);
                        (by_old(named), label)
                    };
                    if let Some(src) = expr_rewrite::rename_refs(&s.expression, remap) {
                        s.expression = src;
                    }
                    (group, name, s)
                })
                .collect();
            // A port cannot exist without a scope, so it takes the paste target when its own
            // facade is not in the fragment — the same fallback every other kind gets below.
            let port_scope = subpatch::boundary_type(ty).and(inner.or(scope));
            let uid = self.create_node(ty, Some(idmap[*old]), by_old(&rec.name).as_deref().unwrap_or(""), params, port_scope)?;
            self.restore_extras(
                uid,
                at(rec.pos),
                &sources,
                rec.viewers.as_ref().map(|v| remap_slots(v, &idmap)),
                // NOT slot-remapped: a baseline is keyed by `group/param`, which a paste does not
                // renumber the way it renumbers the slot uids a viewer blob is keyed by.
                rec.baseline.clone(),
                Some(rec.record.clone()),
            );
        }
        for old in &order {
            let inner = nodes[*old].scope.as_deref().and_then(|s| idmap.get(s)).copied();
            // A record naming no scope INSIDE the fragment is a root of it, so it lands where the
            // paste was aimed; one naming a scope in here keeps the shape it was copied with.
            self.reparent(idmap[*old], inner.or(scope))?;
        }
        for l in doc.links.values() {
            let (Some(no), Some(ni)) = (idmap.get(&l.node_out).copied(), idmap.get(&l.node_in).copied()) else {
                continue;
            };
            self.add_link(no, &l.slot_out, ni, &l.slot_in)?;
        }
        Ok(idmap.into_iter().map(|(old, new)| (old, new.to_hex())).collect())
    }

    /// The whole patch as one document: every record, every variable in order, the arrangement.
    fn document(&self) -> doc::PatchDoc {
        let mut patch = self.fragment(&self.all_uids());
        (patch.variables, patch.variable_groups) = self.variable_records();
        patch.machines = (*self.patch.machines).clone();
        // The flat arrangement always exists (at worst the default), so it always rides.
        patch.arrangement = Some(self.patch.arrangement.to_json());
        patch
    }

    /// The document a browser replica holds: the patch, every root present, and beside a
    /// variable's expression why it cannot bind — the one runtime fact the panel shows inline.
    pub fn replica(&self) -> serde_json::Value {
        let document = self.document();
        let variables = self.variables_root(&document.variables);
        let mut doc = serde_json::to_value(document).expect("a plain record");
        for root in ["nodes", "links", "variables", "variable_groups", "machines"] {
            doc[root] = doc.get(root).cloned().unwrap_or_else(|| serde_json::json!({}));
        }
        doc["variables"] = variables;
        doc
    }

    /// The whole patch as its `.gfi` manifest, every narrow variable's value in it; a wide array
    /// is the file [`Self::persist_variables`] writes.
    pub fn serialize(&self) -> String {
        self.archive().to_yaml()
    }

    /// The patch as its `.gfi` manifest, before encoding: what a caller takes under the graph lock
    /// and encodes after releasing it.
    pub fn archive(&self) -> doc::Archive {
        let mut patch = self.document();
        let variables = self.variables();
        // An ephemeral variable is goofi's own to say; writing it into a patch would carry one
        // machine's answer onto another. Goofi's own groups' locks are re-asserted on load likewise.
        patch.variables.retain(|name, _| !variables.is_ephemeral(name));
        for (name, record) in &mut patch.variables {
            record.value = variables.get(name).filter(|v| !is_wide(v)).cloned();
        }
        // A device's group is the session's likewise, and a playhead's is its machine's to re-derive.
        patch.variable_groups.retain(|g, rec| !goofi_core::variables::is_own_group(g) && !rec.owned());
        drop_held(&mut patch, |hex, _| self.manifest(Uid::from_hex(hex)?));
        doc::Archive {
            version: doc::MANIFEST_VERSION,
            goofi: env!("CARGO_PKG_VERSION").to_string(),
            patch,
            viewpoint: (!self.viewpoint.is_null()).then(|| self.viewpoint.clone()),
        }
    }

    /// Replace the graph from a `.gfi` manifest, born into `workspace`. A rejected document leaves
    /// the graph as it was; what was dropped on the way in is answered as warnings.
    pub fn load_doc(&mut self, text: &str, workspace: &std::path::Path) -> Result<Vec<String>, String> {
        let archive = doc::Archive::parse(text)?;
        let (mut doc, mut warnings) = self.admit(archive.patch)?;
        let mut types = HashMap::new();
        drop_held(&mut doc, |_, record| *types.entry(record.type_id.clone()).or_insert_with(|| self.owner_of(&record.type_id).map(|(_, m)| m)));

        self.clear();
        // The outgoing nodes retire NOW, into the workspace being replaced: an engine saves a
        // node's state where it is halted, and the archive's own must not be written over.
        self.halt_gone();
        self.set_workspace(workspace);
        // Variables load BEFORE nodes so a node's `variables.*` default-expression resolves at
        // instantiation, IN FILE ORDER. Malformed entries are skipped (best-effort load).
        for (name, v) in &doc.variables {
            // A value the manifest left out is the file beside it, or what the widget is born with.
            let value = match &v.value {
                Some(value) => value.clone(),
                None => match read_variable_file(workspace, name) {
                    Ok(value) => value,
                    Err(e) => {
                        warnings.push(format!("variable `{name}`: {e}"));
                        v.control.as_ref().map_or_else(|| Data::number(0.0), goofi_core::variables::Control::born_value)
                    }
                },
            };
            let mut variables = self.variables();
            let _ = variables.apply_change(name, Some(value), None, None);
            if let Some(c) = &v.control {
                let _ = variables.apply_change(name, None, None, Some(Some(c.clone())));
            }
            if let Some(e) = &v.expression {
                let _ = variables.set_expression(name, Some(e.clone()));
            }
            if let Some(l) = v.lock {
                let _ = variables.set_lock(name, l);
            }
        }
        for (group, g) in &doc.variable_groups {
            let _ = self.variables().set_group_lock(group, Some(g.lock));
        }
        // Machines after the variables and before the nodes: a playhead's variables are born here,
        // and a node's expression may read one at instantiation.
        for (name, m) in &doc.machines {
            if let Err(e) = self.set_machine(name, Some(m.clone()), None) {
                warnings.push(format!("machine `{name}` dropped: {e}"));
            }
        }
        // Every uid this load hands out, restored or minted — what keeps two records from landing
        // on one uid when a hand-written file spells the same number two ways.
        let mut claimed: HashSet<Uid> = HashSet::new();
        let mut idmap: HashMap<String, Uid> = HashMap::new();
        // Every uid FIRST, so a record's `scope` and a link's endpoints resolve whatever the
        // iteration order — one uid space, so one pass answers for all three entity kinds.
        for old in doc.nodes.keys() {
            let uid = self.restore_uid(old, &claimed);
            claimed.insert(uid);
            idmap.insert(old.clone(), uid);
        }
        let parent_of = |rec: &doc::NodeRecord| rec.scope.as_deref().and_then(|s| idmap.get(s)).copied();
        let mut order: Vec<(&String, &doc::NodeRecord)> = doc.nodes.iter().collect();
        order.sort_by_key(|(_, rec)| kind_order(&rec.type_id));
        for (old, rec) in order {
            // A port with no scope is not one.
            let port = subpatch::boundary_type(&rec.type_id).is_some();
            let scope = parent_of(rec).filter(|s| port && self.is_facade(*s));
            if port && scope.is_none() {
                continue;
            }
            let params = (!structural(&rec.type_id)).then(|| self.record_params(rec)).transpose()?;
            let uid = self.create_node(&rec.type_id, Some(idmap[old]), &rec.name, params, scope)?;
            self.restore_extras(uid, rec.pos, &sources_of(rec), rec.viewers.clone(), rec.baseline.clone(), Some(rec.record.clone()));
        }
        // Membership, from each record's own `scope`, once every record that can hold one is in.
        for (old, rec) in &doc.nodes {
            if let Some(parent) = parent_of(rec) {
                self.set_member_scope(idmap[old], Some(parent));
            }
        }
        for l in doc.links.values() {
            let (Some(no), Some(ni)) = (idmap.get(&l.node_out).copied(), idmap.get(&l.node_in).copied()) else {
                continue;
            };
            // A link with one end on a port IS that port's inner wire — the same dispatch
            // `add_link` makes, so the file and the op vocabulary say one thing.
            if let Err(e) = self.add_link(no, &l.slot_out, ni, &l.slot_in) {
                warnings.push(format!("link {}.{} > {}.{} dropped: {e}", l.node_out, l.slot_out, l.node_in, l.slot_in));
            }
        }
        self.viewpoint = archive.viewpoint.unwrap_or(serde_json::Value::Null);
        // A corrupt arrangement costs the CHROME, never the patch. The reason is kept for the load
        // reply; an ABSENT arrangement is not a corrupt one and warns about nothing.
        self.patch.arrangement = Arc::new(match doc.arrangement.as_ref().map(layout::Layout::from_json) {
            None => layout::Layout::default(),
            Some(Ok(l)) => l,
            Some(Err(e)) => {
                warnings.push(e);
                layout::Layout::default()
            }
        });
        Ok(warnings)
    }
}

/// One engine's scan outcomes under the `engine:Name` ids the rest of the graph keys them by.
fn qualified(engine: &'static str, scanned: Vec<goofi_node::ScannedType>) -> Vec<goofi_node::ScannedType> {
    scanned
        .into_iter()
        .map(|t| goofi_node::ScannedType { type_name: goofi_node::qualify(engine, &t.type_name), ..t })
        .collect()
}

/// The literals of a typed record — what the patch keeps of it; a pulse has none.
fn values_of(params: &ParamGroups) -> doc::Values {
    params
        .iter()
        .map(|(group, names)| (group.clone(), names.iter().filter_map(|(n, p)| Some((n.clone(), control::data_of(p)?))).collect()))
        .collect()
}

/// A facade before the members that name it, and a port after every facade: a port is a port OF
/// a scope, so it takes its scope at birth where everything else is placed after.
fn kind_order(ty: &str) -> u8 {
    match (ty == subpatch::SCOPE_TYPE, subpatch::boundary_type(ty)) {
        (true, _) => 0,
        (_, Some(_)) => 2,
        _ => 1,
    }
}

/// Is this a type the MODEL owns rather than the palette — a sub-patch facade or a boundary port?
fn structural(ty: &str) -> bool {
    ty == subpatch::SCOPE_TYPE || subpatch::boundary_type(ty).is_some()
}

/// The stem a minted display name counts from: a leaf's type, or the kind's own word. Legal by
/// construction: the graph never mints a name its own rule refuses.
pub fn name_base(type_name: &str) -> String {
    let base: String = match subpatch::boundary_type(type_name) {
        Some((dir, _)) => dir.name().to_string(),
        None if type_name == subpatch::SCOPE_TYPE => "subpatch".to_string(),
        None => type_name.to_lowercase().chars().filter(char::is_ascii_alphanumeric).collect(),
    };
    match base.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        true => base,
        false => format!("node{base}"),
    }
}

/// A viewer blob under the uids a paste minted. A facade keys its blob by PORT UID, which must
/// follow the copy; a leaf keys its by slot NAME, which rides through unchanged.
fn remap_slots(viewers: &serde_json::Value, idmap: &HashMap<String, Uid>) -> serde_json::Value {
    match viewers.as_object() {
        None => viewers.clone(),
        Some(m) => serde_json::Value::Object(
            m.iter()
                .map(|(k, v)| (idmap.get(k).map_or_else(|| k.clone(), |u| u.to_hex()), v.clone()))
                .collect(),
        ),
    }
}

/// One link as the document carries it, under its key.
fn link_record(l: &Link) -> (String, doc::Link) {
    let link = doc::Link {
        node_out: l.node_out.to_hex(),
        slot_out: l.slot_out.to_string(),
        node_in: l.node_in.to_hex(),
        slot_in: l.slot_in.to_string(),
    };
    (link.key(), link)
}

/// What the document is told after a batch: the records that moved, serialized, and the roots that
/// moved, whole. A node with no record was removed.
#[derive(Default)]
pub struct Projection {
    pub nodes: Vec<(String, Option<serde_json::Value>)>,
    pub roots: Vec<(&'static str, serde_json::Value)>,
}

/// The source records a node record carries, in the shape a paste re-applies.
/// A group's ELEMENT sources as entries of their own, `name[i]` with no literal (the literal is
/// the whole param's list), in name order so a written patch is the same patch twice.
fn element_sources(leaf: &Leaf, group: &str, names: &IndexMap<String, Param>) -> Vec<(String, doc::ParamEntry)> {
    let mut out: Vec<(String, doc::ParamEntry)> = leaf
        .sources
        .iter()
        .filter(|(key, _)| {
            let (base, element) = goofi_node::element(&key.name);
            key.group == group && element.is_some() && names.contains_key(base)
        })
        .map(|(key, s)| (key.name.clone(), doc::ParamEntry::default().with_source(s)))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn sources_of(rec: &doc::NodeRecord) -> Vec<(String, String, SourceState)> {
    rec.params
        .iter()
        .flat_map(|(group, names)| names.iter().map(move |(name, e)| (group, name, e)))
        .filter_map(|(group, name, e)| Some((group.clone(), name.clone(), e.source()?)))
        .collect()
}

/// The output `want` names by label, else the only one. `who` names the node in the errors, and
/// `bare` refuses a bare reference to several outputs.
fn pick_output<'a, T>(
    slots: &'a [T],
    label: fn(&T) -> &str,
    want: Option<&str>,
    who: &str,
    bare: impl FnOnce() -> String,
) -> Result<&'a T, String> {
    match (want, slots) {
        (Some(want), _) => slots.iter().find(|s| label(s) == want).ok_or_else(|| format!("{who} has no output `{want}`")),
        (None, [one]) => Ok(one),
        (None, []) => Err(format!("{who} has no outputs")),
        (None, _) => Err(bare()),
    }
}


/// The health plane's one mutator: apply one report off any engine's drain. A free function so
/// the drain can hold the engines and the node map apart.
fn apply_status_to(
    nodes: &IndexMap<Uid, Arc<NodeEntry>>,
    bindings: &HashMap<Uid, HashMap<ParamKey, Binding>>,
    instances: &mut HashMap<Uid, Instance>,
    refreshed: &mut Vec<(Uid, ParamKey)>,
    uid: Uid,
    status: Status,
) {
    // A report from an instance settle has since halted lands nowhere.
    let (Some(entry), Some(health)) = (nodes.get(&uid).and_then(|e| e.leaf()), instances.get_mut(&uid).map(|i| &mut i.health)) else { return };
    match status {
        Status::Stage { stage } => health.stage = stage.as_str(),
        Status::Ufreq { hz } => health.ufreq = Some(hz),
        // The options are the node's answer to a refresh (§8.5). They land in the health
        // OVERLAY, never a reply or the record: the RPC that asked has already returned.
        Status::RefreshOptions { key, options } => {
            if let Some(options) = options {
                health.options.insert(key.clone(), options);
            }
            // Queued whether or not there were any: this IS the answer to a ⟳, and the client
            // lifts its spinner off the echo. A node with no hook for the param answers `None`.
            refreshed.push((uid, key));
        }
        Status::Fault { fault } => match fault {
            // A clean run clears Setup/Process/Boot together and never touches a binding
            // error, which only that binding evaluating successfully clears (§6).
            None => {
                health.setup_error = None;
                health.last_error = None;
            }
            Some(goofi_node::NodeFault::Setup { msg, .. }) => health.setup_error = Some(msg),
            Some(goofi_node::NodeFault::Process { msg, .. }) => health.last_error = Some(msg),
        },
        // One record for what the instance reported, bound param or not. On the binding it would
        // outlive the instance, since a reborn node has nothing to announce clearing.
        Status::BindingErrors { errors } => {
            for (key, msg) in errors {
                match msg {
                    Some(msg) => {
                        health.param_errors.insert(key, msg);
                    }
                    None => {
                        health.param_errors.shift_remove(&key);
                    }
                }
            }
        }
        Status::ParamValues { evaluated } => {
            // A pulse holds no value: what an engine evaluated for one is its edge memory.
            let pulse = |key: &ParamKey| {
                entry.manifest.params.iter().any(|d| d.group == key.group && d.name == key.name && matches!(d.spec.to_param(), Param::Pulse))
            };
            health.evaluated = evaluated.into_iter().filter(|(key, _)| !pulse(key)).collect();
        }
    }
    // Stamp when the error first read the way it does now — re-stamped only when the message
    // changes, so the instant is its onset.
    let current = entry_error(bindings.get(&uid), Some(health)).map(str::to_string);
    if health.error_since.as_ref().map(|(m, _)| m.as_str()) != current.as_deref() {
        health.error_since = current.map(|m| (m, Instant::now()));
    }
}

/// The runtime's part of a settled view, borrowed apart from the engines the view is handed to.
struct Settled<'a> {
    bindings: &'a HashMap<Uid, HashMap<ParamKey, Binding>>,
    generations: &'a HashMap<Uid, u64>,
    watched: &'a HashSet<(Uid, String)>,
    instance: &'a str,
}

/// The settled view, borrowed from the one model — built at the settle point and nowhere else.
fn build_view<'a>(
    nodes: &'a IndexMap<Uid, Arc<NodeEntry>>,
    settled: &Settled<'a>,
    typed: &'a HashMap<Uid, Arc<ParamGroups>>,
    edges: &'a [Edge],
    variables: &'a goofi_core::variables::VariableStore,
) -> GraphView<'a> {
    let Settled { bindings, generations, watched, instance } = *settled;
    let nodes = nodes
        .iter()
        .filter_map(|(uid, e)| {
            let leaf = e.leaf()?;
            let bindings = bindings
                .get(uid)
                .into_iter()
                .flat_map(|m| m.iter())
                .map(|(key, b)| BindingView {
                    key,
                    rewritten: &b.rewritten,
                    vars: &b.vars,
                    id: b.id,
                    live: b.live(),
                })
                .collect();
            Some((
                *uid,
                NodeView {
                    engine: leaf.engine,
                    name: e.name.as_str(),
                    generation: generations.get(uid).copied().unwrap_or(0),
                    manifest: leaf.manifest,
                    params: &typed[uid],
                    bindings,
                    recorded: e.record.as_slice(),
                    watched: leaf.manifest.outputs.iter().filter(|o| watched.contains(&(*uid, o.name.to_string()))).map(|o| o.name).collect(),
                },
            ))
        })
        .collect();
    GraphView::new(instance, edges, nodes, variables)
}

/// One node's current error, derived fresh from the places one can arise. A free function so the
/// status drain can read it while holding the instances apart from the bindings.
fn entry_error<'a>(bindings: Option<&'a HashMap<ParamKey, Binding>>, health: Option<&'a Health>) -> Option<&'a str> {
    // Initialization failure outranks a process error, and is the only thing that CAN be true
    // beside one: if `setup` failed, `process` never ran.
    if let Some(err) = health.and_then(|h| h.setup_error.as_deref()) {
        return Some(err);
    }
    if let Some(err) = health.and_then(|h| h.last_error.as_deref()) {
        return Some(err);
    }
    // Both param-keyed error records, ordered by key together, so which record an error landed in
    // cannot decide whether the badge ever shows it.
    bindings
        .into_iter()
        .flat_map(|m| m.iter())
        .filter_map(|(k, b)| b.bind_error.as_deref().map(|s| (k, s)))
        .chain(health.into_iter().flat_map(|h| h.param_errors.iter().map(|(k, m)| (k, m.as_str()))))
        .min_by(|a, b| a.0.cmp(b.0))
        .map(|(_, s)| s)
}

/// A held value is the node's saved state's to say, so a patch neither writes nor reads one; its
/// source, where it has one, is goofi's and stays. `manifest_of` names a record's class.
fn drop_held(patch: &mut doc::PatchDoc, mut manifest_of: impl FnMut(&str, &doc::NodeRecord) -> Option<&'static NodeManifest>) {
    for (hex, record) in &mut patch.nodes {
        let Some(manifest) = manifest_of(hex, record) else { continue };
        for d in manifest.params.iter().filter(|d| d.role == Some(goofi_node::Role::Held)) {
            let Some(group) = record.params.get_mut(d.group) else { continue };
            if let Some(entry) = group.get_mut(d.name) {
                entry.value = None;
                if entry.mode.is_none() {
                    group.shift_remove(d.name);
                }
            }
            if group.is_empty() {
                record.params.shift_remove(d.group);
            }
        }
    }
}
