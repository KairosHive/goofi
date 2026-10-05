//! The read ops an agent uses to see what it built: a scope as a diagram, a node as a page of text.

use std::path::{Path, PathBuf};

use goofi_graph::{Graph, Uid};
use serde_json::{json, Value};

/// A node as a mermaid id: its NAME, which is what an op takes back. The uid is the fallback, and
/// mermaid ids may not start with a digit, hence the leading `n`.
fn mid(g: &Graph, uid: Uid) -> String {
    match g.name(uid) {
        Some(name) => name.to_string(),
        None => format!("n{}", uid.to_hex()),
    }
}

/// A display name safe inside a mermaid `"…"` label.
fn label(name: &str) -> String {
    name.replace(['"', '\n'], "'")
}

/// The direct members of a scope; `None` is the root, which holds everything `scope_of` places
/// nowhere else.
fn members(g: &Graph, scope: Option<Uid>) -> Vec<Uid> {
    match scope {
        Some(s) => g.scope_members(s),
        None => g.all_uids().into_iter().filter(|u| g.scope_of(*u).is_none()).collect(),
    }
}

/// Which member of `scope` contains `uid` — itself, or the ancestor facade it lives inside.
fn member_in(g: &Graph, scope: Option<Uid>, uid: Uid) -> Option<Uid> {
    let mut at = uid;
    loop {
        if g.scope_of(at) == scope {
            return Some(at);
        }
        at = g.scope_of(at)?;
    }
}

/// A scope's full path from the root, as display names.
fn scope_path(g: &Graph, scope: Uid) -> String {
    let mut parts = vec![g.name(scope).unwrap_or("?").to_string()];
    let mut at = scope;
    while let Some(parent) = g.scope_of(at) {
        parts.push(g.name(parent).unwrap_or("?").to_string());
        at = parent;
    }
    parts.reverse();
    parts.join("/")
}

/// A node's full path, so an error listing says where the node is.
fn node_path(g: &Graph, uid: Uid) -> String {
    let name = g.name(uid).map(str::to_string).unwrap_or_else(|| uid.to_hex());
    match g.scope_of(uid) {
        Some(s) => format!("{}/{name}", scope_path(g, s)),
        None => name,
    }
}

/// How long an error has stood, to one decimal of a second.
fn age(g: &Graph, uid: Uid) -> String {
    match g.error_age(uid) {
        Some(d) => format!(" — for {:.1}s", d.as_secs_f64()),
        None => String::new(),
    }
}

/// `nodes inspect`: ONE scope drawn as a mermaid flowchart. Scope-wide and nothing more — the
/// patch's identity and standing errors are `session status`'s.
pub fn patch(g: &Graph, scope: Option<Uid>) -> Result<String, String> {
    if let Some(s) = scope {
        if !g.is_facade(s) {
            return Err(format!("no sub-patch `{}`", crate::named(g, s)));
        }
    }
    let mut out = format!(
        "scope: {}\n",
        scope.map_or("root".to_string(), |s| scope_path(g, s)),
    );

    let member = members(g, scope);
    if member.is_empty() {
        out.push_str("\n(no nodes)\n");
    } else {
        out.push_str("\n```mermaid\nflowchart LR\n");
        // ONE loop: a port, a facade and a leaf are all members, and only the SHAPE they are drawn
        // in differs — which is the one distinction a diagram is allowed to make.
        for &uid in &member {
            let name = label(g.name(uid).unwrap_or("?"));
            let ty = g.node_type(uid).unwrap_or_else(|| "?".into());
            let warn = if g.last_error(uid).is_some() { "⚠ " } else { "" };
            if g.stub(uid).is_some() {
                out.push_str(&format!("  {}([\"{warn}{name}: {ty}\"])\n", mid(g, uid)));
            } else if g.is_facade(uid) {
                out.push_str(&format!("  {}[[\"{warn}{name}\"]]\n", mid(g, uid)));
            } else {
                out.push_str(&format!("  {}[\"{warn}{name}: {ty}\"]\n", mid(g, uid)));
            }
        }
        // Runtime links are flat leaf→leaf, so each end folds onto the member of THIS scope that
        // contains it.
        let mut edges: Vec<String> = Vec::new();
        for l in g.links_view() {
            let (Some(a), Some(b)) =
                (member_in(g, scope, l.node_out), member_in(g, scope, l.node_in))
            else {
                continue;
            };
            // Both ends on ONE member is a wire internal to a collapsed sub-patch — unless the
            // member IS both endpoints, which is a real self-loop at this level.
            if a == b && (l.node_out != a || l.node_in != b) {
                continue;
            }
            let e = format!("  {} -- {}→{} --> {}\n", mid(g, a), l.slot_out, l.slot_in, mid(g, b));
            if !edges.contains(&e) {
                edges.push(e);
            }
        }
        out.extend(edges);
        out.push_str("```\n\nnames: a node's mermaid id is its name, which every op takes.\n");
    }

    Ok(out)
}

