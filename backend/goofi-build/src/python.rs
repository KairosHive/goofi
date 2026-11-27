//! A `.py` node file: one probe imports it in the interpreter that will run it and answers its
//! declaration, the engine base its class derives included.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

use goofi_core::probe::Introspection;
use goofi_supervisor::sync::Mutex;
use sha2::{Digest, Sha256};

/// The interpreters a file is probed and run with. The subprocess one is the caller's; the
/// free-threaded one is the one this build links, if any, and it is what runs a file in-process.
#[derive(Clone, Debug)]
pub struct Python {
    pub subproc: String,
    pub free_threaded: Option<String>,
    /// Where a probe's answer outlives the process: in the build dir, beside the built nodes.
    pub memo: PathBuf,
}

impl Python {
    pub fn new(subproc: String, free_threaded: Option<String>) -> Python {
        let memo = goofi_supervisor::layout::runtime().build().join("probes");
        Python { subproc, free_threaded, memo }
    }

    pub fn interpreters(&self) -> Vec<&str> {
        std::iter::once(self.subproc.as_str()).chain(self.free_threaded.as_deref()).collect()
    }

    /// The GIL gate: a file whose imports keep the GIL disabled answers `gil_safe` and runs
    /// in-process, any other in a subprocess. One interpreter per probe: re-enabling the GIL
    /// is one-way.
    pub(crate) fn probe(&self, path: &Path) -> Result<Introspection, crate::library::Failed> {
        if let Some(ft) = self.free_threaded.as_deref() {
            // A re-enabled GIL falls through to the subprocess tier, as a failed probe does.
            if let Ok(intro) = introspect_memoised(path, ft, &self.memo) {
                if intro.gil_safe {
                    return Ok(intro);
                }
            }
        }
        introspect_memoised(path, &self.subproc, &self.memo).map(|intro| Introspection { gil_safe: false, ..intro })
    }
}

/// Why a node file could not be introspected, phrased for the palette tooltip.
fn probe_reason(stderr: &str) -> String {
    if let Some(rest) = stderr.split("No module named ").nth(1) {
        let name: String = rest.trim_start().trim_matches(['\'', '"']).chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        if !name.is_empty() {
            return name;
        }
    }
    stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("probe failed").trim().to_string()
}

/// How long one probe may take: an import of a deep learning stack is tens of seconds on a
/// cold cache, never minutes.
const PROBE_WAIT: Duration = Duration::from_secs(300);

/// Run `goofi.introspect(path)` in `python` and parse the result. A file whose import fails
/// still answers the engine its class names, read from its text.
fn introspect(path: &Path, python: &str) -> Result<Introspection, crate::library::Failed> {
    goofi_supervisor::progress::reading(path);
    // The payload is a dup of fd 1 taken before fd 1 is rerouted to stderr, so anything an
    // import prints to stdout — even from a C extension — cannot corrupt the JSON.
    const PROBE: &str = "\
import goofi, json, os, sys
payload = os.fdopen(os.dup(1), 'wb')
os.dup2(2, 1)
sys.stdout = sys.stderr
try:
    payload.write(goofi.introspect(sys.argv[1]).encode())
except BaseException:
    try:
        payload.write(json.dumps({'engine': goofi.declared_engine(sys.argv[1])}).encode())
    except Exception:
        pass
    raise
finally:
    payload.close()
";
    let out = goofi_supervisor::child::output(
        format!("python probe {}", path.file_name().unwrap_or_default().to_string_lossy()),
        Command::new(python)
            .arg("-c")
            .arg(PROBE)
            .arg(path),
        PROBE_WAIT,
    )
    .map_err(|e| (format!("could not run `{python}`: {e}"), None))?;
    if !out.status.success() {
        let named = serde_json::from_slice::<serde_json::Value>(&out.stdout).ok();
        let engine = named.and_then(|v| v["engine"].as_str().map(str::to_string));
        return Err((probe_reason(&String::from_utf8_lossy(&out.stderr)), engine));
    }
    let json = String::from_utf8(out.stdout).map_err(|_| ("probe emitted non-UTF-8".to_string(), None))?;
    goofi_node::parse_introspection(&json).map_err(|e| (format!("malformed introspection: {e}"), None))
}

/// What decides a probe's answer, hashed: goofi's version, the file's bytes, and each interpreter
/// with what it has installed — the names of the distributions in its site-packages, so an
/// installed dependency re-probes the file that lacked it, while an environment provisioned
/// again from the same requirements answers from the memo. `None` only for a file that cannot
/// be read.
pub fn probe_key(path: &Path, pythons: &[&str]) -> Option<String> {
    let mut hash = Sha256::new();
    hash.update(env!("CARGO_PKG_VERSION").as_bytes());
    hash.update(std::fs::read(path).ok()?);
    for python in pythons {
        hash.update(python.as_bytes());
        hash.update(installed(Path::new(python)).as_bytes());
    }
    Some(format!("{:x}", hash.finalize())[..32].to_string())
}

/// What `python`'s environment holds, as one string: its `pyvenv.cfg` (the interpreter's version
/// and base) and the sorted `.dist-info` names of its site-packages. Read once per interpreter
/// per process: a scan asks it for every file.
fn installed(python: &Path) -> String {
    static READ: LazyLock<Mutex<HashMap<PathBuf, String>>> = LazyLock::new(Default::default);
    if let Some(known) = READ.lock().get(python) {
        return known.clone();
    }
    let venv = python.parent().and_then(Path::parent);
    let config = venv.and_then(|v| std::fs::read_to_string(v.join("pyvenv.cfg")).ok()).unwrap_or_default();
    let mut dists: Vec<String> = venv
        .and_then(goofi_supervisor::layout::site_packages)
        .and_then(|site| std::fs::read_dir(site).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".dist-info"))
        .collect();
    dists.sort();
    let answer = format!("{config}\n{}", dists.join("\n"));
    READ.lock().insert(python.to_path_buf(), answer.clone());
    answer
}

/// The probe under `memo`, where its answer outlives the process: an import-heavy node costs
/// seconds to probe, and no boot should pay that twice for one file under one interpreter.
/// A FAILURE is never kept: it is as often the environment as the file — a half-installed wheel, a
/// basename shared with another bundle — and a memo of one outlives its cause silently. Failing is
/// also the cheap case, since an import that is going to raise raises on the first module.
fn introspect_memoised(path: &Path, python: &str, memo: &Path) -> Result<Introspection, crate::library::Failed> {
    let Some(key) = probe_key(path, &[python]) else { return introspect(path, python) };
    let entry = memo.join(format!("{key}.json"));
    // A memo from before a declaration named its engine is not an answer.
    let kept = std::fs::read_to_string(&entry).ok().and_then(|s| serde_json::from_str::<Introspection>(&s).ok());
    if let Some(hit) = kept.filter(|intro| !intro.engine.is_empty()) {
        return Ok(hit);
    }
    let answer = introspect(path, python);
    if let Some(json) = answer.as_ref().ok().and_then(|intro| serde_json::to_string(intro).ok()) {
        let tmp = memo.join(format!("{key}.{}.tmp", goofi_supervisor::session::tag()));
        let _ = std::fs::create_dir_all(memo)
            .and_then(|()| std::fs::write(&tmp, json))
            .and_then(|()| std::fs::rename(&tmp, &entry));
    }
    answer
}
