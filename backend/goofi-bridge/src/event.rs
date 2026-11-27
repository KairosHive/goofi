//! What the control socket pushes: one enum, one wire shape `{event, payload}`, one sender.

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

/// A control-plane event. Variants carrying a `Value` are shaped where they are built.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "event", content = "payload", rename_all = "snake_case")]
pub enum Event {
    Hello(Value),
    GraphReplaced(Value),
    DocState { v: u64, doc: Value },
    DocPatch { from: u64, v: u64, ops: Vec<crate::doc::Op> },
    UnsavedChanges { unsaved_changes: bool },
    SavePathChanged { save_path: Value },
    NodeAdded { uid: String },
    NodeTypes { types: Value },
    StateUpdate(Value),
    ParamValues { nodes: Value },
    NodeStats { stats: Value },
    NodeStage(Value),
    RecordChanged(Value),
    HarnessChanged(Value),
    /// The history moved: what every client's next undo and redo would take back, by label.
    History { undo: Option<String>, redo: Option<String> },
    Logs(Value),
}

impl Event {
    pub fn text(&self) -> String {
        serde_json::to_string(self).expect("an event serializes")
    }
}

/// An event's text as every socket sends it: one buffer, shared by each subscriber.
pub type EventText = axum::extract::ws::Utf8Bytes;

/// The one sender every event rides, serialized once however many sockets listen.
#[derive(Clone)]
pub struct Broadcaster(broadcast::Sender<EventText>);

impl Broadcaster {
    pub fn new(capacity: usize) -> Broadcaster {
        Broadcaster(broadcast::channel(capacity).0)
    }

    /// Send one event, or nothing for `None`. With no listener no text is made.
    pub fn send(&self, event: impl Into<Option<Event>>) {
        if let Some(event) = event.into().filter(|_| self.0.receiver_count() > 0) {
            let _ = self.0.send(event.text().into());
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventText> {
        self.0.subscribe()
    }
}
