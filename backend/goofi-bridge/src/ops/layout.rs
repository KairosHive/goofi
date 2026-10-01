//! The arrangement: tabs, panels and splits, and where this client is looking.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{op, EffectOp, PanelType, ReadOp, WriteOp};
use crate::{inspect, vocab, AppState, Txn};
use goofi_graph::{Graph, Uid};

// ---- layout inspect (Read)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectArgs {
    pub tab: Option<String>,
}

op!(Inspect, "layout inspect", 1, InspectArgs, Value,
    "The arrangement as a tree: every tab, split and panel with its id, order and share of its parent. How a caller discovers the ids every layout op addresses. `tab` narrows it to one tab; no arg = all of them.",
    "{text: string}");

// ---- layout panel add (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PanelAddArgs {
    pub beside: Option<String>,
    pub side: Option<String>,
    pub ratio: Option<f64>,
    pub name: Option<String>,
    pub index: Option<i64>,
}

op!(PanelAdd, "layout panel add", 0, PanelAddArgs, Value,
    "A fresh empty panel. With `--beside` it divides that panel, on its `left`/`right`/`top`/`bottom` (`--side`, default right), taking `--ratio` of its space (default half). Bare, it lands on a new tab at `--index` in the strip, labelled `--name` — minted (`Tab 2`, `Tab 3`, …) unless you give one.",
    "{id, tab, text} — the born panel, the tab it is on, and the arrangement as `layout inspect` draws it");

// ---- layout panel edit (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PanelEditArgs {
    pub panel: String,
    #[serde(rename = "type")]
    pub ty: Option<PanelType>,
    pub state: Option<Value>,
}

op!(PanelEdit, "layout panel edit", 1, PanelEditArgs, Value,
    "Edit a PANEL's content: its type, its state, or both in one call and one undo. State MERGES key by key — send only what changes, and null to clear a key. A new type clears the old type's state, so send both together to rebind. `type` is one of: {panel_types}. A viewer panel's `state.kind` is one of: {viewer_kinds}; a STRING or TABLE slot ignores it and uses its own.",
    "{text} — the resulting arrangement, as `layout inspect` draws it");

// ---- layout move (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveArgs {
    pub entry: String,
    pub beside: Option<String>,
    pub side: Option<String>,
    pub ratio: Option<f64>,
    #[serde(rename = "in")]
    pub within: Option<String>,
    pub index: Option<i64>,
    pub name: Option<String>,
}

op!(Move, "layout move", 1, MoveArgs, Value,
    "Move a layout entry — a panel, a whole split's subtree, or a tab; one op per drag gesture, so a drop is one undo step. With `--beside` (and `--side`, `--ratio`) it lands beside that panel. With `--in` it lands inside that split, at `--index` among its children. Bare, a TAB moves to `--index` in the strip, and anything else wraps onto a tab of its own, labelled `--name`. Taking a tab's last panel takes the tab with it.",
    "{id, tab, text} — what was moved, the tab it is on, and the arrangement as `layout inspect` draws it");

// ---- layout remove (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveArgs {
    pub entry: String,
}

op!(Remove, "layout remove", 1, RemoveArgs, Value,
    "Close a layout entry: a panel, a whole split's subtree, or a tab and every panel on it. Its space goes to its siblings; a tab keeps its last panel, and the last tab stays.",
    "{text} — the resulting arrangement, as `layout inspect` draws it");

// ---- layout tab edit (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabEditArgs {
    pub tab: String,
    pub name: String,
}

op!(TabEdit, "layout tab edit", 1, TabEditArgs, Value,
    "Relabel a TAB. Its id and every panel on it stand; the strip index is where it sits, which `layout move` owns.",
    "{text} — the resulting arrangement, as `layout inspect` draws it");

// ---- layout split edit (Write)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitEditArgs {
    pub split: String,
    pub fraction: Vec<f64>,
}

