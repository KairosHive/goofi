//! The session: its identity and health, the document as a whole, and the save, load, new and
//! recover doors that replace it.

use serde_json::{json, Value};

use super::{op, EffectOp, NoArgs, ReadOp};
use crate::schemas::Detail;
use crate::{autosave, fsbrowse, inspect, schemas, AppState, Caller, Event, Txn};

op!(Status, "session status", 0, NoArgs,
    "The session's identity AND its health: which instance this is, where the patch lives, whether it differs from disk, and every standing error with how long it has stood. One read for `is my patch healthy, and have I saved it`.",
    "{instance_id, save_path: string | null, workspace, dirty: bool, errors: [{node, path, error, standing}], audio, graphics} — `node` is the name to pass back, `path` where it sits; `audio` and `graphics` carry that engine's clock and counters — `graphics.windows` is how many `Window` nodes have a window open — and are null where the engine is not registered");

op!(State, "session state", 0, NoArgs,
    "The whole replicated document, exact and ATOMIC: nodes, links, variables and arrangement in one read — what every client mirrors, read without the sync protocol that carries it. ONE `nodes` map carries leaves, sub-patch facades and boundary ports alike, each naming its scope, and a port's inner wire is in `links` like any other cable. Narrowing is the caller's: pipe it through `jq`.",
    "{nodes, links, variables, arrangement} — nodes and variables keyed by id, links a list.");

op!(Manifest, "session manifest", 0, NoArgs,
    "The open patch as YAML — the manifest a `.gfi` holds, diffable and versionable.",
    "{yaml: string}");

op!(Save, "session save", 1, SaveArgs {
    pub path: Option<String>,
    pub overwrite: Option<bool>,
},
    "Pack the patch and its workspace to a `.gfi`. With no `path` it saves to the patch's home — refused when the patch has never been saved — and a given `path` becomes the new home. Set `overwrite` to false to refuse an existing file; normal Save replaces the current file.",
    "{path: string}");

op!(Load, "session load", 1, LoadArgs {
    pub path: Option<String>,
    pub content: Option<String>,
    pub adopt: Option<bool>,
},
    "Replace the open patch, losing unsaved work. `path` names a `.gfi` and brings its \
                   workspace with it; `--content` is an inline YAML manifest and carries no workspace. \
                   Exactly ONE of the two — the empty patch is `session new`. `adopt` (default true) \
                   decides whether a loaded FILE becomes the patch's home, which is what a later \
                   silent save overwrites; `/patch.gfi` passes false, because the file a browser \
                   upload came from lives on the user's machine and the staged copy this reads is \
                   deleted immediately.",
    "{ok: true, warnings: string[]} — what the load dropped on the way in, if anything");

op!(New, "session new", 0, NoArgs,
    "Replace the open patch with the empty one, losing unsaved work. The undo history is cleared, so this cannot be taken back.",
    "{ok: true, warnings: string[]} — what the load dropped on the way in, if anything");

op!(Recoverable, "session recoverable", 0, NoArgs,
    "Every autosave a goofi that did not shut down cleanly left behind: unsaved work, kept beside the crashed session's workspace. `workspace` is what `session recover` and `session discard` take; `home` is the `.gfi` the patch was saved as, null for one never saved; `at` is when the autosave was taken, in seconds since the epoch.",
    "{recoveries: [{workspace, home: string | null, at: float | null}]}");

op!(Recover, "session recover", 1, RecoverArgs {
    pub workspace: Option<String>,
},
    "Replace the open patch with a crash's autosave, losing unsaved work. The recovered patch is UNSAVED work with its old home: a plain Save writes it where the lost session would have. The autosave is removed once it is open.",
    "{ok: true, warnings: string[]} — what the load dropped on the way in, if anything");

op!(Discard, "session discard", 1, DiscardArgs {
    pub workspace: Option<String>,
},
    "Remove a crash's autosave without opening it. Refused for a workspace a running goofi owns.",
    "{ok: true}");

impl ReadOp for Status {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(json!({
            // The id is what the session-file probe verifies: a listener that answers with another
            // id — or none — is not this session.
            "instance_id": &*tx.state.instance_id,
            "save_path": tx.state.save_path(),
            "workspace": goofi_core::path::to_slash(&tx.state.mount()),
            "dirty": tx.state.is_dirty(),
            "errors": inspect::errors(&tx.g),
            // The clock and counters, null where the engine is not registered: a demo has no audio
            // engine, and a machine with no adapter no graphics one.
            "audio": crate::try_audio_engine(&mut tx.g).map(|a| a.status()),
            "graphics": crate::try_graphics_engine(&mut tx.g).map(|a| a.status()),
            // Every resource this process holds — children, workers, ports, paths, devices — from the
            // one index a lease enters and leaves. What is held at ANY moment, not what was made.
            "resources": goofi_supervisor::scope::inventory(),
        }))
    }
}

