//! JSON projections of the engine graph into the shapes the frontend mirrors (`control.ts`).
//! These are the wire contract: co-edit the frontend when a field or shape changes.

use goofi_core::{control, Data, Param};
use goofi_graph::{Graph, Mode, Origin, SourceInfo, Uid};
use goofi_graph::doc::Dependency;
use serde::Serialize;
use ts_rs::TS;
use goofi_node::{NodeManifest, ParamGroups};
use serde_json::{json, Map, Value};

pub const PROTOCOL_VERSION: i64 = 5;

/// The examples a public deployment offers, compiled in beside the `.gfi` files they name.
const EXAMPLES: &str = include_str!("../../../examples/demo.json");

/// One row per example: where it answers under `base`, and which of them is `current`.
pub(crate) fn examples(base: &str, current: Option<&str>) -> Value {
    let base = base.trim_end_matches('/');
    let manifest: Value = serde_json::from_str(EXAMPLES).expect("the compiled example manifest");
    manifest["examples"]
        .as_array()
        .expect("the manifest lists examples")
        .iter()
        .map(|e| {
            let slug = e["slug"].as_str().unwrap_or_default();
            json!({
                "slug": slug,
                "label": e["label"],
                "url": format!("{base}/{slug}"),
                "current": Some(slug) == current,
            })
        })
        .collect()
}

/// A param descriptor's shared half: the declaration's help and default, and the source record's
/// state. An empty source text is `null`, and a param with no record is a constant.
#[derive(Serialize, TS)]
pub struct ParamBase {
    pub doc: Option<String>,
    /// What the declaration says this param is worth untouched; `None` for a pulse.
    #[ts(type = "Literal | null")]
    pub default: Option<Data>,
    /// The index of the param's section inside its group; the inspector draws a line between two.
    pub section: u8,
    /// The inspector shows the param only while this holds; `None` shows it always.
    pub show: Option<ParamShow>,
    /// The part the param plays in a declared section; `None` for a row of its own.
    pub role: Option<ParamRole>,
    /// True when the node declared a refresh method for this param.
    pub refreshable: bool,
    pub mode: Mode,
    pub expression: Option<String>,
    /// When true, an arrival that changes the value wakes the node's `process()`.
    pub triggers: bool,
    /// The active source's bind, compile or arrival error.
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dependencies: Option<Vec<Dependency>>,
}

/// Holds while the param `group.name` has one of `any_of`, compared as text.
#[derive(Serialize, TS)]
pub struct ParamShow {
    pub group: String,
    pub name: String,
    pub any_of: Vec<String>,
}

/// The count a list section repeats by, or a member of a section — of one `slot` of a list,
/// where `base` is its name inside the section.
#[derive(Serialize, TS)]
#[serde(tag = "as", rename_all = "lowercase")]
pub enum ParamRole {
    Count { section: String },
    Member { section: String, base: String, slot: Option<u32> },
    Feed { slot: String, group: String },
}

/// A descriptor's typed half: the value with the bounds or options its type carries.
#[derive(Serialize, TS)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ParamKind {
    /// A number, or a vector of them: `value` is bare for one dimension and a list for more.
    Num {
        #[ts(type = "number | number[]")]
        value: Data,
        vmin: f64,
        vmax: f64,
        int: bool,
        options: Vec<i64>,
        color: bool,
    },
    Bool { value: bool },
    #[serde(rename = "string")]
    Str { value: String, options: Option<Vec<String>> },
    /// A request rather than a value: it holds none, and firing it is the whole edit.
    Pulse { value: () },
}

/// A single param descriptor, discriminated on `type`.
#[derive(Serialize)]
pub struct ParamDescriptor {
    #[serde(flatten)]
    pub base: ParamBase,
    #[serde(flatten)]
    pub kind: ParamKind,
}

