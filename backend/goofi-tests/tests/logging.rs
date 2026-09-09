//! One console session, from component output to retained groups and counter-only updates.
use goofi_core::log::{self, Level, Log, Message, Source};
use goofi_tests::{Goofi, j};

#[test]
fn console_session_groups_interleaved_messages_and_keeps_ops_out_of_patch_history() {
    let g = Goofi::new();
    let component = format!("log-test-{}", g.state.instance_id);
    let before = g.doc();
    for text in ["first", "second", "first"] {
        g.call("log write", j!({ "component": component, "text": text, "level": "warning" }));
    }
    let listed = g.call("log list", j!({}));
    let groups: Vec<_> = listed["groups"].as_array().unwrap().iter()
        .filter(|row| row["component"] == component).collect();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["text"], "second");
    assert_eq!(groups[1]["text"], "first");
    assert_eq!(groups[1]["count"], 2);
    assert_eq!(groups[1]["level"], "warning");
    assert_eq!(g.doc(), before);
    assert!(g.refuse("log write", j!({ "text": "bad", "level": "typo" })).contains("Level"));

    let cursor = listed["cursor"].as_u64().unwrap();
    g.call("log write", j!({ "component": component, "text": "second", "level": "warning" }));
    let delta = serde_json::to_value(log::global().lock().unwrap().since(Some(cursor))).unwrap();
    let update = delta["groups"].as_array().unwrap().iter().find(|row| row["id"] == groups[0]["id"]).unwrap();
    assert_eq!(update["count"], 2);
    assert!(update.get("text").is_none(), "repeat packets do not carry text");
}

#[test]
fn byte_streams_preserve_split_lines_invalid_utf8_and_final_unterminated_text() {
    let source = Source::component("byte-stream-session");
    log::drain(std::io::Cursor::new(b"hello\r\n\xff\nlast"), source, "stderr");
    let snapshot = serde_json::to_value(log::global().lock().unwrap().since(None)).unwrap();
    let rows: Vec<_> = snapshot["groups"].as_array().unwrap().iter()
        .filter(|row| row["component"] == "byte-stream-session").collect();
    assert_eq!(rows.iter().map(|row| row["text"].as_str().unwrap()).collect::<Vec<_>>(), ["hello", "�", "last"]);
    assert!(rows.iter().all(|row| row["stream"] == "stderr" && row["level"] == "error"));
}

#[test]
fn bounded_log_evicts_the_least_recent_group() {
    let mut logs = Log::default();
    let message = |text: String| Message { source: Source::component("retention"), level: Level::Info, stream: None, text };
    for i in 0..log::MAX_GROUPS { logs.record(message(i.to_string())); }
    logs.record(message("0".into()));
    logs.record(message("new".into()));
    let snapshot = serde_json::to_value(logs.since(None)).unwrap();
    let groups = snapshot["groups"].as_array().unwrap();
    assert_eq!(groups.len(), log::MAX_GROUPS);
    assert_eq!(groups[0]["text"], "2");
    assert_eq!(groups[groups.len() - 2]["text"], "0");
    assert_eq!(groups[groups.len() - 2]["count"], 2);
}

#[test]
fn process_capture_child() {
    let Some(path) = std::env::var_os("GOOFI_LOG_CAPTURE_RESULT") else { return };
    log::capture_stdio().unwrap();
    println!("native stdout marker");
    eprintln!("native stderr marker");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let snapshot = serde_json::to_value(log::global().lock().unwrap().since(None)).unwrap();
        let rows = snapshot["groups"].as_array().unwrap();
        if ["native stdout marker", "native stderr marker"].iter()
            .all(|text| rows.iter().any(|row| row["text"] == *text)) {
            std::fs::write(path, snapshot.to_string()).unwrap();
            break;
        }
        assert!(std::time::Instant::now() < deadline, "capture did not drain both streams");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn native_process_output_reaches_logs_and_never_the_launch_shell() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("logs.json");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "process_capture_child", "--nocapture"])
        .env("GOOFI_LOG_CAPTURE_RESULT", &file).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("native stdout marker"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("native stderr marker"));
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    for (stream, text) in [("stdout", "native stdout marker"), ("stderr", "native stderr marker")] {
        assert!(saved["groups"].as_array().unwrap().iter().any(|row| row["stream"] == stream && row["text"] == text));
    }
}
