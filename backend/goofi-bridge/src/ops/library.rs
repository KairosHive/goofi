//! The node library: the types `node add` can build, and the private library they are saved to.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{op, EffectOp, NoArgs, ReadOp};
use crate::schemas::Detail;
use crate::{inspect, schemas, AppState, Event, Txn};

// ---- library list (Read)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListArgs {
    pub full: Option<bool>,
}

op!(List, "library list", 0, ListArgs, Value,
    "The node library as an INDEX: every registered type, and the first line of its doc. Everything else about the one type you then pick — where it came from, its slots, its params, the rest of its doc — is `library get`'s, so choosing from this list costs a catalog and not a manual. `--full` answers the palette a client draws the add-menu from instead.",
    "{types: [{type, doc}]}, an unloadable one also carrying `available: false` and saying so in its doc. With `--full`, every entry carries {source, bundle, tags, available, missing_deps, editor, input_slots, input_multi, output_slots, params} and the WHOLE doc.");

// ---- library get (Read)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetArgs {
    #[serde(rename = "type")]
    pub ty: String,
    pub source: Option<bool>,
}

op!(Get, "library get", 1, GetArgs, Value,
    "ONE library entry in full: the palette fields — slots, params, availability — plus where the type came from. `--source` reads the file itself too, under `text`. Copy a node into the patch workspace to modify one.",
    "the `library list --full` entry plus {language, tier, provenance, path, shadowed}, and `text` under `--source` — `shadowed` being the same type's files in the roots BEHIND the winner, each {provenance, path}");

// ---- library save (Effect)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveArgs {
    #[serde(rename = "type")]
    pub ty: String,
    pub overwrite: Option<bool>,
    pub name: Option<String>,
}

op!(Save, "library save", 1, SaveArgs, Value,
    "Save a patch or custom node in the private library. An existing file requires --overwrite. Use --name to save under another file name with the same extension; this keeps the source for existing instances. Without a new name, a patch file moves into the library and future patch saves bundle it from there.",
    "{type, path} — the type as stored, and the library file it now lives in");

// ---- library refresh (Effect)
op!(Refresh, "library refresh", 0, NoArgs, Value,
    "Re-read the shipped and patch node directories; live instances of a changed type restart onto the new code. Call after writing a node file.",
    "{added: [type], changed: [type], removed: [type]}");

impl ReadOp for List {
    /// The library as an INDEX; `--full` answers the palette a client builds every node from.
    fn run(tx: &mut Txn, a: ListArgs) -> Result<Value, String> {
        let detail = if a.full.unwrap_or(false) { Detail::Full } else { Detail::Index };
        Ok(json!({ "types": schemas::catalog_types(&tx.g, detail) }))
    }
}

impl ReadOp for Get {
    /// ONE library entry: the palette entry with its provenance, and `--source` the file behind it.
    fn run(tx: &mut Txn, a: GetArgs) -> Result<Value, String> {
        let mount = tx.state.mount();
        inspect::node_source(&tx.g, &a.ty, &mount, &tx.state.node_roots(), a.source.unwrap_or(false))
    }
}

