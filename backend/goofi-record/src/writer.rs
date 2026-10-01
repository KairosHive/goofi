//! The threads that FORMAT. A drain takes frames off a transport at the producer's rate and must
//! keep up with it; turning one into a `.npy` row and a JSON line is work that does not belong on
//! that thread. So a frame is copied into a pooled buffer, queued, and written here — the same
//! shape the video path has always had, for the same reason.
//!
//! ONE LANE PER STREAM, because a shared queue makes one stream's overload another stream's loss:
//! a 100 kHz stream filled a common queue and the audio beside it lost blocks it could easily have
//! kept. A lane that overflows is its own stream's problem and nobody else's.
//!
//! A full lane is a counted DROP, never a stall: the caller leaves the frame's index unreached and
//! the next frame written carries the gap, so a loss here is witnessed exactly as one at the subscriber.

use std::collections::HashMap;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use goofi_supervisor::sync::Mutex;

use crate::stream::{StreamMeta, Timeline};
use crate::StreamId;

/// How many frames one stream may have waiting. Deep enough to ride out a scheduler that parks the
/// lane, shallow enough that a disk which cannot keep up says so instead of eating memory.
const LANE: usize = 1024;

/// Buffers finished with, at most `cap`, kept so a frame costs a copy and never an allocation.
#[derive(Clone)]
pub(crate) struct Pool {
    free: Arc<Mutex<Vec<Vec<u8>>>>,
    cap: usize,
}

impl Pool {
    pub(crate) fn new(cap: usize) -> Pool {
        Pool { free: Arc::default(), cap }
    }

    /// A buffer that holds a copy of `bytes`.
    pub(crate) fn filled(&self, bytes: &[u8]) -> Vec<u8> {
        let mut buffer = self.free.lock().pop().unwrap_or_default();
        buffer.clear();
        buffer.extend_from_slice(bytes);
        buffer
    }

    pub(crate) fn give_back(&self, buffer: Vec<u8>) {
        let mut free = self.free.lock();
        if free.len() < self.cap {
            free.push(buffer);
        }
    }
}

pub struct Queued {
    pub id: StreamId,
    pub bytes: Vec<u8>,
    pub rate: Option<f64>,
    pub timeline: Timeline,
    pub at: f64,
}

enum Job {
    Frame(Queued),
    /// Everything queued before this has been written. What a close waits on, so no file is
    /// finalized over a frame still in flight.
    Flush(SyncSender<()>),
}

struct Lane {
    jobs: SyncSender<Job>,
    thread: Option<goofi_supervisor::worker::Worker>,
}

pub struct Writer {
    rec: std::sync::Weak<crate::Recorder>,
    lanes: Mutex<HashMap<StreamId, Lane>>,
    free: Pool,
}

impl Writer {
    /// WEAK, because the recorder owns the writer: an `Arc` back would be a cycle neither drops.
    pub fn new(rec: std::sync::Weak<crate::Recorder>) -> Writer {
        Writer { rec, lanes: Mutex::new(HashMap::new()), free: Pool::new(LANE) }
    }

    /// Take one frame off the caller. `false` is a lane that is full or DEAD — which the CALLER
    /// accounts for by the NEXT frame that is written, whose own number says what went missing.
    ///
    pub fn take(
        &self,
        id: &StreamId,
        bytes: &[u8],
        rate: Option<f64>,
        timeline: Timeline,
        at: f64,
        wait: bool,
    ) -> bool {
        let queued = Queued { id: id.clone(), bytes: self.free.filled(bytes), rate, timeline, at };
        let mut lanes = self.lanes.lock();
        let lane = lanes.entry(id.clone()).or_insert_with(|| self.lane());
        if wait {
            return lane.jobs.send(Job::Frame(queued)).is_ok();
        }
        match lane.jobs.try_send(Job::Frame(queued)) {
            Ok(()) => true,
            Err(TrySendError::Full(job) | TrySendError::Disconnected(job)) => {
                if let Job::Frame(q) = job {
                    self.free.give_back(q.bytes);
                }
                false
            }
        }
    }

    fn lane(&self) -> Lane {
        let (tx, rx) = sync_channel(LANE);
        let (rec, free) = (self.rec.clone(), self.free.clone());
        let thread = goofi_supervisor::worker::thread("goofi-record-write")
            .spawn(move || run(rx, &rec, &free))
            .ok();
        Lane { jobs: tx, thread }
    }

    /// End the stream's lane: a stream closed on purpose starts afresh on its next frame, where a
    /// lane that died with the stream before it would refuse every frame of the stream after it.
    pub fn forget(&self, id: &StreamId) {
        if let Some(lane) = self.lanes.lock().remove(id) {
            end(lane);
        }
    }

    /// Wait for everything already queued, on every lane, to reach its file. Called with NO lock a
    /// lane needs. A lane that died drops its ack, and a slow one is waited for.
    pub fn flush(&self) {
        let waits: Vec<Receiver<()>> = {
            let lanes = self.lanes.lock();
            lanes
                .values()
                .filter_map(|lane| {
                    let (ack, done) = sync_channel(1);
                    lane.jobs.send(Job::Flush(ack)).ok().map(|()| done)
                })
                .collect()
        };
        for done in waits {
            let _ = done.recv();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        for (_, lane) in self.lanes.lock().drain() {
            end(lane);
        }
    }
}

fn end(mut lane: Lane) {
    drop(lane.jobs);
    if let Some(thread) = lane.thread.take() {
        let _ = thread.join();
    }
}

fn run(rx: Receiver<Job>, rec: &std::sync::Weak<crate::Recorder>, free: &Pool) {
    // A stream that could not be written is DEAD and its lane ends with it, so every later frame
    // is refused at `take` instead of counted as written; the close is what files the reason.
    for job in rx.iter() {
        match job {
            Job::Frame(q) => {
                let Some(rec) = rec.upgrade() else { return };
                let written = rec.write_queued(&q);
                free.give_back(q.bytes);
                if let Err(why) = written {
                    rec.close_now(&q.id, &format!("the frame could not be written: {why}"));
                    return;
                }
            }
            Job::Flush(ack) => {
                let _ = ack.try_send(());
            }
        }
    }
}

/// What a stream's meta is, for the open a queued frame may cause. The timeline is the ENGINE's
/// word and rides with the frame, because a lane has no engine to ask.
pub fn meta_of(meta: Option<&goofi_core::Meta>, timeline: Timeline) -> StreamMeta {
    let sfreq = meta.and_then(|m| m.sfreq());
    let held = match timeline {
        Timeline::Measured => StreamMeta::measured(sfreq),
        Timeline::Derived => StreamMeta::derived(sfreq),
    };
    match meta.and_then(|m| m.channels().dims().next().map(|(_, c)| c.len())) {
        Some(n) => held.with_channels(n),
        None => held,
    }
}
