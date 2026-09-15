//! The subprocess Python tier: one GIL interpreter per node, one run per `[u32 seq][frame]`
//! request/response over iceoryx2 shared memory.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iceoryx2::prelude::*;

use goofi_core::Data;
use goofi_transport::{iox_node, subprocess_service, BytePublisher, ByteSubscriber, IoxNode};
use goofi_node::{ParamKey, Params};
use goofi_host_sdk::{Inputs, Node, NodeCtx, NodeError, NodeResult, Outputs};

/// Unique iceoryx2 service-name base per spawned subprocess, so concurrent nodes never collide.
static SUBPROC_SEQ: AtomicU64 = AtomicU64::new(0);

/// iceoryx2 byte-slice pool ceiling per publisher (matches the child's `serve` config).
pub const MAX_PAYLOAD: usize = 64 * 1024;

/// How long a request waits on a child that has stopped answering.
pub const TICK_TIMEOUT: Duration = Duration::from_secs(10);

/// The deadline for the FIRST request after a spawn, which also pays interpreter boot, the node
/// module's imports and `setup()` — seconds, not milliseconds, for a heavy import like numba.
pub const COLD_START_TIMEOUT: Duration = Duration::from_secs(60);

/// The iceoryx2 node + its ports. The node must outlive the ports it created.
struct Ports {
    req_pub: BytePublisher,
    resp_sub: ByteSubscriber,
    _node: IoxNode,
}

/// The spawned child plus the iceoryx2 ports it talks over.
struct Running {
    child: goofi_core::child::Child,
    ports: Ports,
    seq: u32,
}

fn build_ports(req_name: &str, resp_name: &str) -> std::result::Result<Ports, String> {
    let node = iox_node()?;
    let mk_pubsub = |name: &str| subprocess_service(&node, name);
    let req_pub = mk_pubsub(req_name)?
        .publisher_builder()
        .initial_max_slice_len(MAX_PAYLOAD)
        .allocation_strategy(AllocationStrategy::PowerOfTwo)
        .create()
        .map_err(|e| format!("req publisher: {e}"))?;
    let resp_sub = mk_pubsub(resp_name)?
        .subscriber_builder()
        .create()
        .map_err(|e| format!("resp subscriber: {e}"))?;
    Ok(Ports { _node: node, req_pub, resp_sub })
}

