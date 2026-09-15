//! The one way goofi runs a process: a child joins the session, is listed while it lives, leaves
//! when asked and is killed when it will not, and never outlives the value that owns it.

use std::process::Command;
use std::time::{Duration, Instant};

use goofi_core::child;
use goofi_core::registry::{self, Kind};

/// Turns this binary into the sleeping child. Set, the test below runs and sleeps; unset — every
/// ordinary suite run — it does nothing.
const SLEEPER: &str = "GOOFI_TEST_SLEEPER";

#[test]
fn sleeper() {
    if std::env::var(SLEEPER).is_err() {
        return;
    }
    // As every goofi child does: end when the parent dies.
    child::watch_parent().expect("watch the parent");
    // Deaf to a polite stop, as an agent saving its state is, so the insist meets a LIVE child.
    #[cfg(unix)]
    // SAFETY: installing a signal disposition in a single-threaded test child.
    unsafe {
        libc::signal(libc::SIGTERM, libc::SIG_IGN);
    }
    println!("SLEEPING {}", std::env::var(goofi_core::session::ENV).unwrap_or_default());
    std::thread::sleep(Duration::from_secs(60));
}

fn sleeper_command() -> Command {
    let mut cmd = Command::new(std::env::current_exe().expect("this test binary"));
    cmd.args([&format!("{}::sleeper", crate::situation(module_path!())), "--exact", "--nocapture"]).env(SLEEPER, "1").stderr(std::process::Stdio::null());
    cmd
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    // SAFETY: signal 0 probes for existence and delivers nothing.
    return unsafe { libc::kill(pid as i32, 0) } == 0;
    #[cfg(not(unix))]
    return Command::new("tasklist").args(["/FI", &format!("PID eq {pid}")]).output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()));
}

/// Turns this binary into the intermediate parent: it spawns a sleeper through the child type,
/// names the grandchild's pid, and holds it until killed.
const INTERMEDIATE: &str = "GOOFI_TEST_INTERMEDIATE";

#[test]
fn intermediate() {
    if std::env::var(INTERMEDIATE).is_err() {
        return;
    }
    let grandchild = child::spawn("sleeper", &mut sleeper_command()).expect("spawn");
    println!("GRANDCHILD {}", grandchild.id());
    std::thread::sleep(Duration::from_secs(60));
}

/// The liveness pipe, end to end: a parent killed with no chance to clean up takes its child
/// with it, because the pipe's write end closes with the parent and the child exits on EOF.
#[cfg(unix)]
#[test]
fn a_hard_killed_parent_still_stops_its_child() {
    goofi_tests::walled_home();
    goofi_transport::session();
    let mut parent = Command::new(std::env::current_exe().expect("this test binary"))
        .args([&format!("{}::intermediate", crate::situation(module_path!())), "--exact", "--nocapture"])
        .env(INTERMEDIATE, "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the intermediate parent");
    let mut out = std::io::BufReader::new(parent.stdout.take().expect("piped"));
    let mut line = String::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !line.starts_with("GRANDCHILD") && Instant::now() < deadline {
        line.clear();
        std::io::BufRead::read_line(&mut out, &mut line).expect("read the parent");
    }
    let grandchild: u32 = line.trim().strip_prefix("GRANDCHILD ").and_then(|p| p.parse().ok()).expect("a pid");
    assert!(alive(grandchild));
    // SAFETY: SIGKILL to a child this test spawned and has not reaped.
    unsafe {
        libc::kill(parent.id() as i32, libc::SIGKILL);
    }
    let _ = parent.wait();
    let deadline = Instant::now() + Duration::from_secs(15);
    while alive(grandchild) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let orphaned = alive(grandchild);
    if orphaned {
        let _ = child::force_kill(grandchild);
    }
    assert!(!orphaned, "pid {grandchild} outlived its hard-killed parent: the liveness pipe did not fire");
}

#[test]
fn a_child_is_listed_while_it_lives_and_leaves_when_stopped() {
    goofi_tests::walled_home();
    goofi_transport::session();
    let listed = || registry::inventory().into_iter().filter(|e| e.kind == Kind::Child).map(|e| e.name).collect::<Vec<_>>();

    let mut cmd = sleeper_command();
    cmd.stdout(std::process::Stdio::piped());
    let mut child = child::spawn("sleeper", &mut cmd).expect("spawn");
    let pid = child.id();
    // Other situations in this binary spawn their own; this one's entry is the one checked.
    let mine = |n: &String| n.starts_with("sleeper") && n.contains(&format!("pid {pid})"));
    assert!(listed().iter().any(mine), "{:?}", listed());

    // It was told the session — the same one this process runs under.
    let mut line = String::new();
    let mut out = std::io::BufReader::new(child.stdout.take().expect("piped"));
    let deadline = Instant::now() + Duration::from_secs(30);
    while !line.starts_with("SLEEPING") && Instant::now() < deadline {
        line.clear();
        std::io::BufRead::read_line(&mut out, &mut line).expect("read the child");
    }
    assert_eq!(line.trim(), format!("SLEEPING {}", goofi_transport::session()));

    // Deaf to the ask; the stop closes the pipe first, which is what ends it, and insists after
    // the grace for a child that watches nothing. Either way it is gone when `stop` returns.
    let started = Instant::now();
    let _ended = child.stop(Duration::from_millis(300));
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(!alive(pid), "pid {pid} is gone");
    drop(child);
    assert!(!listed().iter().any(mine), "the entry went with the lease");
}

#[test]
fn a_dropped_child_does_not_outlive_its_owner() {
    goofi_tests::walled_home();
    goofi_transport::session();
    let pid = {
        let child = child::spawn("sleeper", &mut sleeper_command()).expect("spawn");
        child.id()
    };
    assert!(!alive(pid), "dropping the handle killed and reaped pid {pid}");
}

#[test]
fn a_tool_past_its_deadline_is_killed_and_reported() {
    goofi_tests::walled_home();
    goofi_transport::session();
    let started = Instant::now();
    let refused = child::output("sleeper", &mut sleeper_command(), Duration::from_millis(500));
    assert!(started.elapsed() < Duration::from_secs(10));
    let err = refused.expect_err("a tool that never finishes is refused");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut, "{err}");

    // One that finishes answers with its output, status and all.
    let mut quick = Command::new(std::env::current_exe().expect("this test binary"));
    quick.args(["--list", "--format", "terse"]);
    let out = child::output("list tests", &mut quick, Duration::from_secs(30)).expect("a quick tool");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("sleeper: test"), "{}", String::from_utf8_lossy(&out.stdout));
}
