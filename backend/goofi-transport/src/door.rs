//! A door: the one event a node's thread or a reducer parks on, rung with an id by every
//! producer feeding it and by the engine that owns it.

use std::sync::Arc;
use std::time::Instant;

use goofi_supervisor::timing::{Event, Woken};

use crate::EventId;

pub struct Door {
    event: Event,
}

impl Door {
    pub(crate) fn new() -> Door {
        Door { event: Event::new() }
    }

    pub fn ring(&self, id: EventId) {
        self.event.ring(id);
    }

    /// Park until a ring.
    pub fn wait(&self) -> Woken {
        self.event.wait()
    }

    /// Park until a ring or `deadline`.
    pub fn wait_until(&self, deadline: Instant) -> Woken {
        self.event.wait_until(deadline)
    }
}

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
