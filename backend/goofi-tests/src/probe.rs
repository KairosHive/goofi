//! Watching an asynchronous graph from a test.
//!
//! A probe must be opened BEFORE the frame it waits for — the data services carry
//! `history_size(0)`, so a probe opened later has missed what the producer already emitted.

use goofi_core::Data;

use goofi_graph::{Graph, Uid};
use goofi_transport::{iox_node, IoxNode};

/// A subscriber on one output slot — a viewer, with no privileged path into the node (§7).
pub struct OutputProbe {
    subscriber: goofi_transport::ByteSubscriber,
    /// The newest frame seen so far. Kept because the subscriber's queue is one deep and
    /// latest-wins.
    latest: std::cell::RefCell<Option<Data>>,
    /// Frames taken so far. `latest` alone cannot tell a stopped stream from a quiet one.
    seen: std::cell::Cell<u64>,
    /// Must outlive the subscriber built from it, so it is declared LAST — Rust drops a struct's
    /// fields in declaration order.
    _node: IoxNode,
}

impl OutputProbe {
    /// Open a probe on `(uid, slot)`. Panics for a missing node or slot, which would otherwise
    /// answer "silent" forever and read as a passing test.
    pub fn open(g: &Graph, uid: Uid, slot: &str) -> OutputProbe {
        let manifest = g.manifest(uid).unwrap_or_else(|| panic!("no node {uid}"));
        assert!(
            manifest.outputs.iter().any(|o| o.name == slot),
            "`{}` declares no output slot `{slot}`",
            manifest.type_name,
        );
        let node = iox_node().expect("an iceoryx2 node for the probe");
        let subscriber =
            goofi_transport::open_output_subscriber(&node, &goofi_bridge::output_service_of(g, uid, slot))
                .expect("a subscriber on the producer's output service");
        OutputProbe { _node: node, subscriber, latest: std::cell::RefCell::new(None), seen: std::cell::Cell::new(0) }
    }

    /// Take everything waiting, keeping the newest. Answers whether anything arrived.
    fn poll(&self) -> bool {
        let mut got = false;
        while let Ok(Some(sample)) = self.subscriber.receive() {
            if let Ok(frame) = goofi_codec::decode(sample.payload()) {
                *self.latest.borrow_mut() = Some(frame);
                self.seen.set(self.seen.get() + 1);
                got = true;
            }
        }
        got
    }

    /// How many frames have arrived, looking once and waiting for nothing — so a caller can ask
    /// whether a stream STOPPED, which the newest frame alone cannot say.
    pub fn count(&self) -> u64 {
        self.poll();
        self.seen.get()
    }

    /// The newest frame seen so far, looking once and waiting for nothing.
    pub fn latest(&self) -> Option<Data> {
        self.poll();
        self.latest.borrow().clone()
    }

    /// The newest frame, looking once after the graph drains its status. `None` while silent.
    pub fn frame(&self, g: &mut Graph) -> Option<Data> {
        g.drain_status();
        self.latest()
    }

    /// The newest frame of a graph driven directly, pumped while it waits against the harness
    /// budget; a failure names what was being waited for. A [`crate::Goofi`] waits with `until`.
    pub fn expect_frame(&self, g: &mut Graph, what: &str) -> Data {
        let deadline = std::time::Instant::now() + crate::WAIT;
        loop {
            if let Some(frame) = self.frame(g) {
                return frame;
            }
            assert!(std::time::Instant::now() < deadline, "timed out waiting for a frame: {what}");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
