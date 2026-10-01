//! The subprocess Python tier: one GIL interpreter per node, called over the exchange the hosted
//! tier uses, `[entry][now]` then the codec request.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use goofi_host_sdk::host::CodecNode;
use goofi_node::Isolation;
use goofi_runtime::hosted::Spawned;

use crate::Discovery;

/// A Python node in an isolated GIL subprocess, spawned lazily on its first call.
pub type RemoteNode = CodecNode<Spawned>;

/// The child interpreter that runs `source`. The source rides stdin, never the environment:
/// Windows caps a whole environment block at 32767 characters.
pub fn subproc(iox: Arc<goofi_transport::Iox>, python: impl Into<String>, source: impl Into<String>) -> Spawned {
    let python = python.into();
    let name = format!("python node ({python})");
    let command = move || {
        let mut cmd = Command::new(&python);
        cmd.arg("-c").arg("import goofi; goofi.serve()").env("PYTHONUNBUFFERED", "1");
        cmd
    };
    Spawned::new(iox, name, command, Some(source.into()))
}

/// Probe one file for this tier, reporting all three outcomes.
pub fn probe(path: &Path, python: &str, memo: &Path) -> Discovery {
    crate::discover_one(path, python, Isolation::Subprocess, memo)
}
