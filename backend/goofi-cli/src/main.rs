//! `goofi` — the binary: serve by default, or be the CLIENT of a running server. Client mode
//! holds zero op knowledge: it resolves WHICH server, sends the line, prints the answer.

use std::future::Future;
use std::path::{Path, PathBuf};

use goofi_bridge::{serve_app, spawn_workers, AppState, HEADLESS_BUILD, SPA};
use goofi_cli::{exposure_warning, parse_args, Cli, DEFAULT_PORT, USAGE};
use goofi_core::startup::{report, Startup};
use goofi_node::{Isolation, Scanned};

fn headless_env() -> bool {
    matches!(std::env::var("GOOFI_HEADLESS").as_deref(), Ok("1") | Ok("true"))
}

fn debug_env() -> bool {
    matches!(std::env::var("GOOFI_DEBUG").as_deref(), Ok("1") | Ok("true"))
}

fn demo_env() -> bool {
    matches!(std::env::var("GOOFI_DEMO").as_deref(), Ok("1") | Ok("true"))
}

/// A set variable that is empty names nothing — a platform spells an unset variable that way.
fn named_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn main() {
    // Bare or flag-first argv serves — what `cargo run` depends on. A bare WORD is a command for
    // a running server, except the few the client itself owns (`ops::RESERVED`'s doors). The
    // client path is three blocking syscalls, so only the serve arm builds a runtime.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let rest = match argv.first().map(String::as_str) {
        None => argv,
        Some("help") | Some("--help") | Some("-h") => std::process::exit(help_main(&argv[1..])),
        Some("-") => std::process::exit(client_stdin(&argv[1..])),
        Some("serve") => argv[1..].to_vec(),
        // The binary is its own plugin scanner: a child per bundle, so a crash there is a
        // refusal here.
        Some("vst3-scan") => std::process::exit(goofi_audio::vst3::scan_main(&argv[1..])),
        // …and its own native node host: a node built after boot runs in a child of this binary.
        Some("host") => std::process::exit(goofi_signal::hosted::host_main(&argv[1..])),
        Some(first) if first.starts_with('-') => argv,
        Some(_) => std::process::exit(client_main(argv)),
    };
    let (windows, ui) = match goofi_window::Loop::open() {
        Ok((windows, ui)) => (Some(windows), Some(ui)),
        Err(_) => (None, None),
    };
    let serve = move || {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("the serve runtime")
            .block_on(serve_main(rest, ui))
    };
    // Where a display answers, the main thread is the window thread — a plugin's editor lives
    // there — and the server runs beside it; where none does, it serves as it always did.
    // Always a goofi thread, never the process's own: a main-thread stack is the PE header's on
    // Windows, and opening a service needs more than that.
    // A server that dies must end the process: the window loop below would otherwise outlive it.
    let served = goofi_transport::thread("goofi-serve")
        .spawn(|| {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(serve)).is_err() {
                std::process::exit(101);
            }
        })
        .expect("the serve thread");
    if let Some(windows) = windows {
        windows.run();
    }
    // The server decides the exit code and leaves through `process::exit`; the loop's end is
    // never the process's.
    served.join().expect("the serve thread");
}

