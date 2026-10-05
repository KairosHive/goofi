//! One transaction per read or write op: the graph and history held for its whole run, an outbox
//! of events, and one tail that settles, projects and broadcasts when something changed.

use std::sync::MutexGuard;

use goofi_graph::{Command, CommandHistory, Graph, Outcome};

use crate::{AppState, Caller, Event};

pub struct Txn<'a> {
    pub state: &'a AppState,
    pub caller: &'a Caller,
    pub g: MutexGuard<'a, Graph>,
    pub history: MutexGuard<'a, CommandHistory>,
    outbox: Vec<Event>,
    /// The nodes whose runtime state is echoed after the settle, as a `state_update` each.
    echo: Vec<goofi_node::Uid>,
    /// The history mark this transaction's first command took; none while it has applied nothing.
    mark: Option<usize>,
    edited: bool,
    committed: bool,
    preview: bool,
    /// What the write steps say they did; the history entry names one, or counts several.
    labels: Vec<String>,
}

impl<'a> Txn<'a> {
    /// Hold the graph, then the history — the one lock order — until the drop.
    pub fn begin(state: &'a AppState, caller: &'a Caller, preview: bool, authored: bool) -> Txn<'a> {
        let g = state.graph.lock();
        let history = state.history.lock();
        if authored { g.variables().defer_publication(); }
        Txn { state, caller, g, history, outbox: Vec::new(), echo: Vec::new(), mark: None, edited: false, committed: false, preview, labels: Vec::new() }
    }

    /// Run `cmd` through the history. The first command takes the mark everything after is
    /// coalesced to on commit, or rolled back to on refusal.
    pub fn apply(&mut self, cmd: Command) -> Result<Outcome, String> {
        if self.mark.is_none() && !self.preview {
            self.mark = Some(self.history.mark());
        }
        self.edited = true;
        self.history.apply(&mut self.g, &self.caller.actor, cmd)
    }

    /// Name a write step — what an undo button says it takes back.
    pub fn label(&mut self, label: String) {
        self.labels.push(label);
    }

    /// The graph moved by a path that is no command: the tail runs for this transaction.
    pub fn touch(&mut self) {
        self.edited = true;
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

    /// The tail, once: settle, project, release the graph, then the delta and the outbox under
    /// the document guard. A transaction that moved nothing sends its outbox and no delta.
    pub fn commit(mut self) {
        let state = self.state;
        let mut outbox = std::mem::take(&mut self.outbox);
        if !self.edited {
            self.g.variables().finish_publication(true);
            self.committed = true;
            outbox.extend(self.echoes());
            drop(self);
            for event in outbox {
                state.events.send(event);
            }
            return;
        }
        if let Some(mark) = self.mark.take() {
            // The caller's own label, where it gave one, names the step over the op's.
            let label = self.caller.label.clone().unwrap_or_else(|| match self.labels.as_slice() {
                [one] => one.clone(),
                many => format!("{} edits", many.len()),
            });
            self.history.coalesce(mark, label, self.caller.context.clone(), self.caller.group.clone());
        }
        if !self.preview {
            outbox.extend(state.set_dirty(true));
        }
        let (doc, projection) = crate::settle_and_project(state, &mut self.g);
        outbox.extend(self.echoes());
        self.committed = true;
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
        if self.committed { return; }
        if let Some(mark) = self.mark { self.history.rollback(&mut self.g, mark); }
        if self.edited {
            let (doc, projection) = crate::settle_and_project(self.state, &mut self.g);
            crate::reconcile_and_broadcast(self.state, doc, projection, Vec::new());
        } else {
            self.g.variables().finish_publication(true);
        }
    }
}
