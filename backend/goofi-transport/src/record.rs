//! The recorder's channel: the one FIFO in the transport, one per arming, bounded, the oldest
//! frame dropped when the recorder falls behind — which the next frame's own index witnesses.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use goofi_core::Data;
use goofi_supervisor::sync::Mutex;

use crate::names::RECORD_EVENT_ID;
use crate::Door;

/// How many SIGNAL frames an armed slot's queue holds ahead of the recorder.
pub const RECORD_DEPTH: usize = 64;
/// How many AUDIO blocks: a block is a 64-frame slice, so this is about a second and a third.
pub const AUDIO_RECORD_DEPTH: usize = 1024;

/// How deep an engine's armed slots queue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecordShape {
    pub buffer: usize,
}

/// The depth an engine's armed slots publish with; the recorder reads the fill against it.
pub fn record_shape(engine: &str) -> RecordShape {
    RecordShape { buffer: match engine {
        "audio" => AUDIO_RECORD_DEPTH,
        _ => RECORD_DEPTH,
    } }
}

pub struct Queue {
    depth: usize,
    frames: Mutex<VecDeque<Data>>,
    readers: AtomicUsize,
}

impl Queue {
    pub(crate) fn new(depth: usize) -> Queue {
        Queue { depth, frames: Mutex::new(VecDeque::new()), readers: AtomicUsize::new(0) }
    }
}

/// A producer's end of one armed slot's queue: opened by the arming, released by the reader
/// — a disarm only [retires](RecordPort::retire) it, so delivered frames stay.
pub struct RecordPort {
    queue: Arc<Queue>,
    /// The recorder's door, rung once a frame is queued.
    bell: Arc<Door>,
    retired: bool,
}

impl RecordPort {
    pub(crate) fn new(queue: Arc<Queue>, bell: Arc<Door>) -> RecordPort {
        RecordPort { queue, bell, retired: false }
    }

    /// Queue one frame and ring the recorder. A retired port sends nothing, and says so.
    pub fn send(&self, frame: Data) -> bool {
        if self.retired {
            return false;
        }
        let mut frames = self.queue.frames.lock();
        if frames.len() >= self.queue.depth {
            frames.pop_front();
        }
        frames.push_back(frame);
        drop(frames);
        self.bell.ring(RECORD_EVENT_ID);
        true
    }

    /// Disarmed: stop publishing, but stay open while the recorder still reads.
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
pub struct RecordSubscriber {
    queue: Arc<Queue>,
}

impl RecordSubscriber {
    pub(crate) fn new(queue: Arc<Queue>) -> RecordSubscriber {
        queue.readers.fetch_add(1, Ordering::Relaxed);
        RecordSubscriber { queue }
    }

    pub fn receive(&mut self) -> Option<Data> {
        self.queue.frames.lock().pop_front()
    }
}

impl Drop for RecordSubscriber {
    fn drop(&mut self) {
        self.queue.readers.fetch_sub(1, Ordering::Relaxed);
    }
}
