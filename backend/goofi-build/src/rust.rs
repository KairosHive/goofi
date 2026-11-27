//! An `.rs` node file: the SDK whose `export!` it spells is the engine it declares, and the
//! library built from it answers its declaration.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use goofi_node::{Body, Read};
use goofi_supervisor::sync::Mutex;

/// How long a describe may take: a one-shot probe, bounded like a build.
const DESCRIBE_TIMEOUT: Duration = Duration::from_secs(30);

fn sdk_of(path: &Path) -> Result<&'static crate::Sdk, crate::library::Failed> {
    let source = std::fs::read_to_string(path).map_err(|e| (format!("{}: {e}", path.display()), None))?;
    crate::sdk_of(&source).ok_or((crate::NO_SDK.to_string(), None))
}

/// Build `path` against the SDK it names; what a build said is kept for [`read`].
pub(crate) fn build(path: &Path) {
    if let Ok(sdk) = sdk_of(path) {
        let _ = crate::ensure(sdk, path, &goofi_supervisor::layout::runtime().build());
    }
}

/// The artifact a build left for these bytes, described. One built after boot is described and
/// run by a child of `host`, where its SDK takes one, so a re-authored node's newest build runs.
pub(crate) fn read(path: &Path, host: Option<&Path>, booted: bool, described: &Mutex<HashMap<PathBuf, String>>) -> Result<Read, crate::library::Failed> {
    let sdk = sdk_of(path)?;
    let engine = || Some(sdk.engine.to_string());
    let artifact = crate::built(sdk, path, &goofi_supervisor::layout::runtime().build()).map_err(|why| (why, engine()))?;
    let host = match sdk.hosted && booted && !crate::is_open(&artifact) {
        true => Some(host.ok_or(("no host executable was named, so a node built after boot cannot run".to_string(), engine()))?.to_path_buf()),
        false => None,
    };
    let describe = match &host {
        Some(host) => {
            let known = described.lock().get(&artifact).cloned();
            match known {
                Some(known) => known,
                None => {
                    let fresh = describe(host, &artifact).map_err(|why| (why, engine()))?;
                    described.lock().insert(artifact.clone(), fresh.clone());
                    fresh
                }
            }
        }
        None => crate::open(&artifact).map_err(|why| (why, engine()))?.describe,
    };
    let intro = goofi_node::parse_introspection(&describe).map_err(|why| (why, engine()))?;
    Ok(Read { intro: Arc::new(intro), body: Body::Rust { artifact, host } })
}

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
