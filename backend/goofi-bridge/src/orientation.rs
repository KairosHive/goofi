//! What a workspace tells the agent that lands in it.

use std::path::Path;

/// The whole of an agent's orientation — the very bytes [`seed`] lays in a workspace.
pub const ORIENTATION: &str = include_str!("orientation.md");

/// Seed a NEW workspace with the orientation an agent reads, plus the packaging ignore list —
/// absent-only, and never into a workspace a `.gfi` was unpacked into, whose files are the patch's.
pub fn seed(mount: &Path) {
    for (name, body) in [
        ("AGENTS.md", ORIENTATION),
        ("CLAUDE.md", "@AGENTS.md\n"),
        (goofi_graph::archive::IGNORE_FILE, goofi_graph::archive::DEFAULT_IGNORE),
    ] {
        let at = mount.join(name);
        if !at.exists() {
            let _ = std::fs::write(at, body);
        }
    }
}
