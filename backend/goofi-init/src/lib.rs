//! `cargo run -p goofi-init`: what a development checkout needs beyond the runtime the provisioner
//! fills — the system libraries, the goofi wheel built from source, the cargo config pointing
//! pyo3 at the interpreter, the frontend's dependencies. It must not depend on pyo3.

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::process::Command;

use goofi_provision::{answers, query, run, uv};
use goofi_supervisor::layout::{self, Tool};

/// The repo root. Nothing is resolved, so a checkout reached through a symlink stays spelled the
/// way the caller reached it.
pub fn repo_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(2).unwrap_or(manifest).to_path_buf()
}

/// The generated cargo config. Machine-specific and gitignored.
fn config_path(root: &Path) -> PathBuf {
    root.join(".cargo").join("config.toml")
}

/// The interpreter this build links against, as cargo resolved it — `None` until [`init`] has run.
/// Read from the ENVIRONMENT, which cargo has already expanded, not from the config file's text.
pub fn interpreter() -> Option<PathBuf> {
    std::env::var_os("PYO3_PYTHON").map(PathBuf::from).filter(|p| p.is_file())
}

/// The instruction printed wherever readiness is demanded, so the wording exists once.
pub const RUN_ME: &str = "run `cargo run -p goofi-init` first — it provisions the Python \
                          interpreters in goofi's runtime and the frontend's dependencies \
                          (needs `uv` and `npm` on PATH)";

/// Provision everything, from nothing, idempotently.
pub fn init(root: &Path) -> Result<(), String> {
    let runtime = layout::runtime();
    require_uv(&runtime)?;

    // Both tools asked for BEFORE either is used, so a long provision cannot stop for want of npm.
    let frontend = root.join("frontend");
    let needs_npm = frontend.join("package.json").is_file();
    if needs_npm {
        require_npm(&runtime)?;
    }
    require_audio_libs()?;

    let pins = &goofi_provision::manifest().python;
    let ft = goofi_provision::ensure_venv(&runtime, &runtime.python_ft(), &pins.ft)?;
    let gil = goofi_provision::ensure_venv(&runtime, &runtime.python_gil(), &pins.gil)?;

    // The config BEFORE the wheels, so a failed wheel build still leaves a config a re-run can use.
    write_config(root, &ft)?;

    // The bundles' packages every time, never gated on presence: a bundle added since the last
    // run names new ones, and uv answers a satisfied list in milliseconds.
    let dirs = bundle_dirs(root);
    let (shared, gil_only) = goofi_provision::requirement_sets(&dirs);
    for (label, py, reqs) in [("ft", &ft, &shared), ("gil", &gil, &gil_only)] {
        if !goofi_provision::has_goofi(py) {
            let wheels = build_wheel(root, &runtime, label, py)?;
            goofi_provision::ensure_wheels(&runtime, py, &wheels)?;
        }
        if !reqs.is_empty() {
            println!("  installing the bundles' packages into {label}");
            goofi_provision::install_packages(&runtime, py, reqs)?;
        }
    }

    // Run every time, never gated on `node_modules`: no lockfile ships, so `npm install` IS the
    // resolve step and a presence check would sail past a new dependency.
    if needs_npm {
        println!("  installing the frontend's dependencies");
        run(runtime.command(Tool::Npm).arg("install").current_dir(&frontend), "install the frontend's dependencies")?;
    }
    let (entries, bytes) = sweep_target(&root.join("target"), STALE_AFTER);
    if entries > 0 {
        println!("  removed {entries} build artifacts untouched for {} days ({} MB)", STALE_AFTER.as_secs() / 86_400, bytes >> 20);
    }
    Ok(())
}

/// How long a build artifact may go unused before the next provision removes it.
pub const STALE_AFTER: Duration = Duration::from_secs(3 * 86_400);

/// Remove every artifact under `target/<profile>/{incremental,deps,build,.fingerprint}` that no
/// build has touched for `stale`. Cargo never removes a superseded one, so a hash that moved on —
/// a feature set, a profile, a dependency — leaves its whole output behind; cargo rebuilds what
/// this takes and it still needs. Answers how many entries went and how many bytes with them.
pub fn sweep_target(target: &Path, stale: Duration) -> (usize, u64) {
    let Ok(profiles) = std::fs::read_dir(target) else { return (0, 0) };
    let now = std::time::SystemTime::now();
    let (mut entries, mut bytes) = (0, 0);
    for profile in profiles.flatten().filter(|p| p.path().is_dir()) {
        for kind in ["incremental", "deps", "build", ".fingerprint"] {
            let Ok(artifacts) = std::fs::read_dir(profile.path().join(kind)) else { continue };
            for artifact in artifacts.flatten() {
                let path = artifact.path();
                let touched = artifact.metadata().and_then(|m| m.modified()).ok();
                let old = touched.and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age > stale);
                if !old {
                    continue;
                }
                let size = size_of(&path);
                let removed = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
                if removed.is_ok() {
                    entries += 1;
                    bytes += size;
                }
            }
        }
    }
    (entries, bytes)
}

