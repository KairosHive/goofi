//! One thread per armed stream: it takes frames off the stream's queue and writes them, opens the
//! file a frame belongs in and closes the one it no longer fits, and reports what it holds. It
//! waits on nothing but its queue, so a slow disk or a slow encoder costs this stream alone.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Weak};

use goofi_core::record::Frame;
use goofi_supervisor::sync::Mutex;
use goofi_transport::Receiver;

use crate::stream::{self, Kind, Stream};
use crate::{manifest, Armed, Recorder, Take, Wake};

/// What a worker says of its stream: the entry of the file it has open, how full its queue is,
/// and why it could not open one.
#[derive(Default)]
pub struct Live {
    pub entry: Option<manifest::Entry>,
    /// The take the entry's file was opened in.
    pub take: u64,
    pub fill: f32,
    pub error: Option<String>,
}

/// What the recorder and a worker share.
pub(crate) struct Shared {
    pub live: Mutex<Live>,
    /// The take this worker has closed its file in, or `u64::MAX` once it has left.
    pub finished: AtomicU64,
    /// Why the worker is to leave, once it is: after what its queue already holds.
    pub leave: Mutex<Option<String>>,
    /// The queue's event, which the worker parks on and the recorder rings.
    pub event: Wake,
}

pub(crate) struct Handle {
    pub armed: Arc<Mutex<Armed>>,
    pub shared: Arc<Shared>,
    thread: Option<goofi_supervisor::worker::Worker>,
}

impl Handle {
    /// Send the worker away: it closes its file as `why` and leaves on its own time.
    pub fn leave(self, why: &str) -> Handle {
        *self.shared.leave.lock() = Some(why.to_string());
        self.shared.event.ring(goofi_transport::POKED);
        self
    }

    /// Whether the worker has left: its file closed, its entry filed, nothing more to list.
    pub fn gone(&self) -> bool {
        self.shared.finished.load(std::sync::atomic::Ordering::Acquire) == u64::MAX
    }

    /// Wait for the thread to end, up to `within`.
    pub fn join_within(mut self, within: std::time::Duration) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join_within(within);
        }
    }
}

pub(crate) fn spawn(rec: &Arc<Recorder>, armed: Armed) -> Option<Handle> {
    let receiver = rec.hub.receiver(&armed.service, armed.depth);
    let shared = Arc::new(Shared {
        live: Mutex::new(Live::default()),
        finished: AtomicU64::new(0),
        leave: Mutex::new(None),
        event: receiver.event(),
    });
    let name = format!("goofi-record {}/{}", armed.id.node, armed.id.slot);
    let armed = Arc::new(Mutex::new(armed));
    let (weak, held, said) = (Arc::downgrade(rec), armed.clone(), shared.clone());
    let thread = goofi_supervisor::worker::thread(name).spawn(move || run(weak, receiver, held, said)).ok()?;
    Some(Handle { armed, shared, thread: Some(thread) })
}

/// The file open right now, and the take it belongs to.
struct Open {
    take: Arc<Take>,
    stream: Stream,
}

fn run(rec: Weak<Recorder>, mut receiver: Receiver, armed: Arc<Mutex<Armed>>, shared: Arc<Shared>) {
    let mut open: Option<Open> = None;
    // The take this stream died in: a frame the disk refused, or a file that would not open.
    // Refused rather than counted until the next take, or the next arming.
    let mut dead: Option<u64> = None;
    loop {
        let Some(rec) = rec.upgrade() else { break };
        if rec.held() {
            drop(rec);
            receiver.wait();
            continue;
        }
        if let Some(frame) = receiver.receive() {
            handle(&rec, &armed, &shared, &mut open, &mut dead, frame);
            shared.live.lock().fill = receiver.fill();
            continue;
        }
        // The queue is empty: what a departure or a stop waited for.
        let leaving = shared.leave.lock().clone();
        let id = armed.lock().id.clone();
        if let Some(why) = leaving {
            if let Some(o) = open.take() {
                close(&rec, &shared, &id, o, &why);
            }
            rec.finished(&shared, u64::MAX);
            break;
        }
        if let Some(take) = rec.current() {
            if take.stopping() {
                if let Some(o) = open.take() {
                    close(&rec, &shared, &id, o, "stopped");
                }
                rec.finished(&shared, take.id);
            }
        }
        shared.live.lock().fill = 0.0;
        drop(rec);
        receiver.wait();
    }
}

