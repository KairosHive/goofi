//! The hosted native tier: a Rust node built after boot runs its library in a child of goofi's
//! own binary, spoken to over the exchange the Python subprocess tier uses. A library once loaded
//! is never unloaded, so this is what lets a node authored in the session run its newest build.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use goofi_core::Data;
use goofi_signal_sdk::host::Entry;
use goofi_signal_sdk::{Inputs, Node, NodeCtx, NodeError, NodeResult, Outputs};
use goofi_node::{NodeManifest, ParamKey, Params};
use goofi_transport::{Exchange, Served};

static HOSTED_SEQ: AtomicU64 = AtomicU64::new(0);

/// How long a request waits on a child that stopped answering; the first one also pays the load.
const TICK_TIMEOUT: Duration = Duration::from_secs(10);
const COLD_START_TIMEOUT: Duration = Duration::from_secs(30);

/// A frame across the exchange: the entry byte, the patch time, then the codec request.
fn frame(entry: Entry, now: f64, request: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(9 + request.len());
    out.push(entry as u8);
    out.extend_from_slice(&now.to_le_bytes());
    out.extend_from_slice(request);
    out
}

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
pub struct HostedNode {
    host: PathBuf,
    artifact: PathBuf,
    manifest: &'static NodeManifest,
    live: Option<(goofi_core::child::Child, Exchange)>,
}

impl HostedNode {
    pub fn new(host: PathBuf, artifact: PathBuf, manifest: &'static NodeManifest) -> HostedNode {
        HostedNode { host, artifact, manifest, live: None }
    }

    fn spawn(&self) -> Result<(goofi_core::child::Child, Exchange), String> {
        let base = format!("goofi_host_{}_{}", std::process::id(), HOSTED_SEQ.fetch_add(1, Ordering::Relaxed));
        let mut cmd = std::process::Command::new(&self.host);
        cmd.arg("host").arg("serve").arg(&self.artifact).arg(self.manifest.type_name)
            .env("GOOFI_IOX_REQ", format!("{base}_req"))
            .env("GOOFI_IOX_RESP", format!("{base}_resp"));
        let child = goofi_core::child::run(format!("native node {} (hosted)", self.manifest.type_name), &mut cmd)
            .source(goofi_core::log::source())
            .spawn()
            .map_err(|e| format!("spawn the host: {e}"))?;
        let exchange = Exchange::open(&base)?;
        Ok((child, exchange))
    }

    /// One request to the child, spawning it first if need be; a child that failed is dropped so
    /// the next request starts a fresh one.
    fn ask(&mut self, entry: Entry, now: f64, request: &[u8]) -> Result<goofi_codec::Response, String> {
        let timeout = if self.live.is_none() { COLD_START_TIMEOUT } else { TICK_TIMEOUT };
        if self.live.is_none() {
            self.live = Some(self.spawn()?);
        }
        let (child, exchange) = self.live.as_mut().expect("spawned");
        let reply = exchange.ask(child, &frame(entry, now, request), timeout);
        if reply.is_err() {
            self.live = None;
        }
        goofi_codec::decode_response(&reply?)
    }

    fn done(answer: Result<goofi_codec::Response, String>) -> NodeResult {
        match answer {
            Ok(goofi_codec::Response::Process(_)) => Ok(()),
            Ok(goofi_codec::Response::NodeError(msg)) => Err(NodeError(msg)),
            Ok(goofi_codec::Response::Options(_)) => Err(NodeError("the node answered options where none were asked".into())),
            Err(e) => Err(NodeError(e)),
        }
    }
}

impl Node for HostedNode {
    fn setup(&mut self, ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        Self::done(self.ask(Entry::Setup, ctx.now, &goofi_codec::encode_request(p.groups(), &[])))
    }

    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let mut present: Vec<(&str, &str, &Data)> = Vec::new();
        for slot in self.manifest.inputs {
            if slot.multi {
                present.extend(inp.get_multi(slot.name).iter().map(|(source, d)| (slot.name, source.as_str(), d)));
            } else if let Some(d) = inp.get(slot.name) {
                present.push((slot.name, "", d));
            }
        }
        match self.ask(Entry::Process, ctx.now, &goofi_codec::encode_request(p.groups(), &present)) {
            Ok(goofi_codec::Response::Process(result)) => {
                for slot in result.clear_inputs {
                    ctx.clear_input(&slot);
                }
                for (slot, data) in result.outputs {
                    out.set(&slot, data);
                }
                Ok(())
            }
            other => Self::done(other),
        }
    }

    fn on_param_changed(&mut self, key: &ParamKey, v: &goofi_core::Param) -> NodeResult {
        let request = rmp_serde::to_vec(&(&key.group, &key.name, v)).map_err(|e| NodeError(e.to_string()))?;
        Self::done(self.ask(Entry::ParamChanged, 0.0, &request))
    }

    fn on_param_refreshed(&mut self, key: &ParamKey, p: &Params<'_>) -> Option<Vec<String>> {
        match self.ask(Entry::Refresh, 0.0, &goofi_codec::encode_refresh_request(p.groups(), &key.group, &key.name)) {
            Ok(goofi_codec::Response::Options(options)) => options,
            _ => None,
        }
    }

    fn on_pulse(&mut self, key: &ParamKey, p: &Params<'_>) -> NodeResult {
        Self::done(self.ask(Entry::Pulse, 0.0, &goofi_codec::encode_pulse_request(p.groups(), &key.group, &key.name)))
    }
}

impl Drop for HostedNode {
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
                raw.call(entry, now, &rest[8..]).unwrap_or_else(|e| goofi_codec::encode_error_response(&e))
            }
            _ => goofi_codec::encode_error_response("a malformed request"),
        };
        served.answer(seq, &reply)?;
    }
}
