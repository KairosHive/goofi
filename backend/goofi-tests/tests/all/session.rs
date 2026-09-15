//! A whole authoring session: build a patch, save it, find it again, and open it somewhere else.

use goofi_tests::{hex, j, Goofi};
use serde_json::Value;

fn panel(g: &Goofi) -> String {
    goofi_tests::panel_ids(&g.doc()["arrangement"]).first().cloned().expect("the default panel")
}

#[test]
fn a_patch_is_built_saved_and_opened_somewhere_else_unchanged() {
    let g = Goofi::new();

    let types = g.call("library list", j!({}))["types"].as_array().cloned().unwrap();
    for want in ["signal:LFO", "signal:Buffer"] {
        assert!(types.iter().any(|t| t["type"] == want), "{want} is in the palette");
    }
    assert!(!types.iter().any(|t| t["type"] == "signal:_TestEcho"), "test nodes are not");

    let osc = g.add("LFO");
    let buf = g.add("Buffer");
    let sink = g.add("Buffer");
    g.call("node edit", j!({ "node": hex(osc), "name": "carrier" }));
    g.set_param(buf, "buffer", "size", 128);
    g.link(osc, "out", buf, "input");
    g.link(buf, "out", sink, "input");

    g.call("variable entry add", j!({ "name": "patch.gain", "value": 2.0, "type": "float" }));
    g.call("node param edit", j!({ "node": hex(sink), "param": "buffer/size",
                                   "expression": "variables.patch.gain * 64" }));
    // A control element is a variable that carries a widget, and the archive carries the record.
    g.call("variable entry edit", j!({ "name": "patch.gain", "control":
        { "kind": "knob", "min": 0.0, "max": 4.0, "step": 0.01, "x": 2.0, "y": 1.0, "w": 2.0, "h": 2.0 } }));
    let why = g.refuse("variable entry edit", j!({ "name": "patch.gain", "control": { "kind": "toggle" } }));
    assert!(why.contains("toggle") && why.contains("float"), "a widget that cannot draw the type: {why}");
    // A lock rides the archive too, the entry's own and its group's, and so does what it follows.
    g.call("variable entry lock", j!({ "name": "patch.gain", "value": true }));
    g.call("variable entry source", j!({ "name": "patch.gain", "reference": "level.out", "index": 0 }));
    g.call("variable group lock", j!({ "group": "patch", "config": true }));
    // …and a reference over it: the archive carries the whole record, the expression retained.
    let level = g.add("_TestScalar");
    g.call("node edit", j!({ "node": hex(level), "name": "level" }));
    g.call("node param edit", j!({ "node": hex(sink), "param": "buffer/size", "reference": "level.out" }));

    let scope = g.call("nodes group", j!({ "nodes": [hex(buf)], "pos": [40.0, 10.0] }))["inst_id"]
        .as_str().unwrap().to_string();
    assert_eq!(g.ports(&scope).len(), 2, "both cuts are exposed: {:?}", g.ports(&scope));

    // Nest it, and leave one port with nothing behind it. Between them these are every shape the
    // archive's one entity kind has to carry: a facade inside a facade, a port wired to another
    // scope's port, and a port whose inner wire is simply absent.
    let outer = g.call("nodes group", j!({ "nodes": [&scope], "pos": [80.0, 10.0] }))["inst_id"]
        .as_str().unwrap().to_string();
    let spare = g.call("node add", j!({ "type": "OutTable", "inst_id": outer, "pos": [5.0, 6.0] }))
        ["uid"].as_str().unwrap().to_string();
    g.call("node edit", j!({ "node": spare, "name": "spare" }));

    // The file says it in ONE vocabulary: a facade and a port are node records like any other, and
    // a port's inner wire is a link like any other.
    let yaml = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    let saved: serde_json::Value = serde_yaml_ng::from_str(&yaml).unwrap();
    assert_eq!(saved["goofi"], env!("CARGO_PKG_VERSION"), "the manifest names its writer");
    assert!(saved["root"].get("scopes").is_none(), "no block of its own for the structure");
    let recs = saved["root"]["nodes"].as_object().unwrap();
    assert_eq!(recs[&outer]["type"], "SubPatch", "the facade is a node record: {:?}", recs[&outer]);
    assert_eq!(recs[&spare]["type"], "OutTable", "…and so is the port");
    assert_eq!(recs[&scope]["scope"], outer, "membership rides the record it belongs to");
    let source = &recs[&hex(sink)]["sources"][0];
    assert_eq!((&source["mode"], &source["expression"], &source["reference"]),
               (&j!("reference"), &j!("variables.patch.gain * 64"), &j!("level.out")), "{source}");

    let saved_gain = saved["variables"].as_array().unwrap().iter()
        .find(|e| e["name"] == "patch.gain").cloned().expect("the element is in the file");
    assert_eq!((&saved_gain["control"]["kind"], &saved_gain["control"]["x"]), (&j!("knob"), &j!(2.0)),
               "the widget and its place ride the archive: {saved_gain}");
    assert_eq!(saved_gain["lock"], j!({ "config": false, "value": true }), "{saved_gain}");
    assert_eq!(saved_gain["source"], j!({ "reference": "level.out", "index": 0 }), "{saved_gain}");
    assert_eq!(saved["variable_groups"]["patch"]["lock"]["config"], true, "{}", saved["variable_groups"]);
    assert!(saved["variable_groups"].get("system").is_none(), "the system group's lock is goofi's, not the file's");

    g.call("layout panel edit", j!({ "panel": panel(&g), "type": "viewer",
                                        "state": { "node": hex(osc), "slot": "out" } }));

    let before = g.doc();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("patch.gfi");
    std::fs::write(g.state.mount().join("notes.md"), b"the EEG source is on channel 3").unwrap();

    // Step: the skills goofi ships are laid into every workspace, so an agent spawned into one
    // reads them from its own cwd — and seeded BEFORE the baseline, so they never dirty a patch.
    let skills = g.state.mount().join(goofi_bridge::SKILLS_DIR);
    let shipped: Vec<String> = std::fs::read_dir(&skills)
        .expect("a workspace carries the shipped skills")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect();
    assert!(shipped.contains(&"designing-emergent-shaders".to_string()), "{shipped:?}");
    assert!(skills.join("designing-emergent-shaders/SKILL.md").is_file(), "with its files beside it");
    // An edit of goofi's own copy is the PATCH's from here on, and must survive the round trip.
    std::fs::write(skills.join("designing-emergent-shaders/SKILL.md"), b"mine now").unwrap();
    // …and a skill this patch invented travels like any other workspace file.
    std::fs::create_dir_all(skills.join("mine")).unwrap();
    std::fs::write(skills.join("mine/SKILL.md"), b"a skill of the patch's own").unwrap();

    g.call("session save", j!({ "path": path.to_string_lossy() }));
    assert_eq!(g.call("session status", j!({}))["dirty"], false, "a saved patch is clean");

    // Opened in an instance that has already held other nodes — a fresh one renumbers to the saved uids.
    let other = Goofi::new();
    for _ in 0..5 {
        other.add("LFO");
    }
    other.call("session load", j!({ "path": path.to_string_lossy() }));

    let after = other.doc();
    assert_eq!(after["nodes"], before["nodes"],
               "every node came back as it was, uid for uid — facades and ports among them");
    assert_eq!(after["links"], before["links"], "and so did every wire, inner ones included");
    assert_eq!(after["variables"], before["variables"]);
    assert_eq!(after["arrangement"], before["arrangement"],
               "…so the panel still names a node that exists");
    assert_eq!(std::fs::read(other.state.mount().join("notes.md")).unwrap(),
               b"the EEG source is on channel 3", "the workspace travelled with the patch");
    assert_eq!(other.call("session status", j!({}))["dirty"], false,
               "a patch is not unsaved the moment it finishes loading");

    // Step: the skills rode inside the `.gfi`, and the patch's own copies are what came back —
    // goofi seeds the absent ones and never writes over one the patch carries.
    let landed = other.state.mount().join(goofi_bridge::SKILLS_DIR);
    assert_eq!(std::fs::read(landed.join("designing-emergent-shaders/SKILL.md")).unwrap(),
               b"mine now", "an edited skill is the PATCH's, and a load does not restore goofi's");
    assert_eq!(std::fs::read(landed.join("mine/SKILL.md")).unwrap(),
               b"a skill of the patch's own", "…and a skill the patch invented travels with it");

    // Step: a patch saved WITHOUT a skill goofi has — which is every patch saved before that skill
    // existed — is given it on load, and packages it from then on.
    std::fs::remove_dir_all(landed.join("designing-emergent-shaders")).unwrap();
    let older = dir.path().join("older.gfi");
    other.call("session save", j!({ "path": older.to_string_lossy() }));
    let reopened = Goofi::new();
    reopened.call("session load", j!({ "path": older.to_string_lossy() }));
    let regained = reopened.state.mount().join(goofi_bridge::SKILLS_DIR);
    assert!(regained.join("designing-emergent-shaders/SKILL.md").is_file(),
            "a skill goofi gained since the patch was saved is laid in on load");
    assert_ne!(std::fs::read(regained.join("designing-emergent-shaders/SKILL.md")).unwrap(),
               b"mine now", "and it is goofi's own copy, the patch having carried none");
    assert_eq!(std::fs::read(regained.join("mine/SKILL.md")).unwrap(),
               b"a skill of the patch's own", "the patch's own is untouched beside it");
    assert_eq!(reopened.call("session status", j!({}))["dirty"], false,
               "seeding a skill on load never leaves the patch dirty");

    // …and reopened over ITSELF, in the session that has been running it all along.
    let mut ev = g.events();
    let late = g.add("LFO");
    g.ready(late);
    // A uid the status worker has never reported on, so its `ready` is the tick that also memoized
    // every node already running.
    loop {
        let told = ev.next("node_stage");
        if told["node"] == hex(late) && told["stage"] == "ready" {
            break;
        }
    }
    g.call("session save", j!({ "path": path.to_string_lossy() }));
    g.call("session load", j!({ "path": path.to_string_lossy() }));

    // An EPHEMERAL variable is goofi's own: the manifest never carries it, and the load re-derives it.
    let manifest = g.call("session manifest", j!({}));
    assert!(!manifest["yaml"].as_str().unwrap().contains("goofi_home"),
            "a machine path in a patch file travels to the wrong machine");
    // An archive from before groups existed cannot come up half-alive: every expression reading
    // its variables would be broken, so the load stops and names the variable.
    let flat = g.call("session manifest", j!({}))["yaml"].as_str().unwrap()
        .replace("name: patch.gain", "name: gain");
    let why = g.refuse("session load", j!({ "content": flat }));
    assert!(why.contains("gain") && why.contains("group.element"), "the load names the variable: {why}");

    let reborn = g.call("variable list", j!({}))["variables"].as_array().unwrap().iter()
        .find(|e| e["name"] == "patch.gain").cloned().expect("the element came back");
    assert_eq!((&reborn["control"]["kind"], &reborn["control"]["w"]), (&j!("knob"), &j!(2.0)),
               "the load restored the widget and its place: {reborn}");
    assert_eq!(reborn["lock"], j!({ "config": true, "value": true }), "the load restored both locks: {reborn}");
    assert_eq!(reborn["source"]["reference"], "level.out", "…and what it follows: {reborn}");
    let held = g.call("variable list", j!({}))["variables"].as_array().unwrap().iter()
        .find(|e| e["name"] == "system.goofi_home").cloned().unwrap();
    assert_eq!(held["value"], j!(goofi_core::path::to_slash(&goofi_core::home::dir())));

    let snap = ev.next("graph_replaced");
    assert_eq!(snap["runtime"][hex(late)]["stage"], "creating",
               "the load rebuilt it at the uid it was saved with, and the snapshot caught it starting");
    g.ready(late);
    // The stage stream is a delta over that snapshot, so the node reaching `ready` again HAS to be
    // said — the uid it came back at reported the same thing in its previous life.
    loop {
        let told = ev.next("node_stage");
        if told["node"] == hex(late) && told["stage"] == "ready" {
            break;
        }
    }
}

