//! goofi's two Python interpreters: the pinned venvs uv makes in the runtime, the goofi wheel in
//! each, and the bundles' packages. goofi-init provisions with these; the binary checks with them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use goofi_supervisor::layout::{self, Runtime, Tool};
use goofi_supervisor::progress;
use sha2::{Digest, Sha256};

/// The free-threaded interpreter, in uv's spelling: the embedded one, and the in-process tier.
pub const PYTHON_FT: &str = "3.14.6t";

/// The GIL interpreter the subprocess tier runs on, for packages with no free-threaded wheel.
pub const PYTHON_GIL: &str = "3.12.13";

/// `uv` with the runtime's own cache and Python installs, before the caller's arguments. The
/// listing a dry run parses must be plain, so colour is off; the caller's config is not read.
pub fn uv<'a>(runtime: &Runtime, args: impl IntoIterator<Item = &'a str>) -> Command {
    let mut cmd = layout::command(Tool::Uv);
    cmd.args(["--color", "never", "--no-config"]).args(args);
    cmd.env("UV_CACHE_DIR", runtime.cache().join("uv"));
    cmd.env("UV_PYTHON_INSTALL_DIR", runtime.cache().join("python"));
    // uv drives a DIFFERENT interpreter: the caller's stdlib kills it with "SRE module mismatch".
    cmd.env_remove("PYTHONHOME").env_remove("PYTHONPATH");
    cmd
}

/// Run `cmd` to completion with its output held, or say what could not be done and the last
/// lines the tool printed about it.
pub fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    let out = cmd.stdin(Stdio::null()).output().map_err(|e| format!("could not {what}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let said = [&out.stderr, &out.stdout].map(|b| String::from_utf8_lossy(b).into_owned());
    let text = said.iter().find(|t| !t.trim().is_empty()).map(String::as_str).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(20)..].join("\n");
    Err(format!("could not {what} ({})\n{tail}", out.status).trim_end().to_string())
}

/// Whether `cmd` runs and succeeds, with its output discarded.
pub fn answers(cmd: &mut Command) -> bool {
    cmd.stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// What `py` prints for `code`, with the host's Python variables stripped as a child sees them.
pub fn query(py: &Path, code: &str) -> Option<String> {
    let out = Command::new(py).args(["-c", code]).env_remove("PYTHONPATH").env_remove("PYTHONHOME").output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The venv at `venv` on exactly the pinned interpreter, made or remade: a pin that moved
/// re-provisions, and a venv on another python is not this one.
pub fn ensure_venv(runtime: &Runtime, venv: &Path, pin: &str) -> Result<PathBuf, String> {
    if let Some(py) = layout::venv_python(venv) {
        let spelled = "import sys,sysconfig;print(sys.version.split()[0]+('t' if sysconfig.get_config_var('Py_GIL_DISABLED') else ''))";
        if query(&py, spelled).as_deref() == Some(pin) {
            return Ok(py);
        }
        let _ = std::fs::remove_dir_all(venv);
    }
    progress::report(format!("Creating a Python {pin} environment"));
    run(uv(runtime, ["venv", "--managed-python", "--python", pin]).arg(venv), &format!("create {}", venv.display()))?;
    layout::venv_python(venv).ok_or_else(|| format!("`uv venv` left no interpreter in {}", venv.display()))
}

/// Does this interpreter hold THIS goofi? `introspect` separates the Rust wheel from the old Python
/// package, and the version makes a bump re-provision.
pub fn has_goofi(py: &Path) -> bool {
    let probe = format!(
        "import goofi, importlib.metadata as m; goofi.introspect; \
         raise SystemExit(0 if m.version('goofi') == '{}' else 1)",
        env!("CARGO_PKG_VERSION")
    );
    answers(Command::new(py).args(["-c", &probe]).env_remove("PYTHONPATH").env_remove("PYTHONHOME"))
}

/// Install the goofi wheel in `wheels` into `py`, unless this goofi is there already.
pub fn install_wheel(runtime: &Runtime, py: &Path, wheels: &Path) -> Result<(), String> {
    if has_goofi(py) {
        return Ok(());
    }
    let wheel = std::fs::read_dir(wheels)
        .map_err(|e| format!("{}: {e}", wheels.display()))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "whl"))
        .ok_or_else(|| format!("no goofi wheel in {}", wheels.display()))?;
    progress::report("Installing the goofi wheel");
    run(uv(runtime, ["pip", "install", "--python"]).arg(py).arg("--reinstall").arg(&wheel), "install the goofi wheel")
}

/// The `requirements.txt` files of `dirs`, asked of both interpreters, and those plus every
/// `requirements-gil.txt` (no free-threaded wheel), asked of the subprocess interpreter alone.
pub fn requirement_sets(dirs: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let shared = named_in(dirs, "requirements.txt");
    let gil = shared.iter().cloned().chain(named_in(dirs, "requirements-gil.txt")).collect();
    (shared, gil)
}

fn named_in(dirs: &[PathBuf], file: &str) -> Vec<PathBuf> {
    dirs.iter().map(|d| d.join(file)).filter(|p| p.is_file()).collect()
}

fn pip_install(runtime: &Runtime, py: &Path, reqs: &[PathBuf], dry_run: bool) -> Result<Command, String> {
    let mut cmd = uv(runtime, ["pip", "install", "--python"]);
    cmd.arg(py);
    if dry_run {
        cmd.arg("--dry-run");
    }
    for r in reqs {
        std::fs::read_to_string(r)
            .map_err(|e| format!("cannot read requirements file {} as UTF-8: {e}", r.display()))?;
        cmd.arg("-r").arg(r);
    }
    Ok(cmd)
}

fn names(reqs: &[PathBuf]) -> String {
    reqs.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", ")
}

/// What `py` lacks to satisfy `reqs`, as uv would install it. uv audits site-packages before it
/// resolves anything, so a satisfied set answers in milliseconds and without the network.
pub fn missing_packages(runtime: &Runtime, py: &Path, reqs: &[PathBuf]) -> Result<Vec<String>, String> {
    if reqs.is_empty() {
        return Ok(Vec::new());
    }
    let out = pip_install(runtime, py, reqs, true)?.output().map_err(|e| format!("could not run uv: {e}"))?;
    let text = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(format!("uv could not resolve {}: {}", names(reqs), text.trim()));
    }
    // ponytail: reads uv's dry-run listing; a `--format json` on `uv pip install` replaces this.
    Ok(text.lines().filter_map(|l| l.strip_prefix(" + ")).map(str::to_string).collect())
}

/// Install `reqs` into `py`.
pub fn install_packages(runtime: &Runtime, py: &Path, reqs: &[PathBuf]) -> Result<(), String> {
    run(&mut pip_install(runtime, py, reqs, false)?, &format!("install {}", names(reqs)))
}

/// One digest over `salt` and every file under `roots`, each by its path relative to its root and
/// its bytes, in path order: an artifact built from them is found again by content, not by date.
pub fn tree_digest(salt: &str, roots: &[PathBuf]) -> String {
    fn files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut hash = Sha256::new();
    hash.update(salt.as_bytes());
    for root in roots {
        let mut found = Vec::new();
        if root.is_dir() {
            files(root, &mut found);
        } else {
            found.push(root.clone());
        }
        found.sort();
        for path in found {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
            hash.update(rel.as_bytes());
            hash.update(std::fs::read(&path).unwrap_or_default());
        }
    }
    format!("{:x}", hash.finalize())
}
