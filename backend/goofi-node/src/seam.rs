//! The engine seam: the trait an engine registers behind, and the settled view its settle reads.
//! The graph looks down at this and nothing engine-specific; an engine looks down at this and the
//! transport, and never at the graph.

use goofi_core::record::RecordedOutput;
use std::any::Any;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use goofi_core::Param;
use indexmap::IndexMap;

use crate::{
    BindingId, ExprEvaluator, Isolation, IsolationCell, NodeManifest, ParamDecl, ParamGroups,
    ParamKey, Status, Uid,
};

/// A doorbell id: `0` is a control message, `1..=64` the index of an input slot in
/// `manifest.inputs` (the slots past 63 share 64), `65..=128` an expression channel.
pub type EventId = u8;

/// One resolved expression variable, graph-side: the model's spelling, which a view exposes and
/// an engine projects onto its own wire vocabulary.
#[derive(Clone, Debug, PartialEq)]
pub enum BoundVar {
    /// A producer's output slot, and the doorbell id it rings this consumer with.
    Stream { var: String, producer: Uid, slot: &'static str, event_id: EventId },
    /// A `variables.*` read, resolved and shipped inline — a variables edit re-sends the binding.
    Value { var: String, value: Param },
    /// The graph could not resolve it: an unknown node, a slot that does not exist, an ambiguous
    /// bare `nd()` on a multi-output producer, a variable that is not defined.
    Missing { var: String, reason: String },
}

impl BoundVar {
    /// The producer wire a stream variable subscribes, `None` for the other kinds.
    pub fn wire(&self) -> Option<(Uid, &'static str)> {
        match self {
            BoundVar::Stream { producer, slot, .. } => Some((*producer, slot)),
            _ => None,
        }
    }
}

/// Wakes the status drain the moment any engine has something to hand over — the alternative to a
/// poll-to-discover. Every node report notifies; the worker parks on it between paced duties.
#[derive(Default)]
pub struct DrainWaker(goofi_supervisor::timing::Event);

impl DrainWaker {
    pub fn notify(&self) {
        self.0.ring(0);
    }

    /// Park until a notify or `timeout`, and consume the wake either way. Answers whether a
    /// notify came, so a caller can tell a quiet window from a wake.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        !self.0.wait_for(timeout).is_empty()
    }

    /// Park until a notify or `deadline`; whether a notify came.
    pub fn wait_until(&self, deadline: std::time::Instant) -> bool {
        !self.0.wait_until(deadline).is_empty()
    }

    /// Park until no notify has come for `quiet`, or `cap` passes; whether one came at all.
    pub fn wait_quiet(&self, quiet: Duration, cap: goofi_supervisor::timing::Deadline) -> bool {
        !self.0.wait_quiet(quiet, cap).is_empty()
    }
}

/// One port-resolved leaf-to-leaf edge, in link order — which IS a multi input's wire order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Edge {
    pub producer: (Uid, &'static str),
    pub consumer: (Uid, &'static str),
}

/// A file's size and mtime — what a rescan diffs. `None` when it could not be stat'd, which
/// compares equal to itself and so reads as "unchanged".
pub type Stamp = (u64, std::time::SystemTime);

/// One node file's outcome from an engine's scan of its folder.
pub struct ScannedType {
    pub type_name: String,
    pub stamp: Option<Stamp>,
    pub outcome: Scanned,
}

pub enum Scanned {
    /// Registered; `replaced` says an earlier file already held the name, and this one now does.
    Registered { isolation: Isolation, replaced: bool },
    /// Not loadable; the palette lists the type greyed with this reason.
    Unavailable(String),
}

/// One binding as settle ships it: the derived state an engine reads, never the authored record.
pub struct BindingView<'a> {
    pub key: &'a ParamKey,
    pub rewritten: &'a str,
    pub vars: &'a [BoundVar],
    pub id: Option<BindingId>,
    /// Whether the graph ships it — a disabled or unbindable binding leaves the literal standing.
    pub live: bool,
}