/// Project every standing node and machine failure into the session inspection.
pub fn errors(g: &Graph) -> Vec<Value> {
    let mut errors: Vec<Value> = g.node_uids()
        .into_iter()
        .filter(|u| g.last_error(*u).is_some())
        .map(|uid| {
            serde_json::json!({
                "node": crate::named(g, uid),
                "path": node_path(g, uid),
                "error": g.last_error(uid).unwrap_or(""),
                "standing": g.error_age(uid).map(|d| d.as_secs_f64()),
            })
        })
        .collect();
    for (machine, health) in g.machine_health() {
        for issue in health.expressions {
            errors.push(serde_json::json!({
                "machine": machine,
                "playhead": issue.playhead,
                "transition": issue.transition,
                "surface": issue.surface,
                "expression": issue.expression,
                "error": issue.error,
                "standing": null,
            }));
        }
        for (playhead, error) in health.playheads {
            errors.push(serde_json::json!({
                "machine": machine,
                "playhead": playhead,
                "error": error,
                "standing": null,
            }));
        }
    }
    errors
}

fn param_line(p: &goofi_core::Param, source: Option<&goofi_graph::SourceInfo>) -> String {
    use goofi_core::Param as P;
    let (value, ty) = match p {
        P::Num { value, vmin, vmax, int, color, .. } => {
            let shown: Vec<String> = value.iter().map(|v| if *int { (v.round() as i64).to_string() } else { v.to_string() }).collect();
            let kind = if *color { "color" } else if *int { "int" } else { "num" };
            let dims = if value.len() > 1 { format!("{}d ", value.len()) } else { String::new() };
            (if value.len() > 1 { format!("[{}]", shown.join(", ")) } else { shown.join("") }, format!("{dims}{kind} {vmin}..{vmax}"))
        }
        P::Bool { value } => (format!("{value}"), "bool".to_string()),
        P::Str { value, options: Some(o), .. } => {
            (format!("\"{value}\""), format!("string one of [{}]", o.join(", ")))
        }
        P::Str { value, .. } => (format!("\"{value}\""), "string".to_string()),
        P::Pulse => ("—".to_string(), "pulse".to_string()),
    };
    let error = |s: &goofi_graph::SourceInfo| {
        s.error.as_ref().map(|e| format!(" [error: {e}]")).unwrap_or_default()
    };
    match source {
        Some(s) if s.state.mode == goofi_graph::Mode::Expression => {
            format!("expr: {} → {value}{}", s.state.expression, error(s))
        }
        _ => format!("{value} ({ty})"),
    }
}

