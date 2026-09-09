use std::io::{IsTerminal, Write};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

static ACTIVE: Mutex<Option<(String, Instant)>> = Mutex::new(None);

pub struct Startup {
    stop: mpsc::Sender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
    started: Instant,
}

fn line(mark: &str, message: &str) {
    let mut out = std::io::stdout().lock();
    let color = out.is_terminal() && std::env::var_os("NO_COLOR").is_none();
    if color {
        let _ = writeln!(out, "  \x1b[36m{mark}\x1b[0m {message}");
    } else {
        let _ = writeln!(out, "  {mark} {message}");
    }
    let _ = out.flush();
}

impl Startup {
    pub fn begin(version: &str) -> Self {
        let started = Instant::now();
        line("goofi", &format!("{version} · starting"));
        *ACTIVE.lock().unwrap() = Some(("Starting".into(), started));
        let (stop, receive) = mpsc::channel();
        let worker = std::thread::Builder::new().name("goofi-startup".into()).spawn(move || {
            while receive.recv_timeout(Duration::from_secs(5)) == Err(mpsc::RecvTimeoutError::Timeout) {
                let active = ACTIVE.lock().unwrap();
                if let Some((message, since)) = active.as_ref().filter(|(_, since)| since.elapsed() >= Duration::from_secs(5)) {
                    line("…", &format!("{message} · {:.0}s elapsed", since.elapsed().as_secs_f64()));
                }
            }
        }).ok();
        Self { stop, worker, started }
    }

    pub fn finish(self, message: &str) {
        let elapsed = self.started.elapsed();
        drop(self);
        line("✓", &format!("{message} in {:.1}s", elapsed.as_secs_f64()));
    }
}

impl Drop for Startup {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        *ACTIVE.lock().unwrap() = None;
    }
}

pub fn report(message: impl Into<String>) {
    let mut active = ACTIVE.lock().unwrap();
    if let Some(current) = active.as_mut() {
        let message = message.into();
        line("›", &message);
        *current = (message, Instant::now());
    }
}