op!(SplitEdit, "layout split edit", 1, SplitEditArgs, Value,
    "Set the shares of ALL of a SPLIT's children at once, in child order — what a resize drag commits. Renormalized to fill the slot.",
    "{text} — the resulting arrangement, as `layout inspect` draws it");

// ---- layout viewpoint edit (Effect)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewpointEditArgs {
    pub value: Value,
}

op!(ViewpointEdit, "layout viewpoint edit", 0, ViewpointEditArgs, Value,
    "Store where this client is looking — active tab, maximize, camera, each panel's sub-patch path. ONE stored value, replaced whole, last writer wins; persisted in the `.gfi`, never converged, never dirtying.",
    "{ok: true}");

/// Route a layout planner's per-entry writes through the command history as ONE undo step, and
/// answer with the arrangement they produced, drawn as `layout inspect` draws it.
fn apply_layout(tx: &mut Txn, cmd: goofi_graph::Command) -> Result<Value, String> {
    tx.apply(cmd)?;
    Ok(json!({ "text": inspect::layout_tree(&tx.g, None) }))
}

/// Which side of a target a newcomer lands on. ONE argument, because an axis and a half are two
/// halves of one answer; absent defaults right, and a word that is not a side is refused.
fn parse_side(side: Option<&str>, op: &str) -> Result<goofi_graph::layout::Side, String> {
    match side {
        None => Ok(goofi_graph::layout::Side::Right),
        Some(v) => goofi_graph::layout::Side::parse(v)
            .ok_or_else(|| format!("{op}: side is `left`, `right`, `top` or `bottom`, not {v}")),
    }
}

/// Is `node` something a panel could bind to? A UID, and only a uid: a display name stops resolving
/// the moment somebody renames the node. A boundary port counts — it exposes a real stream.
fn bindable_node(g: &Graph, node: &str) -> bool {
    Uid::from_hex(node).is_some_and(|u| g.exists(u))
}

fn index_of(i: Option<i64>) -> Option<usize> {
    i.and_then(|i| usize::try_from(i).ok())
}

impl ReadOp for Inspect {
    fn run(tx: &mut Txn, a: InspectArgs) -> Result<Value, String> {
        Ok(json!({ "text": inspect::layout_tree(&tx.g, a.tab.as_deref()) }))
    }
}

impl WriteOp for PanelAdd {
    /// A fresh empty panel: beside a target, or on a new tab of its own.
    fn run(tx: &mut Txn, a: PanelAddArgs) -> Result<Value, String> {
        const OP: &str = "layout panel add";
        let ratio = a.ratio.unwrap_or(0.5);
        match a.beside.as_deref() {
            // Beside a target, dividing it — the drop on a panel's edge.
            Some(target) => {
                let side = parse_side(a.side.as_deref(), OP)?;
                let (plan, fresh) = tx.g.arrangement().split_panel(target, side, ratio)?;
                let cmd = goofi_graph::Command::LayoutBirth { plan, born: fresh.clone() };
                let text = apply_layout(tx, cmd)?;
                let tab = tx.g.arrangement().tab_of(&fresh).unwrap_or_default();
                Ok(json!({ "id": fresh, "tab": tab, "text": text["text"] }))
            }
            // On a tab of its own, at `index` in the strip, labelled `name` or minted.
            None => {
                let (plan, tab) = tx.g.arrangement().add_tab(a.name.as_deref(), index_of(a.index), None)?;
                let cmd = goofi_graph::Command::LayoutBirth { plan, born: tab.clone() };
                let text = apply_layout(tx, cmd)?;
                // The root panel's id, which a caller cannot otherwise know.
                let id = tx.g.arrangement().root_of(&tab).unwrap_or_default();
                Ok(json!({ "id": id, "tab": tab, "text": text["text"] }))
            }
        }
    }

    fn label(a: &PanelAddArgs, _: &Value) -> String {
        match a.beside {
            Some(_) => "Split panel".into(),
            None => "Add tab".into(),
        }
    }
}

