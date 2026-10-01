//! One JSON projection of the engine `Graph` — exactly the shape of the control-plane document.

use goofi_graph::Graph;
use serde_json::{json, Map, Value};

/// `g`'s whole control-plane state, in the shape a `.gfi` holds it: one node map carrying leaves,
/// sub-patch facades and boundary ports alike, one link list, variables, and the panel arrangement.
/// The sub-patch forest is not a block of its own — a member names its scope, and that is the only
/// place membership lives.
pub fn of(g: &Graph) -> Value {
    let mut nodes = Map::new();
    // ONE loop over ONE namespace: a leaf, a facade and a boundary port are all node records, and
    // what differs between them is only what they HAVE — a facade and a port run nothing, so the
    // params key is simply empty, as it is on a node with none.
    for uid in g.all_uids() {
        let mut node = Map::new();
        node.insert("type".into(), json!(g.node_type(uid).unwrap_or_default()));
        node.insert("name".into(), json!(g.name(uid).unwrap_or("")));
        node.insert("pos".into(), json!(g.pos(uid).unwrap_or([0.0, 0.0])));
        let mut params = Map::new();
        if let Some(ps) = g.params(uid) {
            for (group, pg) in &ps {
                let mut gmap = Map::new();
                for (pname, p) in pg {
                    let mut entry = Map::new();
                    // A pulse has no value, and a merge patch spends `null` on "delete this key" —
                    // so the key stays OUT rather than reaching a replica as a deletion.
                    if !matches!(p, goofi_core::Param::Pulse) {
                        entry.insert("value".into(), goofi_graph::param_value_json(p));
                    }
                    if let Some(s) = g.param_source(uid, group, pname) {
                        entry.insert("mode".into(), json!(s.state.mode));
                        if !s.state.expression.is_empty() {
                            entry.insert("expression".into(), json!(s.state.expression));
                        }
                        if !s.state.reference.is_empty() {
                            entry.insert("reference".into(), json!(s.state.reference));
                        }
                        if s.state.triggers {
                            entry.insert("triggers".into(), json!(true));
                        }
                    }
                    gmap.insert(pname.clone(), Value::Object(entry));
                }
                params.insert(group.clone(), Value::Object(gmap));
            }
        }
        node.insert("params".into(), Value::Object(params));
        // The same `json_string` shape every kind's viewers ride in: a merge patch spends `null`
        // on a key delete, so a viewer blob must not reach the document as a tree of leaves.
        if let Some(v) = g.viewers(uid).filter(|v| v.as_object().is_some_and(|m| !m.is_empty())) {
            node.insert("viewers".into(), json!(v.to_string()));
        }
        // The touched filter's zero point, in that same `json_string` shape and for the same reason:
        // its keys are `group/param`, so as a tree of leaves a merge patch could not tell one
        // param's zero being deleted from the whole blob being replaced.
        if let Some(v) = g.baseline(uid).filter(|v| v.as_object().is_some_and(|m| !m.is_empty())) {
            node.insert("baseline".into(), json!(v.to_string()));
        }
        node.insert("record".into(), json!(g.recorded(uid).unwrap_or(&[])));
        nodes.insert(uid.to_hex(), Value::Object(node));
    }
    // Membership rides the member. Absent means ROOT — never a null, which a merge patch spends on
    // "delete this key" and could not tell from a move out of a scope.
    for (uid, parent) in g.all_uids().into_iter().filter_map(|u| g.scope_of(u).map(|p| (u, p))) {
        if let Some(Value::Object(rec)) = nodes.get_mut(&uid.to_hex()) {
            rec.insert("scope".into(), json!(parent.to_hex()));
        }
    }

    // Keyed as the archive keys them, so a wire made or cut is one key's delta.
    let links: Map<String, Value> = g
        .links_view()
        .into_iter()
        .map(|l| {
            let link = goofi_graph::doc::Link {
                node_out: l.node_out.to_hex(), slot_out: l.slot_out.to_string(),
                node_in: l.node_in.to_hex(), slot_in: l.slot_in.to_string(),
            };
            (link.key(), serde_json::to_value(&link).expect("a plain record"))
        })
        .collect();

    let mut variables = Map::new();
    for (name, v) in g.variables().entries() {
        let mut entry = serde_json::to_value(v).expect("a plain record");
        // Why a source delivers nothing rides beside it, so the panel can say so.
        if let Some(error) = v.source.as_ref().and_then(|s| g.variable_source_error(s)) {
            entry["source"]["error"] = Value::String(error);
        }
        variables.insert(name.to_string(), entry);
    }
    let variable_groups: Map<String, Value> =
        g.variables().groups().map(|(group, lock)| (group.to_string(), json!({ "lock": lock }))).collect();

    json!({ "nodes": nodes, "links": links,
        "variables": variables, "variable_groups": variable_groups, "arrangement": g.arrangement().to_json() })
}