fn size_of(path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => std::fs::read_dir(path).map(|d| d.flatten().map(|e| size_of(&e.path())).sum()).unwrap_or(0),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

fn require_npm(runtime: &layout::Runtime) -> Result<(), String> {
    answers(runtime.command(Tool::Npm).arg("--version")).then_some(()).ok_or_else(|| {
        "goofi needs a working `npm` on PATH — the app is compiled into the binary, so building it \
         is part of building goofi. Install Node.js from https://nodejs.org and re-run."
            .to_string()
    })
}

/// The system libraries cpal's Linux hosts link, each with the Debian package that carries it.
/// Linux only: `jack` dlopens on Windows and macOS, and cpal target-gates the rest.
#[cfg(target_os = "linux")]
const AUDIO_LIBS: &[(&str, &str)] = &[("alsa", "libasound2-dev"), ("jack", "libjack-jackd2-dev")];

#[cfg(target_os = "linux")]
fn pkg_config(args: &[&str]) -> bool {
    answers(Command::new("pkg-config").args(args))
}

/// goofi offers every audio host the machine runs, and on Linux each links a system library. A
/// missing one surfaces as a `pkg-config` failure deep in someone else's build script — a setup
/// step in disguise — so they are named here together, with the command that installs them.
#[cfg(target_os = "linux")]
fn require_audio_libs() -> Result<(), String> {
    if !pkg_config(&["--version"]) {
        return Err("goofi needs `pkg-config` on PATH to find the audio libraries its hosts link. \
                    Install it (`sudo apt install pkg-config`) and re-run."
            .to_string());
    }
    let missing: Vec<&str> =
        AUDIO_LIBS.iter().filter(|(lib, _)| !pkg_config(&["--exists", lib])).map(|(_, pkg)| *pkg).collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "goofi plays through every audio host this machine runs, and each links a system library. \
         Install the missing ones and re-run:\n  sudo apt install {}",
        missing.join(" ")
    ))
}

/// The same statement for Windows, where the library the audio hosts need is a BUILD one: ASIO is
/// the only way past an interface's first stereo pair, and its binding is generated by bindgen,
/// which loads libclang. Nothing else is asked of the user — asio-sys downloads its own SDK — so
/// this one library is the whole of the opt-out, and `--no-default-features` is it.
#[cfg(windows)]
fn require_audio_libs() -> Result<(), String> {
    let named = std::env::var_os("LIBCLANG_PATH").is_some();
    let installed = ["C:/Program Files/LLVM/bin/libclang.dll", "C:/Program Files (x86)/LLVM/bin/libclang.dll"]
        .iter()
        .any(|p| Path::new(p).is_file());
    let on_path = answers(Command::new("clang").arg("--version"));
    if named || installed || on_path {
        return Ok(());
    }
    Err("goofi reaches a multi-channel interface through ASIO, whose binding is generated at build \
         time by bindgen — so it needs libclang. Install LLVM (`winget install LLVM.LLVM`) and \
         re-run, or build without it: `cargo run --no-default-features --features python`."
        .to_string())
}

#[cfg(not(any(target_os = "linux", windows)))]
fn require_audio_libs() -> Result<(), String> {
    Ok(())
}

fn require_uv(runtime: &layout::Runtime) -> Result<(), String> {
    answers(&mut uv(runtime, ["--version"])).then_some(()).ok_or_else(|| {
        "goofi needs a working `uv` on PATH — it owns the Python interpreters the node tiers run \
         on. Install it from https://docs.astral.sh/uv/ and re-run."
            .to_string()
    })
}

/// Build the wheel for THIS interpreter into a directory of its own, emptied first, so the wheel
/// just built is the only file in it. Answers that directory.
fn build_wheel(root: &Path, runtime: &layout::Runtime, label: &str, py: &Path) -> Result<PathBuf, String> {
    println!("  building the goofi wheel for {label}");
    let out = root.join("target").join("wheels").join(label);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|e| format!("wheel output directory: {e}"))?;
    run(
        uv(runtime, ["tool", "run", "maturin", "build", "--release", "-i"])
            .arg(py)
            .arg("-o")
            .arg(&out)
            .arg("-m")
            .arg(root.join("backend").join("signal").join("goofi-pymod").join("Cargo.toml"))
            // Run from OUTSIDE the repo, or maturin's nested cargo picks `.cargo/config.toml` up
            // and builds against the free-threaded interpreter's home. Hence the absolute paths.
            .current_dir(std::env::temp_dir()),
        "build the goofi wheel",
    )?;
    Ok(out)
}

/// The bundles this repo ships: every directory under `node-bundles/`, sorted.
pub fn bundle_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join("node-bundles"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Point pyo3 at the free-threaded venv, for every cargo command from here on. The link step is
/// all this serves: goofi finds its interpreter and its home from the layout at run time.
fn write_config(root: &Path, ft: &Path) -> Result<(), String> {
    // `-Wl,-rpath` is a GNU/Clang flag `link.exe` rejects, so this is keyed on the TARGET's linker
    // rather than on the reported libdir: a Windows CPython reports one and cannot use it.
    let host = host_triple()?;
    let rpath = query(ft, "import sysconfig;print(sysconfig.get_config_var('LIBDIR'))")
        .filter(|d| d != "None" && !host.ends_with("windows-msvc"))
        .map(|libdir| {
            // Debug-quoted: a raw Windows libdir makes `\U` an invalid TOML escape.
            let flag = format!("link-arg=-Wl,-rpath,{libdir}");
            format!("\n[target.{host}]\nrustflags = [\"-C\", {flag:?}]\n")
        })
        .unwrap_or_default();
    let contents = format!(
        "# Generated by `cargo run -p goofi-init` — machine-specific, gitignored, never committed.\n\
         # Points pyo3 at the free-threaded interpreter in goofi's runtime so cargo needs no env\n\
         # vars. Delete this file and re-run goofi-init to reprovision.\n\
         [env]\n\
         PYO3_PYTHON = {ft:?}\n\
         {rpath}",
    );
    let config = config_path(root);
    if let Some(parent) = config.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(&config, contents).map_err(|e| format!("{}: {e}", config.display()))
}

/// The triple cargo will build for, asked of `rustc` rather than assumed.
fn host_triple() -> Result<String, String> {
    let out = layout::runtime().command(Tool::Rustc).arg("-vV").output().map_err(|e| format!("run rustc: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
        .ok_or_else(|| "`rustc -vV` reported no host triple".to_string())
}
