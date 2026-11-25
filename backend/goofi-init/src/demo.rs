//! What a public demo does not have, read off `demo.toml` at the repository root. That file is
//! the one list: the op table, the server's doors, the library and the provisioning all ask here.

use std::sync::OnceLock;

use serde::Deserialize;

/// A part of the server that reaches the host, which a demo can withhold by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Module {
    /// The `/patch.gfi` download and upload.
    PatchFile,
    /// The CLI's `/exec` door.
    Exec,
    /// The `/term` socket of an agent's terminal.
    Terminal,
    /// The `/mcp` endpoint.
    Mcp,
    /// The plugins of the host's home, and their assets.
    Plugins,
    /// The VST3 plugins of the host.
    Vst3,
    /// Every audio device but `browser`.
    SoundCard,
    /// Native windows.
    Windows,
}

/// The four lists of `demo.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Demo {
    pub ops: Vec<String>,
    pub modules: Vec<Module>,
    pub bundles: Vec<String>,
    pub nodes: Vec<String>,
}

impl Demo {
    /// Whether the op `name` is withheld: named itself, or under a group that is.
    pub fn op(&self, name: &str) -> bool {
        self.ops.iter().any(|held| name == held || name.strip_prefix(held.as_str()).is_some_and(|rest| rest.starts_with(' ')))
    }

    pub fn module(&self, module: Module) -> bool {
        self.modules.contains(&module)
    }

    pub fn bundle(&self, name: &str) -> bool {
        self.bundles.iter().any(|held| held == name)
    }

    /// Whether the node type `engine:Type` is withheld.
    pub fn node(&self, engine: &str, name: &str) -> bool {
        self.nodes.iter().any(|held| held.split_once(':') == Some((engine, name)))
    }
}

/// The lists, parsed once. The file is checked in, so one that does not parse is a broken build.
pub fn demo() -> &'static Demo {
    static DEMO: OnceLock<Demo> = OnceLock::new();
    DEMO.get_or_init(|| toml::from_str(include_str!("../../../demo.toml")).unwrap_or_else(|why| panic!("demo.toml does not parse: {why}")))
}
