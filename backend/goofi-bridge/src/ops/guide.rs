//! The guides an agent reads: the orientation, which says how goofi is driven, and the skills of
//! the open patch — goofi's own, laid into every workspace, and the ones the patch carries.

use serde_json::{json, Value};

use super::{op, NoArgs, ReadOp};
use crate::Txn;

/// The guide every other one assumes.
const ORIENTATION: &str = "orientation";

/// The file a skill's directory under `skills/` is read by.
const SKILL_FILE: &str = "SKILL.md";

op!(List, "guide list", 0, NoArgs,
    "Every guide by name: the orientation first, then the skills under the workspace's `skills/` — goofi's own and the patch's. Read the orientation before anything else, then the skill matching the task.",
    "{guides: [{name, doc}]} — a skill's `doc` is its frontmatter description, or its first line");

op!(Get, "guide get", 1, GetArgs {
    pub name: String,
},
    "One guide in full: `orientation` is how goofi is driven — read it first; any other name is that skill's SKILL.md from the workspace.",
    "{text}");

/// The skills under the workspace, sorted: each a directory holding a `SKILL.md`.
fn skills(tx: &Txn) -> Vec<(String, std::path::PathBuf)> {
    let dir = tx.state.mount().join(crate::SKILLS_DIR);
    let mut found: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().join(SKILL_FILE)))
        .filter(|(_, file)| file.is_file())
        .collect();
    found.sort();
    found
}

/// A skill's one-line doc: the `description` of its frontmatter, else its first line of text.
fn description(text: &str) -> String {
    let (front, body) = match text.strip_prefix("---").and_then(|rest| rest.split_once("\n---")) {
        Some(parts) => parts,
        None => ("", text),
    };
    front
        .lines()
        .find_map(|l| l.strip_prefix("description:"))
        .map(str::trim)
        .or_else(|| body.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or_default()
        .to_string()
}

impl ReadOp for List {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        let mut guides = vec![json!({ "name": ORIENTATION, "doc": "how goofi is driven — read first" })];
        for (name, file) in skills(tx) {
            let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            guides.push(json!({ "name": name, "doc": description(&text) }));
        }
        Ok(json!({ "guides": guides }))
    }
}

impl ReadOp for Get {
    fn run(tx: &mut Txn, a: GetArgs) -> Result<Value, String> {
        if a.name == ORIENTATION {
            return Ok(json!({ "text": crate::orientation::ORIENTATION }));
        }
        let skills = skills(tx);
        match skills.iter().find(|(name, _)| *name == a.name) {
            Some((_, file)) => {
                let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
                Ok(json!({ "text": text }))
            }
            None => {
                let names: Vec<&str> = std::iter::once(ORIENTATION).chain(skills.iter().map(|(n, _)| n.as_str())).collect();
                Err(format!("no guide `{}` — the guides: {}", a.name, names.join(", ")))
            }
        }
    }
}