impl WriteOp for PanelEdit {
    /// Edit a PANEL's content: its type, its state, or both — one call, one undo.
    fn run(tx: &mut Txn, a: PanelEditArgs) -> Result<Value, String> {
        let ty = a.ty.map(|t| t.0);
        let panel_state = a.state.filter(|v| !v.is_null());
        if ty.is_none() && panel_state.is_none() {
            return Err("layout panel edit: give a type, a state, or both".into());
        }
        // A panel bound to a node that is not there renders empty and explains nothing.
        let named = panel_state
            .as_ref()
            .and_then(|s| s.get("node"))
            .and_then(|v| v.as_str())
            .filter(|n| !n.is_empty());
        if let Some(node) = named {
            if !bindable_node(&tx.g, node) {
                return Err(format!("layout panel edit: no node `{node}` in this patch"));
            }
        }
        // The slot is checked against the node this write LEAVES the panel bound to: its own, or
        // the one already stored, since a state write merges.
        let bound = named
            .or_else(|| {
                tx.g.arrangement().panel_state(&a.panel).and_then(|s| s.get("node")).and_then(|v| v.as_str())
            })
            .and_then(Uid::from_hex);
        vocab::check_panel(&tx.state.plugins, &tx.g, ty.as_deref(), panel_state.as_ref(), bound)?;
        // A control panel is born naming a group of its own, `control0`, `control1`, … — the first
        // that nothing holds — so no panel ever waits on a name.
        let panel_state = match (ty.as_deref(), panel_state) {
            (Some("control"), state) if state.as_ref().and_then(|s| s.get("group")).is_none() => {
                let taken = tx.g.arrangement().control_panels();
                let fresh = (0..)
                    .map(|n| format!("control{n}"))
                    .find(|c| !tx.g.variables().has_group(c) && !taken.iter().any(|(_, held)| held == c))
                    .expect("the integers do not run out");
                let mut s = state.and_then(|s| s.as_object().cloned()).unwrap_or_default();
                s.insert("group".into(), json!(fresh));
                Some(Value::Object(s))
            }
            (_, state) => state,
        };
        let writes = tx.g.arrangement().set_panel(&a.panel, ty.as_deref(), panel_state)?;
        apply_layout(tx, goofi_graph::Command::LayoutContents { writes })
    }

    fn label(a: &PanelEditArgs, _: &Value) -> String {
        match &a.ty {
            Some(t) => format!("Change panel to {}", t.0),
            None => "Edit panel".into(),
        }
    }
}

impl WriteOp for Move {
    /// Move a layout entry — a panel, a subtree or a tab; ONE op per drag gesture, so a drop is
    /// one undo step and peers never see an arrangement that was not on somebody's screen.
    fn run(tx: &mut Txn, a: MoveArgs) -> Result<Value, String> {
        const OP: &str = "layout move";
        let entry = a.entry;
        let index = index_of(a.index);
        let ratio = a.ratio.unwrap_or(0.5);
        // A tab already has a tab, so a destination-less move is a reorder rather than a wrap. The
        // id says which — as it does for the edit trio and `layout remove`.
        let is_tab = tx.g.arrangement().tab_index(&entry).is_some();
        let (plan, placed) = match (a.beside.as_deref(), a.within.as_deref()) {
            (Some(_), Some(_)) => {
                return Err(format!("{OP}: `--beside` and `--in` are two destinations — give one"))
            }
            // Beside a target, dividing it; the side defaults right, as a birth's does.
            (Some(target), None) => {
                let side = parse_side(a.side.as_deref(), OP)?;
                (tx.g.arrangement().insert_at_panel(&entry, target, side, ratio)?, entry.clone())
            }
            // Inside a split, at an index — the drop into a container that exists.
            (None, Some(parent)) => {
                (tx.g.arrangement().move_subtree(&entry, parent, index.unwrap_or(0))?, entry.clone())
            }
            (None, None) if is_tab => {
                let at = index.ok_or(format!("{OP}: a tab moves to an `--index` in the strip"))?;
                tx.g.arrangement().reorder_tab(&entry, at)?;
                let cmd = goofi_graph::Command::LayoutReorderTab { tab: entry.clone(), to_index: at };
                let text = apply_layout(tx, cmd)?;
                return Ok(json!({ "id": entry, "tab": entry, "text": text["text"] }));
            }
            // Onto a tab of its own — the drag onto the tab bar. A tab built AROUND an existing
            // subtree is a MOVE, so its undo gives the subtree back.
            (None, None) => {
                let (plan, tab) = tx.g.arrangement().add_tab(a.name.as_deref(), index, Some(&entry))?;
                let cmd = goofi_graph::Command::LayoutMove { plan: Some(plan), root: entry.clone(), home: None };
                let text = apply_layout(tx, cmd)?;
                return Ok(json!({ "id": entry, "tab": tab, "text": text["text"] }));
            }
        };
        let cmd = goofi_graph::Command::LayoutMove { plan: Some(plan), root: placed.clone(), home: None };
        let text = apply_layout(tx, cmd)?;
        let tab = tx.g.arrangement().tab_of(&placed).unwrap_or_default();
        Ok(json!({ "id": placed, "tab": tab, "text": text["text"] }))
    }

