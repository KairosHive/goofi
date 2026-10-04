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
    Status, Touched,
};
use indexmap::IndexMap;

pub mod archive;
pub mod doc;
pub use doc::MANIFEST_VERSION;

pub mod subpatch;
pub mod layout;

pub mod command;
pub use command::{open_preview, Applied, Command, CommandHistory, Ctx, Flip, Outcome, PreviewScope, Skip, SourceState};

pub mod expr_rewrite;

pub use goofi_node::Uid;

use expr_rewrite::Target;
use goofi_core::variables::NAME_RULE;
use subpatch::Dir;

/// What a node IS. The thin distinction the backend keeps and the frontend never sees: a leaf runs,
/// so it carries a thread and params; a facade and a port do not, so they carry neither.
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
struct Leaf {
    manifest: &'static NodeManifest,
    /// The type's cell, captured at birth — shared per type, so a runtime demotion of a Python
    /// type reads through here too.
    isolation: &'static IsolationCell,
    /// The id of the engine whose library resolved this node's type — its runtime authority.
    engine: &'static str,
    /// The param RECORD — the literals `serialize` writes, every param of the class. An evaluated
    /// value must never reach it; the typed params with bounds derive from the class on read.
    values: doc::Values,
    /// The graph resolves each source's references and ships it; the NODE evaluates it.
    sources: HashMap<ParamKey, ParamSource>,
}

/// One running node: the engine it was born into and what it reports. Born and removed by
/// `settle` alone, so a batch's adds and removes net before any engine hears of them.
struct Instance {
    engine: &'static str,
    health: Health,
}

