//! JSON projections of the engine graph into the shapes the frontend mirrors (`control.ts`).
//! These are the wire contract: co-edit the frontend when a field or shape changes.

use goofi_core::Param;
use goofi_graph::doc::Scalar;
use goofi_graph::{Graph, Mode, SourceInfo, Uid};
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
    pub default: Option<Scalar>,
    /// The index of the param's section inside its group; the inspector draws a line between two.
    pub section: u8,
    /// The inspector shows the param only while this holds; `None` shows it always.
    pub show: Option<ParamShow>,
    /// True when the node declared a refresh method for this param.
    pub refreshable: bool,
    pub mode: Mode,
    pub expression: Option<String>,
    pub reference: Option<String>,
    /// When true, an arrival that changes the value wakes the node's `process()`.
    pub triggers: bool,
    /// The active source's bind, compile or arrival error.
    pub error: Option<String>,
}

/// Holds while the param `group.name` has one of `any_of`, compared as text.
#[derive(Serialize, TS)]
pub struct ParamShow {
    pub group: String,
    pub name: String,
    pub any_of: Vec<String>,
}

/// A descriptor's typed half: the value with the bounds or options its type carries.
#[derive(Serialize, TS)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ParamKind {
    Float { value: f64, vmin: f64, vmax: f64 },
    Int { value: i64, vmin: i64, vmax: i64, options: Vec<i64> },
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
        default: decl.and_then(|d| Scalar::of(&d.spec.to_param())),
        section: decl.map_or(0, |d| d.section),
        show: decl.and_then(|d| Some((d.group, d.show?))).map(|(group, s)| {
            let (group, name) = s.controller(group);
            ParamShow { group: group.into(), name: name.into(), any_of: s.any_of.iter().map(|a| a.to_string()).collect() }
        }),
        refreshable: matches!(p, Param::Str { refresh: true, .. }),
        mode: source.map(|s| s.state.mode).unwrap_or_default(),
        expression: source.and_then(|s| text(&s.state.expression)),
        reference: source.and_then(|s| text(&s.state.reference)),
        triggers: source.is_some_and(|s| s.state.triggers),
        error: source.and_then(|s| s.error.clone()),
    };
    let kind = match p {
        Param::Float { value, vmin, vmax } => ParamKind::Float { value: *value, vmin: *vmin, vmax: *vmax },
        Param::Int { value, vmin, vmax, options } => {
            ParamKind::Int { value: *value, vmin: *vmin, vmax: *vmax, options: options.clone() }
        }
        Param::Bool { value } => ParamKind::Bool { value: *value },
        Param::Str { value, options, .. } => ParamKind::Str { value: value.clone(), options: options.clone() },
        Param::Pulse => ParamKind::Pulse { value: () },
    };
    ParamDescriptor { base, kind }
}

/// A param's declaration; a node's own wins over the owning engine's universal one — resolved
/// once per node by the caller, not once per param. The help text and the default both come off
/// it, so the two cannot name different params.
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

