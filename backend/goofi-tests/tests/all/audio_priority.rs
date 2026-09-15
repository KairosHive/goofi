//! Linux real-time promotion must not impose a CPU limit when native permission exists.


use std::process::Command;
use std::time::{Duration, Instant};

fn limit(resource: libc::__rlimit_resource_t) -> libc::rlimit {
    let mut value = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    assert_eq!(unsafe { libc::getrlimit(resource, &mut value) }, 0);
    value
}

#[test]
fn native_audio_priority_keeps_the_process_cpu_limits() {
    const CHILD: &str = "GOOFI_TEST_NATIVE_AUDIO_PRIORITY";
    if std::env::var_os(CHILD).is_none() {
        if limit(libc::RLIMIT_RTPRIO).rlim_cur < 10 {
            eprintln!("skipped native promotion: this test needs RLIMIT_RTPRIO >= 10");
            return;
        }
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &format!("{}::native_audio_priority_keeps_the_process_cpu_limits", crate::situation(module_path!())), "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(result.status.success(), "priority child: {}\n{}\n{}", result.status,
            String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
        return;
    }

    // Resource limits are process-wide. A child keeps a failing implementation
    // from changing the test runner's limits or killing its other sessions.
    let before = limit(libc::RLIMIT_RTTIME);
    let handle = audio_thread_priority::promote_current_thread_to_real_time(1024, 48_000)
        .expect("native real-time permission is available");
    let after = limit(libc::RLIMIT_RTTIME);
    assert_eq!((after.rlim_cur, after.rlim_max), (before.rlim_cur, before.rlim_max));
    let policy = unsafe { libc::sched_getscheduler(0) };
    assert_eq!(policy & !libc::SCHED_RESET_ON_FORK, libc::SCHED_RR);
    assert_ne!(policy & libc::SCHED_RESET_ON_FORK, 0);
    let mut param = libc::sched_param { sched_priority: 0 };
    assert_eq!(unsafe { libc::sched_getparam(0, &mut param) }, 0);
    assert_eq!(param.sched_priority, 10);

    // More than one 21.3 ms device period, with no blocking call. Viewer work
    // must not turn this bounded burst into a process termination.
    if before.rlim_cur == libc::RLIM_INFINITY {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(40) {
            std::hint::spin_loop();
        }
    }
    audio_thread_priority::demote_current_thread_from_real_time(handle).unwrap();
    assert_eq!(unsafe { libc::sched_getscheduler(0) } & !libc::SCHED_RESET_ON_FORK, libc::SCHED_OTHER);
}