/// One running node as the settled view carries it.
pub struct NodeView<'a> {
    pub engine: &'static str,
    pub name: &'a str,
    pub generation: u64,
    pub manifest: &'static NodeManifest,
    pub params: &'a ParamGroups,
    pub bindings: Vec<BindingView<'a>>,
    /// The output slots armed for recording.
    pub recorded: &'a [RecordedOutput],
    /// The output slots a reducer watches: each rings the slot's view door once its frame is out.
    pub watched: Vec<&'static str>,
}

/// One live stream binding reading a producer output: the consumer, the index of the binding on
/// it, and the doorbell id it is rung with.
type Reader = (Uid, usize, EventId);

/// The settled graph, as every engine reads it after a batch: the WHOLE graph, indexed by node
/// at both ends of every wire and every stream binding, so a node's question costs its degree.
pub struct GraphView<'a> {
    pub instance: &'a str,
    pub edges: &'a [Edge],
    pub nodes: HashMap<Uid, NodeView<'a>>,
    pub variables: &'a goofi_core::variables::VariableStore,
    /// The edges into each consumer, in wire order.
    into: HashMap<Uid, Vec<Edge>>,
    /// The edges out of each producer, in wire order.
    out_of: HashMap<Uid, Vec<Edge>>,
    /// Per producer output, every live stream binding reading it: the consumer, the index of the
    /// binding on it, and the doorbell id.
    readers: HashMap<(Uid, &'static str), Vec<Reader>>,
}

impl<'a> GraphView<'a> {
    pub fn new(
        instance: &'a str,
        edges: &'a [Edge],
        nodes: HashMap<Uid, NodeView<'a>>,
        variables: &'a goofi_core::variables::VariableStore,
    ) -> GraphView<'a> {
        let mut into: HashMap<Uid, Vec<Edge>> = HashMap::new();
        let mut out_of: HashMap<Uid, Vec<Edge>> = HashMap::new();
        for e in edges {
            into.entry(e.consumer.0).or_default().push(*e);
            out_of.entry(e.producer.0).or_default().push(*e);
        }
        let mut readers: HashMap<(Uid, &'static str), Vec<Reader>> = HashMap::new();
        for (uid, n) in &nodes {
            for (i, b) in n.bindings.iter().enumerate().filter(|(_, b)| b.live) {
                for v in b.vars {
                    if let BoundVar::Stream { producer, slot, event_id, .. } = v {
                        readers.entry((*producer, slot)).or_default().push((*uid, i, *event_id));
                    }
                }
            }
        }
        GraphView { instance, edges, nodes, variables, into, out_of, readers }
    }

    /// A consumer input slot's desired producers, in wire order.
    pub fn wires_into<'s>(
        &'s self,
        uid: Uid,
        slot: &'s str,
    ) -> impl Iterator<Item = (Uid, &'static str)> + 's {
        self.into.get(&uid).into_iter().flatten().filter(move |e| e.consumer.1 == slot).map(|e| e.producer)
    }
}

/// One value a node reported of its own: what the document is asked to hold.
pub struct Edit {
    pub uid: Uid,
    pub key: ParamKey,
    pub value: Param,
    pub origin: Origin,
}

/// What moved a value a node reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A hand in the node's own window: authored, through the param op, with its undo step.
    Authored,
    /// The node itself, as a preset does: the patch changed, with no step to undo.
    Changed,
    /// The node's saved state, restored: the document shows it, and the patch is as it was.
    Restored,
}

/// What [`Engine::editor`] hands back: the open or close itself, run once the graph lock is
/// released, because a plugin's window takes its time to come up and every other op would wait.
pub type EditorAction = Box<dyn FnOnce() -> Result<bool, String> + Send>;

/// One node class an engine advertises: the shared manifest plus the display tier. The engine a
/// type belongs to is WHICH library advertises it — no tag field exists anywhere.
#[derive(Clone, Copy)]
pub struct LibraryEntry {
    pub manifest: &'static NodeManifest,
    /// The display tier's live cell — a Python type's runtime demotion reads through it.
    pub isolation: &'static IsolationCell,
}