/// Type-level params for the palette, and the projection param tooltips are rendered from.
pub fn describe_params(g: &Graph, engine: &str, p: &ParamGroups, m: &'static NodeManifest) -> Value {
    let universal = g.universal_decls(engine, m);
    let mut groups = Map::new();
    for (gname, grp) in p {
        let mut names = Map::new();
        for (n, param) in grp {
            names.insert(n.clone(), json!(describe_param(param, None, param_decl(m, &universal, gname, n))));
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
    let universal = g.universal_decls(goofi_node::split_type_id(&ty).0.unwrap_or_default(), m);
    let mut groups = Map::new();
    for (gname, group) in &params {
        let mut names = Map::new();
        for (n, param) in group {
            let source = g.param_source(uid, gname, n);
            let mut d = describe_param(param, source.as_ref(), param_decl(m, &universal, gname, n));
            if let (ParamKind::Str { options, .. }, Some(live)) = (&mut d.kind, g.refreshed_options(uid, gname, n)) {
                *options = Some(live.to_vec());
            }
            names.insert(n.clone(), json!(d));
        }
        groups.insert(gname.clone(), Value::Object(names));
    }
    Value::Object(groups)
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
    if g.is_patch_type(type_name) {
        "patch"
    } else if g.is_custom_type(type_name) {
        "custom"
    } else if g.is_plugin_type(type_name) {
        "plugin"
    } else {
        "builtin"
    }
}

/// How much of a palette entry to project. [`Detail::Index`] is what a catalog READ wants — the
/// name and the doc's FIRST LINE, and nothing else; where the type came from, its slots and its
/// params are all `library get`'s. [`Detail::Full`] is the descriptor a client builds nodes from,
/// and every field the index leaves out is present in it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    Index,
    Full,
}

impl Detail {
    pub fn full(self) -> bool {
        self == Detail::Full
    }
}

/// A node doc's FIRST LINE — the nutshell every doc opens with, and all an index shows.
pub fn nutshell(doc: &str) -> &str {
    doc.split('\n').next().unwrap_or(doc).trim_end()
}

pub fn node_type_info(g: &Graph, engine: &'static str, m: &'static NodeManifest, d: Detail) -> Value {
    let ty = goofi_node::qualify(engine, m.type_name);
    let mut info = json!({
        "type": ty,
        "doc": if d.full() { m.doc } else { nutshell(m.doc) },
    });
    if d.full() {
        info["source"] = json!(source_of(g, &ty));
        info["tags"] = json!(m.tags.iter().map(|t| t.as_str()).collect::<Vec<_>>());
        info["available"] = json!(true);
        if let Some(bundle) = g.bundle_of(&ty) {
            info["bundle"] = json!(bundle);
        }
        info["missing_deps"] = json!([]);
        info["editor"] = json!(g.type_has_editor(engine, m.type_name));
        info["input_slots"] = input_slots(m);
        info["input_multi"] = input_multi(m);
        info["output_slots"] = output_slots(m);
        // The owning engine's own normalization, so palette and instance agree.
        info["params"] =
            describe_params(g, engine, &g.default_params_of(&ty, None).unwrap_or_default(), m);
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
            let info = node_type_info(g, engine, l.manifest, d);
            (engine.to_string(), l.manifest.type_name.to_string(), info)
        })
        .collect();
    // Node files that exist but cannot load are listed too, greyed and with the reason — carrying
    // the shape they last had, because the instances born from it are still running and wired.
    let greyed: Vec<(String, String)> = g
        .unavailable_types()
        .map(|(name, reason)| (name.to_string(), reason.to_string()))
        .collect();
    items.extend(greyed.into_iter().map(|(name, reason)| {
        let last = g.last_manifest(&name);
        let (engine, bare) = goofi_node::split_type_id(&name);
        // `available` rides the INDEX too, and only where it is false: a greyed row is the one row
        // a chooser must not skim past, and the doc it carries names the reason in words.
        let mut info = json!({
            "type": name,
            "doc": format!("This node could not be loaded: {reason}"),
            "available": false,
        });
        if d.full() {
            info["source"] = json!(source_of(g, &name));
            info["bundle"] = g.bundle_of(&name).map_or(Value::Null, |b| json!(b));
            info["tags"] = json!([]);
            info["missing_deps"] = json!([reason]);
            info["editor"] = json!(false);
            info["input_slots"] = last.map_or_else(|| json!({}), input_slots);
            info["input_multi"] = last.map_or_else(|| json!([]), input_multi);
            info["output_slots"] = last.map_or_else(|| json!({}), output_slots);
            info["params"] = last.map_or_else(|| json!({}), |m| {
                let params = g.default_params_of(&name, None).unwrap_or_default();
                describe_params(g, engine.unwrap_or_default(), &params, m)
            });
        }
        (engine.unwrap_or_default().to_string(), bare.to_string(), info)
    }));
    items.extend(crate::vocab::boundary_catalog(d));
    // By the order the engines were REGISTERED, not by their names: signal is the plane a patch
    // starts on, and an alphabet would put audio ahead of it.
    let order: Vec<&'static str> = g.engine_ids();
    let rank = |id: &str| order.iter().position(|e| *e == id).unwrap_or(order.len());
    items.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
    Value::Array(items.into_iter().map(|(_, _, v)| v).collect())
}

/// The per-node RUNTIME overlay that never enters the doc. It rides the snapshot because its live
/// stream pushes only transitions. EVERY node is in it — a facade's health is its members' and a
/// port reaches no stage, and a client that had to work either out would be a second owner.
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
/// It carries NO graph structure — that lives in the document alone. `harnesses` is passed rather
/// than read here because its config half is a disk read, which a caller holding the graph lock
/// has already done off it.
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

/// The frontend's generated types: the document, the deltas, the param descriptor and the
/// variable records, each declared once in Rust and checked into the tree.
pub fn typescript() -> String {
    use goofi_core::record::{RecordedOutput, VideoQuality};
    use goofi_core::variables::{Control, ControlKind, Lock, Variable, VariableSource, VariableValue};
    use goofi_graph::doc::{Archive, Group, Link, NodeRecord, ParamEntry, PatchDoc};
    let cfg = ts_rs::Config::new().with_large_int("number");
    let decls = [
        serde_json::Value::decl(&cfg),
        Scalar::decl(&cfg),
        Mode::decl(&cfg),
        ParamEntry::decl(&cfg),
        VideoQuality::decl(&cfg),
        RecordedOutput::decl(&cfg),
        NodeRecord::decl(&cfg),
        Link::decl(&cfg),
        VariableValue::decl(&cfg),
        ControlKind::decl(&cfg),
        Control::decl(&cfg),
        Lock::decl(&cfg),
        VariableSource::decl(&cfg),
        Variable::decl(&cfg),
        Group::decl(&cfg),
        PatchDoc::decl(&cfg),
        Archive::decl(&cfg),
        crate::doc::Op::decl(&cfg),
        ParamShow::decl(&cfg),
        ParamBase::decl(&cfg),
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
