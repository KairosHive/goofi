//! A door: the one event a node's thread or a reducer parks on, rung with an id by every
//! producer feeding it and by the engine that owns it.

use std::sync::Arc;

use goofi_supervisor::timing::Event;

use crate::EventId;

/// A door is an event: parked on by its owner, rung by anyone holding it.
pub type Door = Event;

/// The end a thread parks on.
pub type Listener = Arc<Door>;

/// A bell onto a door; the ringer knows nothing else about the door's owner.
#[derive(Clone)]
pub struct Doorbell(Arc<Door>);

impl Doorbell {
    pub fn on(door: Arc<Door>) -> Doorbell {
        Doorbell(door)
    }

    pub fn ring(&self, id: EventId) {
        self.0.ring(id);
    }
}
