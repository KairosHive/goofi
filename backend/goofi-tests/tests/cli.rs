//! The CLI session: the serve command line the binary parses, then the client library against the
//! real `/exec` door — target resolution over the machine's sessions, every phrase reachable, the
//! batch as one undo step per actor, and the raw read round-tripped through NPY. The spawned
//! binary's argv-to-process path is e2e's.

use goofi_client as client;
use goofi_core::session;
use goofi_tests::Goofi;

fn lines(cmds: &[&str]) -> Vec<String> {
    cmds.iter().map(|c| c.to_string()).collect()
}

/// One entry's rendered text, for a command that must succeed.
fn ok(url: &str, actor: &str, cmd: &str) -> String {
    let entries = client::exec(url, &lines(&[cmd]), Some(actor))
        .unwrap_or_else(|e| panic!("`{cmd}` was refused: {e}"));
    entries.into_iter().next().map(|e| e["text"].as_str().unwrap_or_default().to_string()).unwrap_or_default()
}

#[test]
fn the_serve_command_line_reads_its_flags_and_names_what_it_refuses() {
    let parse = |args: &[&str]| goofi_cli::parse_args(args.iter().map(|s| s.to_string()));
    let bare = parse(&[]).expect("no arguments is a valid invocation");
    assert_eq!((bare.port, bare.bind.as_str()), (None, "127.0.0.1"), "the port is the doors' to decide");
    let cli = parse(&["--port", "9001", "--extra-nodes", "theirs", "--bind", "a", "--list-nodes",
                      "--extra-nodes", "mine", "--bind", "0.0.0.0", "--load", "patch.gfi"])
        .expect("a well-formed invocation");
    assert_eq!((cli.port, cli.bind.as_str(), cli.load.as_deref()), (Some(9001), "0.0.0.0", Some("patch.gfi")),
               "a repeated --bind replaces");
    assert_eq!(cli.extra_nodes, ["theirs", "mine"], "…while --extra-nodes adds");
    assert!(cli.list_nodes && !cli.help);
    for flag in ["--port", "--bind", "--extra-nodes", "--load"] {
        let err = parse(&[flag]).expect_err(&format!("`{flag}` alone must not be ignored"));
        assert!(err.contains(flag), "the message names the flag: {err}");
    }
    assert!(parse(&["--port", "nope"]).unwrap_err().contains("--port"));
    assert!(parse(&["--frobnicate"]).unwrap_err().contains("unknown argument `--frobnicate`"));

    for safe in ["127.0.0.1", "localhost", "::1", "127.0.0.53"] {
        assert!(goofi_cli::exposure_warning(safe).is_none(), "`{safe}` is this machine");
    }
    for open in ["0.0.0.0", "::", "192.168.7.5", "goofi.local"] {
        let warn = goofi_cli::exposure_warning(open).unwrap_or_else(|| panic!("`{open}` warns"));
        assert!(warn.contains(open) && warn.contains("shell") && warn.contains("no authentication"),
                "the warning names the address, the exposure and that nothing else guards it: {warn}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shell_finds_its_server_and_drives_the_whole_vocabulary_through_exec() {
    // The one test here that touches the process-global env, so it is nobody else's.
    let tmp = std::env::temp_dir().join(format!("goofi-cli-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::env::set_var("GOOFI_HOME", &tmp);
    std::env::remove_var("GOOFI_SESSION");

    // The server is in-process (`serve_app`): the harness holds THIS process's session, and the
    // record under test is written here, as the binary's serve path writes its own.
    let g = Goofi::new();
    let base = g.serve().await;
    let url = format!("http://{}", base.trim_start_matches("ws://"));
    let _sole = goofi_tests::sole_session();
    let id = goofi_transport::session().to_string();
    goofi_transport::record_url(&url);

    // A record nobody holds is DEAD: not listed, and left for the server's sweep; the held one
    // is listed. Records are named per process: another run of this suite shares the listing.
    let gone = format!("long_gone_{}", std::process::id());
    let dead = session::system_dir(&gone);
    std::fs::create_dir_all(&dead).unwrap();
    std::fs::File::create(dead.join("alive.lock")).unwrap();
    std::fs::write(dead.join("session.json"), format!(r#"{{"id":"{gone}","url":"http://127.0.0.1:1"}}"#)).unwrap();
    let rows = client::list();
    assert!(rows.iter().any(|s| s.id == id && s.url == url), "{rows:?}");
    assert!(rows.iter().all(|s| s.id != gone) && dead.exists(), "a list reads only: {rows:?}");
    goofi_transport::sweep_dead();
    assert!(!dead.exists(), "the sweep removes it; the live one stays");

    // The listing is machine-wide, so only named rows are asserted. A second held session makes
    // the bare resolution ambiguous, and it says so by naming both.
    let busy = format!("busy_peer_{}", std::process::id());
    let peer = session::hold(&busy).unwrap();
    peer.record_url("http://127.0.0.1:1");
    let listed = |who: &str| client::list().iter().any(|s| s.id == who);
    assert!(listed(&busy) && listed(&id), "both alive: {:?}", client::list());
    let why = client::resolve_target().unwrap_err();
    assert!(why.contains("several") && why.contains(&busy) && why.contains(&id), "{why}");
    // GOOFI_SESSION breaks the tie — and one naming NOTHING is refused by pointing at
    // the listing.
    std::env::set_var("GOOFI_SESSION", "no_such_goofi");
    let why = client::resolve_target().unwrap_err();
    assert!(why.contains("no_such_goofi") && why.contains("session list"), "{why}");
    std::env::set_var("GOOFI_SESSION", &id);
    let target = client::resolve_target().unwrap();
    assert_eq!(target.id, id);
    // Released cleanly: gone from the listing at once.
    drop(peer);
    assert!(!listed(&busy) && listed(&id), "{:?}", client::list());

    // Every phrase is reachable through the real door: `--help` on each resolves and answers.
    let ops: serde_json::Value = serde_json::from_str(&ok(&url, "default", "op list")).unwrap();
    let all: Vec<String> = ops["ops"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["op"].as_str().unwrap().to_string())
        .collect();
    assert!(all.len() > 40, "the whole registry rides the index: {}", all.len());
    for phrase in &all {
        let text = ok(&url, "default", &format!("{phrase} --help"));
        assert!(text.contains(phrase) && text.contains("answers:"),
                "`{phrase} --help` explains itself, result shape included: {text}");
    }
    // The reserved client set is pinned AS the list, and help teaches the door words.
    assert_eq!(
        goofi_bridge::ops::RESERVED,
        ["serve", "help", "session list", "agent term", "completions"]
    );
    let top = ok(&url, "default", "help");
    assert!(top.contains("session list") && top.contains("node"), "{top}");
    let group = ok(&url, "default", "help node");
    assert!(group.contains("node param edit"), "a group listing: {group}");
    let err = client::exec(&url, &lines(&["frobnicate"]), None).unwrap_err();
    assert!(err.contains("unknown op"), "{err}");

    // Completion is a vocabulary read: every candidate is `word<TAB>doc`, and the walk follows
    // the phrase tree — groups with their own doc line, then an op's flags.
    let complete = |line: &str| ok(&url, "default", &format!("op complete --line '{line}'"));
    let top = complete("");
    assert!(top.lines().any(|l| l.starts_with("node\t") && l.contains("one node instance")),
            "a group word completes with its own doc: {top}");
    let sub = complete("node ");
    assert!(sub.lines().any(|l| l.starts_with("param\t")) && !sub.contains("session"),
            "inside a group only its children answer: {sub}");
    let flags = complete("node add --");
    assert!(flags.contains("--type\t<string>, required") && flags.contains("--pos\t<float2>"),
            "an op completes its flags, typed and marked: {flags}");
    assert!(complete("node ad").lines().all(|l| l.starts_with("ad")),
            "a partial word filters");

    // A param edit lands on the graph, and a multi-line batch is ONE step in ITS actor's stack.
    let born: serde_json::Value =
        serde_json::from_str(&ok(&url, "shell_a", "node add --type LFO --name osc")).unwrap();
    let uid = born["uid"].as_str().unwrap().to_string();
    ok(&url, "shell_a", &format!("node param edit {uid} output/sfreq --value 99"));
    assert_eq!(
        g.doc()["nodes"][&uid]["params"]["output"]["sfreq"]["value"], 99.0,
        "the edit reached the graph"
    );
    // …and completion turns LIVE: a `uid` position offers the patch's own nodes, by the NAME a
    // human types, with the uid riding in the doc column.
    assert!(ok(&url, "default", "op complete --line 'node edit '")
                .contains(&format!("osc\t(signal:LFO) {uid}")),
            "a uid position completes from the graph");
    // A node reference is the uid OR the unique name — resolved server-side, in the one
    // namespace nd() reads, so every transport takes both and the endpoint's node half follows.
    ok(&url, "names", "node param edit osc output/sfreq --value 55");
    assert_eq!(
        g.doc()["nodes"][&uid]["params"]["output"]["sfreq"]["value"], 55.0,
        "the name reached the same node"
    );
    let why = client::exec(&url, &lines(&["node state nosuch"]), None).unwrap_err();
    assert!(why.contains("names no node"), "{why}");
    let batch = lines(&[
        "node add --type Buffer --name win",
        "node add --type Buffer --name sink",
    ]);
    assert_eq!(client::exec(&url, &batch, Some("shell_a")).unwrap().len(), 2);
    // Both halves of a wire spelled by name, made and taken back — the round-trip proves both
    // endpoint spellings resolve to the same wire.
    ok(&url, "names", "link add osc/out win/input");
    let gone: serde_json::Value =
        serde_json::from_str(&ok(&url, "names", "link remove osc/out win/input")).unwrap();
    assert_eq!(gone["removed"], true, "the named wire was there to remove");
    // Another actor's undo takes back ITS work, never shell_a's; bare shells share `default`.
    let d: serde_json::Value = serde_json::from_str(&ok(&url, "default", "node add --type Buffer")).unwrap();
    ok(&url, "default", "undo");
    assert!(g.doc()["nodes"][d["uid"].as_str().unwrap()].is_null(), "default undid its own");
    assert_eq!(g.doc()["nodes"].as_object().unwrap().len(), 3, "shell_a's three still stand");
    ok(&url, "shell_a", "undo");
    assert_eq!(
        g.doc()["nodes"].as_object().unwrap().len(), 1,
        "one undo took back the whole batch, in shell_a's stack"
    );

    // `--raw` round-trips: the entry's rendered form IS the NPY bytes, ready for a pipe.
    let npy = g.until("a frame to reach the raw read", |_| {
        let entries =
            client::exec(&url, &lines(&[&format!("node snapshot {uid}/out --raw")]), None).unwrap();
        Some(client::rendered(&entries[0])).filter(|bytes| bytes.starts_with(b"\x93NUMPY"))
    });
    let hlen = u16::from_le_bytes([npy[8], npy[9]]) as usize;
    assert!((npy.len() - 10 - hlen).is_multiple_of(4) && npy.len() > 10 + hlen, "a whole f32 payload");

    let _ = std::fs::remove_dir_all(&tmp);
}
