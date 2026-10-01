//! The hosted native tier: a Rust node built after boot runs its library in a child of goofi's
//! own binary, spoken to over the exchange the Python subprocess tier uses. A library once loaded
//! is never unloaded, so this is what lets a node authored in the session run its newest build.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use goofi_node::NodeManifest;
use goofi_signal_sdk::host::{Ask, Entry, Handle};
use goofi_transport::{Exchange, Served};

static HOSTED_SEQ: AtomicU64 = AtomicU64::new(0);

/// How long a request waits on a child that stopped answering; the first one also pays the load.
const TICK_TIMEOUT: Duration = Duration::from_secs(10);
const COLD_START_TIMEOUT: Duration = Duration::from_secs(30);

/// What a built library says it is, read by a child so the library never enters this process.
pub fn describe(host: &Path, artifact: &Path) -> Result<String, String> {
    let mut cmd = std::process::Command::new(host);
    cmd.arg("host").arg("describe").arg(artifact);
    let name = artifact.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let out = goofi_core::child::output(format!("native describe {name}"), &mut cmd, COLD_START_TIMEOUT).map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A node whose library runs in a child, spawned on its first call and replaced on a failure.
pub type HostedNode = Handle<Hosted>;

/// The child that holds a hosted node's library, asked over the exchange: `[entry][f64 now]`,
/// then the codec request.
pub struct Hosted {
    iox: Arc<goofi_transport::Iox>,
    host: PathBuf,
    artifact: PathBuf,
    type_name: &'static str,
    live: Option<(goofi_core::child::Child, Exchange)>,
}

impl Hosted {
    pub fn node(iox: Arc<goofi_transport::Iox>, host: PathBuf, artifact: PathBuf, manifest: &'static NodeManifest) -> HostedNode {
        Handle::new(Hosted { iox, host, artifact, type_name: manifest.type_name, live: None }, manifest)
    }

    fn spawn(&self) -> Result<(goofi_core::child::Child, Exchange), String> {
        let base = format!("goofi_host_{}_{}", std::process::id(), HOSTED_SEQ.fetch_add(1, Ordering::Relaxed));
        let mut cmd = std::process::Command::new(&self.host);
        cmd.arg("host").arg("serve").arg(&self.artifact).arg(self.type_name)
            .env("GOOFI_IOX_REQ", format!("{base}_req"))
            .env("GOOFI_IOX_RESP", format!("{base}_resp"));
        let child = goofi_core::child::run(format!("native node {} (hosted)", self.type_name), &mut cmd)
            .source(goofi_core::log::source())
            .spawn()
            .map_err(|e| format!("spawn the host: {e}"))?;
        let exchange = Exchange::open(&self.iox, &base)?;
        Ok((child, exchange))
    }
}

impl Ask for Hosted {
    /// One request to the child, spawning it first if need be; a child that failed is dropped so
    /// the next request starts a fresh one.
    fn ask(&mut self, entry: Entry, now: f64, request: &[&[u8]]) -> Result<Vec<u8>, String> {
        let timeout = if self.live.is_none() { COLD_START_TIMEOUT } else { TICK_TIMEOUT };
        if self.live.is_none() {
            self.live = Some(self.spawn()?);
        }
        let (child, exchange) = self.live.as_mut().expect("spawned");
        let mut head = vec![entry as u8];
        head.extend_from_slice(&now.to_le_bytes());
        let frame: Vec<&[u8]> = std::iter::once(&head[..]).chain(request.iter().copied()).collect();
        let reply = exchange.ask(child, &frame, timeout);
        if reply.is_err() {
            self.live = None;
        }
        reply
    }
}

impl Drop for Hosted {
    fn drop(&mut self) {
        if let Some((mut child, _)) = self.live.take() {
            child.stop(Duration::ZERO);
        }
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
    goofi_core::child::watch_parent().map_err(|e| format!("parent-liveness watcher: {e}"))?;
    let opened = goofi_build::open(artifact)?;
    let intro = goofi_node::parse_introspection(&opened.describe)?;
    let manifest = goofi_node::leak_manifest(type_name.to_string(), &intro)?;
    // SAFETY: `open` matched the library's version before handing it out.
    let loaded = unsafe { goofi_signal_sdk::host::Loaded::open(opened.library, manifest) }?;
    let mut raw = loaded.raw();
    let mut served = Served::open_from_env()?;
    loop {
        let Some((seq, body)) = served.request()? else {
            std::thread::sleep(Duration::from_micros(500));
            continue;
        };
        let reply = match body.split_first().and_then(|(e, rest)| Some((Entry::from_u8(*e)?, rest))) {
            Some((entry, rest)) if rest.len() >= 8 => {
                let now = f64::from_le_bytes(rest[..8].try_into().unwrap());
                raw.ask(entry, now, &[&rest[8..]]).unwrap_or_else(|e| goofi_codec::encode_error_response(&e))
            }
            _ => goofi_codec::encode_error_response("a malformed request"),
        };
        served.answer(seq, &reply)?;
    }
}