async fn serve_main(rest: Vec<String>, ui: Option<goofi_window::Ui>) {
    let mut cli = match parse_args(rest.into_iter()) {
        Ok(cli) => cli,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    // The three doors meet here, once: a binary built headless has no app to serve at all.
    cli.headless |= headless_env() || HEADLESS_BUILD;
    cli.debug |= debug_env();
    cli.demo |= demo_env();
    if cli.help {
        // `goofi serve --help` / a flag mix that asked: the SERVE usage, not the op help door.
        println!(
            "{USAGE}\n\
             \n  \
             Scans every --extra-nodes ROOT — a folder of node files, `.py` and `.rs` — and then the \
             open patch's own workspace, which wins a shared type name. \
             Each node is routed in-process if free-threading-safe, else to a subprocess on \
             `{}`, which `cargo run -p goofi-init` provisions.\n  \
             GOOFI_HEADLESS=1 in the environment is --headless; setting it for the BUILD leaves \
             the app out of the binary entirely. GOOFI_DEBUG=1 is --debug, which opens `/dev/*` \
             — the UI primitive gallery and the other development surfaces. GOOFI_LOAD is --load, \
             the patch to open at start; on a demo it is what `session new` returns to, and \
             GOOFI_DEMO_BASE names where that set's other examples answer.",
            goofi_init::GIL_VENV
        );
        return;
    }
    let shutdown = watch_shutdown();
    let startup = Startup::begin(env!("CARGO_PKG_VERSION"));
    report("Checking the Python environment");
    let python = match default_subproc_python() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if !cli.list_nodes {
        if let Err(e) = goofi_core::log::capture_stdio() {
            eprintln!("Could not capture application output: {e}");
            std::process::exit(1);
        }
    }
    let mode = goofi_bridge::Mode { headless: cli.headless, demo: cli.demo };
    report("Cleaning up after earlier sessions");
    // The session is held BEFORE the engines exist: every iceoryx2 port they open is its.
    let session = goofi_transport::session().to_string();
    let swept = goofi_transport::swept_at_boot();
    goofi_core::startup::note(match (swept.directories, swept.segments) {
        (0, 0) => "nothing left behind".to_string(),
        (d, s) => format!("removed {d} directories and {s} shared-memory segments of dead sessions"),
    });
    report("Starting signal, audio and graphics engines");
    let mut state = AppState::with_instance(session, mode, goofi_bridge::Clock::Device, goofi_bridge::RenderClock::Timer);
    state.load = cli.load.clone().or_else(|| named_env("GOOFI_LOAD")).map(PathBuf::from);
    state.demo_base = named_env("GOOFI_DEMO_BASE");
    let window = ui.clone();
    let code = run(cli, python, state, async { let _ = shutdown.await; }, ui, Some(startup)).await;
    // The window loop ends once every plugin editor and window was unmade by the shutdown above.
    if let Some(window) = window {
        window.stop();
    }
    // Last, after every port is gone: the record, then the ephemeral directory and shared memory.
    // The PROCESS releases its session, never `run` — a test runs several servers in one.
    goofi_transport::release_session();
    std::process::exit(code);
}

/// Send lines to the resolved server and print each entry — decoded NPY bytes when the result
/// carries them, the rendered text otherwise, or the raw JSON under `--json`.
fn forward(lines: &[String], json: bool) -> i32 {
    let target = match goofi_client::resolve_target() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let actor = std::env::var("GOOFI_ACTOR").ok();
    match goofi_client::exec(&target.url, lines, actor.as_deref()) {
        Ok(entries) => {
            use std::io::Write;
            let mut out = std::io::stdout().lock();
            for e in &entries {
                let bytes = match json {
                    true => {
                        let mut b = serde_json::to_string_pretty(&e["result"])
                            .unwrap_or_default()
                            .into_bytes();
                        b.push(b'\n');
                        b
                    }
                    false => goofi_client::rendered(e),
                };
                let _ = out.write_all(&bytes);
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// Split off the client-consumed `--json`; everything else is the server's line, re-quoted by
/// the same word rules bash used to split it.
fn take_json(words: &mut Vec<String>) -> bool {
    let n = words.len();
    words.retain(|w| w != "--json");
    words.len() != n
}

fn client_main(mut words: Vec<String>) -> i32 {
    let json = take_json(&mut words);
    match (words.first().map(String::as_str), words.get(1).map(String::as_str)) {
        (Some("session"), Some("list")) => return print_sessions(json),
        (Some("completions"), shell) => return print_completions(shell),
        (Some("op"), Some("complete")) => return complete_line(&words[2..]),
        (Some("agent"), Some("term")) => {
            eprintln!("`agent term` is not built yet — the app's agent panel serves the terminal.");
            return 1;
        }
        (Some("plugin"), _) => {
            eprintln!("`plugin` is not built yet — the word is reserved for plugin ops.");
            return 1;
        }
        _ => {}
    }
    forward(&[shell_words::join(words.iter().map(String::as_str))], json)
}

/// The completion callback: a running server answers with its LIVE vocabulary (its node uids,
/// its types); with none, the compiled-in registry answers the static half — same fallback shape
/// as [`help_main`]. Quiet on every failure: a completion must never print an error into a
/// half-typed command line.
fn complete_line(rest: &[String]) -> i32 {
    let line = rest.first().map(String::as_str).unwrap_or_default();
    if let Ok(target) = goofi_client::resolve_target() {
        let cmd = shell_words::join(["op", "complete", line]);
        if let Ok(entries) = goofi_client::exec(&target.url, &[cmd], None) {
            if let Some(text) = entries.first().and_then(|e| e["text"].as_str()) {
                println!("{text}");
                return 0;
            }
        }
    }
    let ops = goofi_bridge::ops::table(goofi_bridge::Mode::default());
    for (word, doc) in goofi_bridge::phrase::complete(&ops, None, line) {
        println!("{word}\t{doc}");
    }
    0
}

/// `goofi completions zsh|bash` — the script that wires a shell's TAB to [`complete_line`]. The
/// script holds NO vocabulary: every keystroke asks `goofi op complete`, so completions are as
/// current as the server answering them.
fn print_completions(shell: Option<&str>) -> i32 {
    // zsh: `words` holds the current (partial) word last; joining keeps its emptiness, so the
    // callback can tell `node<TAB>` from `node <TAB>`. compinit is bootstrapped when the rc file
    // has not run it yet — `compdef` does not exist before it has.
    const ZSH: &str = r#"# goofi completion — add to ~/.zshrc:  eval "$(goofi completions zsh)"
_goofi() {
	local -a cands lines
	local line="${(j: :)${(@)words[2,$CURRENT]}}"
	lines=("${(@f)$("__GOOFI__" op complete "$line" 2>/dev/null)}")
	for l in "${lines[@]}"; do
		[[ -n "$l" ]] && cands+=("${l%%$'\t'*}:${l#*$'\t'}")
	done
	(( ${#cands} )) && _describe -V goofi cands
}
if ! typeset -f compdef >/dev/null; then
	autoload -Uz compinit
	compinit
fi
compdef _goofi goofi "__GOOFI__""#;
    const BASH: &str = r#"# goofi completion — add to ~/.bashrc:  eval "$(goofi completions bash)"
_goofi() {
	local line="${COMP_LINE#* }"
	[[ "$COMP_LINE" == *' '* ]] || line=""
	local IFS=$'\n'
	COMPREPLY=($("__GOOFI__" op complete "$line" 2>/dev/null | cut -f1))
}
complete -F _goofi goofi "__GOOFI__""#;
    // The script pins THIS binary's path: a dev shell has no `goofi` on PATH, and the eval
    // re-resolves on every shell start, so an installed binary pins its installed path.
    let me = std::env::current_exe()
        .ok()
        .and_then(|p| p.into_os_string().into_string().ok())
        .unwrap_or_else(|| "goofi".into());
    match shell {
        Some("zsh") => println!("{}", ZSH.replace("__GOOFI__", &me)),
        Some("bash") => println!("{}", BASH.replace("__GOOFI__", &me)),
        _ => {
            eprintln!(
                "usage: goofi completions zsh|bash — register with\n  eval \"$(goofi completions zsh)\"\nin the shell or its rc file"
            );
            return 2;
        }
    }
    0
}

/// `goofi -`: stdin lines as ONE batch — several ops, one undo step.
fn client_stdin(rest: &[String]) -> i32 {
    let mut rest = rest.to_vec();
    let json = take_json(&mut rest);
    if let Some(stray) = rest.first() {
        eprintln!("`goofi -` reads its commands from stdin — `{stray}` has no meaning here");
        return 2;
    }
    let lines: Vec<String> = std::io::stdin()
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.trim().is_empty())
        .collect();
    forward(&lines, json)
}

fn print_sessions(json: bool) -> i32 {
    let rows = goofi_client::list();
    let current = std::env::var("GOOFI_SESSION").ok();
    let current = |s: &goofi_core::session::Session| current.as_deref() == Some(&s.id);
    if json {
        let rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|s| serde_json::json!({ "id": s.id, "url": s.url, "current": current(s) }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        return 0;
    }
    if rows.is_empty() {
        println!("no running goofi — start one with `goofi`");
        return 0;
    }
    for s in rows {
        let mark = if current(&s) { "  ← GOOFI_SESSION" } else { "" };
        println!("{}  {}{mark}", s.id, s.url);
    }
    0
}

/// `goofi help [words…]`: any live session answers — help does not depend on which — and with
/// none, the COMPILED-IN registry answers through the same renderer, so there is one help text.
fn help_main(rest: &[String]) -> i32 {
    let mut rest = rest.to_vec();
    take_json(&mut rest); // help is text; the flag is not a word to look up
    let rows = goofi_client::list();
    let Some(live) = rows.first() else {
        let words: Vec<String> = std::iter::once("help".to_string()).chain(rest.clone()).collect();
        match goofi_bridge::phrase::help(&goofi_bridge::ops::table(goofi_bridge::Mode::default()), &words) {
            Some(h) => {
                println!("no running server — the built-in index answers; `goofi serve` starts one.");
                println!("{h}");
                return 0;
            }
            None => {
                eprintln!("nothing under `{}`", rest.join(" "));
                return 1;
            }
        }
    };
    let words: Vec<&str> =
        std::iter::once("help").chain(rest.iter().map(String::as_str)).collect();
    match goofi_client::exec(&live.url, &[shell_words::join(words)], None) {
        Ok(entries) => {
            for e in &entries {
                println!("{}", e["text"].as_str().unwrap_or_default());
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// The interpreter the subprocess tier runs on: the venv `goofi-init` made, and only that one.
fn default_subproc_python() -> Result<String, String> {
    goofi_init::venv_python(&goofi_init::repo_root().join(goofi_init::GIL_VENV))
        .map(|p| p.display().to_string())
        .ok_or_else(|| format!("no {} — {}", goofi_init::GIL_VENV, goofi_init::RUN_ME))
}

/// Everything the process does once it has a state, returning its exit code: `std::process::exit`
/// unwinds nothing, so the workspace mount is reclaimed here rather than by a destructor.
async fn run(
    cli: Cli,
    subproc_python: String,
    mut state: AppState,
    shutdown: impl Future<Output = ()>,
    ui: Option<goofi_window::Ui>,
    mut startup: Option<Startup>,
) -> i32 {
    // Before ANY use of the embedded interpreter.
    point_embedded_python_at_its_venv();

    let Cli { port, bind, extra_nodes, list_nodes, headless, debug, demo, load: _, help: _ } = cli;
    let port = port.unwrap_or(DEFAULT_PORT);

    report("Preparing plugins");
    if let Err(error) = goofi_bridge::plugins::Plugins::load(&mut state, &goofi_core::home::dir(), std::path::Path::new(&subproc_python)) {
        let _ = goofi_core::log::terminal_line(&format!("Could not load plugins: {error}"));
    }
    state.roots.extend(extra_nodes.iter().map(PathBuf::from));
    // Every root the scan reads, the private library included: a node saved there may name
    // packages exactly as a bundle's does.
    let scanned: Vec<PathBuf> = state.node_roots().into_iter().map(|(d, _)| d).collect();
    report("Checking node package requirements");
    let ready = ensure_packages(&scanned, &subproc_python).and_then(|()| {
        if list_nodes {
            return Ok(());
        }
        report("Preparing parameter expressions");
        register_evaluator(&state)
    });
    if ready.is_ok() {
        // Handed to the engine before anything scans, so the boot scan and every rescan share it.
        goofi_bridge::signal_engine(&mut state.graph.lock().unwrap())
            .set_python(goofi_signal::Python::new(subproc_python.clone()));
        if let Some(graphics) = goofi_bridge::try_graphics_engine(&mut state.graph.lock().unwrap()) {
            graphics.set_python(goofi_signal::Python::new(subproc_python.clone()));
        }
        {
            let mut g = state.graph.lock().unwrap();
            // This binary is its own node host and its own plugin scanner.
            if let Ok(own) = std::env::current_exe() {
                goofi_bridge::signal_engine(&mut g).set_host(own);
            }
            if !demo {
                let audio = goofi_bridge::audio_engine(&mut g);
                if let Ok(own) = std::env::current_exe() {
                    audio.set_vst3(own, goofi_audio::vst3::platform_dirs());
                }
                audio.set_ui(ui.clone());
            }
            // One screen for both engines: a plugin's editor and a `Window` node are the same thread.
            if let Some(graphics) = goofi_bridge::try_graphics_engine(&mut g) {
                graphics.set_ui(ui);
            }
        }
        boot_scan(&state);
        if !demo {
            report("Checking audio hosts");
            goofi_core::startup::note(format!("{}{}", goofi_audio::hosts(), goofi_audio::NO_ASIO_NOTE));
        }
    }

    let code = if let Err(error) = ready {
        let _ = goofi_core::log::terminal_line(&format!("Startup failed: {error}"));
        1
    } else if list_nodes {
        let names = goofi_bridge::catalog_type_names(&state.graph.lock().unwrap());
        if let Some(startup) = startup.take() {
            startup.finish("Node library ready");
        }
        println!("{} node types: {}", names.len(), names.join(", "));
        0
    // An arm of this chain rather than an early `return`: only the tail of this function gives
    // the workspace mount back.
    } else if !headless && SPA.is_empty() {
        let _ = goofi_core::log::terminal_line("refusing to start: no app is compiled into this binary.");
        let _ = goofi_core::log::terminal_line(
            "  The app is compiled in, so building it is not enough — build it, then rebuild \
             goofi:"
        );
        let _ = goofi_core::log::terminal_line("    npm install && npm run build   (in frontend/)");
        let _ = goofi_core::log::terminal_line("    cargo build");
        let _ = goofi_core::log::terminal_line("  Or serve the API alone: --headless, or GOOFI_HEADLESS=1.");
        1
    } else if let Err(e) = {
        if let Some(patch) = &state.load {
            report(format!("Opening patch {}", patch.display()));
        }
        goofi_bridge::open_load(&state)
    } {
        let _ = goofi_core::log::terminal_line("refusing to start: the patch --load named did not open.");
        let _ = goofi_core::log::terminal_line(&format!("  {e}"));
        1
    } else {
        report(format!("Starting services on {bind}:{port}"));
        spawn_workers(&state);
        match tokio::net::TcpListener::bind((bind.as_str(), port)).await {
            Err(e) => {
                let _ = goofi_core::log::terminal_line(&format!("failed to bind {bind}:{port}: {e}"));
                let _ = goofi_core::log::terminal_line("  A goofi that already runs holds it: `goofi session list` names them, and `--port` picks another.");
                1
            }
            Ok(listener) => {
                let addr = listener.local_addr().unwrap();
                // The session file needs the REAL address: `--port 0` makes it knowable
                // nowhere else.
                state.set_bound(addr);
                // Only a real server writes into the home: its record, and the config seed.
                goofi_core::home::seed_config();
                goofi_transport::record_url(&state.local_url());
                // The OPENABLE spelling, as the session file records it — `http://0.0.0.0` is
                // not an address a browser can visit.
                let url = state.local_url();
                if let Some(startup) = startup.take() {
                    startup.finish("Ready");
                }
                let _ = goofi_core::log::print_url(&url);
                if !demo {
                    println!("  MCP endpoint → {url}/mcp");
                }
                let spa = if headless { &[][..] } else { SPA };
                if headless {
                    println!("  headless: the API only, no app served");
                } else {
                    println!("  open {url} to use it");
                }
                if demo {
                    println!("  demo: no terminal, no agents, no filesystem, no audio");
                }
                if let Some(patch) = &state.load {
                    println!("  opened {}", patch.display());
                }
                if debug && !headless {
                    println!("  debug: {url}/dev/ui is open — the UI primitive gallery");
                }
                println!("  Ctrl+C to stop\n");
                // Last, and on stderr, so it is the line still on screen and survives a `> log`.
                if let Some(warning) = exposure_warning(&bind).filter(|_| !demo) {
                    let _ = goofi_core::log::terminal_line(&warning);
                }
                // The stop is here, not in `serve_app`, whose other callers serve forever.
                tokio::select! {
                    served = serve_app(listener, state.clone(), spa, debug) => match served {
                        Ok(()) => 0,
                        Err(e) => {
                            let _ = goofi_core::log::terminal_line(&format!("server error: {e}"));
                            1
                        }
                    },
                    _ = shutdown => 0,
                }
            }
        }
    };
    drop(startup);
    let _ = goofi_core::log::terminal_line("  Stopping engines · press Ctrl+C again to force exit");
    if state.recorder.running() {
        let _ = goofi_core::log::terminal_line("  Draining recording · waiting for queued frames to reach disk");
    }
    // The manager releases what it holds, in its one order; the window loop is the process's.
    state.shutdown();
    let _ = goofi_core::log::terminal_line("  Stopped");
    code
}

fn watch_shutdown() -> tokio::sync::oneshot::Receiver<()> {
    let (stop, stopped) = tokio::sync::oneshot::channel();
    goofi_transport::thread("goofi-signals").spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("the signal runtime")
            .block_on(async {
                shutdown_signal().await;
                let _ = stop.send(());
                shutdown_signal().await;
                // Every child goofi spawned watches its liveness pipe, which this exit closes.
                std::process::exit(130);
            });
    }).expect("the signal thread");
    stopped
}

/// Resolve on the first request to stop. A door that cannot be installed must **never** resolve —
/// an immediately-ready arm would shut the server down at startup.
async fn shutdown_signal() {
    let interrupt = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        _ = interrupt => {}
        _ = managed_stop() => {}
    }
}

/// The stop a service manager sends, which ctrl-C does not cover — `SIGTERM` where signals exist.
#[cfg(unix)]
async fn managed_stop() {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut sig) => {
            sig.recv().await;
        }
        Err(e) => {
            eprintln!("SIGTERM handler unavailable: {e}");
            std::future::pending::<()>().await;
        }
    }
}

/// Windows has no SIGTERM: the console closing and the machine going down stand in for it.
#[cfg(windows)]
async fn managed_stop() {
    let doors = (tokio::signal::windows::ctrl_close(), tokio::signal::windows::ctrl_shutdown());
    let (mut close, mut shutdown) = match doors {
        (Ok(close), Ok(shutdown)) => (close, shutdown),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("Windows shutdown handlers unavailable: {e}");
            return std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        _ = close.recv() => {}
        _ = shutdown.recv() => {}
    }
}

/// Install the pyo3 param-expression evaluator into the graph.
#[cfg(feature = "python")]
fn register_evaluator(state: &AppState) -> Result<(), String> {
    let ev = goofi_python::inproc::PyExprEvaluator::new()
        .map_err(|e| format!("param-expression evaluator unavailable: {e}"))?;
    state.graph.lock().unwrap().set_evaluator(std::sync::Arc::new(ev));
    println!("  param-expression evaluator ready (free-threaded Python)");
    Ok(())
}

/// Hand the EMBEDDED interpreter the venv pyo3 was linked against: pyo3 links `libpython` from
/// that venv's BASE install, so the venv's own site-packages is on no search path.
#[cfg(feature = "python")]
fn point_embedded_python_at_its_venv() {
    // An existing value is the documented override.
    if std::env::var_os("PYTHONPATH").is_some() {
        return;
    }
    let Some(python) = goofi_python::inproc::interpreter_path() else { return };
    let Some(venv) = Path::new(&python).parent().and_then(Path::parent) else { return };
    if let Some(dir) = goofi_init::site_packages(venv) {
        std::env::set_var("PYTHONPATH", dir);
    }
}

#[cfg(not(feature = "python"))]
fn point_embedded_python_at_its_venv() {}

/// Every node directory's requirements, checked against the interpreter each is asked of before the
/// scan imports anything. Startup requires a successful check and all required packages.
/// A terminal can approve installation; a failed or declined installation stops startup.
#[cfg(feature = "python")]
fn ensure_packages(dirs: &[PathBuf], subproc_python: &str) -> Result<(), String> {
    use std::io::IsTerminal;
    let shared = goofi_init::requirements_in(dirs);
    let gil_only: Vec<PathBuf> =
        shared.iter().cloned().chain(goofi_init::gil_requirements_in(dirs)).collect();
    if gil_only.is_empty() {
        return Ok(());
    }
    let root = goofi_init::repo_root();
    let interpreters = [
        (goofi_init::venv_python(&root.join(goofi_init::FT_VENV)), &shared),
        (Some(PathBuf::from(subproc_python)), &gil_only),
    ];
    let mut lacking = Vec::new();
    for (py, reqs) in interpreters {
        if reqs.is_empty() {
            continue;
        }
        let py = py.ok_or_else(|| format!("missing Python environment; {}", goofi_init::RUN_ME))?;
        let shown = py.strip_prefix(&root).unwrap_or(&py).display().to_string();
        match goofi_init::missing_packages(&py, reqs) {
            Ok(missing) if missing.is_empty() => {}
            Ok(missing) => {
                let _ = goofi_core::log::terminal_line(&format!("  {shown} lacks {}", missing.join(", ")));
                lacking.push((py, reqs.clone()));
            }
            Err(e) => return Err(format!("could not check {shown}: {e}")),
        }
    }
    if lacking.is_empty() {
        return Ok(());
    }
    let _ = goofi_core::log::terminal_line("  Requirements files:");
    for path in &gil_only {
        let _ = goofi_core::log::terminal_line(&format!("    {}", path.display()));
    }
    if !std::io::stdin().is_terminal() {
        return Err(format!("required Python packages are missing and no terminal can approve installation; {}", goofi_init::RUN_ME));
    }
    let _ = goofi_core::log::terminal_line("  Install missing packages now? [y/N]");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).map_err(|e| format!("could not read installation approval: {e}"))?;
    if !answer.trim().eq_ignore_ascii_case("y") {
        return Err(format!("required Python packages were not installed; {}", goofi_init::RUN_ME));
    }
    for (py, reqs) in lacking {
        goofi_init::install_packages(&py, &reqs)?;
    }
    Ok(())
}

#[cfg(not(feature = "python"))]
fn ensure_packages(_dirs: &[PathBuf], _subproc_python: &str) -> Result<(), String> { Ok(()) }

#[cfg(not(feature = "python"))]
fn register_evaluator(_state: &AppState) -> Result<(), String> {
    println!("  param expressions DISABLED — rebuild with `--features python` to enable the evaluator");
    Ok(())
}

/// One boot registration, reported — the boot registry starts empty, so a replacement here can
/// only be two files claiming one name.
fn note_replaced(name: &str, replaced: bool) {
    if replaced {
        eprintln!("warning: two node files claim the type name `{name}`; the later one wins");
    }
}

#[cfg(feature = "python")]
const NO_PYTHON_NOTE: &str = "";
#[cfg(not(feature = "python"))]
const NO_PYTHON_NOTE: &str = " (embedded Python disabled)";

/// The boot scan, reported. It runs the bridge's own `rescan`, so the baseline the first refresh
/// diffs against IS this scan.
fn boot_scan(state: &AppState) {
    report("Preparing native nodes (cached builds are reused)");
    goofi_bridge::prebuild(state, &state.mount());
    report("Indexing the node library");
    let found = {
        let mut g = state.graph.lock().unwrap();
        let patch = state.mount();
        let found = goofi_bridge::rescan(state, &mut g, &patch).1;
        g.boot_done();
        found
    };
    let (mut n_native, mut n_in, mut n_sub, mut n_shader, mut n_bad) = (0u32, 0u32, 0u32, 0u32, 0u32);
    for t in found {
        match t.outcome {
            Scanned::Registered { isolation, replaced } => {
                note_replaced(&t.type_name, replaced);
                match isolation {
                    // Nothing at boot is hosted: a hosted node is one authored later.
                    Isolation::Native | Isolation::Hosted => n_native += 1,
                    Isolation::InProcess => n_in += 1,
                    Isolation::Subprocess => n_sub += 1,
                    Isolation::Shader => n_shader += 1,
                }
            }
            Scanned::Unavailable(reason) => {
                eprintln!("  node `{}` unavailable: {reason}", t.type_name);
                n_bad += 1;
            }
        }
    }
    let bad = if n_bad > 0 { format!(", {n_bad} unavailable") } else { String::new() };
    let total = n_native + n_in + n_sub + n_shader;
    goofi_core::startup::note(format!("Node library: {total} available{bad}"));
    goofi_core::startup::note(format!("{n_native} native · {n_in} in-process · {n_sub} subprocess · {n_shader} shaders{NO_PYTHON_NOTE}"));
}
