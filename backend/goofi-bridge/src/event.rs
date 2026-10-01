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
    Error { node: String, error: Option<String> },
    NodeStage(Value),
    ControlPaint { name: String, marks: Value },
    RecordChanged(Value),
    HarnessChanged(Value),
    Logs(Value),
}

impl Event {
    pub fn text(&self) -> String {
        serde_json::to_string(self).expect("an event serializes")
    }
}

/// The one sender every event rides, serialized once however many sockets listen.
#[derive(Clone)]
pub struct Broadcaster(broadcast::Sender<String>);

impl Broadcaster {
    pub fn new(capacity: usize) -> Broadcaster {
        Broadcaster(broadcast::channel(capacity).0)
    }

    /// Send one event, or nothing for `None`. A send with no listener is not a fault.
    pub fn send(&self, event: impl Into<Option<Event>>) {
        if let Some(event) = event.into() {
            let _ = self.0.send(event.text());
        }
    }

    pub fn listeners(&self) -> usize {
        self.0.receiver_count()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.0.subscribe()
    }
}
