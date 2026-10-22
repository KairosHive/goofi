//! A slot: one cell per output, the newest payload in it, read by any number of subscribers
//! each once. Latest wins by construction: there is no queue, so nothing overflows.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use goofi_core::Data;
use goofi_supervisor::sync::Mutex;

/// What crosses a wire: the frame as the node made it, or a GOOF frame a producer encoded on
/// purpose — the 8-bit texture tap, the one case where the bytes are the product.
#[derive(Clone, Debug)]
pub enum Payload {
    Data(Data),
    Wire(bytes::Bytes),
}

#[derive(Default)]
struct State {
    /// Bumped per publish; a subscriber takes a payload whose sequence it has not seen.
    seq: u64,
    latest: Option<Payload>,
}

/// One slot's cell. Public to the crate: the hub makes it, the ends hold it.
#[derive(Default)]
pub struct Cell {
    state: Mutex<State>,
    /// Whether a late subscriber is handed the newest payload.
    pub(crate) held: AtomicBool,
    readers: AtomicUsize,
}

impl Cell {
    pub(crate) fn readers(&self) -> usize {
        self.readers.load(Ordering::Relaxed)
    }
}

/// A slot's one writer.
pub struct Publisher {
    cell: Arc<Cell>,
}

impl Publisher {
    pub(crate) fn new(cell: Arc<Cell>) -> Publisher {
        Publisher { cell }
    }

    /// Put `payload` on the slot; every subscriber takes it at its next receive.
    pub fn publish(&self, payload: Payload) {
        let mut state = self.cell.state.lock();
        state.seq += 1;
        state.latest = Some(payload);
    }

    /// Whether anyone reads this slot right now.
    pub fn readers(&self) -> usize {
        self.cell.readers.load(Ordering::Relaxed)
    }
}

/// A slot's reader: the newest payload, once.
pub struct Subscriber {
    cell: Arc<Cell>,
    seen: u64,
}

impl Subscriber {
    pub(crate) fn new(cell: Arc<Cell>) -> Subscriber {
        cell.readers.fetch_add(1, Ordering::Relaxed);
        // A held slot replays its newest payload; any other starts from what comes next.
        let seen = if cell.held.load(Ordering::Relaxed) { 0 } else { cell.state.lock().seq };
        Subscriber { cell, seen }
    }

    /// The newest payload not yet taken, or nothing.
    pub fn receive(&mut self) -> Option<Payload> {
        let state = self.cell.state.lock();
        if state.seq == self.seen {
            return None;
        }
        self.seen = state.seq;
        state.latest.clone()
    }
}

impl Drop for Subscriber {
    fn drop(&mut self) {
        self.cell.readers.fetch_sub(1, Ordering::Relaxed);
    }
}
