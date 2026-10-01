//! The subprocess Python tier: one GIL interpreter per node, called over the exchange the hosted
//! tier uses, `[entry][now]` then the codec request.

use std::io::Write;
use std::sync::Arc;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use goofi_codec::rpc::{self, Entry};
use goofi_transport::Exchange;
use goofi_host_sdk::host::{Call, CodecNode};
use goofi_host_sdk::Node;

/// Unique iceoryx2 service-name base per spawned subprocess, so concurrent nodes never collide.
static SUBPROC_SEQ: AtomicU64 = AtomicU64::new(0);

/// How long a request waits on a child that has stopped answering.
pub const TICK_TIMEOUT: Duration = Duration::from_secs(10);

/// The deadline for the FIRST request after a spawn, which also pays interpreter boot, the node
/// module's imports and `setup()` — seconds, not milliseconds, for a heavy import like numba.
pub const COLD_START_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the stop waits for `stop()` to release what the node holds before the child is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// The spawned child plus the exchange it answers on.
struct Running {
    child: goofi_supervisor::child::Child,
    exchange: Exchange,
}

impl Running {
    fn spawn(iox: &goofi_transport::Iox, python: &str, source: &str) -> std::result::Result<Running, String> {
        let base = format!("goofi_sub_{}_{}", std::process::id(), SUBPROC_SEQ.fetch_add(1, Ordering::Relaxed));
        // The parent's end stands first, so the child's ready ring finds a listener. The child
        // JOINS this session — `spawn` tells it which — so its ports live under the same root
        // and prefix and are swept with it.
        let exchange = Exchange::open(iox, &base)?;
        let mut cmd = Command::new(python);
        cmd.arg("-c")
            .arg("import goofi; goofi.serve()")
            .env("GOOFI_IOX_BASE", base)
            .env("PYTHONUNBUFFERED", "1");
        // The source rides stdin, never the environment: Windows caps a whole environment
        // block at 32767 characters, and a node file is text of no stated size.
        let mut child = goofi_supervisor::child::run(format!("python node ({python})"), &mut cmd)
            .source(goofi_supervisor::log::source())
            .stdin_piped()
            .spawn()
            .map_err(|e| format!("spawn `{python}`: {e}"))?;
        // The write end is dropped as this ends, and that EOF is where the child stops reading.
        // A failure here drops `child`, which kills and reaps it.
        match child.stdin.take() {
            Some(mut w) => w.write_all(source.as_bytes()).map_err(|e| format!("hand the source over: {e}"))?,
            None => return Err("the child took no stdin".to_string()),
        }
        Ok(Running { child, exchange })
    }
}

/// A Python node in an isolated GIL subprocess, spawned lazily on its first call.
pub type RemoteNode = CodecNode<Subproc>;

/// The child interpreter a Python node runs in, called over the exchange.
pub struct Subproc {
    iox: Arc<goofi_transport::Iox>,
    python: String,
    source: String,
    live: Option<Running>,
}

impl Subproc {
    pub fn new(iox: Arc<goofi_transport::Iox>, python: impl Into<String>, source: impl Into<String>) -> Subproc {
        Subproc { iox, python: python.into(), source: source.into(), live: None }
    }
}

impl Call for Subproc {
    fn needs_seed(&mut self) -> bool {
        self.live.is_none()
    }

    /// One call to the child, spawning it first if need be; an IO failure drops the child so the
    /// next call starts a fresh one. A node RAISE does not kill the child: its state is preserved
    /// and the error is instant.
    fn call(&mut self, entry: Entry, now: f64, request: &[&[u8]]) -> Result<Vec<u8>, String> {
        if entry == Entry::Stop {
            if let Some(mut live) = self.live.take() {
                // An answered child leaves by itself and releases its ports on the way; a signal
                // would cut that short, so only the deadline kills it. One that did not answer is stopped.
                match live.exchange.ask(&mut live.child, &[&rpc::call_head(Entry::Stop, now)], STOP_TIMEOUT) {
                    Ok(_) => {
                        let _ = live.child.wait_within(STOP_TIMEOUT);
                    }
                    Err(_) => {
                        live.child.stop(Duration::ZERO);
                    }
                }
            }
            return Ok(rpc::done());
        }
        let timeout = if self.live.is_none() { COLD_START_TIMEOUT } else { TICK_TIMEOUT };
        if self.live.is_none() {
            self.live = Some(Running::spawn(&self.iox, &self.python, &self.source)?);
        }
        let live = self.live.as_mut().expect("spawned");
        let head = rpc::call_head(entry, now);
        let frame: Vec<&[u8]> = std::iter::once(&head[..]).chain(request.iter().copied()).collect();
        let reply = live.exchange.ask(&mut live.child, &frame, timeout).map_err(|e| format!("subprocess io: {e}"));
        if reply.is_err() {
            if let Some(mut live) = self.live.take() {
                live.child.stop(Duration::ZERO);
            }
        }
        reply
    }
}

use std::path::Path;

use crate::Discovered;
use goofi_host_sdk::NodeFactory;
use goofi_node::{Isolation, NodeManifest};

/// A discovered subprocess node type, ready to `register_dyn_type` into a Graph.
pub struct SubprocNodeType {
    pub manifest: &'static NodeManifest,
    pub isolation: &'static goofi_node::IsolationCell,
    pub factory: NodeFactory,
}

use crate::Discovery;

/// Probe one file for this tier, reporting all three outcomes.
pub fn probe(path: &Path, python: &str, memo: &Path) -> Discovery {
    crate::discover_one(path, python, Isolation::Subprocess, memo)
}

/// Turn a probe-[`Discovered`] into a [`SubprocNodeType`], without a second spawn.
pub fn node_type_from(iox: Arc<goofi_transport::Iox>, python: &str, d: Discovered) -> SubprocNodeType {
    subproc_type_from_discovered(iox, python, d)
}

fn subproc_type_from_discovered(iox: Arc<goofi_transport::Iox>, python: &str, d: Discovered) -> SubprocNodeType {
    let manifest = d.manifest;
    let in_slots = goofi_host_sdk::host::in_slots(manifest);
    let source = std::fs::read_to_string(&d.source).unwrap_or_default();
    let python = python.to_string();
    let factory: NodeFactory = Box::new(move |_p| {
        Box::new(RemoteNode::new(Subproc::new(iox.clone(), &python, &source), in_slots.clone())) as Box<dyn Node>
    });
    SubprocNodeType { manifest, isolation: d.isolation, factory }
}
