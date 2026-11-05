//! `cargo run -p goofi-init` — the one setup step a fresh clone needs. Idempotent.
//! Each step is a spinner that becomes a `✓` line; the banner and the outcome close the run.

use std::time::{Duration, Instant};

use console::style;
use goofi_supervisor::progress::{self, Progress};
use goofi_supervisor::sync::Mutex;
use indicatif::{ProgressBar, ProgressStyle};

const BANNER: [&str; 6] = [
    " ██████╗  ██████╗  ██████╗ ███████╗██╗",
    "██╔════╝ ██╔═══██╗██╔═══██╗██╔════╝██║",
    "██║  ███╗██║   ██║██║   ██║█████╗  ██║",
    "██║   ██║██║   ██║██║   ██║██╔══╝  ██║",
    "╚██████╔╝╚██████╔╝╚██████╔╝██║     ██║",
    " ╚═════╝  ╚═════╝  ╚═════╝ ╚═╝     ╚═╝",
];

/// The step in progress: its name and its spinner.
static STEP: Mutex<Option<(String, ProgressBar)>> = Mutex::new(None);

fn main() {
    let _ = progress::set_sink(|p| match p {
        Progress::Report(m) => begin(m),
        Progress::Note(m) => note(&m),
        _ => {}
    });
    let started = Instant::now();
    let outcome = goofi_init::init(&goofi_init::repo_root());
    let step = end(outcome.is_ok());
    println!();
    match outcome {
        Ok(()) => {
            banner(|l| style(l).green().bold().to_string());
            println!("  {} {}", style("✓ Welcome to goofi.").green().bold(), style(format!("Set up in {:.1}s.", started.elapsed().as_secs_f64())).dim());
            println!("    `cargo run` starts goofi; `cargo build` and `cargo test` work too.");
            println!("    {}", style(goofi_supervisor::layout::runtime().versioned().join("python").display()).dim());
        }
        Err(e) => {
            banner(|l| style(l).red().bold().to_string());
            let mut lines = e.lines();
            let at = step.map(|s| format!(" at \"{s}\"")).unwrap_or_default();
            println!("  {}", style(format!("✗ goofi-init failed{at}.")).red().bold());
            println!("    {}", style(lines.next().unwrap_or_default()).red());
            for line in lines {
                println!("    {}", style(line).dim());
            }
            println!("    Fix the cause and run `cargo run -p goofi-init` again.");
            std::process::exit(1);
        }
    }
}

fn banner(paint: impl Fn(&str) -> String) {
    for line in BANNER {
        println!("  {}", paint(line));
    }
    println!();
}

/// A step begins: the previous one is done.
fn begin(message: String) {
    end(true);
    let bar = ProgressBar::new_spinner();
    bar.set_style(ProgressStyle::with_template("  {spinner:.cyan} {msg} {elapsed:.dim}").expect("a template"));
    bar.set_message(message.clone());
    bar.enable_steady_tick(Duration::from_millis(100));
    *STEP.lock() = Some((message, bar));
}

/// A fact under the current step, printed above its spinner.
fn note(message: &str) {
    let line = format!("    {} {}", style("·").dim(), style(message).dim());
    match STEP.lock().as_ref() {
        Some((_, bar)) => bar.suspend(|| println!("{line}")),
        None => println!("{line}"),
    }
}

/// Close the current step as passed or failed, and answer its name.
fn end(passed: bool) -> Option<String> {
    let (message, bar) = STEP.lock().take()?;
    bar.finish_and_clear();
    let mark = if passed { style("✓").green() } else { style("✗").red() };
    println!("  {mark} {message}");
    Some(message)
}
