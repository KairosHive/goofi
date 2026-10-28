//! A door: the one event a node's thread or a reducer parks on, rung with an id by every
//! producer feeding it and by the engine that owns it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use goofi_supervisor::timing::{Event, Woken};

use crate::EventId;

pub struct Door {
    name: String,
    event: Event,
}

impl Door {
    pub(crate) fn new(name: &str) -> Door {
        Door { name: name.to_string(), event: Event::new() }
    }

    pub fn name(&self) -> &str {
        &self.name
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

    /// What rang since the last wait, without parking.
    pub fn take(&self) -> Woken {
        self.event.take()
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

/// Park on `listener` for at most `within`, handing `f` each id that rang.
pub fn wait_within(listener: &Door, within: Duration, mut f: impl FnMut(EventId)) {
    for id in listener.wait_until(Instant::now() + within).ids() {
        f(id);
    }
}