    fn label(_: &MoveArgs, _: &Value) -> String {
        "Move panel".into()
    }
}

impl WriteOp for Remove {
    fn run(tx: &mut Txn, a: RemoveArgs) -> Result<Value, String> {
        // A tab is closed whole; anything else is closed with promote. Planned here only so a bad
        // id answers teachably: `LayoutClose` re-plans it under this same lock, and DEGRADES
        // rather than errors.
        match tx.g.arrangement().tab_index(&a.entry) {
            Some(_) => tx.g.arrangement().remove_tab(&a.entry)?,
            None => tx.g.arrangement().remove_subtree(&a.entry)?,
        };
        apply_layout(tx, goofi_graph::Command::LayoutClose { born: a.entry })
    }

    fn label(_: &RemoveArgs, _: &Value) -> String {
        "Close panel".into()
    }
}

impl WriteOp for TabEdit {
    /// Relabel a TAB — refused for any other kind of id, because an `edit` op edits ONE kind.
    fn run(tx: &mut Txn, a: TabEditArgs) -> Result<Value, String> {
        let writes = tx.g.arrangement().rename_tab(&a.tab, &a.name)?;
        apply_layout(tx, goofi_graph::Command::LayoutContents { writes })
    }

    fn label(a: &TabEditArgs, _: &Value) -> String {
        format!("Rename tab to {}", a.name)
    }
}

impl WriteOp for SplitEdit {
    /// Set the shares of ALL of a SPLIT's children at once — what a resize drag commits.
    fn run(tx: &mut Txn, a: SplitEditArgs) -> Result<Value, String> {
        // Planned here only so a bad split or a wrong fraction count answers teachably; the
        // command re-plans it under this same lock.
        tx.g.arrangement().resize_split(&a.split, &a.fraction)?;
        apply_layout(tx, goofi_graph::Command::LayoutResizeSplit { split: a.split, fractions: a.fraction })
    }

    fn label(_: &SplitEditArgs, _: &Value) -> String {
        "Resize split".into()
    }
}

impl EffectOp for ViewpointEdit {
    /// Where THIS client is looking: not a doc root, so it neither drags a peer nor raises the
    /// unsaved dot, but it still rides the `.gfi` and `hello`.
    fn run(state: &AppState, a: ViewpointEditArgs, _: &str) -> Result<Value, String> {
        state.graph.lock().set_viewpoint(a.value);
        // No projection: the viewpoint is the manifest's alone. The pulse is for the autosave,
        // which takes the new viewpoint on its next tick of an already dirty patch.
        state.changed.notify();
        Ok(json!({ "ok": true }))
    }
}