fn save_path(g: &Goofi) -> Option<String> {
    g.call("session status", j!({}))["save_path"].as_str().map(str::to_string)
}

fn dirty(g: &Goofi) -> bool {
    g.call("session status", j!({}))["dirty"] == true
}

/// A path as goofi spells it back: canonical and `/`-separated, the same resolution a save
/// applies — so an OS shorthand for the same file (a Windows 8.3 name) still compares equal.
fn spelled(p: &std::path::Path) -> String {
    goofi_core::path::to_slash(&goofi_core::path::canonical(p).unwrap())
}

#[test]
fn only_a_file_gives_a_patch_a_home_a_refused_load_changes_nothing_and_a_broken_layout_still_opens() {
    // The manager owns the stored path, because a plain Save overwrites it silently from any tab.
    let g = Goofi::new();
    assert_eq!(save_path(&g), None, "an unsaved patch has no home yet");
    g.add("LFO");

    // A save's ONLY job is writing to a backend path, so a save with no path is malformed.
    let why = g.refuse("session save", j!({}));
    assert!(why.contains("session save") && why.contains("path"), "{why}");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("patch.gfi");
    let mut ev = g.events();
    g.call("session save", j!({ "path": path.to_string_lossy() }));
    assert_eq!(ev.next("save_path_changed")["save_path"].as_str(), Some(spelled(&path).as_str()));
    assert_eq!(save_path(&g).as_deref(), Some(spelled(&path).as_str()));
    // Readable only through `read_gfi` — a bare-YAML write would leave it "not a zip archive".
    let dest = dir.path().join("unpacked");
    let manifest = goofi_graph::archive::read_gfi(&path, &dest).unwrap();
    assert!(manifest.contains("LFO"), "the manifest is the serialized patch: {manifest}");
    assert!(dest.is_dir(), "the workspace tree rides along, empty or not");

    // A save that FAILS leaves the previous home standing; naming it would aim the next overwrite at it.
    let nowhere = dir.path().join("no-such-dir").join("patch.gfi");
    g.refuse("session save", j!({ "path": nowhere.to_string_lossy() }));
    assert_eq!(save_path(&g).as_deref(), Some(spelled(&path).as_str()), "the old home stands");

    // A refused load leaves the open patch exactly as it was, workspace included.
    std::fs::write(g.state.mount().join("notes.md"), b"work in progress").unwrap();
    let before = g.doc();
    let mount = g.state.mount();
    // In the order the arm reaches them; the third pins commit-AFTER-parse.
    let junk = dir.path().join("junk.gfi");
    std::fs::write(&junk, "this: is: not: a patch").unwrap();
    let packed = dir.path().join("ws");
    std::fs::create_dir(&packed).unwrap();
    std::fs::write(packed.join("intruder.txt"), b"from the refused archive").unwrap();
    let bad = dir.path().join("bad.gfi");
    goofi_graph::archive::write_gfi(&bad, "this: is: not: a patch", &packed, &[]).unwrap();
    for target in [dir.path().join("absent.gfi"), junk, bad] {
        g.refuse("session load", j!({ "path": target.to_string_lossy() }));
    }
    // Valid YAML from a FUTURE goofi: the version gate refuses, and the refusal names the writer.
    let future = dir.path().join("future.gfi");
    goofi_graph::archive::write_gfi(&future, "version: 99\ngoofi: \"9.9.9\"\nroot: {}", &packed, &[]).unwrap();
    let refusal = g.refuse("session load", j!({ "path": future.to_string_lossy() }));
    assert!(refusal.contains("written by goofi 9.9.9"), "the writer is named: {refusal}");
    // The two doors are one op, and never both at once: a manifest inline, or an archive at a path.
    let yaml = g.call("session manifest", j!({}))["yaml"].as_str().unwrap().to_string();
    let why = g.refuse("session load", j!({ "content": yaml.clone(), "path": "/tmp/nope.gfi" }));
    assert!(why.contains("never both"), "{why}");

    assert_eq!(g.doc(), before, "the open patch is untouched");
    assert_eq!(g.state.mount(), mount, "on the mount it was already using");
    assert_eq!(std::fs::read(mount.join("notes.md")).unwrap(), b"work in progress");
    assert!(!mount.join("intruder.txt").exists(), "and nothing from the refused archive landed");
    assert_eq!(save_path(&g).as_deref(), Some(spelled(&path).as_str()), "and the home stands");

    // A layout the flat model admits but cannot render must never make a patch unopenable.
    // A DUPLICATE id is the one corruption the tree admits and a flat map could not.
    let broken = yaml.replace("id: panel-2", "id: tab-1");
    assert_ne!(broken, yaml, "the fixture actually corrupted something");
    let r = g.call("session load", j!({ "content": broken }));
    assert_eq!(r["ok"], true, "the patch still opens: {r}");
    assert!(r["layout_warning"].as_str().is_some_and(|w| w.contains("appears twice")),
            "…and says why the arrangement was dropped: {r}");
    assert_eq!(g.nodes().len(), 1, "with the graph intact");
    // An upload carries no file, so inheriting the previous path would save a different patch over it.
    assert_eq!(save_path(&g), None, "an uploaded patch has no home");
}

