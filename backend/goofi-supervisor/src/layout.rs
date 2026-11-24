//! Where goofi's files live: the HOME the user owns, the RUNTIME goofi curates for itself, and
//! the session (`crate::session`). Each override is read PER CALL, so a child is scoped by its
//! environment alone.

use std::path::{Path, PathBuf};

/// The goofi version a runtime tree is bound to.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Env var naming the directory the `.goofi` home sits in; unset, it is the user's home.
pub const HOME_ENV: &str = "GOOFI_HOME";

/// Env var naming the runtime root itself; unset, it is the platform's data directory.
pub const RUNTIME_ENV: &str = "GOOFI_RUNTIME";

fn user_home() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The `.goofi` folder: what a user edits and keeps — custom nodes, recordings, plugins, config.
pub fn home() -> PathBuf {
    std::env::var_os(HOME_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(user_home)
        .join(".goofi")
}

/// The folders of the last patches loaded or saved, newest first: one path per line. The user's,
/// as the patches are, so two goofis with homes of their own keep lists of their own.
pub fn recent_folders() -> PathBuf {
    home().join("recent-folders")
}

/// The private node library: one flat node root the user owns, scanned after every other root
/// and before the patch's own, so a node saved here beats a shipped one and loses to the patch.
pub fn custom_nodes() -> PathBuf {
    home().join("custom")
}

/// Where a recording lands, and where a bare playback name is looked for.
pub fn recordings() -> PathBuf {
    home().join("recordings")
}

/// Where a dead session's autosaved workspace is kept for the user to recover or discard.
pub fn recovery() -> PathBuf {
    home().join("recovery")
}

pub fn config_file() -> PathBuf {
    home().join("config.toml")
}

/// One launchable agent: a display name, the bash command line that starts it, and the flag
/// each launch option becomes — a template whose `{}` takes the value — when the harness has one.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Agent {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Config {
    #[serde(default)]
    agents: Vec<Agent>,
}

/// The compiled-in TEMPLATE — written once by the serve path when no config exists, so the FILE
/// stays the one owner and a test process never writes into a real home.
pub const DEFAULT_CONFIG: &str = "# goofi — the agents the app can launch, each a bash command line. `model` and `effort`\n\
    # are the flags a launch option becomes, `{}` standing for the value; leave one out and\n\
    # the agent takes no such option.\n\
    [[agents]]\nname = \"claude\"\ncommand = \"claude\"\nmodel = \"--model {}\"\neffort = \"--effort {}\"\n\n\
    [[agents]]\nname = \"codex\"\ncommand = \"codex\"\nmodel = \"-m {}\"\neffort = \"-c model_reasoning_effort={}\"\n\n\
    [[agents]]\nname = \"opencode\"\ncommand = \"opencode\"\nmodel = \"-m {}\"\n";

/// Seed the default config, absent-only — the serve path calls this once at start. The
/// session-file idiom: whole or absent, so two first boots cannot tear it for a third reader.
pub fn seed_config() {
    let at = config_file();
    if !at.exists() {
        let _ = std::fs::create_dir_all(home());
        let tmp = home().join("config.toml.part");
        let _ = std::fs::write(&tmp, DEFAULT_CONFIG).and_then(|()| std::fs::rename(&tmp, at));
    }
}

/// The launchable agents: the config file's list. Absent reads as the default; a config that
/// does not parse degrades to the default and answers WHY — it never stops the app.
pub fn agents() -> (Vec<Agent>, Option<String>) {
    let default = || toml::from_str::<Config>(DEFAULT_CONFIG).expect("the template parses").agents;
    match std::fs::read_to_string(config_file()) {
        Err(_) => (default(), None),
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(c) => (c.agents, None),
            Err(e) => {
                (default(), Some(format!("{} does not parse: {e}", config_file().display())))
            }
        },
    }
}

/// The platform's per-user data directory, where the runtime goes by default.
fn data_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join("AppData").join("Local"))
    } else if cfg!(target_os = "macos") {
        user_home().join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join(".local").join("share"))
    }
}

/// The runtime this process uses: `$GOOFI_RUNTIME`, else the platform's data directory.
pub fn runtime() -> Runtime {
    let root = std::env::var_os(RUNTIME_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir().join("goofi"));
    Runtime::at(root)
}

