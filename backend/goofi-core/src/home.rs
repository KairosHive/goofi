//! The `$GOOFI_HOME/.goofi/` folder: path resolution and creation. Sessions are `crate::session`.
//! `GOOFI_HOME` is read PER CALL, so a spawned process is scoped by its environment alone.

use std::path::PathBuf;

/// The `.goofi` folder itself.
pub fn dir() -> PathBuf {
    std::env::var_os("GOOFI_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(std::env::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".goofi")
}

/// Everything goofi keeps for itself — build caches, the recovery, materialised bundles —
/// under one folder, so the `.goofi` root shows only what a user edits.
pub fn system() -> PathBuf {
    dir().join("system")
}

/// The private node library: one flat node root the user owns, scanned after every other root
/// and before the patch's own, so a node saved here beats a shipped one and loses to the patch.
pub fn custom_nodes() -> PathBuf {
    dir().join("custom")
}

/// Where a recording lands, and where a bare playback name is looked for.
pub fn recordings() -> PathBuf {
    dir().join("recordings")
}

/// One launchable agent: a display name and the bash command line that starts it.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Agent {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Config {
    #[serde(default)]
    agents: Vec<Agent>,
}

/// The compiled-in TEMPLATE — written once by the serve path when no config exists, so the FILE
/// stays the one owner and a test process never writes into a real home.
pub const DEFAULT_CONFIG: &str = "# goofi — the agents the app can launch, each a bash command line.\n\
    [[agents]]\nname = \"claude\"\ncommand = \"claude\"\n\n\
    [[agents]]\nname = \"codex\"\ncommand = \"codex\"\n\n\
    [[agents]]\nname = \"opencode\"\ncommand = \"opencode\"\n";

pub fn config_file() -> PathBuf {
    dir().join("config.toml")
}

/// Seed the default config, absent-only — the serve path calls this once at start. The
/// session-file idiom: whole or absent, so two first boots cannot tear it for a third reader.
pub fn seed_config() {
    let at = config_file();
    if !at.exists() {
        let _ = std::fs::create_dir_all(dir());
        let tmp = dir().join("config.toml.part");
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