#[test]
fn a_save_packs_the_live_mount_refuses_to_pack_into_it_and_never_truncates_a_good_archive() {
    // Two of `save_archive`'s three properties can only be staged by calling it directly.
    let tmp = tempfile::tempdir().unwrap();
    let mount = tmp.path().join("goofi-0123").join("workspace");
    std::fs::create_dir_all(&mount).unwrap();
    std::fs::write(mount.join("agent.md"), b"notes").unwrap();

    let target = tmp.path().join("patch.gfi");
    goofi_bridge::save_archive(&target, "version: 7\n", &mount, &[], true).unwrap();
    let before = std::fs::read(&target).unwrap();
    assert!(goofi_bridge::save_archive(&target, "version: 8\n", &mount, &[], false).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), before);
    let dest = tmp.path().join("unpacked");
    assert_eq!(goofi_graph::archive::read_gfi(&target, &dest).unwrap(), "version: 7\n");
    assert_eq!(std::fs::read(dest.join("agent.md")).unwrap(), b"notes", "the LIVE mount is packed");

    // The workspace walk fails after the first zip entry is written. It sits OUTSIDE the target's
    // directory, or the mount refusal below answers first and the pack never runs.
    let good = tmp.path().join("previous.gfi");
    std::fs::write(&good, b"the previous save").unwrap();
    let gone = tmp.path().join("mnt").join("gone").join("workspace");
    let err = goofi_bridge::save_archive(&good, "version: 7\n", &gone, &[], true).unwrap_err();
    assert!(err.contains("save failed"), "the refusal names the operation: {err}");
    assert_eq!(std::fs::read(&good).unwrap(), b"the previous save");
    assert!(!tmp.path().join("previous.gfi.tmp").exists(), "the half-written sibling is cleaned up");

    // A target inside the mount would pack the archive into itself.
    for inside in [mount.join("patch.gfi"), mount.parent().unwrap().join("patch.gfi")] {
        let err = goofi_bridge::save_archive(&inside, "version: 7\n", &mount, &[], true).unwrap_err();
        assert!(err.contains("temporary workspace"), "the refusal says why: {err}");
        assert!(!inside.exists(), "a refused save writes nothing");
    }

    // Two names that fold to one file: packed here and unpacked on macOS, the second would take
    // the first's place and the load would report success. Both ends refuse instead — but such a
    // PAIR only exists where the filesystem tells the two apart, and where it folds them the
    // second write IS the first file, so there is nothing for a save to refuse.
    std::fs::write(mount.join("Agent.md"), b"the other one").unwrap();
    if std::fs::read(mount.join("agent.md")).is_ok_and(|b| b == b"notes") {
        let clash = tmp.path().join("clash.gfi");
        let err = goofi_bridge::save_archive(&clash, "version: 7\n", &mount, &[], true).unwrap_err();
        assert!(err.contains("fold case"), "the refusal names the reason: {err}");
        assert!(!clash.exists(), "and it writes nothing");
    }
    std::fs::remove_file(mount.join("Agent.md")).unwrap();

    // The same pair reaching a load from anywhere else — an older goofi, another tool.
    let made = tmp.path().join("made.gfi");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&made).unwrap());
    for entry in ["patch.yaml", "workspace/agent.md", "workspace/Agent.md"] {
        zip.start_file(entry, zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut zip, b"version: 7\n").unwrap();
    }
    zip.finish().unwrap();
    let err = goofi_graph::archive::read_gfi(&made, &tmp.path().join("never")).unwrap_err();
    assert!(err.contains("fold case"), "the load refuses it too: {err}");
    assert!(!tmp.path().join("never").exists(), "and unpacks nothing");
}