fn handle(rec: &Recorder, armed: &Mutex<Armed>, shared: &Shared, open: &mut Option<Open>, dead: &mut Option<u64>, frame: Frame) {
    let a = armed.lock().clone();
    let Some(take) = rec.current() else {
        if let Some(o) = open.take() {
            close(rec, shared, &a.id, o, "stopped");
        }
        return;
    };
    if open.as_ref().is_some_and(|o| o.take.id != take.id) {
        close(rec, shared, &a.id, open.take().expect("read above"), "stopped");
    }
    let meta = frame.meta();
    let at = meta.time().unwrap_or_else(|| rec.time().now());
    if !take.holds(a.timeline, at, meta.index()) || *dead == Some(take.id) {
        return;
    }
    let rate = (a.id.engine == "audio").then(|| meta.sfreq().unwrap_or_default());
    let kind = match Kind::of(&frame, rate, a.quality) {
        Ok(kind) => kind,
        Err(why) => {
            if let Some(o) = open.take() {
                close(rec, shared, &a.id, o, &format!("the frame could not be written: {why}"));
            }
            *dead = Some(take.id);
            return;
        }
    };
    if let Some(o) = open.as_ref() {
        if !o.stream.takes(&kind, stream::size(&frame)) {
            let why = o.stream.reopened_because(&kind);
            close(rec, shared, &a.id, open.take().expect("read above"), why);
        }
    }
    if open.is_none() {
        let t0_utc = rec.time().utc_at(at);
        let base = format!("{}-{}__{}Z", a.id.node, a.id.slot, goofi_core::time::stamp_nanos(t0_utc));
        let encoders = rec.encoders();
        let file = take.free_name(&base, kind.extension(&*encoders));
        let channels = meta.channels().dims().next().map(|(_, c)| c.len());
        match Stream::create(&*encoders, &take.folder, file.clone(), kind.clone(), meta.sfreq(), a.timeline, channels, at, t0_utc) {
            Ok(stream) => {
                let mut live = shared.live.lock();
                live.entry = Some(stream.entry(&a.id, None, None));
                live.take = take.id;
                live.error = None;
                drop(live);
                let _ = rec.publish(&take);
                *open = Some(Open { take: take.clone(), stream });
            }
            // A stream that could not open is an ENTRY, not an absence: a recording says what it
            // was asked for and did not get, or nobody reading it later can tell.
            Err(why) => {
                let base = manifest::Entry::base(&a.id, &file, &kind, meta.sfreq(), a.timeline, channels, at, t0_utc);
                take.closed.lock().push(manifest::Entry { closed_because: Some("never opened".into()), error: Some(why.clone()), ..base });
                shared.live.lock().error = Some(why);
                *dead = Some(take.id);
                let _ = rec.publish(&take);
                return;
            }
        }
    }
    let o = open.as_mut().expect("opened above");
    match o.stream.write(at, &frame) {
        Ok(()) => {
            let mut live = shared.live.lock();
            if let Some(entry) = live.entry.as_mut() {
                o.stream.count_into(entry);
            }
            drop(live);
            rec.note(&take);
        }
        Err(why) => {
            close(rec, shared, &a.id, open.take().expect("read above"), &format!("the frame could not be written: {why}"));
            *dead = Some(take.id);
        }
    }
}

/// Finish the file — an encoder is waited for here, on this stream's own thread — and file its
/// entry in the take it was opened in, which may be one that already stopped.
fn close(rec: &Recorder, shared: &Shared, id: &crate::StreamId, mut open: Open, why: &str) {
    let error = open.stream.finish().err();
    let entry = open.stream.entry(id, Some(why.to_string()), error);
    // Filed and let go in one step: a status that no longer lists the stream is one whose entry
    // the manifest already has.
    {
        let mut live = shared.live.lock();
        open.take.closed.lock().push(entry);
        live.entry = None;
    }
    let _ = rec.publish(&open.take);
}
