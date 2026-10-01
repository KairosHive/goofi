//! One transaction per read or write op: the graph and history held for its whole run, an outbox
//! of events, and one tail that settles, projects and broadcasts when something changed.

use std::sync::MutexGuard;

use goofi_graph::{Command, CommandHistory, Graph, Outcome};

use crate::{AppState, Caller, Event};

pub struct Txn<'a> {
    pub state: &'a AppState,
    pub caller: &'a Caller,
    pub actor: &'a str,
    pub g: MutexGuard<'a, Graph>,
    pub history: MutexGuard<'a, CommandHistory>,
    outbox: Vec<Event>,
    /// The nodes whose runtime state is echoed after the settle, as a `state_update` each.
    echo: Vec<goofi_node::Uid>,
    /// The history mark this transaction's first command took; none while it has applied nothing.
    mark: Option<usize>,
    edited: bool,
    preview: bool,
    committed: bool,
    /// What the history entry this transaction leaves says it did.
    label: String,
}

impl<'a> Txn<'a> {
    /// Hold the graph, then the history — the one lock order — until the drop.
    pub fn begin(state: &'a AppState, caller: &'a Caller, preview: bool) -> Txn<'a> {
        let g = state.graph.lock();
        let history = state.history.lock();
        Txn { state, caller, actor: &caller.actor, g, history, outbox: Vec::new(), echo: Vec::new(), mark: None, edited: false, preview, committed: false, label: String::new() }
    }

    /// Run `cmd` through the history. The first command clears the actor's redo run and takes
    /// the mark everything after is coalesced to, or rolled back to.
    pub fn apply(&mut self, cmd: Command) -> Result<Outcome, String> {
        if self.mark.is_none() && !self.preview {
            self.history.clear_redo(self.actor);
            self.mark = Some(self.history.mark());
        }
        self.edited = true;
        self.history.apply(&mut self.g, self.actor, cmd)
    }

    /// Name the history entry this transaction leaves — what an undo button says it takes back.
    pub fn label(&mut self, label: String) {
        self.label = label;
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
        self.committed = true;
        let state = self.state;
        let mut outbox = std::mem::take(&mut self.outbox);
        if !self.edited {
            outbox.extend(self.echoes());
            drop(self);
            for event in outbox {
                state.events.send(event);
            }
            return;
        }
        if let Some(mark) = self.mark {
            // The caller's own label, where it gave one, names the step over the op's.
            let label = self.caller.label.clone().unwrap_or_else(|| std::mem::take(&mut self.label));
            self.history.coalesce(mark, label, self.caller.context.clone(), self.caller.group.clone());
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
        if self.committed {
            return;
        }
        if let Some(mark) = self.mark {
            self.history.rollback(&mut self.g, mark);
        }
    }
}