#[test]
fn the_workspace_counts_as_unsaved_work_a_load_is_clean_and_a_new_patch_inherits_nothing() {
    // There is no watcher: the manager compares the mount against the fingerprint of the last pack.
    let g = Goofi::new();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("patch.gfi");
    g.add("LFO");
    g.call("layout panel add", j!({ "name": "Second" }));
    std::fs::write(g.state.mount().join("agent.md"), b"notes").unwrap();
    g.call("session save", j!({ "path": target.to_string_lossy() }));
    assert!(!dirty(&g), "the patch was just written to disk, workspace and all");

    std::fs::write(g.state.mount().join("scratch.txt"), b"written since the save").unwrap();
    assert!(dirty(&g), "a workspace file the archive lacks is an unsaved change");

    // The fingerprint carries more than the set of names, including an edit that keeps the length.
    g.call("session save", j!({ "path": target.to_string_lossy() }));
    assert!(!dirty(&g), "saving again re-baselines the workspace");
    std::fs::write(g.state.mount().join("agent.md"), b"NOTES").unwrap();
    assert!(dirty(&g), "an edit to a packed file is an unsaved change too");
    g.call("session save", j!({ "path": target.to_string_lossy() }));
    std::fs::write(g.state.mount().join("agent.md"), b"note!").unwrap();
    assert!(dirty(&g), "a same-length in-place edit is an unsaved change");

    // A save that FAILED packed no file, so those edits still live only in the mount.
    g.refuse("session save", j!({ "path": tmp.path().join("no-such-dir").join("patch.gfi").to_string_lossy() }));
    assert!(dirty(&g), "a save that wrote nothing cannot call the workspace packed");

    // A SECOND manager, which is the real case: it has no baseline of its own to fall back on.
    let opened = Goofi::new();
    opened.call("session load", j!({ "path": target.to_string_lossy() }));
    assert_eq!(std::fs::read(opened.state.mount().join("agent.md")).unwrap(), b"NOTES");
    assert!(!dirty(&opened), "a patch is not unsaved the moment it finishes loading");

    // The same file at BOOT — `--load`, open before the first client connects, and as clean.
    let mut booted = Goofi::new();
    booted.state.load = Some(target.clone());
    goofi_bridge::open_load(&booted.state).unwrap();
    assert_eq!(std::fs::read(booted.state.mount().join("agent.md")).unwrap(), b"NOTES");
    assert!(!dirty(&booted), "a boot load is no more unsaved work than any other load");

    // `new` is the EMPTY patch, reached from a patch with a graph, an arrangement, a history and a
    // file; each half fails separately. Only a demo reads the reset as a return to what it booted into.
    let old_mount = g.state.mount();
    g.call("session new", j!({}));
    assert!(g.nodes().is_empty(), "no nodes");
    assert_eq!(g.call("layout inspect", j!({}))["text"].as_str().unwrap().matches("tab `").count(), 1,
               "no tabs of the previous patch");
    assert_eq!(save_path(&g), None, "no file behind it");
    assert!(!dirty(&g), "and nothing to save");
    assert_eq!(g.call("undo", j!({}))["changed"], false, "the history went with the patch");

    let mount = g.state.mount();
    assert_ne!(mount, old_mount, "a fresh workspace");
    assert!(!old_mount.exists(), "and the one it replaced is released, not leaked");
    // `new` MINTS the workspace, so it seeds the orientation while `load`, one line away, must not.
    assert!(!mount.join("agent.md").exists());
    assert!(std::fs::read_to_string(mount.join("AGENTS.md")).unwrap().contains("goofi is a live"));
    assert_eq!(std::fs::read_to_string(mount.join("CLAUDE.md")).unwrap(), "@AGENTS.md\n");
}

