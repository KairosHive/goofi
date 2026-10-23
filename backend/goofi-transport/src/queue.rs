//! The transport's one FIFO: a bounded queue, oldest dropped when the reader falls behind, which
//! the next frame's own index witnesses. The sender never waits and never blocks; the receiver
//! parks on the queue's own event. A slot carries what is newest between nodes; a queue carries
//! everything, which only the recorder wants.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use goofi_core::record::Frame;
use goofi_supervisor::sync::Mutex;
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

pub struct Queue {
    depth: usize,
    frames: Mutex<VecDeque<Frame>>,
    /// Rung on every send; the receiver parks on it.
    event: Arc<Event>,
    readers: AtomicUsize,
}

impl Queue {
    pub(crate) fn new(depth: usize) -> Queue {
        Queue { depth, frames: Mutex::new(VecDeque::new()), event: Arc::new(Event::new()), readers: AtomicUsize::new(0) }
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
        let mut frames = self.queue.frames.lock();
        if frames.len() >= self.queue.depth {
            frames.pop_front();
        }
        frames.push_back(frame);
        drop(frames);
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
        self.queue.frames.lock().pop_front()
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
        self.queue.frames.lock().len() as f32 / self.queue.depth as f32
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.queue.readers.fetch_sub(1, Ordering::Relaxed);
    }
}
