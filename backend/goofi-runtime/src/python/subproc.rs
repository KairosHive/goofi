//! The subprocess Python tier: one GIL interpreter per node, called over the exchange the hosted
//! tier uses, `[entry][now]` then the codec request.

use std::process::Command;
use std::sync::Arc;

use crate::loaded::CodecNode;
use crate::hosted::Spawned;

/// A Python node in an isolated GIL subprocess, spawned lazily on its first call.
pub type RemoteNode = CodecNode<Spawned>;

/// The child interpreter that runs `source`. The source rides stdin, never the environment:
/// Windows caps a whole environment block at 32767 characters.
pub fn subproc(hub: Arc<goofi_transport::Hub>, python: impl Into<String>, source: impl Into<String>) -> Spawned {
    subproc_in(hub, python, source, Vec::new())
}

/// [`subproc`], with `env` set in the child besides.
pub fn subproc_in(hub: Arc<goofi_transport::Hub>, python: impl Into<String>, source: impl Into<String>, env: Vec<(&'static str, String)>) -> Spawned {
    let python = python.into();
    let name = format!("python node ({python})");
    let command = move || {
        let mut cmd = Command::new(&python);
        cmd.arg("-c").arg("import goofi; goofi.serve()").env("PYTHONUNBUFFERED", "1").envs(env.iter().map(|(key, value)| (*key, value.as_str())));
        cmd
    };
    Spawned::new(hub, name, command, Some(source.into()))
}