#[test]
fn the_file_browser_answers_a_path_the_way_save_and_load_take_it() {
    // Expectations built from `goofi_core::path`'s own primitives: the function under test on both
    // sides would accept a normalizer that reversed the segments it re-attaches.
    use goofi_core::path::{canonical, to_slash};
    let g = Goofi::new();
    let list = |p: Option<&str>| match p {
        Some(p) => g.call("dir list", j!({ "path": p })),
        None => g.call("dir list", j!({})),
    };
    let list_hidden = |p: &str| g.call("dir list", j!({ "path": p, "hidden": true }));
    let names = |l: &Value| -> Vec<String> {
        l["entries"].as_array().unwrap().iter()
            .map(|e| e["name"].as_str().unwrap().to_string()).collect()
    };

    let tmp = tempfile::tempdir().unwrap();
    for f in ["Beta.txt", "alpha.txt", ".hidden", "patch.gfi"] {
        std::fs::write(tmp.path().join(f), b"x").unwrap();
    }
    for d in ["Zeta", "apples"] {
        std::fs::create_dir_all(tmp.path().join(d)).unwrap();
    }
    // A name this platform accepts but UTF-8 cannot express; a lossy entry can collide with
    // another. Best-effort: a filesystem that refuses the bytes (macOS, EILSEQ) has no
    // undecodable names to filter, and Linux CI proves the filter either way.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let bad = std::ffi::OsString::from_vec(b"bad\xff".to_vec()); // starts no valid sequence
        let _ = std::fs::write(tmp.path().join(bad), b"x");
    }

    let here = to_slash(&canonical(tmp.path()).unwrap());
    let listing = list(Some(&tmp.path().to_string_lossy()));
    assert_eq!(names(&listing), ["apples", "Zeta", "alpha.txt", "Beta.txt", "patch.gfi"],
               "directories first, then case-insensitively by name, and nothing undecodable");
    // A dot-name is LEFT OUT rather than sent and dropped — the browser drew none of them, and a
    // home directory is mostly dotfiles.
    assert_eq!(names(&list_hidden(&tmp.path().to_string_lossy()))
                   .into_iter().filter(|n| n.starts_with('.')).collect::<Vec<_>>(),
               [".hidden"], "`--hidden` is the door to them");

    let by_name = |n: &str| listing["entries"].as_array().unwrap().iter()
        .find(|e| e["name"] == n).unwrap_or_else(|| panic!("{n} is listed")).clone();
    let entry = by_name("alpha.txt");
    for key in ["name", "path", "kind", "is_gfi"] {
        assert!(entry.get(key).is_some(), "an entry is missing `{key}`");
    }
    assert_eq!(entry["kind"], "file");
    assert_eq!(entry["path"], format!("{here}/alpha.txt"));
    // The browser renders neither a size nor a date column, so the row carries neither — nor a
    // `hidden` flag, now that a listing holds nothing for it to mark.
    for key in ["size", "mtime", "hidden"] {
        assert!(entry.get(key).is_none(), "an entry carries an unrendered `{key}`");
    }
    assert_eq!((&by_name("patch.gfi")["is_gfi"], &by_name("alpha.txt")["is_gfi"]),
               (&j!(true), &j!(false)));

    // A FILE path lists its parent, so a path typed into Save-As navigates.
    assert_eq!(list(Some(&tmp.path().join("patch.gfi").to_string_lossy()))["path"], here);

    // The frontend omits `path` on the first open; a cleared input sends "".
    let home = std::env::home_dir().expect("a home directory in the test environment");
    let home = to_slash(&canonical(&home).unwrap_or_else(|_| home.clone()));
    assert_eq!(list(None)["path"].as_str().unwrap(), home);
    assert_eq!(list(Some("")), list(None));
    assert_eq!(list(Some("~"))["path"].as_str().unwrap(), home, "a leading tilde expands");
    // The sidebar marks a root active by raw string equality against `path`.
    let roots = list(None)["roots"].as_array().unwrap().clone();
    assert!(roots.iter().any(|r| r["label"] == "Home"), "expected a Home root");
    for root in &roots {
        let path = root["path"].as_str().unwrap();
        assert_eq!(list(Some(path))["path"].as_str().unwrap(), path, "root {path} is not stable");
    }
    // The topmost ancestor of a real path — `/` is not a root on Windows.
    let top = to_slash(canonical(&std::env::temp_dir()).unwrap().ancestors().last().unwrap());
    assert!(list(Some(&top))["parent"].is_null(), "root has no parent to climb to");

    // Navigation must NOT error, or the browser keeps showing the previous directory. `..` has no
    // `file_name()`, so an ancestor walk would silently drop it and land elsewhere.
    let missing = format!("{top}definitely/not/a/directory");
    let listing = list(Some(&missing));
    assert_eq!((names(&listing), listing["path"].as_str().unwrap()), (Vec::new(), missing.as_str()));
    assert_eq!(list(Some(&tmp.path().join("new/deeper").to_string_lossy()))["path"],
               format!("{here}/new/deeper"));
    assert_eq!(list(Some(&tmp.path().join("missing/../also-missing").to_string_lossy()))["path"],
               format!("{here}/also-missing"));

    // Proven through a REFUSAL that names the expanded path: saving into `$HOME` is not something
    // a test may do.
    let why = g.refuse("session load", j!({ "path": "~/definitely-not-a-patch-goofi-wrote.gfi" }));
    assert!(why.contains(&home), "the refusal names the expanded path, not the tilde: {why}");
}