/// `node state`: what the node is, what its params say, which output slots it has and whether it
/// is emitting on them. The frames themselves are only on `/data/<node>/<slot>`.
pub fn node(
    g: &Graph,
    uid: Uid,
    slot: Option<&str>,
    want_params: bool,
    want_error: bool,
) -> Result<String, String> {
    let type_name = g.node_type(uid)
        .ok_or_else(|| format!("no node `{}`", uid.to_hex()))?;
    // A port and a facade never run, so they wear no tier and reach no stage; everything else a
    // read says about a node, they answer.
    let runtime = match g.node_tier(uid) {
        Some(tier) => format!(", {}, stage {}", tier.wire(), g.node_stage(uid)),
        None => String::new(),
    };
    let mut out =
        format!("{}: {type_name} (uid {}{runtime})\n", g.name(uid).unwrap_or("?"), uid.to_hex());
    // `(key, label, dtype)`: a facade keys its slots by port uid so a rename cannot break a wire,
    // and carries the port's display name beside it — so nothing here re-derives a label.
    let outputs = crate::vocab::output_slots(g, uid);
    if let Some(s) = slot {
        crate::vocab::resolve_slot(g, uid, s)?;
    }

    if want_params {
        out.push_str("\nparams:\n");
        // A driven param reads as what its source last evaluated to, the literal standing in.
        let live: std::collections::HashMap<(String, String), goofi_core::Param> =
            g.driven_values(uid).into_iter().map(|(gr, n, p)| ((gr.to_string(), n.to_string()), p.clone())).collect();
        for (group, names) in g.params(uid).iter().flat_map(|p| p.iter()) {
            for (name, p) in names {
                let source = g.param_source(uid, group, name);
                let mut shown = g.shown_param(uid, group, name, p);
                if let Some(v) = live.get(&(group.clone(), name.clone())) {
                    shown = v.clone();
                }
                // An element with a source of its own gets a line under its param, and its value
                // reads in the param's line too.
                let mut elements = Vec::new();
                for k in 0..shown.dims() {
                    let element = format!("{name}[{k}]");
                    if let Some(source) = g.param_source(uid, group, &element) {
                        let dim = live.get(&(group.clone(), element.clone())).cloned().unwrap_or_else(|| shown.dim(k));
                        if let Some(v) = dim.as_f64() {
                            let mut values = shown.as_vec().unwrap_or_default().to_vec();
                            values[k] = v;
                            shown = goofi_core::control::read(&goofi_core::Data::numbers(values), &shown);
                        }
                        elements.push(format!("    {group}.{element} = {}\n", param_line(&dim, Some(&source))));
                    }
                }
                out.push_str(&format!("  {group}.{name} = {}\n", param_line(&shown, source.as_ref())));
                for line in elements {
                    out.push_str(&line);
                }
            }
        }
    }

    out.push_str("\noutputs:\n");
    if outputs.is_empty() {
        out.push_str("  (none)\n");
    }
    for (key, name, kind) in outputs.iter().filter(|(k, l, _)| slot.is_none_or(|s| s == k || s == l)) {
        // `ufreq` measures how often a node RUNS, so it is read off whichever node the frames
        // really come from — itself for a leaf, the node behind it for a port.
        let rate = match g.node_ufreq(crate::stream_behind(g, uid, key).map_or(uid, |(leaf, _)| leaf)) {
            Some(hz) => format!("emitting at {hz:.1} Hz"),
            None => "nothing emitted yet".to_string(),
        };
        out.push_str(&format!("  {name}: {kind} — {rate}\n"));
    }

    if want_error {
        out.push_str(&match g.last_error(uid) {
            Some(e) => format!("\nerror: {e}{}\n", age(g, uid)),
            None => "\nerror: none\n".to_string(),
        });
    }
    Ok(out)
}

/// One variable as `variable list` and `control list` answer it.
pub(crate) fn variable_json(g: &Graph, store: &goofi_core::variables::VariableStore, name: &str, v: &goofi_core::variables::Variable) -> Value {
    let mut e = json!({ "name": name, "value": v.value });
    // What holds it, its own lock and its group's together — the answer a writer needs.
    e["lock"] = json!(store.lock_of(name));
    if let Some(c) = &v.control {
        e["control"] = json!(c);
    }
    if let Some(x) = &v.expression {
        e["expression"] = json!(x);
        if let Some(error) = g.variable_binding_error(name).or_else(|| v.error.clone()) {
            e["error"] = json!(error);
        }
    }
    e
}

/// `variable list`: what an expression can read and the variable writes can set.
pub fn variables(g: &Graph) -> Value {
    let store = g.variables();
    let entries: Vec<Value> = store.entries().map(|(name, v)| variable_json(g, &store, name, v)).collect();
    let groups: serde_json::Map<String, Value> =
        store.groups().map(|(group, rec)| (group.to_string(), json!(rec))).collect();
    json!({ "variables": entries, "groups": groups })
}