/// What a node is asked to do once, on its own thread, and stores nothing of. The `node param
/// request` op is this, word for word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub kind: RequestKind,
    pub key: ParamKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestKind {
    /// Re-enumerate a refreshable `Str` param's options; they come back as a `RefreshOptions` status.
    Refresh,
    /// Fire a pulse param.
    Pulse,
}

/// An engine: the runtime authority for its nodes' instances, health, transport and library. The
/// graph applies every op to the MODEL and propagates through these in-memory doors.
pub trait Engine: Send {
    /// The id a registration is keyed by, and the palette's provenance for this library.
    fn id(&self) -> &'static str;
    /// Whether this engine's drain marked work only a settle can finish — a Ready it collected,
    /// an ack that completed a phase. What makes the memo rule honest.
    fn dirty(&self) -> bool;
    /// Every node class this engine can build, advertised on request.
    fn library(&self) -> Vec<LibraryEntry>;
    /// What a scan of `dir` would wait on that needs no engine — a probe, a build — as work the
    /// host runs off the graph lock first. None where the scan has nothing to wait on.
    fn prepare(&self, _dir: &Path) -> Option<Box<dyn FnOnce() + Send>> {
        None
    }
    /// Scan ONE folder of this engine's node files and register what loads, a later file taking
    /// a name an earlier one held. A file that cannot load answers why instead.
    fn scan(&mut self, _dir: &Path) -> Vec<ScannedType> {
        Vec::new()
    }
    /// Types the engine finds on its own account — the platform's plugin folders — scanned
    /// after every root, so a name a root holds is known when these register.
    fn scan_own(&mut self) -> Vec<ScannedType> {
        Vec::new()
    }

