//! One transaction per read or write op: the graph and history held for its whole run, an outbox
//! of events, and one tail that settles, projects and broadcasts when something changed. A write
//! takes the snapshot before it runs; what it changed, less the drags in flight, is the history's
//! next entry, and a refusal puts the snapshot back.

use std::sync::MutexGuard;

use goofi_graph::{Graph, History, Snapshot};

use crate::{AppState, Caller, Event};

pub struct Txn<'a> {
    pub state: &'a AppState,
    pub caller: &'a Caller,
    pub g: MutexGuard<'a, Graph>,
    pub history: MutexGuard<'a, History>,
    outbox: Vec<Event>,
    /// The nodes whose runtime state is echoed after the settle, as a `state_update` each.
    echo: Vec<goofi_node::Uid>,
    /// The snapshot from before a write; none for a read.
    before: Option<Snapshot>,
    /// What a drag is keyed by, named by the op that previews: the commit that ends it takes the
    /// snapshot from before its first step.
    key: Option<String>,
    /// The graph moved by a path no snapshot shows (a flip, a request): the tail runs regardless.
    touched: bool,
    preview: bool,
    /// What the write steps say they did; the history entry names one, or counts several.
    labels: Vec<String>,
}

impl<'a> Txn<'a> {
    /// Hold the graph, then the history — the one lock order — until the drop. A write takes
    /// the snapshot it may have to put back.
    pub fn begin(state: &'a AppState, caller: &'a Caller, preview: bool, write: bool) -> Txn<'a> {
        let g = state.graph.lock();
        let history = state.history.lock();
        let before = write.then(|| g.snapshot());
        Txn { state, caller, g, history, outbox: Vec::new(), echo: Vec::new(), before, key: None, touched: false, preview, labels: Vec::new() }
    }

    /// Name a write step — what an undo button says it takes back.
    pub fn label(&mut self, label: String) {
        self.labels.push(label);
    }

    /// Name the drag this write belongs to: one key per thing dragged, the same for every step
    /// and for the commit that ends it.
    pub fn key(&mut self, key: String) {
        self.key = Some(key);
    }

    /// Whether the op named the drag it belongs to — what a preview needs to keep a way back.
    pub fn keyed(&self) -> bool {
        self.key.is_some()
    }

    /// The graph moved by a path no snapshot shows: the tail runs for this transaction.
    pub fn touch(&mut self) {
        self.touched = true;
    }

    /// Queue an event for the tail, which sends it after the document delta it belongs with.
    pub fn emit(&mut self, event: Event) {
        self.outbox.push(event);
    }

    /// Echo a node's runtime state after the settle: what the doc does not carry — its error,
    /// its descriptors — read from settled state, not from the step that asked.
    pub fn echo(&mut self, uid: goofi_node::Uid) {
        if !self.echo.contains(&uid) {
            self.echo.push(uid);
        }
    }

    /// The tail, once: the history entry, then settle, project, release the graph, then the delta
    /// and the outbox under the document guard. A transaction that moved nothing sends its outbox
    /// and no delta.
    pub fn commit(mut self) {
        let state = self.state;
        let mut outbox = std::mem::take(&mut self.outbox);
        let before = self.before.take();
        let edited = before.as_ref().is_some_and(|b| self.g.differs(b));
        if !edited && !self.touched {
            outbox.extend(self.echoes());
            drop(self);
            for event in outbox {
                state.events.send(event);
            }
            return;
        }
        if let Some(before) = before.filter(|_| edited) {
            let actor = self.caller.actor.as_str();
            let after = self.g.snapshot();
            match (self.preview, &self.key) {
                (true, Some(key)) => self.history.preview(actor, key, before, &after),
                (true, None) => {}
                (false, key) => {
                    // The caller's own label, where it gave one, names the step over the op's.
                    let label = self.caller.label.clone().unwrap_or_else(|| match self.labels.as_slice() {
                        [one] => one.clone(),
                        many => format!("{} edits", many.len()),
                    });
                    let step = goofi_graph::Step { label, context: self.caller.context.clone(), group: self.caller.group.clone() };
                    self.history.record(actor, key.as_deref(), before, &after, step);
                    outbox.push(crate::history_event(&self.history));
                }
            }
        }
        if !self.preview {
            outbox.extend(state.set_dirty(true));
        }
        let (doc, projection) = crate::settle_and_project(state, &mut self.g);
        outbox.extend(self.echoes());
        drop(self);
        state.settled_now();
        crate::reconcile_and_broadcast(state, doc, projection, outbox);
    }

    /// The echoes, for the nodes the batch left standing.
    fn echoes(&mut self) -> Vec<Event> {
        let g = &*self.g;
        std::mem::take(&mut self.echo).into_iter().filter(|u| g.name(*u).is_some()).map(|u| crate::param_state_update(g, u, &[])).collect()
    }
}

impl Drop for Txn<'_> {
    /// Not committed — refused, or unwinding: what this transaction applied is taken back.
    fn drop(&mut self) {
        if let Some(before) = self.before.take() {
            if self.g.differs(&before) {
                self.g.restore(&before);
            }
        }
    }
}
