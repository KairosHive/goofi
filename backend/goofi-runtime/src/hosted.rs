//! The hosted native tier, for every engine: a Rust node built after boot runs its library in a
//! child of goofi's own binary, spoken to over the exchange the Python subprocess tier uses. A library once loaded
//! is never unloaded, so this is what lets a node authored in the session run its newest build.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use goofi_codec::rpc::{self, Entry};
use goofi_node::NodeManifest;
use goofi_host_sdk::host::{in_slots, Call, CodecNode};
use goofi_transport::{Exchange, Served};

static HOSTED_SEQ: AtomicU64 = AtomicU64::new(0);

/// How long a request waits on a child that stopped answering; the first one also pays the load.
const TICK_TIMEOUT: Duration = Duration::from_secs(10);
const COLD_START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the stop waits for the child to release what it holds before it is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
/// How long the child's wait for a request lasts before it looks at the parent again.
const SLICE: Duration = Duration::from_millis(100);

/// What a built library says it is, read by a child so the library never enters this process.
pub fn describe(host: &Path, artifact: &Path) -> Result<String, String> {
    let mut cmd = std::process::Command::new(host);
    cmd.arg("host").arg("describe").arg(artifact);
    let name = artifact.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let out = goofi_supervisor::child::output(format!("native describe {name}"), &mut cmd, COLD_START_TIMEOUT).map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A node whose library runs in a child, spawned on its first call and replaced on a failure.
pub type HostedNode = CodecNode<Hosted>;

/// The child that holds a hosted node's library, called over the exchange.
pub struct Hosted {
    iox: Arc<goofi_transport::Iox>,
    host: PathBuf,
    artifact: PathBuf,
    type_name: &'static str,
    live: Option<(goofi_supervisor::child::Child, Exchange)>,
}

impl Hosted {
    pub fn node(iox: Arc<goofi_transport::Iox>, host: PathBuf, artifact: PathBuf, manifest: &'static NodeManifest) -> HostedNode {
        CodecNode::new(Hosted { iox, host, artifact, type_name: manifest.type_name, live: None }, in_slots(manifest))
    }

    fn spawn(&self) -> Result<(goofi_supervisor::child::Child, Exchange), String> {
        let base = format!("goofi_host_{}_{}", std::process::id(), HOSTED_SEQ.fetch_add(1, Ordering::Relaxed));
        // The parent's end stands first, so the child's ready ring finds a listener.
        let exchange = Exchange::open(&self.iox, &base)?;
        let mut cmd = std::process::Command::new(&self.host);
        cmd.arg("host").arg("serve").arg(&self.artifact).arg(self.type_name).env("GOOFI_IOX_BASE", base);
        let child = goofi_supervisor::child::run(format!("native node {} (hosted)", self.type_name), &mut cmd)
            .source(goofi_supervisor::log::source())
            .spawn()
            .map_err(|e| format!("spawn the host: {e}"))?;
        Ok((child, exchange))
    }
}

impl Call for Hosted {
    fn needs_seed(&mut self) -> bool {
        self.live.is_none()
    }

    /// One call to the child, spawning it first if need be; a child that failed is dropped so
    /// the next call starts a fresh one. A stop is the child's last call, then its end.
    fn call(&mut self, entry: Entry, now: f64, request: &[&[u8]]) -> Result<Vec<u8>, String> {
        if entry == Entry::Stop {
            if let Some((mut child, mut exchange)) = self.live.take() {
                let _ = exchange.ask(&mut child, &[&rpc::call_head(Entry::Stop, now)], STOP_TIMEOUT);
                child.stop(Duration::ZERO);
            }
            return Ok(rpc::done());
        }
        let timeout = if self.live.is_none() { COLD_START_TIMEOUT } else { TICK_TIMEOUT };
        if self.live.is_none() {
            self.live = Some(self.spawn()?);
        }
        let (child, exchange) = self.live.as_mut().expect("spawned");
        let head = rpc::call_head(entry, now);
        let frame: Vec<&[u8]> = std::iter::once(&head[..]).chain(request.iter().copied()).collect();
        let reply = exchange.ask(child, &frame, timeout);
        if reply.is_err() {
            self.live = None;
        }
        reply
    }
}

/// The child side, `goofi host …`: `describe <artifact>` prints what the library says it is;
/// `serve <artifact> <type>` answers the parent's requests until the parent is gone.
pub fn host_main(args: &[String]) -> i32 {
    let ran = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["describe", artifact] => goofi_build::open(Path::new(artifact)).map(|opened| println!("{}", opened.describe)),
        ["serve", artifact, type_name] => serve(Path::new(artifact), type_name),
        _ => Err("usage: goofi host describe <artifact> | serve <artifact> <type>".into()),
    };
    match ran {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("goofi host: {e}");
            1
        }
    }
}

fn serve(artifact: &Path, type_name: &str) -> Result<(), String> {
    // FIRST, before the library loads, so a child orphaned during a slow load still stops.
    goofi_supervisor::child::watch_parent().map_err(|e| format!("parent-liveness watcher: {e}"))?;
    let opened = goofi_build::open(artifact)?;
    let intro = goofi_node::parse_introspection(&opened.describe)?;
    let manifest = goofi_node::leak_manifest(type_name.to_string(), &intro)?;
    // SAFETY: `open` matched the library's version before handing it out.
    let loaded = unsafe { goofi_host_sdk::host::Loaded::open(opened.library, manifest) }?;
    let mut raw = loaded.raw();
    let mut served = Served::open_from_env()?;
    loop {
        let Some((seq, body)) = served.wait(SLICE)? else { continue };
        let call = rpc::split_call(&body);
        let reply = match &call {
            Ok((entry, now, request)) => raw.call(*entry, *now, &[request]).unwrap_or_else(|e| rpc::encode_error_response(&e)),
            Err(e) => rpc::encode_error_response(e),
        };
        served.answer(seq, &reply)?;
        if matches!(call, Ok((Entry::Stop, ..))) {
            return Ok(());
        }
    }
}