impl ReadOp for State {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(tx.state.doc.lock().to_json())
    }
}

impl ReadOp for Manifest {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(json!({ "yaml": tx.g.serialize() }))
    }
}

impl EffectOp for Save {
    fn run(state: &AppState, a: SaveArgs, _: &Caller) -> Result<Value, String> {
        // Expand `~` exactly as the browser does — the two must agree on what a path means. No path
        // means the patch's HOME, and a patch that never had one is refused rather than guessed at.
        let path = match a.path.as_deref() {
            Some(p) => fsbrowse::resolve(p),
            None => state.save_path().ok_or("this patch has no home yet — give a path")?,
        };
        // Held through the pack, so a load that replaces the mount meanwhile cannot delete the
        // directory being zipped.
        let held = state.hold_mount().ok_or("the session has no workspace")?;
        let mount = held.path();
        // Taken under the guard, zipped off it. The workspace is sampled BEFORE the pack:
        // baselining after would call a file written during the zip packed, which LOSES an edit.
        let (manifest, extra, packed, revision) = {
            let mut g = state.graph.lock();
            g.persist();
            let revision = state.doc.lock().version();
            (g.serialize(), crate::bundled_custom(&g, &state.custom), goofi_graph::archive::fingerprint(&mount), revision)
        };
        crate::save_archive(std::path::Path::new(&path), &manifest, &mount, &extra, a.overwrite.unwrap_or(true))?;
        // Adopted under the graph guard, which orders every commit, and only while the packed patch
        // is still the open one: a load during the zip minted a new mount.
        let _g = state.graph.lock();
        if state.mount() != mount {
            return Ok(json!({ "path": path }));
        }
        // An edit that landed during the zip is not in the file, so the patch stays dirty. Announced
        // unconditionally: a patch dirtied by a workspace file alone has no flag transition.
        if state.doc.lock().version() == revision {
            *state.workspace_baseline.lock() = packed;
            state.set_dirty(false);
            state.events.send(Event::UnsavedChanges { unsaved_changes: false });
        }
        // The patch's home, stored ONLY on success and announced as well as stored: an
        // already-connected peer gets no new snapshot to read it from.
        *state.save_path.lock() = Some(path.clone());
        state.events.send(Event::SavePathChanged { save_path: json!(&path) });
        drop(_g);
        fsbrowse::remember(&path);
        Ok(json!({ "path": path }))
    }
}

/// Where a replacement patch comes from.
pub(crate) enum Source {
    Empty,
    /// A `.gfi`, and whether it becomes the home a later silent save overwrites.
    File { path: String, adopt: bool },
    /// An inline YAML manifest, which carries no workspace.
    Inline(String),
    /// A crash's autosave workspace.
    Recover(String),
}