/// `library get`: one type's provenance, what its file hides, and with `source` the file itself
/// under `text`, since the entry's own `source` is where the TYPE came from.
pub fn node_source(
    g: &Graph,
    ty: &str,
    mount: &Path,
    roots: &[(PathBuf, goofi_graph::Origin)],
    source: bool,
) -> Result<Value, String> {
    let (engine, entry) = g.resolve_type(ty)?;
    let ty = &goofi_node::qualify(engine, entry.manifest.type_name);
    let mut info = crate::schemas::palette_row(g, engine, ty, Some(entry.manifest), None, crate::schemas::Detail::Full);
    // `.rev()` is load-bearing: `rescan` scans the roots forwards and lets each overwrite the
    // last, so a first-match search walks them backwards.
    let workspace: Vec<PathBuf> = g.engine_ids().into_iter().map(|id| mount.join(goofi_node::folder_of(id))).collect();
    let word = |o: &goofi_graph::Origin| match o {
        goofi_graph::Origin::Patch => "patch",
        goofi_graph::Origin::Custom => "custom",
        goofi_graph::Origin::Root(_) | goofi_graph::Origin::Plugin => "shipped",
    };
    let dirs = workspace
        .into_iter()
        .filter(|_| g.origin(ty) == Some(&goofi_graph::Origin::Patch))
        .map(|d| (d, "patch"))
        .chain(roots.iter().rev().map(|(d, o)| (d.clone(), word(o))));
    // The file names the type, so the path re-derives without a registry; the registry says only
    // whether the patch's folder is where it lives.
    let mut files = dirs.into_iter().filter_map(|(dir, provenance)| {
        Some((crate::node_file_in(&dir, goofi_node::bare(ty), engine)?, provenance))
    });
    let found = files.next();
    let tier = g.type_tier(ty);
    info["language"] = json!(tier.map(goofi_node::Isolation::language));
    info["tier"] = json!(tier.map(goofi_node::Isolation::wire));
    info["provenance"] = json!(match &found {
        Some((_, p)) => *p,
        None => "no source file under any node root",
    });
    info["path"] =
        found.as_ref().map(|(p, _)| json!(goofi_core::path::to_slash(p))).unwrap_or(Value::Null);
    // What the winner HIDES: the same type's file in every root behind it — which is what a
    // `library save` would land on, and why an edit to one of them changes nothing.
    info["shadowed"] = files
        .map(|(p, provenance)| json!({ "provenance": provenance, "path": goofi_core::path::to_slash(&p) }))
        .collect();
    if source {
        info["text"] = found
            .as_ref()
            .and_then(|(p, _)| std::fs::read_to_string(p).ok())
            .map(Value::String)
            .unwrap_or(Value::Null);
    }
    Ok(info)
}

use goofi_graph::layout::Node;

/// One node's line in the arrangement tree, and its children under it.
fn layout_line(g: &Graph, n: &Node, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    match n {
        Node::Split { id, size, axis, children } => {
            out.push_str(&format!("{pad}{} split {size:.2}  [{id}]\n", axis.name()));
            for c in children {
                layout_line(g, c, depth + 1, out);
            }
        }
        Node::Panel { id, size, panel_type, state } => {
            // The doc binds a panel by uid; a reader is shown the name, which is what rebinds it.
            let bound = state.get("node").and_then(|v| v.as_str()).map(|b| {
                format!(" → {}", g.resolve_ref(b).map_or_else(|| b.to_string(), |u| crate::named(g, u)))
            });
            out.push_str(&format!("{pad}{panel_type}{} {size:.2}  [{id}]\n", bound.unwrap_or_default()))
        }
    }
}

pub fn layout_tree(g: &Graph, tab: Option<&str>) -> String {
    let l = g.arrangement();
    let mut out = String::from(
        "The editor arrangement. Every entry — tab, split and panel — is addressed by the id in []. \
         The number on each entry is its share of its parent — what `layout split edit` sets.\n\n",
    );
    let tabs = match tab {
        Some(t) => vec![t.to_string()],
        None => l.tabs(),
    };
    for t in tabs {
        let Some(name) = l.name_of(&t) else { continue };
        out.push_str(&format!("tab `{name}`  [{t}]\n"));
        if let Some(root) = l.root_of(&t).and_then(|r| l.node(&r).cloned()) {
            layout_line(g, &root, 1, &mut out);
        }
    }
    out
}
