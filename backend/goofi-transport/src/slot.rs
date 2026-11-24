//! A slot: one cell per output, the newest payload in it, read by any number of subscribers
//! each once. Latest wins by construction: there is no queue, so nothing overflows.
//!
//! Neither end takes a lock. The writer swaps the newest payload in; a reader loads it and clones
//! the handle out. A producer is never held by a reader that is away, and a reader never by a
//! producer mid-publish — which is what lets a viewer read an engine's output without ever being
//! on that engine's path.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwapOption;
use goofi_core::Data;

/// What crosses a wire: the frame as the node made it, or a GOOF frame a producer encoded on
/// purpose — the 8-bit texture tap, the one case where the bytes are the product.
#[derive(Clone, Debug)]
pub enum Payload {
    Data(Data),
    Wire(bytes::Bytes),
}

/// The newest payload and its number: a subscriber takes a payload whose number it has not seen.
struct Latest {
    seq: u64,
    payload: Payload,
}

/// One slot's cell. Public to the crate: the hub makes it, the ends hold it.
#[derive(Default)]
pub struct Cell {
    latest: ArcSwapOption<Latest>,
    /// Numbers the payloads: the cell's, so every publisher of the slot counts on.
    seq: AtomicU64,
    /// Whether a late subscriber is handed the newest payload.
    pub(crate) held: AtomicBool,
    readers: AtomicUsize,
}

impl Cell {
    pub(crate) fn readers(&self) -> usize {
        self.readers.load(Ordering::Relaxed)
    }

    /// The number of the newest payload; zero before any.
    fn seq(&self) -> u64 {
        self.latest.load().as_ref().map_or(0, |l| l.seq)
    }
}

/// A slot's writer.
pub struct Publisher {
    cell: Arc<Cell>,
}

impl Publisher {
    pub(crate) fn new(cell: Arc<Cell>) -> Publisher {
        Publisher { cell }
    }

    /// Put `payload` on the slot; every subscriber takes it at its next receive.
    pub fn publish(&self, payload: Payload) {
        let seq = self.cell.seq.fetch_add(1, Ordering::Relaxed) + 1;
        self.cell.latest.store(Some(Arc::new(Latest { seq, payload })));
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
        let seen = if cell.held.load(Ordering::Relaxed) { 0 } else { cell.seq() };
        Subscriber { cell, seen }
    }

    /// The newest payload not yet taken, or nothing.
    pub fn receive(&mut self) -> Option<Payload> {
        let latest = self.cell.latest.load();
        let latest = latest.as_ref()?;
        if latest.seq == self.seen {
            return None;
        }
        self.seen = latest.seq;
        Some(latest.payload.clone())
    }
}

impl Drop for Subscriber {
    fn drop(&mut self) {
        self.cell.readers.fetch_sub(1, Ordering::Relaxed);
    }
}