/// What a batch did to a node's instance and `settle` has not yet delivered to its engine.
#[derive(Clone, Copy, PartialEq)]
enum Change {
    Added(Uid),
    Removed(Uid),
    Restart(Uid),
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
    /// `ParamSource::bind_error` survives because the source does.
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

/// The one params bag, `{group: {param: value | {value, expression, reference, mode, triggers}}}`,
/// as one [`Command::EditParam`] per entry, typed against the params the node holds NOW.
pub fn param_commands(
    g: &Graph,
    uid: Uid,
    params: &serde_json::Value,
) -> Result<Vec<Command>, String> {
    let groups = params.as_object().ok_or("params is {group: {param: …}}")?;
    let declared = g.params(uid);
    let mut cmds = Vec::new();
    for (group, entries) in groups {
        let entries =
            entries.as_object().ok_or_else(|| format!("params.{group} is {{param: …}}"))?;
        for (name, spec) in entries {
            // `name[i]` is one dimension of a vector: it edits that element's literal or source.
            let existing = declared
                .as_ref()
                .and_then(|p| goofi_node::param_dim(p, group, name))
                .ok_or_else(|| format!("no param {group}.{name}"))?;
            let cur = g.param_source(uid, group, name).map(|s| s.state);
            let (value, source) = param_change(&existing, cur, spec)
                .map_err(|e| format!("params.{group}.{name}: {e}"))?;
            // A LIST of expressions on a vector is one expression per element: each element takes
            // its own, and the whole param steps back to its literal with the list retained.
            if let Some(items) = element_expressions(&existing, name, spec) {
                let mut element_spec = spec.clone();
                element_spec.as_object_mut().expect("a list expression spec").shift_remove("value");
                for (k, item) in items.iter().enumerate() {
                    let element = format!("{name}[{k}]");
                    let cur = g.param_source(uid, group, &element).map(|s| s.state);
                    element_spec["expression"] = serde_json::json!(item);
                    let (_, source) = param_change(&existing.dim(k), cur, &element_spec)
                        .map_err(|e| format!("params.{group}.{element}: {e}"))?;
                    cmds.push(Command::EditParam { uid, group: group.clone(), name: element, value: None, source });
                }
                let whole = SourceState { mode: Mode::Constant, ..source.expect("a list expression source") };
                cmds.push(Command::EditParam { uid, group: group.clone(), name: name.clone(), value, source: Some(whole) });
                continue;
            }
            if value.is_none() && source.is_none() {
                return Err(format!("params.{group}.{name} sets neither a value nor a source"));
            }
            cmds.push(Command::EditParam {
                uid,
                group: group.clone(),
                name: name.clone(),
                value,
                source,
            });
        }
    }
    Ok(cmds)
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
    if let Some(k) = o
        .keys()
        .find(|k| !matches!(k.as_str(), "value" | "expression" | "reference" | "mode" | "triggers"))
    {
        return Err(format!("unknown field `{k}` — value, expression, reference, mode, triggers"));
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
    let reference = text("reference")?;
    let mode = field("mode")
        .map(|v| {
            serde_json::from_value::<Mode>(v.clone())
                .ok()
                .ok_or_else(|| format!("mode is `constant`, `expression` or `reference`, not {v}"))
        })
        .transpose()?;
    let triggers = field("triggers")
        .map(|v| v.as_bool().ok_or_else(|| format!("triggers is a bool, not {v}")))
        .transpose()?;
    if expression.is_none() && reference.is_none() && mode.is_none() && triggers.is_none() {
        return Ok((value, None));
    }
    let cur = cur.unwrap_or_default();
    let given = |t: &Option<String>| t.as_deref().is_some_and(|s| !s.is_empty());
    let mode = match (mode, given(&expression), given(&reference)) {
        (Some(m), _, _) => m,
        (None, true, true) => {
            return Err("an expression and a reference at once: say which with `mode`".to_string())
        }
        (None, true, false) => Mode::Expression,
        (None, false, true) => Mode::Reference,
        // Clearing the active text is what switches it off.
        (None, false, false) => match cur.mode {
            Mode::Expression if expression.as_deref() == Some("") => Mode::Constant,
            Mode::Reference if reference.as_deref() == Some("") => Mode::Constant,
            m => m,
        },
    };
    let state = SourceState {
        mode,
        expression: expression.unwrap_or(cur.expression),
        reference: reference.unwrap_or(cur.reference),
        triggers: triggers.unwrap_or(cur.triggers),
    };
    match state.mode {
        Mode::Expression if state.expression.is_empty() => {
            return Err("mode `expression` with no expression to evaluate".to_string())
        }
        Mode::Reference if state.reference.is_empty() => {
            return Err("mode `reference` with no reference to follow".to_string())
        }
        _ => {}
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
    Reference,
}

/// The one variable a reference rewrites to: the bare-variable source the runtime copies without
/// an evaluator.
const REF_VAR: &str = "ref";

/// A param's source record: the AUTHORED state, both texts retained whatever the mode, and
/// everything below it DERIVED from the active one at settle.
struct ParamSource {
    state: SourceState,
    /// Compiled from [`Self::rewritten`], never from the authored text: the evaluator is handed
    /// variables, not names. `None` when the compile failed, or there is no evaluator.
    id: Option<goofi_node::BindingId>,
    /// Derived: the active text with every reference replaced by a generated variable.
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

impl ParamSource {
    /// Whether the graph SHIPS this source. A constant mode and a source the graph could not bind
    /// both leave the param on its literal, and the node is TOLD so.
    fn live(&self) -> bool {
        self.state.mode != Mode::Constant && self.bind_error.is_none()
    }
}

/// [`ParamSource`] projected for the bridge and the `.gfi`: the authored state, and why it does
/// not hold — the graph could not bind it, or the node could not evaluate it.
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

/// The document: what the archive, the replica and a paste fragment project.
struct Patch {
    nodes: IndexMap<Uid, NodeEntry>,
    /// Keyed by [`Link::key`], in connection order — which IS a multi input's wire order.
    links: IndexMap<String, Link>,
    /// The panel arrangement, held FLAT — the fifth doc root. Every mutation is an ordinary
    /// command, so the layout has exactly one projection, as nodes and links do.
    arrangement: layout::Layout,
    /// Patch-scoped variables, system ones seeded and re-asserted by every `clear`/load. Under a
    /// lock of its own: a value write is the store's and the plane's, never the graph's.
    variables: Arc<goofi_supervisor::sync::Mutex<goofi_core::variables::VariableStore>>,
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
    time: Arc<goofi_core::time::Time>,
    /// `None` ⇒ bindings are stored and round-trip but never evaluate; the literal stands.
    evaluator: Option<Arc<dyn goofi_node::ExprEvaluator>>,
    /// Types that exist on disk but cannot load here → why. Greyed in the palette, so a node
    /// needing an uninstalled dependency explains itself instead of silently not existing.
    unavailable: std::collections::BTreeMap<String, Greyed>,
    /// Where each scanned type came from — the one thing about a type that only the scan can
    /// know. Re-derived wholesale by each scan.
    origins: std::collections::HashMap<String, Origin>,
    /// A process-lifetime counter: undo restores deleted uids, so none is ever handed out twice.
    next_uid: u64,
    /// A `system.*` value moved: the next settle runs whatever else moved, so engines re-read it.
    resettle: bool,
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
    /// The births, removals and restarts the batch asked for, in order, netted by `settle`.
    changed: Vec<Change>,
}

pub struct Graph {
    patch: Patch,
    /// Where a client is LOOKING. Not a doc root — converging it would drag peers and dirty the
    /// patch on mere navigation — but persistence is the other axis, so it rides the `.gfi`.
    viewpoint: serde_json::Value,
    runtime: Runtime,
    /// What the current batch changed and [`Self::settle`] has not yet delivered.
    touched: Vec<Touched>,
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
    let (shape, samples) = goofi_record::npy::read(&file).map_err(|e| format!("{}: {e}", path.display()))?;
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
        Graph {
            patch: Patch {
                nodes: IndexMap::new(),
                links: IndexMap::new(),
                arrangement: layout::Layout::default(),
                variables: Arc::new(goofi_supervisor::sync::Mutex::new(goofi_core::variables::VariableStore::new())),
            },
            viewpoint: serde_json::Value::Null,
            runtime: Runtime {
                engines: Vec::new(),
                waker: Arc::new(DrainWaker::default()),
                instance,
                epoch: Arc::new(std::sync::atomic::AtomicU64::new(0)),
                time: Arc::new(goofi_core::time::Time::new()),
                evaluator: None,
                unavailable: std::collections::BTreeMap::new(),
                origins: std::collections::HashMap::new(),
                next_uid: 1,
                resettle: false,
                generations: HashMap::new(),
                arm_serial: 0,
                refreshed: Vec::new(),
                watched: HashSet::new(),
                view_wants: HashMap::new(),
                instances: HashMap::new(),
                changed: Vec::new(),
            },
            touched: Vec::new(),
        }
    }

    /// Stop every node and WAIT for each to release its shared memory. The waiting is why this is
    /// not what `clear()` does: only a process about to EXIT has no "a moment later".
    pub fn shutdown(&mut self) {
        for e in self.engines_mut() {
            e.shutdown();
        }
        self.patch.nodes.clear();
        self.runtime.instances.clear();
        self.runtime.changed.clear();
        self.touched.clear();
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
            for b in leaf.sources.values().filter(|b| b.live()) {
                for v in &b.vars {
                    if let BoundVar::Stream { producer: Uid::VARIABLES, slot, event_id, .. } = v {
                        out.push((slot.to_string(), *uid, self.node_generation(*uid), *event_id));
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
        // An engine reads `system.*` off the settled view, so a value there is delivered by a settle.
        if name.starts_with("system.") {
            self.runtime.resettle = true;
        }
        Ok(())
    }

    /// Set or clear what a variable follows, answering what it followed. The reference is held to
    /// the spelling a param's is; what it names is resolved by the follower, as nodes come and go.
    pub fn set_variable_source(
        &mut self,
        name: &str,
        source: Option<goofi_core::variables::VariableSource>,
    ) -> Result<Option<goofi_core::variables::VariableSource>, String> {
        if let Some(s) = &source {
            parse_reference(&s.reference)?;
        }
        self.variables().set_source(name, source)
    }

    /// Every followed variable, resolved: the variable, the producer's uid and slot, and the index.
    /// A source that does not resolve is left out and carries its error instead.
    pub fn variable_sources(&self) -> Vec<(String, Uid, String, Option<usize>)> {
        let followed: Vec<(String, goofi_core::variables::VariableSource)> =
            self.variables().entries().filter_map(|(n, v)| Some((n.to_string(), v.source.clone()?))).collect();
        followed
            .into_iter()
            .filter_map(|(name, s)| {
                let (uid, slot) = self.resolve_variable_source(&s).ok()?;
                Some((name, uid, slot.to_string(), s.index))
            })
            .collect()
    }

    /// Why a variable's source delivers nothing: the node or the output it names is not there.
    /// The one derivation, so the projection and the follower cannot answer differently.
    pub fn variable_source_error(&self, source: &goofi_core::variables::VariableSource) -> Option<String> {
        self.resolve_variable_source(source).err()
    }

    fn resolve_variable_source(&self, source: &goofi_core::variables::VariableSource) -> Result<(Uid, &'static str), String> {
        let (node, slot) = source.reference.split_once('.').ok_or("a source is `node.slot`")?;
        self.resolve_stream(node, Some(slot))
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

    /// Remove an empty group that no panel uses.
    pub fn remove_variable_group(&mut self, group: &str) -> Result<(), String> {
        if self.patch.arrangement.control_panels().iter().any(|(_, held)| held == group) {
            return Err(format!("variable group `{group}` is used by a control panel"));
        }
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
        let writes = self.patch.arrangement.regroup(from, to);
        if writes.is_empty() && !self.variables().has_group(from) {
            return Err(format!("no variable group `{from}`"));
        }
        let moved = self.variables().rename_group(from, to)?;
        let touched = self.rewrite_variable_reads(&moved);
        self.patch.arrangement.set_contents(&writes);
        Ok(touched)
    }

    /// Follow a set of variable renames into every expression that spells one, answering the nodes
    /// whose source text changed.
    fn rewrite_variable_reads(&mut self, moved: &[(String, String)]) -> Vec<Uid> {
        let rename = |name: &str| moved.iter().find(|(old, _)| old == name).map(|(_, new)| new.clone());
        self.rewrite_sources(|s| {
            Some(SourceState { expression: expr_rewrite::rename_variables(&s.expression, rename)?, ..s.clone() })
        })
    }

    /// Re-set every source `edit` rewrites, answering the distinct nodes whose source changed.
    fn rewrite_sources(&mut self, edit: impl Fn(&SourceState) -> Option<SourceState>) -> Vec<Uid> {
        let edit = &edit;
        let edits: Vec<(Uid, ParamKey, SourceState)> = self
            .leaves()
            .flat_map(|(uid, l)| l.sources.iter().filter_map(move |(k, b)| Some((uid, k.clone(), edit(&b.state)?))))
            .collect();
        let mut referrers: Vec<Uid> = Vec::new();
        for (uid, key, state) in edits {
            if self.set_source(uid, &key.group, &key.name, state).is_ok() && !referrers.contains(&uid) {
                referrers.push(uid);
            }
        }
        referrers
    }

    /// Derive every binding from its authored texts against the graph as it stands, and touch the
    /// ones that moved, so their nodes are re-sent. Runs at settle.
    fn derive_bindings(&mut self) {
        let mut keys: Vec<(Uid, ParamKey)> =
            self.leaves().flat_map(|(uid, e)| e.sources.keys().map(move |k| (uid, k.clone()))).collect();
        keys.sort();
        for (uid, key) in keys {
            self.derive_one(uid, &key);
        }
    }

    /// Derive one record from its authored texts, and touch it when the derivation moved.
    fn derive_one(&mut self, uid: Uid, key: &ParamKey) {
        let Some(prior) = self.leaf(uid).and_then(|e| e.sources.get(key)) else { return };
        let state = prior.state.clone();
        let derived = self.derive(uid, key, &state);
        let edited: Vec<(Uid, ParamKey)> = self
            .touched
            .iter()
            .filter_map(|t| match t {
                Touched::Param(uid, key) => Some((*uid, key.clone())),
                _ => None,
            })
            .collect();
        if !self.moved(uid, prior, &derived, &edited) {
            return;
        }
        let (id, error) = self.compiled(prior, &derived);
        let record = ParamSource {
            state,
            id,
            rewritten: derived.rewritten,
            vars: derived.vars,
            refs: derived.refs,
            bind_error: error,
        };
        if let Some(e) = self.leaf_mut(uid) {
            e.sources.insert(key.clone(), record);
        }
        self.notify_param(uid, key);
    }

    /// Whether a derivation says something the record does not. A param read follows an authored
    /// edit of that param, never its evaluation, or driven params would cascade through every settle.
    fn moved(&self, consumer: Uid, prior: &ParamSource, next: &Derived, edited: &[(Uid, ParamKey)]) -> bool {
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
    /// compiled, else a fresh compile, and the prior released. A refused compile is the error.
    fn compiled(&self, prior: &ParamSource, next: &Derived) -> (Option<goofi_node::BindingId>, Option<String>) {
        let evaluator = self.runtime.evaluator.as_ref();
        if prior.state.mode == Mode::Expression && next.error.is_none() {
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
        self.leaf(uid).and_then(|e| e.sources.get(key)).map(|s| s.state.clone())
    }

    /// Inject the param-expression evaluator (pyo3, from goofi-python). Wired by the CLI at
    /// startup; without it, expression bindings are stored but not evaluated.
    pub fn set_evaluator(&mut self, evaluator: Arc<dyn goofi_node::ExprEvaluator>) {
        self.runtime.evaluator = Some(evaluator.clone());
        for e in self.engines_mut() {
            e.set_evaluator(evaluator.clone());
        }
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
        let name = self.leaf(uid).and_then(|l| l.manifest.outputs.iter().find(|o| o.name == slot)).map(|o| o.name);
        // A node removed since the caller looked is never marked watched: its removal ran past.
        let Some(name) = name else { return !on && self.runtime.watched.remove(&key) };
        let changed = if on { self.runtime.watched.insert(key) } else { self.runtime.watched.remove(&key) };
        if changed {
            self.touched.push(Touched::Watch(uid, name));
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
        let wide: Vec<(String, Data)> = self
            .variables()
            .entries()
            .filter(|(_, v)| is_wide(&v.value))
            .map(|(n, v)| (n.to_string(), v.value.clone()))
            .collect();
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
            let (path, bytes) = (variable_file(workspace, &name), goofi_record::npy::bytes(a.shape(), a.as_bytes()));
            // An unchanged file is left alone: a write is an edit to the watcher and the baseline.
            if std::fs::read(&path).is_ok_and(|held| held == bytes) {
                continue;
            }
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(())
    }

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
    /// a command, so undo/redo and the CRDT mirror come for free.
    pub fn arrangement(&self) -> &layout::Layout {
        &self.patch.arrangement
    }

    /// The one write door, held by [`command::Command::EditLayoutEntry`] and by a load.
    pub fn arrangement_mut(&mut self) -> &mut layout::Layout {
        &mut self.patch.arrangement
    }

    /// The client-local viewpoint blob (see the field).
    pub fn viewpoint(&self) -> &serde_json::Value {
        &self.viewpoint
    }

    pub fn set_viewpoint(&mut self, viewpoint: serde_json::Value) {
        self.viewpoint = viewpoint;
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
        self.patch.nodes.get_mut(&uid)?.leaf_mut()
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
        self.subtree_of(&[uid]).into_iter().find_map(|u| entry_error(self.leaf(u)?, self.health(u)))
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
        self.patch.nodes.insert(uid, NodeEntry::new(kind, born, [0.0, 0.0]));
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
                    reference: String::new(),
                    triggers: e.trigger,
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
        self.runtime.changed.push(Change::Added(uid));
        let leaf = Leaf { manifest: entry.manifest, isolation: entry.isolation, engine, values, sources: HashMap::new() };
        self.patch.nodes.insert(uid, NodeEntry::new(Kind::Leaf(Box::new(leaf)), name, [0.0, 0.0]));
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
    pub fn params(&self, uid: Uid) -> Option<ParamGroups> {
        self.leaf(uid).map(|l| self.typed(l))
    }

    /// The literals a leaf holds, as the document spells them.
    pub fn values(&self, uid: Uid) -> Option<doc::Values> {
        self.leaf(uid).map(|l| l.values.clone())
    }

    /// A leaf's params as its class declares them NOW, with the record's literals folded in:
    /// the one derivation of a typed param, so no record can carry a stale bound or option.
    fn typed(&self, leaf: &Leaf) -> ParamGroups {
        self.folded(&goofi_node::qualify(leaf.engine, leaf.manifest.type_name), &leaf.values).unwrap_or_default()
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
        let e = self.patch.nodes.get_mut(&uid).ok_or_else(|| format!("no such node {uid}"))?;
        let old_name = std::mem::replace(&mut e.name, name.to_string());
        // `name_taken` guarantees `name != old_name`, so the rename moved the
        // display name — propagate it into every expression that referenced it.
        let touched = self.rewrite_nd_refs_for_rename(uid, &old_name, name);
        // A multi slot names its senders, so the new name must reach every leaf this node feeds —
        // the resolved consumers, because a port relays and no engine plans a port.
        let fed: Vec<(Uid, &'static str)> =
            self.resolved_edges().into_iter().filter(|e| e.producer.0 == uid).map(|e| e.consumer).collect();
        for (node_in, slot_in) in fed {
            self.touched.push(Touched::Slot(node_in, slot_in));
        }
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
        let referrers = self.rewrite_sources(|s| {
            let expression = expr_rewrite::rename_refs(&s.expression, rename);
            let reference = expr_rewrite::rename_reference(&s.reference, rename);
            (expression.is_some() || reference.is_some()).then(|| SourceState {
                expression: expression.unwrap_or_else(|| s.expression.clone()),
                reference: reference.unwrap_or_else(|| s.reference.clone()),
                ..s.clone()
            })
        });
        // A variable follows a producer by the same spelling, so the one rename reaches it too.
        let followed: Vec<(String, goofi_core::variables::VariableSource)> = self
            .variables()
            .entries()
            .filter_map(|(name, v)| {
                let s = v.source.as_ref()?;
                let reference = expr_rewrite::rename_reference(&s.reference, rename)?;
                Some((name.to_string(), goofi_core::variables::VariableSource { reference, index: s.index }))
            })
            .collect();
        for (name, source) in followed {
            let _ = self.variables().set_source(&name, Some(source));
        }
        referrers
    }

    pub fn set_node_pos(&mut self, uid: Uid, pos: [f64; 2]) -> Result<(), String> {
        let e = self.patch.nodes.get_mut(&uid).ok_or_else(|| format!("no such node {uid}"))?;
        e.pos = pos;
        Ok(())
    }

    /// Replace a node's opaque viewer view-state blob (persisted to `.gfi`, echoed in node
    /// info). The backend never interprets it — it is the editor's per-slot kind/settings.
    pub fn set_node_viewers(&mut self, uid: Uid, viewers: serde_json::Value) -> Result<(), String> {
        let e = self.patch.nodes.get_mut(&uid).ok_or_else(|| format!("no such node {uid}"))?;
        e.viewers = viewers;
        Ok(())
    }

    /// The viewer view-state blob of anything a uid can name (empty object if never set).
    pub fn viewers(&self, uid: Uid) -> Option<&serde_json::Value> {
        self.patch.nodes.get(&uid).map(|e| &e.viewers)
    }

    /// Replace the values the touched filter counts from, WHOLE — the whole blob, which is what
    /// makes the command's inverse exact.
    pub fn set_node_baseline(&mut self, uid: Uid, baseline: serde_json::Value) -> Result<(), String> {
        let e = self.patch.nodes.get_mut(&uid).ok_or_else(|| format!("no such node {uid}"))?;
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
        for (group, names) in &self.typed(leaf) {
            for (name, p) in names {
                if matches!(p, Param::Pulse) {
                    continue;
                }
                let source = leaf.sources.iter().find(|(k, _)| k.group == *group && k.name == *name).map(|(_, b)| b);
                out.insert(
                    format!("{group}/{name}"),
                    serde_json::json!({
                        "value": param_value_json(p),
                        "mode": source.map(|b| b.state.mode).unwrap_or_default(),
                        "expression": source.map(|b| b.state.expression.clone()).unwrap_or_default(),
                        "reference": source.map(|b| b.state.reference.clone()).unwrap_or_default(),
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

    /// Replace the output slots armed for recording. The whole vector, which is what makes the
    /// command's inverse exact.
    pub fn set_recorded(&mut self, uid: Uid, mut record: Vec<RecordedOutput>) -> Result<(), String> {
        let e = self.patch.nodes.get_mut(&uid).ok_or_else(|| format!("no such node {uid}"))?;
        // A slot still armed keeps its serial; every other arming is a new one, minted at settle,
        // an undo's re-arm included: its old service name is one the recorder may have let go of.
        for output in &mut record {
            output.serial = e.record.iter().find(|held| held.slot == output.slot).map_or(0, |held| held.serial);
        }
        e.record = record;
        self.touched.push(Touched::Record(uid));
        Ok(())
    }

    /// Every arming the batch left unnumbered gets the next serial: one per slot however often
    /// the batch armed it.
    fn mint_serials(&mut self, uid: Uid) {
        let Some(e) = self.patch.nodes.get_mut(&uid) else { return };
        for output in e.record.iter_mut().filter(|o| o.serial == 0) {
            self.runtime.arm_serial += 1;
            output.serial = self.runtime.arm_serial;
        }
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

    /// Every real leaf INPUT an output address reaches, walking forward through any chain of ports.
    /// A port relays, so what a producer really feeds is whatever sits past it — and it may fan out.
    fn sinks(&self, node: Uid, slot: &'static str) -> Vec<(Uid, &'static str)> {
        let (mut out, mut seen) = (Vec::new(), Vec::new());
        let mut stack = vec![(node, slot)];
        while let Some((n, s)) = stack.pop() {
            for l in self.patch.links.values().filter(|l| l.node_out == n && l.slot_out == s) {
                match self.stub(l.node_in).is_some() {
                    // A hand-edited `.gfi` can persist a cyclic chain; walking it must stop.
                    true if !seen.contains(&l.node_in) => {
                        seen.push(l.node_in);
                        stack.push((l.node_in, l.slot_in));
                    }
                    true => {}
                    false => out.push((l.node_in, l.slot_in)),
                }
            }
        }
        out
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
        if let Some(e) = self.patch.nodes.get_mut(&member) {
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
    /// a nested scope's existing stub, or a freshly MINTED chain of ports, each recorded in `minted`.
    fn expose_in_nested_member(
        &mut self,
        member: Uid,
        leaf: Uid,
        slot: &str,
        dir: subpatch::Dir,
        minted: &mut Vec<(Uid, Uid)>,
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
            let child_stub = self.expose_in_nested_member(child, leaf, slot, dir, minted);
            (child, child_stub)
        };
        let at = self.pos(member).unwrap_or([0.0, 0.0]);
        let Ok(id) = self.mint_port(member, dir, leaf, slot, at) else {
            return slot.to_string();
        };
        minted.push((member, id));
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

    /// Group `members` into a new scope at `pos`, recording into `minted` every port it MINTS on a
    /// pre-existing nested member, so the `Group` inverse can un-mint them.
    pub fn group_nodes_capturing(
        &mut self,
        members: &[Uid],
        pos: [f64; 2],
        minted: &mut Vec<(Uid, Uid)>,
    ) -> Result<Uid, String> {
        // 1. Validate BEFORE any mutation: each exists, and all share one parent scope.
        let parent = self.common_parent(members)?;
        let member_set: std::collections::HashSet<Uid> = members.iter().copied().collect();
        let scope_uid = self.mint();
        // Registered BEFORE its ports are minted, so each port's fresh name sees the ones before it
        // and `nd()` can tell two ports apart.
        let disp = goofi_core::fresh_name("subpatch", 0, |name| self.name_taken(name, None));
        self.patch.nodes.insert(scope_uid, NodeEntry::new(Kind::Facade, disp, pos));
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
                    let inner_slot = self.expose_in_nested_member(member, end, at_slot, dir, minted);
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
        self.patch.links.retain(|_, l| !cut.contains(l));

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

    /// Recreate a scope EXACTLY — the inverse of `expand_instance`, moving the members back under
    /// `scope_id` with the captured stubs verbatim, so undo/redo is uid-stable.
    pub fn restore_scope(
        &mut self,
        scope_id: Uid,
        name: String,
        pos: [f64; 2],
        members: &[Uid],
        parent: Option<Uid>,
    ) -> Result<Uid, String> {
        if self.is_facade(scope_id) {
            return Err(format!("restore_scope: scope {scope_id} already live"));
        }
        // The parent is captured, not derived from members, so an EMPTY scope restores. An empty
        // name mints one, which lets a COPY land beside its original.
        let name = self.pick_name(&name, "subpatch", Some(scope_id));
        self.patch.nodes.insert(scope_id, NodeEntry::new(Kind::Facade, name, pos));
        for &m in members {
            if self.patch.nodes.contains_key(&m) {
                self.set_member_scope(m, Some(scope_id));
            }
        }
        // A peer may have dissolved the captured parent since. Writing it verbatim would install a
        // dangling-parent orphan, so degrade to ROOT, as the `SetScope` child does.
        self.set_member_scope(scope_id, parent.filter(|p| self.is_facade(*p)));
        Ok(scope_id)
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
        self.patch.links.retain(|_, l| !ports.contains(&l.node_in) && !ports.contains(&l.node_out));

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
        Some((p, e.name, e.pos))
    }

    /// All links as resolved views (snapshot projection).
    pub fn links_view(&self) -> Vec<Link> {
        self.patch.links.values().copied().collect()
    }

    /// Release every compiled expression handle a node entry holds, so the evaluator's
    /// registry doesn't leak across a node/graph teardown.
    fn release_entry_bindings(&self, entry: &NodeEntry) {
        if let (Some(ev), Some(leaf)) = (&self.runtime.evaluator, entry.leaf()) {
            for b in leaf.sources.values() {
                if let Some(id) = b.id {
                    ev.release(id);
                }
            }
        }
    }

    pub fn remove_node(&mut self, uid: Uid) -> Result<(), String> {
        let Some(removed) = self.patch.nodes.shift_remove(&uid) else {
            return Err(format!("no such node {uid}"));
        };
        self.runtime.watched.retain(|(u, _)| *u != uid);
        self.runtime.view_wants.retain(|(u, _), _| *u != uid);
        self.release_entry_bindings(&removed);
        if removed.leaf().is_some() {
            self.runtime.changed.push(Change::Removed(uid));
        }
        // Drop links touching the node, then re-plan every consumer slot one of them fed. Links
        // INTO it need none: its thread is halted and its services are going with it.
        let dropped: Vec<Link> = self
            .patch
            .links
            .values()
            .filter(|l| l.node_out == uid || l.node_in == uid)
            .copied()
            .collect();
        self.patch.links
            .retain(|_, l| l.node_out != uid && l.node_in != uid);
        for l in dropped.iter().filter(|l| l.node_in != uid) {
            self.touched.push(Touched::Slot(l.node_in, l.slot_in));
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
        self.runtime.changed.push(Change::Restart(uid));
        let entry = self.leaf_mut(uid).expect("looked up above");
        // The MANIFEST goes with the instance: keeping the old one leaves the graph describing a
        // node not running.
        entry.manifest = lib.manifest;
        entry.isolation = lib.isolation;
        entry.engine = engine;
        entry.values.retain(|group, names| {
            names.retain(|name, _| declared.get(group).is_some_and(|g| g.contains_key(name)));
            !names.is_empty()
        });
        for (group, names) in &declared {
            let held = entry.values.entry(group.clone()).or_default();
            for (name, p) in names {
                if let (false, Some(v)) = (held.contains_key(name), control::data_of(p)) {
                    held.insert(name.clone(), v);
                }
            }
        }
        // The rebirth renamed the node's door, so every subscription onto it is re-planned.
        let (keys, retired): (Vec<ParamKey>, Vec<ParamKey>) = entry.sources.keys().cloned()
            .partition(|key| goofi_node::param_dim(&declared, &key.group, &key.name).is_some());
        for key in retired {
            self.unbind(uid, &key);
        }
        for slot in lib.manifest.inputs {
            self.touched.push(Touched::Slot(uid, slot.name));
        }
        for key in keys {
            self.touched.push(Touched::Param(uid, key));
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
            leaf.values.entry(group.to_string()).or_default().insert(base.to_string(), v);
        }
        // A LITERAL on a driven param switches it to constant, which is what the node does with
        // this write's `SetParam`; what the record retained stays retained.
        let key = ParamKey::new(group, name);
        if let Some(mut state) = self.source_state(uid, &key).filter(|s| s.mode != Mode::Constant) {
            state.mode = Mode::Constant;
            let _ = self.set_source(uid, group, name, state);
        }
        // The record has moved and the delivery is recorded for settle; nothing else happens
        // here. `on_param_changed` runs on the node's own thread, so its failure arrives as a fault.
        self.notify_param(uid, &key);
        Ok(())
    }

    /// One request to a node's own thread: a refresh re-enumerates a `Str`'s options, a pulse fires.
    /// Not a command: a request holds no state, so there is nothing to undo.
    pub fn request(&mut self, uid: Uid, group: &str, name: &str, kind: goofi_node::RequestKind) -> Result<(), String> {
        let entry = self.leaf(uid).ok_or_else(|| format!("no such node {uid}"))?;
        let typed = self.typed(entry);
        let param = goofi_node::param(&typed, group, name)
            .ok_or_else(|| format!("no such param `{group}.{name}`"))?;
        match kind {
            goofi_node::RequestKind::Refresh if !matches!(param, Param::Str { refresh: true, .. }) => {
                return Err(format!("param `{group}.{name}` is not refreshable"));
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
            self.notify_param(uid, &key);
            return Ok(());
        }
        // A record binds a real param: a dangling one is invisible in the descriptor and
        // unclearable from the UI.
        if goofi_node::param_dim(&self.typed(leaf), group, name).is_none() {
            return Err(format!("no such param `{group}/{name}`"));
        }
        if let Some(e) = self.leaf_mut(uid) {
            match e.sources.get_mut(&key) {
                Some(b) => b.state = state,
                None => {
                    e.sources.insert(
                        key.clone(),
                        ParamSource { state, id: None, rewritten: String::new(), vars: Vec::new(), refs: Vec::new(), bind_error: None },
                    );
                }
            }
        }
        self.derive_one(uid, &key);
        self.notify_param(uid, &key);
        Ok(())
    }

    /// What a source's active text derives to against the graph: the rewrite, its variables
    /// resolved, and why it cannot bind.
    fn derive(&self, uid: Uid, key: &ParamKey, state: &SourceState) -> Derived {
        let param = self.leaf(uid).and_then(|e| goofi_node::param_dim(&self.typed(e), &key.group, &key.name));
        let scanned = (!state.expression.is_empty()).then(|| expr_rewrite::rewrite(&state.expression));
        let reference = (!state.reference.is_empty()).then(|| goofi_node::mailbox::split_index(&state.reference)
            .and_then(|(base, index)| parse_reference(base).map(|r| (r, index))));
        let missing = |vars: &[BoundVar]| {
            vars.iter().find_map(|v| match v {
                BoundVar::Missing { reason, .. } => Some(reason.clone()),
                _ => None,
            })
        };
        let none = |error: Option<String>| Derived { rewritten: String::new(), vars: Vec::new(), refs: Vec::new(), error };
        let Some(param) = param else {
            return none(Some(format!("no such param `{}/{}`", key.group, key.name)));
        };
        match state.mode {
            Mode::Constant => none(None),
            Mode::Expression => match scanned {
                Some(Ok((rewritten, refs))) => {
                    let vars = self.resolve_vars(uid, key, &refs);
                    let error = missing(&vars);
                    Derived { rewritten, vars, refs, error }
                }
                Some(Err(e)) => Derived { rewritten: state.expression.clone(), vars: Vec::new(), refs: Vec::new(), error: Some(e.0) },
                None => none(Some("no expression to evaluate".to_string())),
            },
            Mode::Reference => match reference {
                Some(Ok((r, index))) => {
                    let refs = vec![r];
                    let vars = self.resolve_vars(uid, key, &refs);
                    let error = missing(&vars).or_else(|| self.reference_kind_error(uid, &refs[0], &param));
                    let rewritten = index.map_or_else(|| REF_VAR.to_string(), |i| format!("{REF_VAR}[{i}]"));
                    Derived { rewritten, vars, refs, error }
                }
                Some(Err(e)) => none(Some(e)),
                None => none(Some("no reference to follow".to_string())),
            },
        }
    }

    /// Why a resolved reference cannot feed the param on `reader`: the producer's slot kind
    /// against the param's type. An engine-local kind drives a param on its own plane as a plan
    /// edge (audio rate), so that pairing passes too. `None` when they agree, or when the
    /// producer was not found (already reported).
    fn reference_kind_error(&self, reader: Uid, r: &expr_rewrite::VarRef, param: &Param) -> Option<String> {
        let Target::Node { name, slot: Some(slot) } = &r.target else { return None };
        let uid = self.uid_by_name(name)?;
        let kind = self.slots(uid, Dir::Out).into_iter().find(|(_, label, _)| label == slot)?.2;
        let wants = match param {
            Param::Str { .. } => goofi_core::SlotType::String,
            _ => goofi_core::SlotType::Array,
        };
        let same_plane = self.leaf(reader).map(|e| e.engine) == self.leaf(uid).map(|e| e.engine);
        let plan_edge = kind.engine_local().is_some() && wants == goofi_core::SlotType::Array && same_plane;
        (!kind.feeds(wants) && !plan_edge).then(|| {
            format!("`{name}.{slot}` is a {} output; this param references a {} one", kind.name(), wants.name())
        })
    }

    /// Drop a source and release its compiled handle — the shared tail of an empty `set_source`
    /// and of a literal write over a driven param. It does NOT re-plan: its callers do, exactly once.
    fn unbind(&mut self, uid: Uid, key: &ParamKey) {
        let Some(binding) = self.leaf_mut(uid).and_then(|e| e.sources.remove(key)) else {
            return;
        };
        if let (Some(ev), Some(id)) = (&self.runtime.evaluator, binding.id) {
            ev.release(id);
        }
    }

    /// Storing the record is only HALF of a param edit: a parked node is never rung by a bare
    /// pointer swap. The delivery is recorded here and runs at [`Self::settle`], once per batch.
    fn notify_param(&mut self, uid: Uid, key: &ParamKey) {
        self.touched.push(Touched::Param(uid, key.clone()));
    }

    /// Resolve a rewrite's variables against the graph: a producer output, a variable's value, or the
    /// reason neither was found. Event ids come from §3.2's `65..=128` budget, lowest free first.
    fn resolve_vars(&self, consumer: Uid, key: &ParamKey, refs: &[expr_rewrite::VarRef]) -> Vec<BoundVar> {
        let mut taken: Vec<EventId> = self
            .leaf(consumer)
            .into_iter()
            .flat_map(|e| e.sources.iter().filter(|(k, _)| *k != key))
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
        let entry = self.leaf(uid)?;
        let key = ParamKey::new(group, name);
        let b = entry.sources.get(&key)?;
        Some(SourceInfo { state: b.state.clone(), error: source_error(entry, self.health(uid), &key) })
    }

    /// Every param error on `uid` as `(group, name, message)` — what the live sweep broadcasts, so
    /// a failure that arrives or clears with no op behind it still reaches a client.
    pub fn param_errors(&self, uid: Uid) -> Vec<(&str, &str, String)> {
        let Some(entry) = self.leaf(uid) else { return Vec::new() };
        entry
            .sources
            .keys()
            .filter_map(|key| {
                source_error(entry, self.health(uid), key).map(|m| (key.group.as_str(), key.name.as_str(), m))
            })
            .collect()
    }

    /// Every source record on a node as `(group, name, state)` — what a delete's inverse must
    /// re-apply, since params alone carry only the literal value.
    pub fn param_sources(&self, uid: Uid) -> Vec<(String, String, SourceState)> {
        self.leaf(uid)
            .map(|e| {
                e.sources.iter().map(|(k, b)| (k.group.clone(), k.name.clone(), b.state.clone())).collect()
            })
            .unwrap_or_default()
    }

    /// Whether any param of `uid` has a source other than its literal.
    pub fn driven(&self, uid: Uid) -> bool {
        self.leaf(uid).is_some_and(|e| e.sources.values().any(|s| s.state.mode != Mode::Constant))
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
            .filter(|(key, _)| entry.sources.get(key).is_some_and(|b| b.state.mode != Mode::Constant))
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

    /// The wire currently feeding a SINGLE input `(node_in, slot)` — the one an `add_link` would
    /// evict, so the `AddLink` command's inverse can restore it. `None` for a multi input.
    pub fn single_input_source(&self, node_in: Uid, slot: &str) -> Option<(Uid, &'static str)> {
        let slot = self.slot_face(node_in, slot, Dir::In).filter(|f| !f.multi)?.name;
        self.patch.links
            .values()
            .find(|l| l.node_in == node_in && l.slot_in == slot)
            .map(|l| (l.node_out, l.slot_out))
    }

    /// Does this exact (resolved) wire already exist? Lets a command detect an idempotent AddLink,
    /// so its inverse is a no-op too instead of destroying the pre-existing wire.
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
            self.patch.links
                .retain(|_, l| !(l.node_in == node_in && l.slot_in == slot_in));
        }
        self.patch.links.insert(new.key(), new);
        self.touched.push(Touched::Slot(node_in, slot_in));
        Ok(())
    }

    pub fn remove_link(
        &mut self,
        node_out: Uid,
        slot_out: &str,
        node_in: Uid,
        slot_in: &str,
    ) -> Result<(), String> {
        if self.patch.links.shift_remove(&doc::link_key(node_out, slot_out, node_in, slot_in)).is_none() {
            return Err("no such link".into());
        }
        if let Some(f) = self.slot_face(node_in, slot_in, Dir::In) {
            self.touched.push(Touched::Slot(node_in, f.name));
        }
        Ok(())
    }

    /// The resolver input every service name is scoped by. The graph MINTS it and carries it;
    /// deriving a name from it is `goofi-transport`'s, which the graph never links.
    pub fn instance(&self) -> &str {
        &self.runtime.instance
    }

    /// The patch's time. An engine holds this handle; there is no second origin anywhere.
    pub fn time(&self) -> Arc<goofi_core::time::Time> {
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

    /// Deliver what the batch changed: one decision per touched item, from settled state, each
    /// item once however often the batch touched it. Free when nothing was.
    pub fn settle(&mut self) {
        self.derive_bindings();
        let changed = std::mem::take(&mut self.runtime.changed);
        let raw = std::mem::take(&mut self.touched);
        if changed.is_empty() && raw.is_empty() && !self.runtime.resettle && !self.engines().any(|e| e.dirty()) {
            return;
        }
        self.runtime.resettle = false;
        self.runtime.epoch.fetch_add(1, std::sync::atomic::Ordering::Release);
        self.net_instances(&changed);
        // Port consumers expand to the leaf inputs behind them, a node the batch also removed is
        // owed nothing, and each item is delivered once however often the batch touched it.
        let mut touched: Vec<Touched> = Vec::new();
        let expanded = raw.into_iter().flat_map(|t| match t {
            Touched::Slot(uid, _) if self.leaf(uid).is_none() => {
                self.sinks(uid, subpatch::BOUNDARY_SLOT).into_iter().map(|(n, sl)| Touched::Slot(n, sl)).collect()
            }
            t => vec![t],
        });
        for t in expanded {
            if self.leaf(t.uid()).is_some() && !touched.contains(&t) {
                touched.push(t);
            }
        }
        for t in &touched {
            if let Touched::Record(uid) = t {
                self.mint_serials(*uid);
            }
        }
        let edges = self.resolved_edges();
        let typed: HashMap<Uid, ParamGroups> = self.leaves().map(|(u, l)| (u, self.typed(l))).collect();
        let published = {
            let Runtime { generations, instance, engines, watched, .. } = &mut self.runtime;
            let variables = self.patch.variables.lock();
            let view = build_view(&self.patch.nodes, &typed, generations, instance, &edges, watched, &variables);
            for e in engines.iter_mut() {
                e.settle(&view, &touched);
            }
            engines.iter().flat_map(|e| e.published()).collect::<Vec<_>>()
        };
        // A wire made or cut, or a node reborn, changes what its producer owes its viewers.
        self.offer_view_wants(&edges);
        // The engines' own facts into `system.*`, from the state this settle reached. Not a
        // command: the user's undoable act is the param they moved.
        for (name, value) in published {
            self.variables().publish(name, value);
        }
    }

    /// Bring the instances level with the leaves the batch left: removals, births, then rebirths.
    /// A uid changed back to where it began costs nothing.
    fn net_instances(&mut self, changed: &[Change]) {
        let mut uids: Vec<Uid> = Vec::new();
        for uid in changed.iter().map(|c| match c {
            Change::Added(u) | Change::Removed(u) | Change::Restart(u) => *u,
        }) {
            if !uids.contains(&uid) {
                uids.push(uid);
            }
        }
        let was = |g: &Graph, u: &Uid| g.runtime.instances.contains_key(u);
        let gone: Vec<Uid> = uids.iter().filter(|u| was(self, u) && !self.is_leaf(**u)).copied().collect();
        let born: Vec<Uid> = uids.iter().filter(|u| !was(self, u) && self.is_leaf(**u)).copied().collect();
        let reborn: Vec<Uid> = uids
            .iter()
            .filter(|u| was(self, u) && self.is_leaf(**u) && changed.iter().any(|c| matches!(c, Change::Restart(r) if r == *u) || matches!(c, Change::Removed(r) if r == *u)))
            .copied()
            .collect();
        for uid in gone {
            self.halt(uid);
        }
        for uid in born {
            self.birth(uid);
        }
        for uid in reborn {
            self.halt(uid);
            self.birth(uid);
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
        self.patch.links
            .values()
            .filter(|l| self.leaf(l.node_in).is_some())
            .filter_map(|l| match self.stream(l.node_out, l.slot_out)? {
                Stream::At(u, s) => {
                    Some(Edge { producer: (u, s), consumer: (l.node_in, l.slot_in) })
                }
                Stream::Open(_) => None,
            })
            .collect()
    }

    /// Take every engine's waiting reports and apply the health plane. Answers how many landed,
    /// so a caller can tell a quiet graph from one it stopped hearing.
    pub fn drain_status(&mut self) -> usize {
        let mut applied = 0;
        {
            let Runtime { instances, refreshed, engines, .. } = &mut self.runtime;
            let nodes = &self.patch.nodes;
            let mut apply =
                |uid: Uid, status: Status| apply_status_to(nodes, instances, refreshed, uid, status);
            for e in engines.iter_mut() {
                applied += e.drain(&mut apply);
            }
        }
        // A direct driver has no bridge to settle for it, and the drain-side settle lands what
        // the drain marked pending.
        self.settle();
        applied
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
        for e in self.patch.nodes.values() {
            self.release_entry_bindings(e);
        }
        // N explicit removes, then the nodes wholesale — a removal derived from absence would be
        // the engine-observes-the-graph mirror the seam rejects.
        let removed: Vec<Uid> = self.leaves().map(|(u, _)| u).collect();
        self.runtime.changed.extend(removed.into_iter().map(Change::Removed));
        self.patch.nodes.clear();
        self.patch.links.clear();
        // Whatever the batch touched addressed nodes this clear destroyed; the generations stay,
        // keeping whatever is born at those uids next clear of what just died.
        self.touched.clear();
        // An un-echoed refresh names a node the patch no longer holds — and a load restores uids,
        // so that number can come back and the echo be read as an answer nobody asked for.
        self.runtime.refreshed.clear();
        // Variables are patch CONTENT, so a load starts from a fresh seeded store.
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
        let mut nodes = IndexMap::new();
        // ONE loop over ONE map: a leaf, a facade and a port are all node records, and membership
        // rides each record rather than a member list beside what `scope_of` owns.
        for (uid, e) in self.patch.nodes.iter().filter(|(u, _)| want.contains(u)) {
            let mut params = IndexMap::new();
            if let Some(leaf) = e.leaf() {
                for (group, names) in &self.typed(leaf) {
                    let entries: IndexMap<String, doc::ParamEntry> = names
                        .iter()
                        .filter_map(|(name, p)| {
                            let entry = doc::ParamEntry { value: control::data_of(p), ..Default::default() };
                            let entry = match leaf.sources.get(&ParamKey::new(group, name)) {
                                Some(b) => entry.with_source(&b.state),
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
            nodes.insert(
                uid.to_hex(),
                doc::NodeRecord {
                    type_id: self.node_type(*uid).unwrap_or_default(),
                    name: e.name.clone(),
                    pos: e.pos,
                    scope: self.scope_of(*uid).filter(|p| want.contains(p)).map(|p| p.to_hex()),
                    params,
                    viewers: blob(&e.viewers),
                    baseline: blob(&e.baseline),
                    record: e.record.clone(),
                },
            );
        }
        // A port's inner wire is a link like any other — the same one `add_link` writes — so a
        // fragment has one relation kind as well as one entity kind.
        let links = self
            .patch
            .links
            .values()
            .filter(|l| want.contains(&l.node_out) && want.contains(&l.node_in))
            .map(|l| {
                let link = doc::Link {
                    node_out: l.node_out.to_hex(),
                    slot_out: l.slot_out.to_string(),
                    node_in: l.node_in.to_hex(),
                    slot_in: l.slot_in.to_string(),
                };
                (link.key(), link)
            })
            .collect();
        doc::PatchDoc { nodes, links, ..Default::default() }
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

    /// Add an admitted fragment under `scope`, shifted by `offset`, on FRESH uids. Answers the ONE
    /// command that does it, and what each record's uid became.
    pub fn import_fragment(
        &mut self,
        doc: &doc::PatchDoc,
        scope: Option<Uid>,
        offset: [f64; 2],
    ) -> Result<(command::Command, HashMap<String, String>), String> {
        use command::Command;
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
        let mut cmds: Vec<Command> = Vec::new();
        for old in &order {
            let rec = &nodes[*old];
            let ty = rec.type_id.as_str();
            let inner = rec.scope.as_deref().and_then(|s| idmap.get(s)).copied();
            cmds.push(Command::AddNode {
                type_name: ty.to_string(),
                pos: at(rec.pos),
                uid: Some(idmap[*old]),
                name: by_old(&rec.name),
                params: (!structural(ty)).then(|| self.record_params(rec)).transpose()?,
                sources: sources_of(rec)
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
                        if let Some(r) = expr_rewrite::rename_reference(&s.reference, remap) {
                            s.reference = r;
                        }
                        (group, name, s)
                    })
                    .collect(),
                viewers: rec.viewers.as_ref().map(|v| remap_slots(v, &idmap)),
                // NOT slot-remapped: a baseline is keyed by `group/param`, which a paste does not
                // renumber the way it renumbers the slot uids a viewer blob is keyed by.
                baseline: rec.baseline.clone(),
                record: Some(rec.record.clone()),
                // A port cannot exist without a scope, so it takes the paste target when its own
                // facade is not in the fragment — the same fallback every other kind gets below.
                scope: subpatch::boundary_type(ty).and(inner.or(scope)),
            });
        }
        for old in &order {
            let inner = nodes[*old].scope.as_deref().and_then(|s| idmap.get(s)).copied();
            // A record naming no scope INSIDE the fragment is a root of it, so it lands where the
            // paste was aimed; one naming a scope in here keeps the shape it was copied with.
            cmds.push(Command::SetScope { uid: idmap[*old], scope: inner.or(scope) });
        }
        for l in doc.links.values() {
            let (Some(no), Some(ni)) = (idmap.get(&l.node_out).copied(), idmap.get(&l.node_in).copied()) else {
                continue;
            };
            cmds.push(Command::AddLink {
                node_out: no,
                slot_out: l.slot_out.clone(),
                node_in: ni,
                slot_in: l.slot_in.clone(),
            });
        }
        let rename = idmap.into_iter().map(|(old, new)| (old, new.to_hex())).collect();
        Ok((Command::Compound(cmds), rename))
    }

    /// The whole patch as one document: every record, every variable in order, the arrangement.
    fn document(&self) -> doc::PatchDoc {
        let mut patch = self.fragment(&self.all_uids());
        let variables = self.variables();
        patch.variables = variables
            .entries()
            .map(|(name, v)| (name.to_string(), doc::VariableRecord { value: None, control: v.control.clone(), source: v.source.clone(), lock: v.lock }))
            .collect();
        patch.variable_groups = variables.groups().map(|(g, lock)| (g.to_string(), doc::Group { lock })).collect();
        // The flat arrangement always exists (at worst the default), so it always rides.
        patch.arrangement = Some(self.patch.arrangement.to_json());
        patch
    }

    /// The document a browser replica holds: the patch, every root present, and beside a
    /// variable's source why it delivers nothing — the one runtime fact the panel shows inline.
    pub fn replica(&self) -> serde_json::Value {
        let mut doc = serde_json::to_value(self.document()).expect("a plain record");
        for root in ["nodes", "links", "variables", "variable_groups"] {
            doc[root] = doc.get(root).cloned().unwrap_or_else(|| serde_json::json!({}));
        }
        let sourced: Vec<(String, goofi_core::variables::VariableSource)> =
            self.variables().entries().filter_map(|(n, v)| Some((n.to_string(), v.source.clone()?))).collect();
        for (name, s) in sourced {
            if let Some(error) = self.variable_source_error(&s) {
                doc["variables"][name]["source"]["error"] = serde_json::Value::String(error);
            }
        }
        doc
    }

    /// The whole patch as its `.gfi` manifest, every narrow variable's value in it; a wide array
    /// is the file [`Self::persist_variables`] writes.
    pub fn serialize(&self) -> String {
        let mut patch = self.document();
        let variables = self.variables();
        // An ephemeral variable is goofi's own to say; writing it into a patch would carry one
        // machine's answer onto another. The system group's lock is re-asserted on load likewise.
        patch.variables.retain(|name, _| !variables.is_ephemeral(name));
        for (name, record) in &mut patch.variables {
            record.value = variables.get(name).filter(|v| !is_wide(v)).cloned();
        }
        patch.variable_groups.shift_remove(goofi_core::variables::SYSTEM_GROUP);
        let archive = doc::Archive {
            version: doc::MANIFEST_VERSION,
            goofi: env!("CARGO_PKG_VERSION").to_string(),
            patch,
            viewpoint: (!self.viewpoint.is_null()).then(|| self.viewpoint.clone()),
        };
        serde_yaml_ng::to_string(&archive).unwrap_or_default()
    }

    /// Replace the graph from a `.gfi` manifest, born into `workspace`. A rejected document leaves
    /// the graph as it was; what was dropped on the way in is answered as warnings.
    pub fn load_doc(&mut self, text: &str, workspace: &std::path::Path) -> Result<Vec<String>, String> {
        let archive = doc::Archive::parse(text)?;
        let (doc, mut warnings) = self.admit(archive.patch)?;

        self.clear();
        // The outgoing nodes retire NOW, into the workspace being replaced: an engine saves a
        // node's state where it is halted, and the archive's own must not be written over.
        let retired = std::mem::take(&mut self.runtime.changed);
        self.net_instances(&retired);
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
            if let Some(s) = &v.source {
                let _ = variables.set_source(name, Some(s.clone()));
            }
            if let Some(l) = v.lock {
                let _ = variables.set_lock(name, l);
            }
        }
        for (group, g) in &doc.variable_groups {
            let _ = self.variables().set_group_lock(group, Some(g.lock));
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
        self.patch.arrangement = match doc.arrangement.as_ref().map(layout::Layout::from_json) {
            None => layout::Layout::default(),
            Some(Ok(l)) => l,
            Some(Err(e)) => {
                warnings.push(e);
                layout::Layout::default()
            }
        };
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

/// The source records a node record carries, in the shape [`command::Command::AddNode`] re-applies.
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
        .map(|(key, b)| (key.name.clone(), doc::ParamEntry::default().with_source(&b.state)))
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

/// A reference's `node.slot` as the one variable its record resolves — the same term an
/// expression's `nd('node').out.slot` rewrites to.
fn parse_reference(reference: &str) -> Result<expr_rewrite::VarRef, String> {
    let Some((name, slot)) = reference.split_once('.') else {
        return Err(format!("a reference spells `node.slot`, not `{reference}`"));
    };
    if !goofi_core::variables::is_valid_name(name) || !goofi_core::variables::is_valid_name(slot) {
        return Err(format!("`{reference}` is not a legal reference: {NAME_RULE}"));
    }
    Ok(expr_rewrite::VarRef {
        var: REF_VAR.to_string(),
        target: Target::Node { name: name.to_string(), slot: Some(slot.to_string()) },
    })
}

/// The health plane's one mutator: apply one report off any engine's drain. A free function so
/// the drain can hold the engines and the node map apart.
fn apply_status_to(
    nodes: &IndexMap<Uid, NodeEntry>,
    instances: &mut HashMap<Uid, Instance>,
    refreshed: &mut Vec<(Uid, ParamKey)>,
    uid: Uid,
    status: Status,
) {
    // A report from an instance settle has since halted lands nowhere.
    let (Some(entry), Some(health)) = (nodes.get(&uid).and_then(NodeEntry::leaf), instances.get_mut(&uid).map(|i| &mut i.health)) else { return };
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
    let current = entry_error(entry, Some(health)).map(str::to_string);
    if health.error_since.as_ref().map(|(m, _)| m.as_str()) != current.as_deref() {
        health.error_since = current.map(|m| (m, Instant::now()));
    }
}

/// The settled view, borrowed from the one model — built at the settle point and nowhere else.
fn build_view<'a>(
    nodes: &'a IndexMap<Uid, NodeEntry>,
    typed: &'a HashMap<Uid, ParamGroups>,
    generations: &HashMap<Uid, u64>,
    instance: &'a str,
    edges: &'a [Edge],
    watched: &HashSet<(Uid, String)>,
    variables: &'a goofi_core::variables::VariableStore,
) -> GraphView<'a> {
    let nodes = nodes
        .iter()
        .filter_map(|(uid, e)| {
            let leaf = e.leaf()?;
            let bindings = leaf
                .sources
                .iter()
                .map(|(key, b)| BindingView {
                    key,
                    rewritten: &b.rewritten,
                    vars: &b.vars,
                    trigger: b.state.triggers,
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
    GraphView { instance, edges, nodes, variables }
}

/// One param's error: the bind the graph refused, or the node's own last evaluation failure. The
/// one derivation, so a descriptor and the live sweep cannot answer differently.
fn source_error(e: &Leaf, health: Option<&Health>, key: &ParamKey) -> Option<String> {
    let b = e.sources.get(key)?;
    b.bind_error.clone().or_else(|| health?.param_errors.get(key).cloned())
}

/// One node's current error, derived fresh from the places one can arise. A free function so the
/// status drain can read it while holding a `&mut NodeEntry`.
fn entry_error<'a>(e: &'a Leaf, health: Option<&'a Health>) -> Option<&'a str> {
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
    e.sources
        .iter()
        .filter_map(|(k, b)| b.bind_error.as_deref().map(|s| (k, s)))
        .chain(health.into_iter().flat_map(|h| h.param_errors.iter().map(|(k, m)| (k, m.as_str()))))
        .min_by(|a, b| a.0.cmp(b.0))
        .map(|(_, s)| s)
}
