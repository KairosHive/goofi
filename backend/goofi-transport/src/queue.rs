//! The transport's one FIFO: a bounded queue, oldest dropped when the reader falls behind, which
//! the next frame's own index witnesses. The sender never waits and never blocks; the receiver
//! parks on the queue's own event. A slot carries what is newest between nodes; a queue carries
//! everything, which only the recorder wants.
//!
//! Neither end takes a lock: each cell of the ring owns its state word, and whoever wins the
//! exchange on it owns the frame in it. A recorder parked on a slow disk is never on the
//! producer's path, and the producer never on the recorder's.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use goofi_core::record::Frame;
use goofi_supervisor::timing::Event;

/// How many frames an engine's armed slots queue ahead of the recorder. Signal: a second of ticks
/// at the default cap. Audio: a block is a 64-frame slice, so about a second and a third. Graphics:
/// a readback is megabytes, so a few, and a drop shows as a gap in the frame numbers.
pub fn record_depth(engine: &str) -> usize {
    match engine {
        "audio" => 1024,
        "graphics" => 4,
        _ => 64,
    }
}

/// A cell nobody holds a frame in.
const EMPTY: u8 = 0;
/// A cell with a frame in it, for the next receive.
const FULL: u8 = 1;
/// A cell one end is writing or taking right now: the other end leaves it alone.
const BUSY: u8 = 2;

struct Cell {
    state: AtomicU8,
    frame: UnsafeCell<Option<Frame>>,
}

pub struct Queue {
    depth: usize,
    cells: Box<[Cell]>,
    /// The next cell a receive takes, counted without wrap; `head == tail` is empty.
    head: AtomicUsize,
    /// The next cell a send fills, counted without wrap.
    tail: AtomicUsize,
    /// Rung on every send; the receiver parks on it.
    event: Arc<Event>,
    readers: AtomicUsize,
}

// SAFETY: a cell's frame is touched only by the end that won its state word, so no two threads
// reach one frame at once; `Frame` itself is `Send`.
unsafe impl Send for Queue {}
unsafe impl Sync for Queue {}

impl Queue {
    pub(crate) fn new(depth: usize) -> Queue {
        let depth = depth.max(1);
        let cells = (0..depth).map(|_| Cell { state: AtomicU8::new(EMPTY), frame: UnsafeCell::new(None) }).collect();
        Queue { depth, cells, head: AtomicUsize::new(0), tail: AtomicUsize::new(0), event: Arc::new(Event::new()), readers: AtomicUsize::new(0) }
    }

    /// Claim the cell at `at` while it is in `from`: the claimant alone touches its frame until
    /// it sets the state again.
    fn claim(&self, at: usize, from: u8) -> Option<&Cell> {
        let cell = &self.cells[at % self.depth];
        cell.state.compare_exchange(from, BUSY, Ordering::AcqRel, Ordering::Acquire).is_ok().then_some(cell)
    }

    /// The frame at the head, taken; `None` while empty. Whoever calls this competes fairly with
    /// a sender dropping the oldest: the exchange on `head` decides who owns that position, and
    /// the winner alone waits for the cell's frame, which the send that counted it is writing.
    fn pop(&self) -> Option<Frame> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            if head == self.tail.load(Ordering::Acquire) {
                return None;
            }
            if self.head.compare_exchange(head, head + 1, Ordering::AcqRel, Ordering::Acquire).is_err() {
                continue;
            }
            let cell = loop {
                if let Some(cell) = self.claim(head, FULL) {
                    break cell;
                }
                std::hint::spin_loop();
            };
            // SAFETY: the claim above made this cell ours.
            let frame = unsafe { (*cell.frame.get()).take() };
            cell.state.store(EMPTY, Ordering::Release);
            return frame;
        }
    }

    fn push(&self, frame: Frame) {
        let mut frame = Some(frame);
        loop {
            let tail = self.tail.load(Ordering::Acquire);
            let head = self.head.load(Ordering::Acquire);
            if tail - head < self.depth {
                let Some(cell) = self.claim(tail, EMPTY) else {
                    std::hint::spin_loop();
                    continue;
                };
                // SAFETY: the claim above made this cell ours.
                unsafe { *cell.frame.get() = frame.take() };
                self.tail.store(tail + 1, Ordering::Release);
                cell.state.store(FULL, Ordering::Release);
                return;
            }
            // Full: the oldest goes, unless the receiver is taking it this instant, in which
            // case there is room by the next look.
            drop(self.pop());
        }
    }

    fn len(&self) -> usize {
        self.tail.load(Ordering::Acquire).saturating_sub(self.head.load(Ordering::Acquire))
    }
}

/// The id a send rings the receiver's event with.
pub const SENT: u8 = 0;
/// The id anything else rings it with: a stop, a hold let go, a worker told to leave.
pub const POKED: u8 = 1;

/// A producer's end of one armed slot's queue: opened by the arming, released by the reader
/// — a disarm only [retires](Sender::retire) it, so delivered frames stay.
pub struct Sender {
    queue: Arc<Queue>,
    retired: bool,
}

impl Sender {
    pub(crate) fn new(queue: Arc<Queue>) -> Sender {
        Sender { queue, retired: false }
    }

    /// Queue one frame and wake the receiver. A retired sender sends nothing, and says so.
    pub fn send(&self, frame: Frame) -> bool {
        if self.retired {
            return false;
        }
        self.queue.push(frame);
        self.queue.event.ring(SENT);
        true
    }

    /// Disarmed: stop publishing, but stay open while the receiver still reads.
    pub fn retire(&mut self) {
        self.retired = true;
    }

    pub fn armed(&mut self) {
        self.retired = false;
    }

    pub fn retired(&self) -> bool {
        self.retired
    }

    /// Retired, and nobody reads it any more — the one state in which dropping it is safe.
    pub fn spent(&self) -> bool {
        self.retired && self.queue.readers.load(Ordering::Relaxed) == 0
    }
}

/// The recorder's end: every frame, in order.
pub struct Receiver {
    queue: Arc<Queue>,
}

impl Receiver {
    pub(crate) fn new(queue: Arc<Queue>) -> Receiver {
        queue.readers.fetch_add(1, Ordering::Relaxed);
        Receiver { queue }
    }

    pub fn receive(&mut self) -> Option<Frame> {
        self.queue.pop()
    }

    /// Park until a send or a poke.
    pub fn wait(&self) {
        self.queue.event.wait();
    }

    /// Park until a send, a poke or `deadline`; whether anything rang.
    pub fn wait_until(&self, deadline: Instant) -> bool {
        !self.queue.event.wait_until(deadline).is_empty()
    }

    /// The event the receiver parks on, for whoever must wake it without sending.
    pub fn event(&self) -> Arc<Event> {
        self.queue.event.clone()
    }

    /// How full the queue is, against its depth.
    pub fn fill(&self) -> f32 {
        self.queue.len() as f32 / self.queue.depth as f32
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.queue.readers.fetch_sub(1, Ordering::Relaxed);
    }
}