/// The core every patch replacement shares, so nothing after the read can drift between the
/// sources.
fn load_patch(state: &AppState, source: &Source) -> Result<Value, String> {
    // Read OFF the graph lock, as the hello does: the roster's config half is a disk read.
    let agents = goofi_supervisor::home::agents();
    // Every source mounts FRESH and swaps in only once the manifest parsed, so a refused load
    // leaves the open patch untouched. Staged off the lock: its Rust nodes may take seconds.
    let fresh = crate::Mount::new(state.iox.id())?;
    let staged = fresh.path();
    if let Source::File { path, .. } = source {
        goofi_supervisor::progress::report(format!("Opening {}", path.rsplit('/').next().unwrap_or(path)));
    }
    let (content, from_path, recovered) = crate::stage_load(&staged, &state.custom, source)?;
    crate::prebuild(state, &staged);
    // Stopped once the source is staged, as the load restarts the clock. Its end is SAID here: the
    // manifest can still refuse below, and then no GraphReplaced would carry it.
    match state.recorder.stop() {
        Ok(Some(_)) => state.events.send(super::record::record_changed(state)),
        Ok(None) => {}
        Err(e) => {
            let mut ended = super::record::record_state(state);
            ended["error"] = json!(format!("the recording could not be finalized: {e}"));
            state.events.send(Event::RecordChanged(ended));
        }
    }
    let opened = from_path.clone();
    let result = {
        let mut g = state.graph.lock();
        // ORDER is load-bearing: the types the patch SHIPS are registered before the manifest
        // resolves, or the unknown-type gate fires on the nodes the archive brought.
        crate::rescan(state, &mut g, &staged);
        // Parse BEFORE anything is announced or committed.
        goofi_supervisor::progress::report("Starting the patch's nodes");
        let warnings = match g.load_doc(&content, &staged) {
            Ok(warnings) => warnings,
            Err(e) => {
                // Refused, so the registry the scan above swapped is re-derived from the mount
                // that is still live; the staged mount goes with `fresh`.
                crate::rescan(state, &mut g, &state.mount());
                return Err(e);
            }
        };
        // Commit, now that nothing left can fail: the loaded patch's workspace becomes the live
        // one, and the replaced mount goes with the harnesses spawned into it.
        let replaced = state.mount.lock().replace(fresh);
        // Off this thread wherever there IS a wait: this runs under the graph lock, and a harness
        // that will not leave takes the whole grace — five seconds no op may be held for.
        if let Some(finish) = replaced.and_then(|mount| state.reclaim(mount)) {
            state.scope.spawn("goofi-reclaim", finish);
        }
        // Projected HERE, so the snapshot names the version the loaded document is at, and a
        // client can hold its fit until its replica reaches it.
        let (doc, projection) = crate::settle_and_project(state, &mut g);
        crate::reconcile_and_broadcast(state, doc, projection, Vec::new());
        // Sent under the lock, not queued for the dispatcher: the status worker's next stage delta
        // needs this lock, so nothing it says can overtake the snapshot it is a delta over.
        state.events.send(Event::HarnessChanged(state.harnesses.roster(&agents)));
        // `read_gfi` restores no mtimes, so without a baseline taken HERE a patch would be dirty
        // from the moment it finished loading.
        *state.workspace_baseline.lock() = goofi_graph::archive::fingerprint(&state.mount());
        // A load fully resets the session: there is nothing to undo across it.
        state.history.lock().clear();
        // A recovery IS unsaved work — that is what it was kept for — and, taken up, it is done
        // with; a load from a file is exactly what the file holds.
        if let Some(e) = state.set_dirty(recovered.is_some()) {
            state.events.send(e);
        }
        if let Some(dir) = &recovered {
            let _ = autosave::discard(dir);
        }
        // NONE for an inline load and for `session new`, neither with a file behind it: an
        // inherited path would aim the next silent save at an unrelated `.gfi`.
        *state.save_path.lock() = from_path.clone();
        state.events.send(Event::GraphReplaced(schemas::snapshot(
            &g,
            state,
            false,
            recovered.is_some(),
            from_path.as_deref(),
            state.harnesses.roster(&agents),
        )));
        // The patch brought its own node types, which `graph_replaced` does not carry.
        state.events.send(Event::NodeTypes { types: schemas::catalog_types(&g, Detail::Full) });
        if let Some(path) = from_path {
            state.events.send(Event::SavePathChanged { save_path: json!(path) });
        }
        // What the load dropped on the way in — a link it could not make, an arrangement it could
        // not render — is said here rather than left unexplained.
        json!({ "ok": true, "warnings": warnings })
    };
    if let Some(path) = &opened {
        fsbrowse::remember(path);
    }
    crate::resync_and_broadcast(state);
    Ok(result)
}

/// The patch at `path`, through the one replacement every source shares.
pub(crate) fn load_file(state: &AppState, path: &std::path::Path) -> Result<Value, String> {
    load_patch(state, &Source::File { path: path.to_string_lossy().into_owned(), adopt: true })
}

impl EffectOp for Load {
    fn run(state: &AppState, a: LoadArgs, _: &Caller) -> Result<Value, String> {
        let source = match (a.path.filter(|p| !p.is_empty()), a.content) {
            (Some(_), Some(_)) => return Err("a `path` to an archive or a `content` manifest, never both".into()),
            (Some(path), None) => Source::File { path, adopt: a.adopt.unwrap_or(true) },
            (None, Some(content)) => Source::Inline(content),
            // A source is REQUIRED: no bare word may be the destructive New.
            (None, None) => return Err("give a `path` or `--content` — `session new` opens the empty patch".into()),
        };
        load_patch(state, &source)
    }
}

impl EffectOp for New {
    fn run(state: &AppState, _: NoArgs, _: &Caller) -> Result<Value, String> {
        // A demo withholds Load, so the reset is the only way back to the example it was given.
        match state.load.as_deref().filter(|_| state.mode.demo) {
            Some(example) => load_file(state, example),
            None => load_patch(state, &Source::Empty),
        }
    }
}

impl ReadOp for Recoverable {
    /// Every crash's autosave on this machine — read from disk on ask, never mirrored.
    fn run(_: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(json!({ "recoveries": autosave::recoverable() }))
    }
}

impl EffectOp for Recover {
    fn run(state: &AppState, a: RecoverArgs, _: &Caller) -> Result<Value, String> {
        let dir = a.workspace.filter(|p| !p.is_empty())
            .ok_or("give the `workspace` a `session recoverable` entry names")?;
        load_patch(state, &Source::Recover(dir))
    }
}

impl EffectOp for Discard {
    fn run(_: &AppState, a: DiscardArgs, _: &Caller) -> Result<Value, String> {
        let dir = a.workspace.filter(|p| !p.is_empty())
            .ok_or("give the `workspace` a `session recoverable` entry names")?;
        autosave::discard(&autosave::recovery(&dir)?)?;
        Ok(json!({ "ok": true }))
    }
}