pub fn describe_param(p: &Param, source: Option<&SourceInfo>, decl: Option<goofi_node::ParamDecl>) -> ParamDescriptor {
    let text = |t: &str| (!t.is_empty()).then(|| t.to_string());
    let base = ParamBase {
        doc: decl.and_then(|d| d.doc).map(str::to_string),
        default: decl.and_then(|d| control::data_of(&d.spec.to_param())),
        section: decl.map_or(0, |d| d.section),
        show: decl.and_then(|d| Some((d.group, d.show?))).map(|(group, s)| {
            let (group, name) = s.controller(group);
            ParamShow { group: group.into(), name: name.into(), any_of: s.any_of.iter().map(|a| a.to_string()).collect() }
        }),
        role: decl.and_then(|d| d.role).map(|r| match r {
            goofi_node::Role::Count { section } => ParamRole::Count { section: section.into() },
            goofi_node::Role::Member { section, base, slot } => ParamRole::Member { section: section.into(), base: base.into(), slot },
            goofi_node::Role::Feed { slot, group } => ParamRole::Feed { slot: slot.into(), group: group.into() },
        }),
        refreshable: matches!(p, Param::Str { refresh: true, .. }),
        mode: source.map(|s| s.state.mode).unwrap_or_default(),
        expression: source.and_then(|s| text(&s.state.expression)),
        triggers: source.is_some_and(|s| s.state.triggers),
        error: source.and_then(|s| s.error.clone()),
        dependencies: source.map(|source| source.dependencies.clone()).filter(|dependencies| !dependencies.is_empty()),
    };
    let kind = match p {
        Param::Num { vmin, vmax, int, options, color, .. } => ParamKind::Num {
            value: control::data_of(p).expect("a number"),
            vmin: *vmin,
            vmax: *vmax,
            int: *int,
            options: options.clone(),
            color: *color,
        },
        Param::Bool { value } => ParamKind::Bool { value: *value },
        Param::Str { value, options, .. } => ParamKind::Str { value: value.clone(), options: options.clone() },
        Param::Pulse => ParamKind::Pulse { value: () },
    };
    ParamDescriptor { base, kind }
}

/// A param's declaration; a node's own wins over the owning engine's universal one. The help
/// text and the default both come off it, so the two cannot name different params.
fn param_decl(
    m: &NodeManifest,
    universal: &[goofi_node::ParamDecl],
    group: &str,
    name: &str,
) -> Option<goofi_node::ParamDecl> {
    m.params
        .iter()
        .copied()
        .chain(universal.iter().copied())
        .find(|d| d.group == group && d.name == name)
}

/// Params as descriptors, `{group: {name: …}}`: a type's for the palette, or with `uid` one
/// node's, each carrying its binding state and its live options.
pub fn describe_params(g: &Graph, engine: &str, p: &ParamGroups, m: &'static NodeManifest, uid: Option<Uid>) -> Value {
    let universal = g.universal_decls(engine, m);
    let mut groups = Map::new();
    for (gname, grp) in p {
        let mut names = Map::new();
        for (n, param) in grp {
            let shown = uid.map(|u| g.shown_param(u, gname, n, param));
            let source = uid.and_then(|u| g.param_source(u, gname, n));
            let d = describe_param(shown.as_ref().unwrap_or(param), source.as_ref(), param_decl(m, &universal, gname, n));
            names.insert(n.clone(), json!(d));
        }
        groups.insert(gname.clone(), Value::Object(names));
    }
    Value::Object(groups)
}

/// A node instance's params, each carrying its real expression binding state.
pub fn describe_node_params(g: &Graph, uid: Uid) -> Value {
    let (Some(params), Some(m)) = (g.params(uid), g.manifest(uid)) else {
        return Value::Object(Map::new());
    };
    let ty = g.node_type(uid).unwrap_or_default();
    describe_params(g, goofi_node::split_type_id(&ty).0.unwrap_or_default(), &params, m, Some(uid))
}

/// The live values of a node's expression-driven params, `{group: {name: value}}`. Values only, so
/// the frontend applies it surgically and cannot clobber a concurrent edit.
pub fn expression_value_map(g: &Graph, uid: Uid) -> Value {
    let mut groups = Map::new();
    for (group, name, p) in g.driven_values(uid) {
        insert_at(&mut groups, group, name, goofi_graph::param_value_json(p));
    }
    Value::Object(groups)
}

/// A node's param errors in that same `{group: {name: …}}` shape — the whole map, so a param it no
/// longer names is one whose failure has cleared.
pub fn param_error_map(g: &Graph, uid: Uid) -> Value {
    let mut groups = Map::new();
    for (group, name, msg) in g.param_errors(uid) {
        insert_at(&mut groups, group, name, json!(msg));
    }
    Value::Object(groups)
}

fn insert_at(groups: &mut Map<String, Value>, group: &str, name: &str, v: Value) {
    let entry = groups.entry(group.to_string()).or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(names) = entry {
        names.insert(name.to_string(), v);
    }
}

/// A node instance's param VALUES, `{group: {name: value}}`, without descriptor metadata.
pub fn param_value_map(params: &goofi_node::ParamGroups) -> Value {
    Value::Object(
        params
            .iter()
            .map(|(gname, group)| {
                let names = group.iter().map(|(n, p)| (n.clone(), goofi_graph::param_value_json(p)));
                (gname.clone(), Value::Object(names.collect()))
            })
            .collect(),
    )
}

/// Project `(slot_name, dtype_name)` pairs into a `{name: dtype}` JSON object.
fn slot_map<'a>(slots: impl Iterator<Item = (&'a str, &'a str)>) -> Value {
    Value::Object(slots.map(|(name, dtype)| (name.to_string(), json!(dtype))).collect())
}
fn input_slots(m: &NodeManifest) -> Value {
    slot_map(m.inputs.iter().map(|s| (s.name, s.kind.name())))
}

