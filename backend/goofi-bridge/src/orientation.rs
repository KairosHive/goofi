//! What an agent reads: the orientation `guide get orientation` answers, and the note a new
//! workspace carries.

use std::path::Path;

/// The whole orientation — how goofi is driven — as `guide get orientation` answers it.
pub const ORIENTATION: &str = include_str!("orientation.md");

/// The `AGENTS.md` a new workspace carries: what the directory is, and where the orientation is.
pub const AGENTS_MD: &str = "\
This directory is the patch workspace. `session save` packages everything here into the `.gfi`, \
and it all comes back when the patch is opened, except what `.goofiignore` lists.
Run `goofi guide get orientation` before driving goofi.
";

/// Seed a NEW workspace with its `AGENTS.md` and the packaging ignore list — absent-only, and
/// never into a workspace a `.gfi` was unpacked into, whose files are the patch's.
pub fn seed(mount: &Path) {
    for (name, body) in [
        ("AGENTS.md", AGENTS_MD),
        (goofi_graph::archive::IGNORE_FILE, goofi_graph::archive::DEFAULT_IGNORE),
    ] {
        let at = mount.join(name);
        if !at.exists() {
            let _ = std::fs::write(at, body);
        }
    }
}
