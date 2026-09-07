//! The recorder's drain: ONE thread that holds a subscriber on every armed stream, drains each to
//! exhaustion and writes what it takes straight to disk. Never the `/data` plane — that one samples
//! on a timer and reduces for viewers, so it is lossy by design at every hop.
//!
//! It parks on the recorder's own door, which every armed slot rings once its frame is out, and it
//! ignores the event id: a burst across every armed slot coalesces into one sweep, and the id
//! budget is untouched.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use goofi_graph::{Graph, Uid};
use goofi_record::{Kind, Recorder, StreamId, StreamMeta};
use goofi_transport::{Halt, RECORD_BUFFER};

/// The longest a sweep waits on the door. Short enough that a `record start` between two frames
/// still opens its files at once, and it is a CEILING on the park rather than a cadence.
const WAKE: Duration = Duration::from_millis(20);

/// The engines whose armed slots publish GOOF frames on a record service. Audio and graphics have
/// doors of their own to come.
const DRAINED: &str = "signal";

/// One armed stream: the recorder's end of its service, and what the drain remembers between wakes.
struct Feed {
    id: StreamId,
    service: String,
    subscriber: goofi_transport::ByteSubscriber,
    /// The recorder holds a file for this stream. Re-derived from the recorder at every resolve,
    /// so a disarm and a re-arm inside one wake cannot leave a stale belief behind.
    opened: bool,
    /// The last frame's index, which is what makes a lost frame a COUNT rather than a silence.
    last: Option<u64>,
}

struct Drain {
    graph: Arc<Mutex<Graph>>,
    recorder: Arc<Recorder>,
    time: Arc<goofi_core::time::Time>,
    feeds: HashMap<(Uid, String), Feed>,
    listener: goofi_transport::Listener,
    /// Declared LAST: fields drop in order, and a node dropped before its ports cannot remove its
    /// own directory.
    node: goofi_transport::IoxNode,
}

/// Every armed signal stream the settled graph names, with the service its CURRENT generation
/// publishes on — so a rebirth reads as a name this drain has never opened.
fn armed(g: &Graph) -> HashMap<(Uid, String), (String, StreamId)> {
    let mut out = HashMap::new();
    for uid in g.all_uids() {
        for slot in g.recorded(uid).unwrap_or(&[]) {
            let id = crate::arms::stream_id(g, uid, slot);
            if id.engine != DRAINED {
                continue;
            }
            let base = goofi_transport::service_base(g.instance(), uid, g.node_generation(uid));
            out.insert((uid, slot.clone()), (goofi_transport::record_service(&base, slot), id));
        }
    }
    out
}

impl Drain {
    /// Reconcile the held feeds against the settled graph: a stream that left is closed, one whose
    /// service moved is closed as `reborn` and re-opened, and one newly armed gets its subscriber.
    fn resolve(&mut self) {
        let wanted = armed(&self.graph.lock().unwrap_or_else(|e| e.into_inner()));
        let recorder = &self.recorder;
        self.feeds.retain(|key, feed| match wanted.get(key) {
            Some((service, _)) if *service == feed.service => true,
            // A name that moved is a REBIRTH: a recording that smoothed over that gap would be
            // worse than one that stops, so the manifest says which it was.
            Some(_) => {
                recorder.close(&feed.id, "reborn");
                false
            }
            None => {
                recorder.close(&feed.id, "disarmed");
                false
            }
        });
        for (key, (service, id)) in wanted {
            match self.feeds.get_mut(&key) {
                Some(feed) => feed.opened &= recorder.is_open(&feed.id),
                None => {
                    if let Ok(subscriber) = goofi_transport::open_record_subscriber(&self.node, &service) {
                        self.feeds
                            .insert(key, Feed { id, service, subscriber, opened: false, last: None });
                    }
                }
            }
        }
    }

    /// Take every frame every feed holds, in one pass. A recorder that kept only the newest frame
    /// is the whole defect this feature exists to prevent, so nothing here stops at one.
    fn sweep(&mut self) {
        let (recorder, time) = (&self.recorder, &self.time);
        for feed in self.feeds.values_mut() {
            let mut taken = 0usize;
            while let Ok(Some(sample)) = feed.subscriber.receive() {
                let bytes = sample.payload();
                let meta = goofi_codec::frame_meta(bytes).ok();
                let at = meta.as_ref().and_then(|m| m.time()).unwrap_or_else(|| time.now());
                if !feed.opened {
                    let stream = match meta.as_ref().and_then(|m| m.sfreq()) {
                        Some(f) => StreamMeta::measured(Some(f)),
                        None => StreamMeta::derived(None),
                    };
                    let channels = meta.as_ref().and_then(|m| m.channels().dims().next().map(|(_, c)| c.len()));
                    let stream = match channels {
                        Some(n) => stream.with_channels(n),
                        None => stream,
                    };
                    if recorder.open(&feed.id, Kind::Frames, at, stream).is_err() {
                        break;
                    }
                    feed.opened = true;
                    feed.last = None;
                }
                let index = meta.as_ref().and_then(|m| m.index());
                // The subscriber overflows the OLDEST frame and says nothing, so the indices are
                // the only witness. An index that RESETS is a rebirth, never a loss.
                let missed = match (index, feed.last) {
                    (Some(i), Some(last)) if i > last + 1 => i - last - 1,
                    _ => 0,
                };
                feed.last = index;
                if recorder.write(&feed.id, bytes).is_err() {
                    break;
                }
                // Counted AFTER the frame that revealed the gap is on disk, so the manifest can
                // never claim a loss the file does not show.
                if missed > 0 {
                    recorder.dropped(&feed.id, missed, at);
                }
                taken += 1;
            }
            if taken > 0 {
                recorder.fill(&feed.id, taken as f32 / RECORD_BUFFER as f32);
            }
        }
    }
}

/// Start the one drain. `halt` is what stops it, and what a teardown waits on to a ceiling.
pub fn spawn(graph: Arc<Mutex<Graph>>, recorder: Arc<Recorder>, halt: Arc<Halt>) {
    let (instance, time) = {
        let g = graph.lock().unwrap_or_else(|e| e.into_inner());
        (g.instance().to_string(), g.time())
    };
    let ran = halt.clone();
    let started = goofi_transport::thread("goofi-record").spawn(move || {
        let Ok(node) = goofi_transport::iox_node() else {
            halt.release();
            return;
        };
        let door = goofi_transport::event_service(&node, &goofi_transport::record_door_service(&instance));
        let listener = door.and_then(|d| d.listener_builder().create().map_err(|e| e.to_string()));
        let Ok(listener) = listener else {
            halt.release();
            return;
        };
        let mut drain = Drain { graph, recorder, time, feeds: HashMap::new(), listener, node };
        while !halt.stopped() {
            let _ = drain.listener.timed_wait_all(|_| {}, WAKE);
            if !drain.recorder.running() {
                drain.feeds.clear();
                continue;
            }
            drain.resolve();
            drain.sweep();
        }
        drop(drain);
        halt.release();
    });
    if started.is_err() {
        // The thread is what releases the halt, so a teardown must not wait on one that never ran.
        ran.release();
    }
}