/// The names of the node type's `multi` (variadic) input slots — static shape, not per-instance.
fn input_multi(m: &NodeManifest) -> Value {
    Value::Array(m.inputs.iter().filter(|s| s.multi).map(|s| json!(s.name)).collect())
}
fn output_slots(m: &NodeManifest) -> Value {
    slot_map(m.outputs.iter().map(|s| (s.name, s.kind.name())))
}

/// Where a palette row's type came from, for the add-menu badge: the open patch, the user's own
/// private library, an engine's own find, or `builtin` — every other root.
pub(crate) fn source_of(g: &Graph, type_name: &str) -> &'static str {
    match g.origin(type_name) {
        Some(Origin::Patch) => "patch",
        Some(Origin::Custom) => "custom",
        Some(Origin::Plugin) => "plugin",
        Some(Origin::Root(_)) | None => "builtin",
    }
}

/// How much of a palette entry to project. [`Detail::Index`] is the name and the doc's FIRST
/// LINE; [`Detail::Full`] is the descriptor a client builds nodes from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    Index,
    Full,
}

/// A node doc's FIRST LINE — the nutshell every doc opens with, and all an index shows.
pub fn nutshell(doc: &str) -> &str {
    doc.split('\n').next().unwrap_or(doc).trim_end()
}

/// One palette row: a type that loads with its manifest `m`, or a `greyed` one that cannot,
/// with the manifest it last had. The full fields are the same keys either way.
pub fn palette_row(g: &Graph, engine: &str, ty: &str, m: Option<&'static NodeManifest>, greyed: Option<&str>, d: Detail) -> Value {
    let doc = m.map_or("", |m| if d == Detail::Full { m.doc } else { nutshell(m.doc) });
    let mut info = match greyed {
        None => json!({ "type": ty, "doc": doc }),
        // `available` rides the INDEX too where it is false: the one row a chooser must not skim past.
        Some(reason) => json!({ "type": ty, "doc": format!("This node could not be loaded: {reason}"), "available": false }),
    };
    if d == Detail::Full {
        info["source"] = json!(source_of(g, ty));
        if let Some(Origin::Root(bundle)) = g.origin(ty) {
            info["bundle"] = json!(bundle);
        }
        info["tags"] = json!(m.map_or(Vec::new(), |m| m.tags.iter().map(|t| t.as_str()).collect()));
        info["available"] = json!(greyed.is_none());
        info["missing_deps"] = json!(greyed.into_iter().collect::<Vec<_>>());
        info["editor"] = json!(greyed.is_none() && m.is_some_and(|m| g.type_has_editor(engine, m.type_name)));
        info["input_slots"] = m.map_or_else(|| json!({}), input_slots);
        info["input_multi"] = m.map_or_else(|| json!([]), input_multi);
        info["output_slots"] = m.map_or_else(|| json!({}), output_slots);
        // The owning engine's own normalization, so palette and instance agree.
        info["params"] = m.map_or_else(|| json!({}), |m| {
            describe_params(g, engine, &g.default_params_of(ty, None).unwrap_or_default(), m, None)
        });
    }
    info
}

/// The `list_nodes` palette catalog, sorted by (engine, bare name). Hidden test nodes
/// (`_`-prefixed) are excluded.
pub fn catalog_types(g: &Graph, d: Detail) -> Value {
    let mut items: Vec<(String, String, Value)> = g
        .library_entries()
        .into_iter()
        .filter(|(_, l)| !l.manifest.type_name.starts_with('_'))
        .map(|(engine, l)| {
            let ty = goofi_node::qualify(engine, l.manifest.type_name);
            (engine.to_string(), l.manifest.type_name.to_string(), palette_row(g, engine, &ty, Some(l.manifest), None, d))
        })
        .collect();
    // Node files that cannot load are listed too, greyed and with the reason, in the shape they
    // last had: the instances born from it still run and are wired.
    items.extend(g.unavailable_types().map(|(name, reason)| {
        let (engine, bare) = goofi_node::split_type_id(name);
        let engine = engine.unwrap_or_default();
        (engine.to_string(), bare.to_string(), palette_row(g, engine, name, g.last_manifest(name), Some(reason), d))
    }));
    items.extend(crate::vocab::boundary_catalog(d));
    // By the order the engines were REGISTERED, not by their names: signal is the plane a patch
    // starts on, and an alphabet would put audio ahead of it.
    let order: Vec<&'static str> = g.engine_ids();
    let rank = |id: &str| order.iter().position(|e| *e == id).unwrap_or(order.len());
    items.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
    Value::Array(items.into_iter().map(|(_, _, v)| v).collect())
}

