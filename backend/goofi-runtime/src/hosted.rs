//! The hosted native tier, for every engine: a Rust node built after boot runs its library in a
//! child of goofi's own binary, over the Python subprocess tier's exchange, so its newest build runs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use goofi_codec::rpc::{self, Entry};
use goofi_node::NodeManifest;
use goofi_host_sdk::host::{in_slots, Call, CodecNode};
use goofi_supervisor::child::Child;
use goofi_transport::{Exchange, Iox, Served};

/// Load a native host type. After boot, its library belongs to a child of `host`.
pub fn load(
    iox: Arc<Iox>,
    artifact: PathBuf,
    type_name: &str,
    own: Option<goofi_core::SlotType>,
    host: Option<&Path>,
    booted: bool,
) -> Result<(&'static NodeManifest, goofi_host_sdk::NodeFactory, &'static goofi_node::IsolationCell), String> {
    let host = if booted {
        Some(host.ok_or("no host executable was named, so a node built after boot cannot run")?.to_path_buf())
    } else {
        None
    };
    let describe = match &host {
        Some(host) => describe(host, &artifact)?,
        None => goofi_build::open(&artifact)?.describe,
    };
    let manifest = goofi_node::manifest_of(type_name, &goofi_node::parse_introspection(&describe)?, own)?;
    let (factory, tier): (goofi_host_sdk::NodeFactory, _) = match host {
        Some(host) => (Box::new(move |_| Box::new(node(iox.clone(), host.clone(), artifact.clone(), manifest))), &goofi_node::HOSTED),
        None => {
            let loaded = unsafe { goofi_host_sdk::host::Loaded::open(goofi_build::open(&artifact)?.library, manifest) }?;
            (Box::new(move |_| loaded.instantiate()), &goofi_node::NATIVE)
        }
    };
    Ok((manifest, factory, tier))
}

/// Makes each exchange's service names unique, so concurrent children never collide.
static SEQ: AtomicU64 = AtomicU64::new(0);

/// How long a describe may take: a one-shot probe, bounded like a build.
const DESCRIBE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the stop waits for the child to release what it holds before it is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
/// How long the child's wait for a request lasts before it looks at the parent again.
const SLICE: Duration = Duration::from_millis(100);

/// What a built library says it is, read by a child so the library never enters this process.
fn describe(host: &Path, artifact: &Path) -> Result<String, String> {
    let mut cmd = std::process::Command::new(host);
    cmd.arg("host").arg("describe").arg(artifact);
    let name = artifact.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let out = goofi_supervisor::child::output(format!("native describe {name}"), &mut cmd, DESCRIBE_TIMEOUT).map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A node whose library runs in a child of `host`, so this process never loads it.
fn node(iox: Arc<Iox>, host: PathBuf, artifact: PathBuf, manifest: &'static NodeManifest) -> CodecNode<Spawned> {
    let type_name = manifest.type_name;
    let command = move || {
        let mut cmd = Command::new(&host);
        cmd.arg("host").arg("serve").arg(&artifact).arg(type_name);
        cmd
    };
    CodecNode::new(Spawned::new(iox, format!("native node {type_name} (hosted)"), command, None), in_slots(manifest))
}

/// A node's child, called over the exchange its `GOOFI_IOX_BASE` names: spawned on the first
/// call, replaced after a failed one, and handed `stdin` once if there is one.
pub struct Spawned {
    iox: Arc<Iox>,
    name: String,
    command: Box<dyn Fn() -> Command + Send>,
    stdin: Option<String>,
    live: Option<(Child, Exchange)>,
}

impl Spawned {
    pub fn new(iox: Arc<Iox>, name: String, command: impl Fn() -> Command + Send + 'static, stdin: Option<String>) -> Spawned {
        Spawned { iox, name, command: Box::new(command), stdin, live: None }
    }

    fn spawn(&self) -> Result<(Child, Exchange), String> {
        let base = format!("goofi_child_{}_{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
        // The parent's end stands first, so the child's ready ring finds a listener.
        let exchange = Exchange::open(&self.iox, &base)?;
        let mut cmd = (self.command)();
        cmd.env("GOOFI_IOX_BASE", base);
        let mut run = goofi_supervisor::child::run(self.name.clone(), &mut cmd).source(goofi_supervisor::log::source());
        if self.stdin.is_some() {
            run = run.stdin_piped();
        }
        let mut child = run.spawn().map_err(|e| format!("spawn {}: {e}", self.name))?;
        if let Some(text) = &self.stdin {
            // Its write end drops here, and that EOF is where the child stops reading.
            let mut pipe = child.stdin.take().ok_or("the child took no stdin")?;
            pipe.write_all(text.as_bytes()).map_err(|e| format!("write the child's stdin: {e}"))?;
        }
        Ok((child, exchange))
    }
}

impl Call for Spawned {
    fn needs_seed(&mut self) -> bool {
        self.live.is_none()
    }

    /// One call to the child, spawning it first if need be; a child that failed is dropped so
    /// the next call starts a fresh one. A stop is the child's last call, then its end.
    fn call(&mut self, entry: Entry, now: f64, request: &[&[u8]]) -> Result<Vec<u8>, String> {
        if entry == Entry::Stop {
            if let Some(live) = self.live.take() {
                end(live, now);
            }
            return Ok(rpc::done());
        }
        if self.live.is_none() {
            self.live = Some(self.spawn()?);
        }
        let (child, exchange) = self.live.as_mut().expect("spawned");
        let head = rpc::call_head(entry, now);
        let frame: Vec<&[u8]> = std::iter::once(&head[..]).chain(request.iter().copied()).collect();
        let reply = exchange.ask(child, &frame);
        if reply.is_err() {
            // Halted mid-call or failed: it still ends by its stop, as a kill would leave its ports.
            end(self.live.take().expect("spawned"), now);
        }
        reply
    }
}

/// A child's last call, then its end. One that answered its stop leaves by itself and releases
/// its ports on the way; a kill would cut that short, so each half has its own grace.
fn end((mut child, mut exchange): (Child, Exchange), now: f64) {
    let stop = &[&rpc::call_head(Entry::Stop, now)[..]];
    let by = std::time::Instant::now() + STOP_TIMEOUT;
    let _ = exchange.send(&mut child, stop).and_then(|seq| exchange.answer(&mut child, seq, Some(by)));
    let _ = child.wait_within(STOP_TIMEOUT);
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