    /// Forget a type a scan registered; whether this engine held it.
    fn remove_type(&mut self, _type_name: &str) -> bool {
        false
    }
    /// The SDK crate an `.rs` file in this engine's folder is built against, when it takes one.
    fn rust_sdk(&self) -> Option<&'static str> {
        None
    }
    /// The boot scan is over: a Rust node registered from now on was authored in the session.
    fn boot_done(&mut self) {}
    /// The open patch's workspace: where a node's opaque state is kept between two births.
    fn set_workspace(&mut self, _dir: &Path) {}
    /// Write every live node's opaque state where the next birth at its uid will find it.
    fn persist(&mut self) {}
    /// The universal params this engine adds to every one of its nodes — declarations, so the
    /// palette's tooltips and the default-expression seeding read one door. Empty by default.
    fn universal_decls(&self, _manifest: &'static NodeManifest) -> Vec<ParamDecl> {
        Vec::new()
    }
    /// The declared defaults with `supplied` folded on top, then each universal group rebuilt LAST
    /// in this engine's order. By MANIFEST, so a type the library no longer answers for still works.
    fn normalize_params(&self, manifest: &'static NodeManifest, supplied: Option<ParamGroups>) -> ParamGroups {
        let mut params = manifest.default_params();
        for (group, entries) in supplied.into_iter().flatten() {
            params.entry(group).or_default().extend(entries);
        }
        for (group, defaults) in crate::params_from_decls(&self.universal_decls(manifest)) {
            let mut held = params.shift_remove(&group).unwrap_or_default();
            let mut page: IndexMap<String, Param> =
                defaults.into_iter().map(|(name, default)| { let value = held.shift_remove(&name).unwrap_or(default); (name, value) }).collect();
            page.extend(held);
            params.insert(group, page);
        }
        if let Some(common) = params.shift_remove("common") {
            params.insert("common".to_string(), common);
        }
        params
    }
    /// Birth at `uid`, with the graph-minted generation. `Some` carries a boot error: the node
    /// then exists holding its place and saying why it is not running.
    fn insert(
        &mut self,
        uid: Uid,
        type_name: &str,
        generation: u64,
        params: &ParamGroups,
    ) -> Option<String>;
    fn remove(&mut self, uid: Uid);
    /// Deliver a settled batch: `changed` names every node of any engine whose desired state may
    /// have moved — its record, a wire at either end, a neighbour reborn, a binding or a watch —
    /// plus whatever the engine's own drain marked pending. The ONLY place an engine composes
    /// messages — drain collects, settle decides.
    fn settle(&mut self, view: &GraphView<'_>, changed: &[Uid]);
    /// Hand over every queued health report. A pull: the caller owns the pace.
    fn drain(&mut self, apply: &mut dyn FnMut(Uid, Status)) -> usize;
    /// The facts this engine ALONE decides, such as a rate or a driver, for the graph to write into
    /// `system.*`. Every name must be an ephemeral variable, which no patch carries.
    fn published(&self) -> Vec<(&'static str, goofi_core::Data)> {
        Vec::new()
    }
    /// One imperative to a node's own thread — what settled state cannot express. The answer, if
    /// any, arrives through the drain.
    fn request(&mut self, uid: Uid, request: Request);
    /// Whether a node of this type has an editor window of its own — on the machine goofi runs
    /// on, so a platform with no window host answers false.
    fn has_editor(&self, _type_name: &str) -> bool {
        false
    }
    /// Show or hide `uid`'s editor: the action, answering whether it changed anything.
    fn editor(&mut self, uid: Uid, _show: bool) -> Result<EditorAction, String> {
        Err(format!("{uid} has no editor"))
    }
    /// Every value an editor wrote since the last call. A pull, like the drain: the worker owns
    /// the pace, and puts each through the param op off the lock.
    fn take_edits(&mut self) -> Vec<Edit> {
        Vec::new()
    }
    /// What a slot's viewers want of its frames, or `None` for the frame itself. A producer that
    /// can make exactly that spends nothing downstream; one that cannot ignores this.
    fn view_demand(&mut self, _uid: Uid, _slot: &str, _want: Option<goofi_view::ViewWant>) {}
    /// The graph's expression evaluator, shared with every engine that evaluates `nd()` bindings
    /// on its own thread. No-op for an engine that never does.
    fn set_evaluator(&mut self, _evaluator: Arc<dyn ExprEvaluator>) {}
    /// The composition root's door to an engine's CONCRETE surface — a runtime type registry, a
    /// device enumeration. Everything generic stays on this trait.
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn shutdown(&mut self);
}

/// One doorbell an output slot rings: the consumer, the id it is rung with, and the subscription
/// the ring is for — a declared input slot, or a binding.
pub struct Ringer<'a> {
    pub consumer: Uid,
    pub event_id: EventId,
    pub via: Via<'a>,
}

pub enum Via<'a> {
    Slot(&'static str),
    Binding(&'a BindingView<'a>),
}

impl GraphView<'_> {
    /// Every doorbell `(producer, slot)` rings: wired slots by manifest position, `nd()` channels
    /// by event id, slots past the budget sharing the last. One order per settled state.
    pub fn ringers(&self, producer: Uid, slot: &str) -> Vec<Ringer<'_>> {
        let wired = self.out_of.get(&producer).into_iter().flatten().filter(|e| e.producer.1 == slot).filter_map(|e| {
            let node = self.nodes.get(&e.consumer.0)?;
            let at = node.manifest.inputs.iter().position(|s| s.name == e.consumer.1)?;
            Some(Ringer { consumer: e.consumer.0, event_id: (at + 1).min(64) as EventId, via: Via::Slot(e.consumer.1) })
        });
        let bound = self.readers.get(&(producer, slot)).into_iter().flatten().filter_map(|(uid, i, event_id)| {
            Some(Ringer { consumer: *uid, event_id: *event_id, via: Via::Binding(self.nodes.get(uid)?.bindings.get(*i)?) })
        });
        let mut ringers: Vec<Ringer<'_>> = wired.chain(bound).collect();
        ringers.sort_by_key(|r| (r.consumer.0, r.event_id));
        ringers
    }
}
