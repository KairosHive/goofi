//! A folder plugin prepares recordings through the same operations as every client.
use goofi_tests::{j, require_python, Goofi};
use std::path::Path;

#[test]
fn a_folder_plugin_registers_ops_nodes_hooks_and_persistent_sessions() {
    let python = require_python();
    let home = tempfile::tempdir().unwrap();
    goofi_tests::fixtures::plugin_package(home.path());
    let mut goofi = Goofi::new();
    goofi_bridge::plugins::Plugins::load(&mut goofi.state, home.path(), Path::new(&python.py))
        .unwrap();
    let listing = goofi.call("plugin list", j!({}));
    assert!(
        listing["plugins"][0]["error"].is_null(),
        "{listing}; {:?}",
        goofi.call("log list", j!({}))
    );
    assert!(listing["plugins"][0]["frontend"]
        .as_str()
        .unwrap()
        .ends_with("index.js"));
    let ops = goofi.call("op list", j!({"doc": true}));
    assert!(ops["ops"]
        .as_array()
        .unwrap()
        .iter()
        .any(|op| op["op"] == "plugin example subject select" && op["args"] == "subject:string!"));
    let lines = vec!["plugin example subject select --subject Alice".into()];
    assert_eq!(
        goofi_bridge::phrase::exec_lines(&goofi.state, &lines, "operator").unwrap()[0]["subject"],
        "Alice"
    );
    assert_eq!(
        goofi.call("plugin example subject select", j!({"subject": " Alice "}))["subject"],
        "Alice"
    );
    let completion = goofi.call("op complete", j!({"line": "plugin example subject "}));
    assert!(completion["text"].as_str().unwrap().contains("select"));
    assert!(goofi
        .state
        .call(
            "plugin example subject select",
            j!({"subject": 9}),
            "operator"
        )
        .unwrap_err()
        .contains("str"));
    assert!(goofi
        .state
        .call("plugin example cycle", j!({}), "operator")
        .unwrap_err()
        .contains("recursive"));
    assert_eq!(goofi.call("plugin example host", j!({}))["running"], false);
    goofi.call("library refresh", j!({}));
    let node = goofi.call("node add", j!({"type": "signal:PluginNumber"}))["uid"]
        .as_str()
        .unwrap()
        .to_string();
    let echo = goofi.call("node add", j!({"type": "signal:PluginEcho"}))["uid"]
        .as_str()
        .unwrap()
        .to_string();
    goofi.call(
        "link add",
        j!({"from": format!("{node}/out"), "to": format!("{echo}/input")}),
    );
    let output = goofi.probe(goofi_tests::Uid::from_hex(&echo).unwrap(), "out");
    let frame = output.expect_frame(
        &mut goofi.state.graph.lock().unwrap(),
        "bundled Rust source through bundled Python node",
    );
    assert_eq!(goofi_tests::f32s(&frame), vec![7.0]);
    let before = goofi.nodes();
    let error = goofi
        .state
        .call(
            "compound",
            j!({"ops": [
                {"op": "node add", "payload": {"type": "signal:PluginNumber"}},
                {"op": "session state", "payload": {}}
            ]}),
            "operator",
        )
        .unwrap_err();
    assert!(error.contains("must run alone"), "{error}");
    assert_eq!(goofi.nodes(), before);
    goofi.call("record arm", j!({"output": format!("{node}/out")}));
    goofi.call("plugin example mode", j!({"mode": "reject"}));
    assert!(goofi
        .state
        .call("record start", j!({}), "operator")
        .unwrap_err()
        .contains("select a session"));
    assert_eq!(goofi.call("record status", j!({}))["running"], false);
    goofi.call("plugin example mode", j!({"mode": "invalid"}));
    assert!(goofi
        .state
        .call("record start", j!({}), "operator")
        .unwrap_err()
        .contains("requires string"));
    goofi.call("plugin example mode", j!({"mode": "normal"}));
    let started = goofi.call("record start", j!({"root": home.path().join("ignored")}));
    let folder = Path::new(started["folder"].as_str().unwrap());
    assert!(folder.starts_with(home.path().join("plugin-data/example/recordings/Alice")));
    let annotation: serde_json::Value = serde_json::from_slice(
        &std::fs::read(folder.join("annotations/example/session.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(annotation["subject"], "Alice");
    goofi.call("plugin example subject select", j!({"subject": "Bob"}));
    goofi.call("plugin example mode", j!({"mode": "post-fail"}));
    goofi.call("record stop", j!({}));
    assert_eq!(goofi.call("record status", j!({}))["running"], false);
    assert_eq!(
        goofi.call("plugin example status", j!({}))["completed"][0],
        started["folder"]
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("manifest.json")).unwrap()).unwrap();
    assert!(manifest["stopped_utc"].is_string());
    assert_eq!(annotation["subject"], "Alice");
    drop(goofi);
    assert!(home.path().join("plugin-data/example/stopped").is_file());
    let mut reopened = Goofi::new();
    goofi_bridge::plugins::Plugins::load(&mut reopened.state, home.path(), Path::new(&python.py))
        .unwrap();
    assert_eq!(
        reopened.call("plugin example status", j!({}))["subject"],
        "Bob"
    );
    assert!(reopened
        .state
        .call("plugin example crash", j!({}), "operator")
        .unwrap_err()
        .contains("stopped"));
    assert!(reopened.call("plugin list", j!({}))["plugins"][0]["error"].is_string());
    assert_eq!(reopened.call("record status", j!({}))["running"], false);
}

#[test]
fn conflicting_hooks_and_invalid_packages_leave_the_recorder_idle() {
    let python = require_python();
    let home = tempfile::tempdir().unwrap();
    goofi_tests::fixtures::plugin_package(home.path());
    let second = home.path().join("plugins/other");
    std::fs::create_dir_all(second.join("backend")).unwrap();
    std::fs::write(
        second.join("goofi-plugin.toml"),
        "id = \"other\"\nversion = \"0.1.0\"\napi = 1\n",
    )
    .unwrap();
    std::fs::write(second.join("backend/plugin.py"), "from goofi_plugin import plugin\n@plugin.pre_op('record start')\nasync def prepare(ctx, call):\n    return call.patch(root=str(ctx.data_dir))\n").unwrap();
    let bad = home.path().join("plugins/broken");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("goofi-plugin.toml"), "invalid TOML").unwrap();
    let mut goofi = Goofi::new();
    goofi_bridge::plugins::Plugins::load(&mut goofi.state, home.path(), Path::new(&python.py))
        .unwrap();
    let listing = goofi.call("plugin list", j!({}));
    assert_eq!(listing["plugins"].as_array().unwrap().len(), 3);
    assert!(listing["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == "broken" && p["error"].is_string()));
    let error = goofi
        .state
        .call("record start", j!({}), "operator")
        .unwrap_err();
    assert!(error.contains("pre_op conflict"), "{error}");
    assert_eq!(goofi.call("record status", j!({}))["running"], false);
}
