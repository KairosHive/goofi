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
use std::sync::{Arc, Mutex};

use crate::stream::{StreamMeta, Timeline};
use crate::StreamId;

/// How many frames one stream may have waiting. Deep enough to ride out a scheduler that parks the
/// lane, shallow enough that a disk which cannot keep up says so instead of eating memory.
const LANE: usize = 1024;

/// Frames a lane has finished with, kept so a frame costs a copy and never an allocation.
type Free = Arc<Mutex<Vec<Vec<u8>>>>;

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
    thread: Option<goofi_core::worker::Worker>,
}

pub struct Writer {
    rec: std::sync::Weak<crate::Recorder>,
    lanes: Mutex<HashMap<StreamId, Lane>>,
    free: Free,
}

impl Writer {
    /// WEAK, because the recorder owns the writer: an `Arc` back would be a cycle neither drops.
    pub fn new(rec: std::sync::Weak<crate::Recorder>) -> Writer {
        Writer { rec, lanes: Mutex::new(HashMap::new()), free: Free::default() }
    }

    /// Take one frame off the caller. `false` is a lane that is full — which the CALLER accounts
    /// for by the NEXT frame that is written, whose own number says what went missing before it.
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
        let mut buffer = self.free.lock().expect("the free frames").pop().unwrap_or_default();
        buffer.clear();
        buffer.extend_from_slice(bytes);
        let queued = Queued { id: id.clone(), bytes: buffer, rate, timeline, at };
        let mut lanes = self.lanes.lock().expect("the lanes");
        let lane = lanes.entry(id.clone()).or_insert_with(|| self.lane());
        if wait {
            return lane.jobs.send(Job::Frame(queued)).is_ok();
        }
        match lane.jobs.try_send(Job::Frame(queued)) {
            Ok(()) => true,
            Err(TrySendError::Full(job) | TrySendError::Disconnected(job)) => {
                if let Job::Frame(q) = job {
                    give_back(&self.free, q.bytes);
                }
                false
            }
        }
    }

    fn lane(&self) -> Lane {
        let (tx, rx) = sync_channel(LANE);
        let (rec, free) = (self.rec.clone(), self.free.clone());
        let thread = goofi_core::worker::thread("goofi-record-write")
            .spawn(move || run(rx, &rec, &free))
            .ok();
        Lane { jobs: tx, thread }
    }

    /// Wait for everything already queued, on every lane, to reach its file. Called with NO lock a
    /// lane needs.
    pub fn flush(&self) {
        let waits: Vec<Receiver<()>> = {
            let lanes = self.lanes.lock().expect("the lanes");
            lanes
                .values()
                .filter_map(|lane| {
                    let (ack, done) = sync_channel(1);
                    lane.jobs.send(Job::Flush(ack)).ok().map(|()| done)
                })
                .collect()
        };
        for done in waits {
            let _ = done.recv_timeout(std::time::Duration::from_secs(5));
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let mut lanes = self.lanes.lock().unwrap_or_else(|e| e.into_inner());
        for (_, mut lane) in lanes.drain() {
            drop(lane.jobs);
            if let Some(thread) = lane.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

fn give_back(free: &Free, buffer: Vec<u8>) {
    let mut free = free.lock().expect("the free frames");
    if free.len() < LANE {
        free.push(buffer);
    }
}

fn run(rx: Receiver<Job>, rec: &std::sync::Weak<crate::Recorder>, free: &Free) {
    // A stream that could not be written is DEAD, and the close is what files the reason: opening
    // a file per frame against a full disk is worse than stopping, and a write that fails
    // silently is the recording lying about what it holds.
    let mut failed = false;
    for job in rx.iter() {
        match job {
            Job::Frame(q) => {
                if let (false, Some(rec)) = (failed, rec.upgrade()) {
                    if let Err(why) = rec.write_queued(&q) {
                        rec.close_now(&q.id, &format!("the frame could not be written: {why}"));
                        failed = true;
                    }
                }
                give_back(free, q.bytes);
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