impl EffectOp for Save {
    /// Move a node file out of the open patch and into the private library, where every later
    /// patch finds it. A MOVE, not a copy: the library is then the one source.
    fn run(state: &AppState, a: SaveArgs, _: &str) -> Result<Value, String> {
        let overwrite = a.overwrite.unwrap_or(false);
        let mount = state.mount();
        let (engine, bare, from) = {
            let g = state.graph.lock();
            let (engine, entry) = g.resolve_type(&a.ty).map_err(|e| format!("library save: {e}"))?;
            let ty = goofi_node::qualify(engine, entry.manifest.type_name);
            if !g.is_patch_type(&ty) && !g.is_custom_type(&ty) {
                return Err(format!("library save: `{ty}` is not a custom node"));
            }
            let bare = entry.manifest.type_name;
            let folder = if g.is_patch_type(&ty) { mount.join(goofi_node::folder_of(engine)) } else { state.custom.clone() };
            let from = crate::node_file_in(&folder, bare, engine)
                .ok_or_else(|| format!("library save: `{ty}` has no source file under {}", folder.display()))?;
            (engine, bare.to_string(), from)
        };
        let library = state.custom.clone();
        let name = match a.name.as_deref() {
            Some(name) => {
                if name.is_empty() || name.contains(['/', '\\']) {
                    return Err("library save: name must be a file name without a directory".into());
                }
                let mut path = std::path::PathBuf::from(name);
                if path.extension().is_none() {
                    path.set_extension(from.extension().ok_or("library save: missing source extension")?);
                }
                if path.extension() != from.extension() || goofi_node::describe::type_name_of(&path).is_none() {
                    return Err("library save: use a valid node file name with the source extension".into());
                }
                path.into_os_string()
            }
            None => from.file_name().ok_or("library save: the source file has no name")?.to_owned(),
        };
        let to = library.join(&name);
        let saved_type = goofi_node::describe::type_name_of(&to).ok_or("library save: invalid file name")?;
        let held = crate::node_file_in(&library, &saved_type, engine).or_else(|| to.exists().then(|| to.clone()));
        if let Some(held) = &held {
            if held.extension() != from.extension() {
                return Err(format!("library save: {} uses another source language; choose a different name", held.display()));
            }
            if !overwrite {
                return Err(format!(
                    "library save: the library already holds {} — rename this node, or pass --overwrite to replace that file",
                    goofi_core::path::to_slash(held)
                ));
            }
        }
        let to = held.unwrap_or(to);
        std::fs::create_dir_all(&library).map_err(|e| format!("library save: {}: {e}", library.display()))?;
        let staged = library.join(format!(".save-{}.part", crate::nonce_hex()?));
        let replace = (|| -> std::io::Result<()> {
            let mut input = std::fs::File::open(&from)?;
            let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&staged)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            drop(output);
            if overwrite {
                std::fs::rename(&staged, &to)
            } else {
                std::fs::hard_link(&staged, &to)?;
                std::fs::remove_file(&staged)
            }
        })();
        if let Err(e) = replace {
            let _ = std::fs::remove_file(&staged);
            return Err(format!("library save: {}: {e}", to.display()));
        }
        // A renamed save is a new library type. Keep the source for existing instances.
        if from != to && saved_type == bare {
            std::fs::remove_file(&from).map_err(|e| format!("library save: {}: {e}", from.display()))?;
        }
        // The file left the mount but the `.gfi` still carries it, from the library — so the patch's
        // saved content did not change, and the unsaved dot must not rise for a move alone.
        if let Ok(rel) = from.strip_prefix(&mount) {
            if !from.exists() { state.forget_baseline(rel); }
        }
        // Rescanned but NOT restarted: the code behind every live instance is byte for byte the file
        // that just moved.
        {
            let mut g = state.graph.lock();
            crate::rescan(state, &mut g, &mount);
            state.events.send(Event::NodeTypes { types: schemas::catalog_types(&g, Detail::Full) });
        }
        crate::resync_and_broadcast(state);
        Ok(json!({ "type": goofi_node::qualify(engine, &saved_type), "path": goofi_core::path::to_slash(&to) }))
    }
}

impl EffectOp for Refresh {
    /// Explicit, never watched: an agent calls it after writing a node file.
    fn run(state: &AppState, _: NoArgs, _: &str) -> Result<Value, String> {
        crate::prebuild(state, &state.mount());
        let result = {
            let mut g = state.graph.lock();
            let (diff, _) = crate::rescan(state, &mut g, &state.mount());
            crate::restart_changed(&mut g, &diff);
            state.events.send(Event::NodeTypes { types: schemas::catalog_types(&g, Detail::Full) });
            json!({ "added": diff.added, "changed": diff.changed, "removed": diff.removed })
        };
        crate::resync_and_broadcast(state);
        Ok(result)
    }
}
