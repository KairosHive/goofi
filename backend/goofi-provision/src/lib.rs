//! Filling goofi's runtime: the pinned tools, the two interpreters, the goofi wheels and the
//! bundles' packages — the ONE place that does it, for `goofi-init` and a distribution build alike.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use goofi_supervisor::layout::{self, Runtime, Tool};
use goofi_supervisor::progress;
use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/dist.rs"));

/// The target this binary was built for: which archives of the manifest are its.
pub const TARGET: &str = env!("GOOFI_TARGET");

/// What a runtime is made of, pinned: `manifest.toml` beside this crate.
#[derive(Debug, serde::Deserialize)]
pub struct Manifest {
    pub python: Python,
    archive: Vec<Archive>,
}

/// The two interpreters, in uv's spelling.
#[derive(Debug, serde::Deserialize)]
pub struct Python {
    pub ft: String,
    pub gil: String,
}

/// One download: unpacked into `tools/<tool>/` with its first `strip` path segments dropped.
#[derive(Debug, serde::Deserialize)]
struct Archive {
    tool: String,
    target: String,
    url: String,
    sha256: String,
    strip: usize,
}

pub fn manifest() -> &'static Manifest {
    static MANIFEST: LazyLock<Manifest> =
        LazyLock::new(|| toml::from_str(include_str!("../manifest.toml")).expect("the manifest parses"));
    &MANIFEST
}

/// `uv` with the runtime's own cache and Python installs, before the caller's arguments. The
/// listing a dry run parses must be plain, so colour is off; the caller's config is not read.
pub fn uv<'a>(runtime: &Runtime, args: impl IntoIterator<Item = &'a str>) -> Command {
    let mut cmd = runtime.command(Tool::Uv);
    cmd.args(["--color", "never", "--no-config"]).args(args);
    cmd.env("UV_CACHE_DIR", runtime.cache().join("uv"));
    cmd.env("UV_PYTHON_INSTALL_DIR", runtime.cache().join("python"));
    // uv drives a DIFFERENT interpreter: the caller's stdlib kills it with "SRE module mismatch".
    cmd.env_remove("PYTHONHOME").env_remove("PYTHONPATH");
    cmd
}

/// Run `cmd` to completion, or say what could not be done.
pub fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("could not {what} ({s})")),
        Err(e) => Err(format!("could not {what}: {e}")),
    }
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

/// Install the goofi wheel under `wheels` that fits `py`, unless a matching one is already there:
/// a free-threaded interpreter takes the `cp3XXt` wheel, any other the `abi3` one.
pub fn ensure_wheels(runtime: &Runtime, py: &Path, wheels: &Path) -> Result<(), String> {
    if has_goofi(py) {
        return Ok(());
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(wheels)
        .map_err(|e| format!("{}: {e}", wheels.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "whl"))
        .collect();
    found.sort();
    let free_threaded = query(py, "import sysconfig;print(sysconfig.get_config_var('Py_GIL_DISABLED') or 0)").as_deref() == Some("1");
    let fits = |p: &Path| p.to_string_lossy().contains("t-") == free_threaded;
    let wheel = found.iter().find(|p| fits(p)).or(found.first()).ok_or_else(|| format!("no goofi wheel in {}", wheels.display()))?;
    progress::report("Installing the goofi wheel");
    run(uv(runtime, ["pip", "install", "--python"]).arg(py).arg("--reinstall").arg(wheel), "install the goofi wheel")
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

/// The bundled `tool`, fetched and unpacked unless it is there or the manifest carries none for
/// this target — then PATH's stands in. Answers whether anything was fetched.
pub fn ensure_tool(runtime: &Runtime, tool: Tool) -> Result<bool, String> {
    let dir = runtime.tools().join(tool.name());
    let archives: Vec<&Archive> =
        manifest().archive.iter().filter(|a| a.tool == tool.name() && a.target == TARGET).collect();
    if runtime.tool(tool).starts_with(&dir) || archives.is_empty() {
        return Ok(false);
    }
    // Into a sibling first, renamed whole: a crash mid-unpack leaves no half tool to be found.
    let work = dir.with_extension(goofi_supervisor::session::tag());
    let _ = std::fs::remove_dir_all(&work);
    for archive in archives {
        let file = fetch(runtime, archive)?;
        progress::report(format!("Unpacking {}", archive.file_name()));
        unpack(&file, archive.strip, &work).map_err(|e| format!("unpack {}: {e}", archive.file_name()))?;
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&work, &dir).map_err(|e| format!("place {}: {e}", dir.display()))?;
    Ok(true)
}

impl Archive {
    fn file_name(&self) -> &str {
        self.url.rsplit('/').next().unwrap_or(&self.url)
    }
}

/// The archive on disk under `cache/downloads`, verified against its digest: downloaded unless a
/// copy that verifies is already there.
fn fetch(runtime: &Runtime, archive: &Archive) -> Result<PathBuf, String> {
    let downloads = runtime.cache().join("downloads");
    let file = downloads.join(archive.file_name());
    if digest_of(&file).is_ok_and(|d| d == archive.sha256) {
        return Ok(file);
    }
    std::fs::create_dir_all(&downloads).map_err(|e| format!("{}: {e}", downloads.display()))?;
    progress::report(format!("Downloading {}", archive.file_name()));
    let response = ureq::get(&archive.url).call().map_err(|e| format!("download {}: {e}", archive.url))?;
    let (_, body) = response.into_parts();
    let mut reader = body.into_reader();
    let part = file.with_extension(format!("part-{}", goofi_supervisor::session::tag()));
    let mut out = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download {}: {e}", archive.url))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", part.display()))?;
    }
    drop(out);
    let got = format!("{:x}", hash.finalize());
    if got != archive.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{} does not match its digest: got {got}, expected {}", archive.url, archive.sha256));
    }
    std::fs::rename(&part, &file).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file)
}