impl Running {
    fn spawn(python: &str, source: &str) -> std::result::Result<Running, String> {
        let id = format!("goofi_sub_{}_{}", std::process::id(), SUBPROC_SEQ.fetch_add(1, Ordering::Relaxed));
        let req_name = format!("{id}_req");
        let resp_name = format!("{id}_resp");
        // The child JOINS this session — `spawn` tells it which — so its ports live under the
        // same root and prefix and are swept with it.
        let mut cmd = Command::new(python);
        cmd.arg("-c")
            .arg("import goofi; goofi.serve()")
            .env("GOOFI_IOX_REQ", &req_name)
            .env("GOOFI_IOX_RESP", &resp_name)
            // The host's PYTHONPATH (the pyo3/FT tier's) must not shadow the child's own numpy/goofi.
            .env_remove("PYTHONPATH")
            .env_remove("PYTHONHOME")
            // The source rides stdin, never the environment: Windows caps a whole environment
            // block at 32767 characters, and a node file is text of no stated size.
            .stdin(Stdio::piped())
            .env("PYTHONUNBUFFERED", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = goofi_core::child::spawn(format!("python node ({python})"), &mut cmd)
            .map_err(|e| format!("spawn `{python}`: {e}"))?;
        if let Some(out) = child.stdout.take() {
            let source = goofi_core::log::source();
            std::thread::spawn(move || goofi_core::log::drain(out, source, "stdout"));
        }
        if let Some(err) = child.stderr.take() {
            let source = goofi_core::log::source();
            std::thread::spawn(move || goofi_core::log::drain(err, source, "stderr"));
        }
        // The write end is dropped as this ends, and that EOF is where the child stops reading.
        let handed = match child.stdin.take() {
            Some(mut w) => w.write_all(source.as_bytes()).map_err(|e| format!("hand the source over: {e}")),
            None => Err("the child took no stdin".to_string()),
        };
        // A failure here drops `child`, which kills and reaps it.
        let ports = handed.and_then(|()| build_ports(&req_name, &resp_name))?;
        Ok(Running { child, ports, seq: 0 })
    }

    fn roundtrip(&mut self, frame: &[u8], timeout: Duration) -> std::result::Result<Vec<u8>, String> {
        self.seq = self.seq.wrapping_add(1);
        one_roundtrip(&self.ports.req_pub, &self.ports.resp_sub, &mut self.child, self.seq, frame, timeout)
            .map_err(|e| format!("subprocess io: {e}"))
    }

    fn shutdown(&mut self) {
        self.child.stop(Duration::ZERO);
    }
}

/// One request/response: publish `[seq][frame]` and poll for the reply with the matching sequence.
/// Re-published each idle millisecond, because the child's subscriber may still be connecting.
fn one_roundtrip(
    req_pub: &BytePublisher,
    resp_sub: &ByteSubscriber,
    child: &mut goofi_core::child::Child,
    seq: u32,
    frame: &[u8],
    timeout: Duration,
) -> std::io::Result<Vec<u8>> {
    while matches!(resp_sub.receive(), Ok(Some(_))) {}

    let mut msg = Vec::with_capacity(4 + frame.len());
    msg.extend_from_slice(&seq.to_le_bytes());
    msg.extend_from_slice(frame);

    let deadline = Instant::now() + timeout;
    loop {
        match req_pub.loan_slice_uninit(msg.len()) {
            Ok(sample) => {
                let _ = sample.write_from_slice(msg.as_slice()).send();
            }
            Err(e) => return Err(std::io::Error::other(format!("iox publish: {e}"))),
        }
        loop {
            match resp_sub.receive() {
                Ok(Some(sample)) => {
                    let payload = sample.payload();
                    if payload.len() >= 4
                        && u32::from_le_bytes(payload[0..4].try_into().unwrap()) == seq
                    {
                        return Ok(payload[4..].to_vec());
                    }
                }
                Ok(None) => break, // drained; re-publish + wait
                Err(e) => return Err(std::io::Error::other(format!("iox receive: {e}"))),
            }
        }
        // Checked AFTER draining, so a child that answered and then exited still gets its answer returned.
        if let Ok(Some(status)) = child.try_wait() {
            return Err(std::io::Error::other(format!("subprocess exited: {status}")));
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::other("subprocess did not respond in time"));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A Python node in an isolated GIL subprocess, spawned lazily on its first `process`.
pub struct RemoteNode {
    python: String,
    source: String,
    /// Declared INPUT slots only, each with whether it is `multi`: the child is authoritative for
    /// output naming.
    in_slots: Vec<(&'static str, bool)>,
    proc: Option<Running>,
}

impl RemoteNode {
    pub fn new(python: impl Into<String>, source: impl Into<String>, in_slots: Vec<(&'static str, bool)>) -> RemoteNode {
        RemoteNode {
            python: python.into(),
            source: source.into(),
            in_slots,
            proc: None,
        }
    }

    fn ensure(&mut self) -> std::result::Result<&mut Running, String> {
        if self.proc.is_none() {
            self.proc = Some(Running::spawn(&self.python, &self.source)?);
        }
        Ok(self.proc.as_mut().unwrap())
    }

    fn reset(&mut self) {
        if let Some(mut p) = self.proc.take() {
            p.shutdown();
        }
    }

    /// One request to the child, spawning it first if need be; an IO failure drops the child so
    /// the next request starts a fresh one.
    fn ask(&mut self, frame: &[u8]) -> Result<goofi_codec::Response, String> {
        let timeout = if self.proc.is_none() { COLD_START_TIMEOUT } else { TICK_TIMEOUT };
        let resp = self.ensure().and_then(|r| r.roundtrip(frame, timeout)).inspect_err(|_| self.reset())?;
        goofi_codec::decode_response(&resp)
    }
}

impl Node for RemoteNode {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        // Only the PRESENT frames cross the wire, a `multi` slot's each with its source; the child
        // rebuilds the declared kwarg set from `INPUTS`.
        let mut present: Vec<(&str, &str, &Data)> = Vec::new();
        for (name, multi) in &self.in_slots {
            if *multi {
                present.extend(inp.get_multi(name).iter().map(|(source, d)| (*name, source.as_str(), d)));
            } else if let Some(d) = inp.get(name) {
                present.push((*name, "", d));
            }
        }
        // A node RAISE does not kill the child: its state is preserved and the error is instant.
        match self.ask(&goofi_codec::encode_request(p.groups(), &present)).map_err(NodeError)? {
            goofi_codec::Response::Process(result) => {
                for slot in result.clear_inputs {
                    ctx.clear_input(&slot);
                }
                for (slot, data) in result.outputs {
                    out.set(&slot, data);
                }
                Ok(())
            }
            goofi_codec::Response::NodeError(msg) => Err(NodeError(msg)),
            goofi_codec::Response::Options(_) => Err(NodeError("the child answered a tick with options".into())),
        }
    }

    fn on_param_refreshed(&mut self, key: &ParamKey, p: &Params<'_>) -> Option<Vec<String>> {
        match self.ask(&goofi_codec::encode_refresh_request(p.groups(), &key.group, &key.name)) {
            Ok(goofi_codec::Response::Options(options)) => options,
            _ => None,
        }
    }

    fn on_pulse(&mut self, key: &ParamKey, p: &Params<'_>) -> NodeResult {
        match self.ask(&goofi_codec::encode_pulse_request(p.groups(), &key.group, &key.name)).map_err(NodeError)? {
            goofi_codec::Response::NodeError(msg) => Err(NodeError(msg)),
            _ => Ok(()),
        }
    }
}

impl Drop for RemoteNode {
    fn drop(&mut self) {
        self.reset();
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
pub fn node_type_from(python: &str, d: Discovered) -> SubprocNodeType {
    subproc_type_from_discovered(python, d)
}

fn subproc_type_from_discovered(python: &str, d: Discovered) -> SubprocNodeType {
    let manifest = d.manifest;
    let in_slots: Vec<(&'static str, bool)> = manifest.inputs.iter().map(|s| (s.name, s.multi)).collect();
    let source = std::fs::read_to_string(&d.source).unwrap_or_default();
    let python = python.to_string();
    let factory: NodeFactory = Box::new(move |_p| {
        Box::new(RemoteNode::new(&python, &source, in_slots.clone())) as Box<dyn Node>
    });
    SubprocNodeType { manifest, isolation: d.isolation, factory }
}

/// The orphan guard, proven end to end: a parent killed with no chance to clean up still takes
/// its child with it. Linux only, for `/proc` and `kill -9`.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Doubles its `data` input into `out`.
    const DOUBLE: &str = r#"
import goofi
class Double(goofi.Node):
    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    def process(self, data):
        return {"out": data.data * 2.0}
"#;

    fn usable_python() -> Option<String> {
        let mut cands: Vec<String> = Vec::new();
        if let Ok(p) = std::env::var("GOOFI_SUBPROC_TEST_PYTHON") {
            cands.push(p);
        }
        // Both venv layouts, because on Windows `python3` is a Store-advert alias that never fails.
        cands.push(format!("{}/../../../.gfivenv/bin/python", env!("CARGO_MANIFEST_DIR")));
        cands.push(format!("{}/../../../.gfivenv/Scripts/python.exe", env!("CARGO_MANIFEST_DIR")));
        cands.push("python3".to_string());
        cands.push("python".to_string());
        for cand in cands {
            if let Ok(out) = Command::new(&cand)
                .arg("-c")
                .arg("import goofi, numpy")
                .env_remove("PYTHONPATH")
                .env_remove("PYTHONHOME")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                if out.success() {
                    return Some(cand);
                }
            }
        }
        None
    }

    /// Serializes the subprocess-tier tests; each of them spawns a Python interpreter.
    static TIER: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The interpreter to spawn children with, plus the tier lock held for the rest of the test.
    struct Tier {
        py: String,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl std::ops::Deref for Tier {
        type Target = str;
        fn deref(&self) -> &str {
            &self.py
        }
    }

    /// Like [`usable_python`] but panics with an actionable message: these tests never skip.
    fn require_python() -> Tier {
        // A panicking test poisons the mutex; recover so its failure does not bury every sibling.
        let _lock = TIER.lock().unwrap_or_else(|e| e.into_inner());
        let py = usable_python().unwrap_or_else(|| {
            panic!(
                "no python with goofi + numpy found (checked $GOOFI_SUBPROC_TEST_PYTHON, \
                 ./.gfivenv/bin/python, python3, python). Run `cargo run -p goofi-init`, which \
                 creates the venvs and installs the goofi wheel into them. The subprocess-tier \
                 tests require one."
            )
        });
        Tier { py, _lock }
    }

    /// The interpreter the helper process spawns its grandchild with.
    const HELPER_ENV: &str = "GOOFI_LIVENESS_HELPER_PYTHON";

    /// The intermediate parent for the hard-kill test, re-entered as a separate process: with
    /// [`HELPER_ENV`] set it spawns a child, announces its pid and blocks; unset it does nothing.
    #[test]
    fn liveness_helper_process() {
        let Ok(py) = std::env::var(HELPER_ENV) else { return };
        let running = Running::spawn(&py, DOUBLE).expect("helper: spawn a child");
        println!("HELPER_CHILD_PID={}", running.child.id());
        std::io::Write::flush(&mut std::io::stdout()).expect("helper: flush the pid");
        loop {
            // Hold `running` — and with it the write end — until we are killed.
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    /// Alive = present in /proc and not already a reaped-pending zombie.
    fn pid_alive(pid: u32) -> bool {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { return false };
        // `pid (comm) STATE …`, and comm may itself contain spaces or parens — scan past the last ')'.
        stat.rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .is_some_and(|state| state != "Z")
    }

    #[test]
    fn a_hard_killed_parent_still_stops_the_child() {
        let py = require_python();
        // libtest names a test by its module path MINUS the crate root; derived, so a move cannot
        // leave the filter matching nothing.
        let module = module_path!().split_once("::").map_or(module_path!(), |(_, rest)| rest);
        let helper_test = format!("{module}::liveness_helper_process");
        let mut helper = Command::new(std::env::current_exe().expect("test binary path"))
            .args(["--exact", &helper_test, "--nocapture", "--test-threads=1"])
            .env(HELPER_ENV, &*py)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the intermediate parent");

        // libtest writes `test <name> ... ` without a newline, so the marker lands mid-line.
        let out = helper.stdout.take().expect("helper stdout");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                if let Some((_, pid)) = line.rsplit_once("HELPER_CHILD_PID=") {
                    let _ = tx.send(pid.trim().to_string());
                    return;
                }
            }
        });
        let announced = rx.recv_timeout(Duration::from_secs(30));
        if announced.is_err() {
            let _ = helper.kill();
            let _ = helper.wait();
        }
        let child_pid: u32 =
            announced.expect("the helper must announce its child's pid").parse().expect("a numeric pid");

        let killed = Command::new("kill").args(["-9", &helper.id().to_string()]).status();
        assert!(killed.is_ok_and(|s| s.success()), "kill -9 the intermediate parent");
        let _ = helper.wait();

        let deadline = Instant::now() + Duration::from_secs(15);
        while pid_alive(child_pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let orphaned = pid_alive(child_pid);
        if orphaned {
            let _ = Command::new("kill").args(["-9", &child_pid.to_string()]).status();
        }
        assert!(
            !orphaned,
            "pid {child_pid} outlived its hard-killed parent by 15s — the liveness pipe did not fire"
        );
    }

}