/// The autosave beside the mount, once it exists: the recovery's nonce directory.
fn autosave_dir(g: &Goofi) -> std::path::PathBuf {
    g.state.mount().parent().unwrap().to_path_buf()
}

/// A copy of `dir` as a session nobody holds left it: what a crash leaves, minted for a test that
/// cannot let go of its own session's lock. Answers the nonce directory, and where the next boot
/// moves it to for safekeeping, both as goofi spells them.
fn crashed_copy(dir: &std::path::Path) -> (String, String) {
    let id = goofi_core::session::fresh_id();
    let dead = goofi_core::session::workspace_dir(&id).join(dir.file_name().unwrap());
    fn copy(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }
    copy(dir, &dead);
    let kept = goofi_core::session::recovery_base().join(&id).join(dir.file_name().unwrap());
    (goofi_core::path::to_slash(&dead), goofi_core::path::to_slash(&kept))
}

#[test]
fn unsaved_work_is_autosaved_beside_the_mount_and_a_crash_leaves_it_for_the_next_manager_to_offer() {
    let g = Goofi::new();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home.gfi");
    let manifest = |g: &Goofi| autosave_dir(g).join(goofi_graph::archive::MANIFEST);

    // A patch with nothing unsaved has no autosave: what a crash leaves is exactly the unsaved work.
    assert!(g.stays(|g| !manifest(g).exists()), "a fresh patch is clean, so nothing is autosaved");
    let osc = g.add("LFO");
    g.call("node edit", j!({ "node": hex(osc), "name": "carrier" }));
    g.until("an edit to be autosaved", |g| manifest(g).exists().then_some(()));
    assert!(std::fs::read_to_string(manifest(&g)).unwrap().contains("carrier"), "the manifest is the patch");
    let unnamed = g.call("session recoverable", j!({}));
    assert!(unnamed["recoveries"].as_array().unwrap().is_empty(), "a running session's autosave is not a recovery");

    // A save cleans the patch, and the autosave goes with the unsaved work it kept.
    g.call("session save", j!({ "path": home.to_string_lossy() }));
    g.until("the autosave to go with the save", |g| (!manifest(g).exists()).then_some(()));

    // Work after the save: a workspace file ALONE — no op, so only the watcher on the mount can
    // say it moved — and then the graph, both kept with the patch's home.
    std::fs::write(g.state.mount().join("notes.md"), b"kept").unwrap();
    g.until("a workspace edit to be autosaved", |g| manifest(g).exists().then_some(()));
    g.set_param(osc, "output", "sfreq", 3.0);
    g.until("the latest edit to be autosaved", |g| {
        std::fs::read_to_string(manifest(g)).ok().filter(|m| m.contains("sfreq: 3.0")).map(|_| ())
    });
    let dir = autosave_dir(&g);

    // The crash: the same directory, as a dead session left it in temp — twice, one to open and
    // one to drop. Nothing is offered until a boot moves it to the recovery base for safekeeping.
    let (left, recover) = crashed_copy(&dir);
    let (_, discard) = crashed_copy(&dir);
    assert!(g.call("session recoverable", j!({}))["recoveries"].as_array().unwrap().is_empty());
    let opened = Goofi::new();
    assert!(!std::path::Path::new(&left).exists(), "the boot moved it out of temp");
    let listed = opened.call("session recoverable", j!({}));
    let names: Vec<&str> = listed["recoveries"].as_array().unwrap().iter().map(|r| r["workspace"].as_str().unwrap()).collect();
    assert!(names.contains(&recover.as_str()) && names.contains(&discard.as_str()), "both offered: {listed}");
    let entry = listed["recoveries"].as_array().unwrap().iter().find(|r| r["workspace"] == recover).unwrap();
    assert_eq!(entry["home"], j!(spelled(&home)), "the home the save gave the patch");
    assert!(entry["at"].as_f64().unwrap() > 0.0, "when it was taken");

    // The one path an op removes wholesale is checked: nothing outside the recovery base.
    let why = opened.refuse("session discard", j!({ "workspace": tmp.path().to_string_lossy() }));
    assert!(why.contains("not a recovery"), "{why}");
    let why = opened.refuse("session recover", j!({ "workspace": goofi_core::path::to_slash(&dir) }));
    assert!(why.contains("not a recovery"), "a live workspace: {why}");
    opened.call("session discard", j!({ "workspace": &discard }));
    assert!(!std::path::Path::new(&discard).exists(), "discarded");
    let why = opened.refuse("session recover", j!({ "workspace": &discard }));
    assert!(why.contains("no autosave"), "{why}");

    // Recovered: the graph, the workspace file and the home come back, as UNSAVED work — a plain
    // save writes the home — and the recovery is taken off the list.
    opened.call("session new", j!({}));
    opened.call("session recover", j!({ "workspace": &recover }));
    let uid = opened.nodes()[0].clone();
    assert_eq!(opened.doc()["nodes"][&uid]["name"], "carrier");
    assert_eq!(opened.doc()["nodes"][&uid]["params"]["output"]["sfreq"]["value"], 3.0);
    assert_eq!(std::fs::read(opened.state.mount().join("notes.md")).unwrap(), b"kept");
    assert_eq!(save_path(&opened), Some(spelled(&home)));
    assert!(dirty(&opened), "a recovery is unsaved work");
    assert!(!std::path::Path::new(&recover).exists(), "taken up, the recovery is gone");
    assert!(opened.call("session recoverable", j!({}))["recoveries"].as_array().unwrap().is_empty());
    opened.call("session save", j!({}));
    assert!(!dirty(&opened), "and the plain save wrote its home");
    assert!(std::fs::read(&home).unwrap().starts_with(b"PK"));

    // The boot pass: a dead session's directory with no autosave held nothing and goes. A clean
    // shutdown leaves neither.
    let husk = goofi_core::session::workspace_dir(&goofi_core::session::fresh_id()).join("nonce");
    std::fs::create_dir_all(husk.join("workspace")).unwrap();
    std::fs::write(husk.join("workspace").join("AGENTS.md"), b"seeded").unwrap();
    let _booted = Goofi::new();
    assert!(!husk.parent().unwrap().exists(), "a husk goes at boot, its session directory with it");
    let nonce = autosave_dir(&g);
    drop(g);
    assert!(!nonce.exists(), "a clean shutdown releases the mount, autosave and all");
}