fn digest_of(file: &Path) -> std::io::Result<String> {
    let mut hash = Sha256::new();
    std::io::copy(&mut std::fs::File::open(file)?, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

/// Unpack `file` under `dest`, each entry's first `strip` segments dropped; an entry with no more
/// than that many is skipped.
fn unpack(file: &Path, strip: usize, dest: &Path) -> std::io::Result<()> {
    let name = file.to_string_lossy();
    let reader = std::fs::File::open(file)?;
    if name.ends_with(".zip") {
        return unzip(reader, strip, dest);
    }
    let decoded: Box<dyn Read> = if name.ends_with(".tar.xz") {
        Box::new(liblzma::read::XzDecoder::new_multi_decoder(reader))
    } else {
        Box::new(flate2::read::GzDecoder::new(reader))
    };
    let mut archive = tar::Archive::new(decoded);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let Some(rest) = stripped(&entry.path()?, strip) else { continue };
        let at = dest.join(rest);
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(&at)?;
    }
    Ok(())
}

fn unzip(reader: std::fs::File, strip: usize, dest: &Path) -> std::io::Result<()> {
    let mut archive = zip::ZipArchive::new(reader)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(path) = entry.enclosed_name() else { continue };
        let Some(rest) = stripped(&path, strip) else { continue };
        let at = dest.join(rest);
        if entry.is_dir() {
            std::fs::create_dir_all(&at)?;
            continue;
        }
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&at)?;
        std::io::copy(&mut entry, &mut out)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&at, std::fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

/// `path` without its first `strip` components, or `None` when nothing is left.
fn stripped(path: &Path, strip: usize) -> Option<PathBuf> {
    let rest: PathBuf = path.components().skip(strip).collect();
    (!rest.as_os_str().is_empty()).then_some(rest)
}

/// A distribution build's start: every bundled tool, both interpreters and the embedded wheels,
/// each step skipped when it is already there. The bundles' packages follow at the scan.
pub fn ensure_runtime(runtime: &Runtime) -> Result<(), String> {
    for tool in [Tool::Uv, Tool::Cargo, Tool::Zig, Tool::Npm, Tool::Ffmpeg] {
        ensure_tool(runtime, tool)?;
    }
    let pins = &manifest().python;
    let ft = ensure_venv(runtime, &runtime.python_ft(), &pins.ft)?;
    let gil = ensure_venv(runtime, &runtime.python_gil(), &pins.gil)?;
    let wheels = runtime.versioned().join("wheels");
    std::fs::create_dir_all(&wheels).map_err(|e| format!("{}: {e}", wheels.display()))?;
    for (name, bytes) in WHEELS {
        let at = wheels.join(name);
        if !at.is_file() {
            std::fs::write(&at, bytes).map_err(|e| format!("{}: {e}", at.display()))?;
        }
    }
    for py in [&ft, &gil] {
        ensure_wheels(runtime, py, &wheels)?;
    }
    Ok(())
}