/// The per-node RUNTIME overlay that never enters the doc; its live stream pushes only
/// transitions. EVERY node is in it, so a client never works out a facade's or a port's.
pub fn runtime_overlay(g: &Graph) -> Value {
    let mut m = Map::new();
    for uid in g.all_uids() {
        m.insert(uid.to_hex(), runtime_json(g, uid));
    }
    Value::Object(m)
}

/// The `{stage, error, runtime}` triple, ONE spelling for every wire site that carries it.
/// `runtime` is absent for a port and a facade, which run nowhere — as `stage` is for inspect.
pub(crate) fn runtime_json(g: &Graph, uid: Uid) -> Value {
    json!({
        "stage": g.node_stage(uid),
        "error": g.last_error(uid),
        "runtime": g.node_tier(uid).map(goofi_node::Isolation::wire),
    })
}

/// The `hello` / `graph_replaced` payload: the session frame plus the truths the doc never holds.
/// `harnesses` is passed in because its config half is a disk read, done off the graph lock.
pub fn snapshot(
    g: &Graph,
    state: &crate::AppState,
    with_protocol: bool,
    unsaved: bool,
    save_path: Option<&str>,
    harnesses: Value,
) -> Value {
    let mut snap = json!({
        "instance_id": &*state.instance_id,
        // The document this session frame goes with; a replica behind it is still mid-load.
        "doc_version": state.doc.lock().version(),
        "runtime": runtime_overlay(g),
        "record": crate::ops::record::record_state_at(state, g.time().now()),
        // Seeded for the same reason the runtime overlay is: `harness_changed` pushes transitions.
        "harnesses": harnesses,
        "save_path": save_path,
        "unsaved_changes": unsaved,
        "viewpoint": g.viewpoint().clone(),
    });
    if with_protocol {
        snap["protocol_version"] = json!(PROTOCOL_VERSION);
        // The palette rides along, so the first render needs no `list_nodes` round-trip.
        snap["node_types"] = catalog_types(g, Detail::Full);
        snap["demo"] = json!(state.mode.demo);
        if let Some(examples) = state.examples() {
            snap["examples"] = examples;
        }
    }
    snap
}

/// The frontend's generated types: the document, the deltas, the param descriptor, the variable
/// records and the machines, each declared once in Rust and checked into the tree.
pub fn typescript() -> String {
    use goofi_core::record::{RecordedOutput, VideoQuality};
    use goofi_core::variables::{Control, ControlKind, Group, Lock, Midi};
    use goofi_graph::doc::{Archive, Link, NodeRecord, ParamEntry, PatchDoc, VariableRecord};
    use goofi_graph::machine::{Attribute, AttributeKind, ExpressionIssue, ExpressionSurface, Health, Machine, Edge, Playhead, Policy, Seconds, Selection, State, Transition, Trigger};
    use goofi_core::ease::Curve;
    let cfg = ts_rs::Config::new().with_large_int("number");
    let decls = [
        serde_json::Value::decl(&cfg),
        // The literal of a frame: a number, a string, a bool or a list, nested for a wider array.
        "type Literal = number | string | boolean | Literal[];".to_string(),
        Mode::decl(&cfg),
        ParamEntry::decl(&cfg),
        VideoQuality::decl(&cfg),
        RecordedOutput::decl(&cfg),
        NodeRecord::decl(&cfg),
        Link::decl(&cfg),
        ControlKind::decl(&cfg),
        Control::decl(&cfg),
        Lock::decl(&cfg),
        VariableRecord::decl(&cfg),
        Midi::decl(&cfg),
        Group::decl(&cfg),
        AttributeKind::decl(&cfg),
        Attribute::decl(&cfg),
        State::decl(&cfg),
        Curve::decl(&cfg),
        Policy::decl(&cfg),
        Selection::decl(&cfg),
        Edge::decl(&cfg),
        Seconds::decl(&cfg),
        Trigger::decl(&cfg),
        Transition::decl(&cfg),
        Playhead::decl(&cfg),
        Machine::decl(&cfg),
        ExpressionSurface::decl(&cfg),
        ExpressionIssue::decl(&cfg),
        Health::decl(&cfg),
        PatchDoc::decl(&cfg),
        Archive::decl(&cfg),
        crate::doc::Op::decl(&cfg),
        ParamShow::decl(&cfg),
        ParamRole::decl(&cfg),
        ParamBase::decl(&cfg),
        Dependency::decl(&cfg),
        ParamKind::decl(&cfg),
    ];
    let body = decls.iter().map(|d| format!("export {d}\n")).collect::<String>();
    format!(
        "// GENERATED from the Rust types in goofi-core, goofi-graph and goofi-bridge — do not edit by\n\
         // hand. The document, its deltas, a variable and a param descriptor are each declared once,\n\
         // in Rust; a field that is not there is a type error here. Regenerate by running\n\
         // `cargo test -p goofi-tests contracts::`, which rewrites this file when it drifts.\n\n{body}"
    )
}