/// A program goofi runs, found on PATH: a development build asks the machine for its tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Cargo,
    Rustc,
    Uv,
    Npm,
    Ffmpeg,
}

/// The program to run for `tool`. Windows needs npm's `.cmd` shim by name.
pub fn tool(tool: Tool) -> PathBuf {
    match tool {
        // `CARGO` is what `cargo run` hands a goofi it started: the toolchain's own cargo, which a
        // bare name on PATH may not resolve to.
        Tool::Cargo => std::env::var_os("CARGO").map(PathBuf::from).unwrap_or_else(|| "cargo".into()),
        Tool::Rustc => "rustc".into(),
        Tool::Uv => "uv".into(),
        Tool::Npm if cfg!(windows) => "npm.cmd".into(),
        Tool::Npm => "npm".into(),
        Tool::Ffmpeg => "ffmpeg".into(),
    }
}

/// A command running `tool`, with no arguments yet.
pub fn command(tool: Tool) -> std::process::Command {
    std::process::Command::new(self::tool(tool))
}

/// Everything goofi keeps for itself, bound to one version: interpreters, build caches, the
/// shipped tree. Only `cache/` outlives a version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runtime {
    root: PathBuf,
}

impl Runtime {
    /// A runtime rooted at `root`, absolutised: a generated crate names the SDK by path, and
    /// cargo resolves that against the crate's own directory.
    pub fn at(root: impl Into<PathBuf>) -> Runtime {
        let root = root.into();
        Runtime { root: std::path::absolute(&root).unwrap_or(root) }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// This version's tree, removed whole once another version runs.
    pub fn versioned(&self) -> PathBuf {
        self.root.join(VERSION)
    }

    /// The free-threaded venv the embedded interpreter is linked against, and the probe runs on.
    pub fn python_ft(&self) -> PathBuf {
        self.versioned().join("python").join("ft")
    }

    /// The GIL venv the subprocess tier runs on: for packages that are not free-threading-safe.
    pub fn python_gil(&self) -> PathBuf {
        self.versioned().join("python").join("gil")
    }

    /// The extracted SDK, the generated crates, one shared cargo target, every artifact, the
    /// probe memos, the VST3 scans and the plugin caches.
    pub fn build(&self) -> PathBuf {
        self.versioned().join("build")
    }

    /// The shipped bundles, written out by content key so a shipped node loads with no toolchain.
    pub fn shipped(&self) -> PathBuf {
        self.versioned().join("shipped")
    }

    /// What every version shares: the uv cache and the Python installs.
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Whether an entry of the root is another version's tree: not this version's, and not the
    /// cache every version shares.
    pub fn is_other_version(&self, name: &std::ffi::OsStr) -> bool {
        name != VERSION && name != "cache"
    }
}

/// Where a venv keeps its interpreter, asked by presence rather than by platform. UNRESOLVED:
/// on unix that symlink points at the base install, which has no `goofi` wheel.
pub fn venv_python(venv: &Path) -> Option<PathBuf> {
    ["bin/python", "Scripts/python.exe"].into_iter().map(|rel| venv.join(rel)).find(|p| p.is_file())
}

/// A venv's `site-packages`, which the embedded interpreter must be handed. The Python version is
/// FOUND, never named, so it cannot go stale when the pinned one moves.
pub fn site_packages(venv: &Path) -> Option<PathBuf> {
    let flat = venv.join("Lib").join("site-packages");
    if flat.is_dir() {
        return Some(flat);
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(venv.join("lib"))
        .ok()?
        .flatten()
        .map(|e| e.path().join("site-packages"))
        .filter(|p| p.is_dir())
        .collect();
    // Sorted so a venv that somehow holds two answers gives a stable one.
    found.sort();
    found.pop()
}

/// The base install a venv runs from — its `PYTHONHOME` — read from `pyvenv.cfg`, whose `home`
/// is the base's `bin` on unix and the base itself on Windows.
pub fn venv_prefix(venv: &Path) -> Option<PathBuf> {
    let cfg = std::fs::read_to_string(venv.join("pyvenv.cfg")).ok()?;
    let home = cfg.lines().find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == "home").map(|(_, v)| v.trim()))?;
    let home = PathBuf::from(home);
    if home.file_name().is_some_and(|n| n == "bin") {
        home.parent().map(Path::to_path_buf)
    } else {
        Some(home)
    }
}
