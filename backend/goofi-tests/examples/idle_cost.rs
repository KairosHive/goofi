//! What a quiet patch costs the manager (not a test): the CPU each manager thread burns per
//! second on N sink nodes wired to one 1 Hz source, with and without an expression source on
//! every sink. Linux only: it reads `/proc/self/task`.
//!   cargo run -p goofi-tests --example idle_cost --release
use std::time::Duration;

use goofi_tests::{hex, j, Goofi};

#[cfg(target_os = "linux")]
fn cpu_by_thread() -> Vec<(String, String, f64)> {
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    let mut out = Vec::new();
    for entry in std::fs::read_dir("/proc/self/task").expect("procfs").flatten() {
        let dir = entry.path();
        let (Ok(comm), Ok(stat)) = (std::fs::read_to_string(dir.join("comm")), std::fs::read_to_string(dir.join("stat"))) else { continue };
        // The command is in parentheses and may hold spaces; the fields after it are fixed.
        let rest = &stat[stat.rfind(')').expect("a stat line") + 2..];
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let utime: f64 = fields[11].parse().unwrap_or(0.0);
        let stime: f64 = fields[12].parse().unwrap_or(0.0);
        out.push((entry.file_name().to_string_lossy().into_owned(), comm.trim().to_string(), (utime + stime) / ticks));
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn cpu_by_thread() -> Vec<(String, String, f64)> {
    Vec::new()
}

fn run(n: usize, bound: bool) {
    let g = Goofi::new();
    let src = g.add("_TestConst");
    g.call("node param edit", j!({ "node": hex(src), "param": "common/max_frequency", "value": 1.0 }));
    let src_name = g.name(&hex(src));
    let mut last = src;
    for _ in 0..n {
        let s = g.add("_TestSink");
        g.link(src, "out", s, "input");
        if bound {
            g.call(
                "node param edit",
                j!({ "node": hex(s), "param": "control/value", "expression": format!("nd('{src_name}').out"), "mode": "expression" }),
            );
        }
        last = s;
    }
    g.ready(last);
    std::thread::sleep(Duration::from_secs(2));
    let before = cpu_by_thread();
    let window = Duration::from_secs(10);
    std::thread::sleep(window);
    let after = cpu_by_thread();
    let mut rows: Vec<(String, f64)> = Vec::new();
    let mut total = 0.0;
    // By thread id, then summed by name: the node threads share one name.
    for (tid, name, now) in &after {
        let Some((_, _, was)) = before.iter().find(|(t, _, _)| t == tid) else { continue };
        let ms_per_s = (now - was) * 1000.0 / window.as_secs_f64();
        total += ms_per_s;
        let row = rows.iter_mut().find(|(n, _)| n == name);
        match row {
            Some((_, t)) => *t += ms_per_s,
            None => rows.push((name.clone(), ms_per_s)),
        }
    }
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("{n} sinks, {}: {total:.1} ms CPU per second over {} threads", if bound { "every sink bound by expression" } else { "no expressions" }, after.len());
    for (name, ms) in rows.iter().filter(|(_, ms)| *ms >= 0.5) {
        println!("    {name:<16} {ms:>7.1} ms/s");
    }
}

fn main() {
    if !cfg!(target_os = "linux") {
        println!("linux only");
        return;
    }
    for bound in [false, true] {
        run(200, bound);
    }
}
