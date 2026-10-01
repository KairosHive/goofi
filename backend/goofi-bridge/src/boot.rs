//! The one boot path: the binary and the harness fill a [`Config`], and [`boot`] answers with a
//! [`Manager`] whose drop is the shutdown.

use std::path::PathBuf;
use std::sync::Arc;

use goofi_node::{Isolation, Scanned};
use goofi_supervisor::progress::{note, report};

use crate::{AppState, Clock, Mode};

/// A check of every scanned root's package requirements, before the scan imports anything.
pub type Requirements = Box<dyn FnOnce(&[PathBuf]) -> Result<(), String>>;
/// A registration of the caller's own node types, before the scan.
pub type Register = Box<dyn FnOnce(&mut goofi_graph::Graph)>;

/// What a boot is asked for. A field left at its default asks for nothing.
pub struct Config {
    pub iox: Arc<goofi_transport::Iox>,
    /// The instance name; none mints a fresh id.
    pub instance: Option<String>,
    pub mode: Mode,
    pub clock: Clock,
    pub render: Clock,
    /// The subprocess tier's interpreter.
    pub python: Option<String>,
    /// The node host and plugin scanner: the binary itself, or the suite's stand-in.
    pub host: Option<PathBuf>,
    pub vst3_dirs: Vec<PathBuf>,
    pub ui: Option<goofi_window::Ui>,
    pub evaluator: Option<Arc<dyn goofi_node::ExprEvaluator>>,
    /// The plugins home; none loads no plugins.
    pub plugins: Option<PathBuf>,
    /// The `--extra-nodes` roots.
    pub roots: Vec<PathBuf>,
    pub load: Option<PathBuf>,
    pub demo_base: Option<String>,
    pub requirements: Option<Requirements>,
    pub register: Option<Register>,
}

impl Config {
    pub fn new(iox: Arc<goofi_transport::Iox>, mode: Mode, clock: Clock, render: Clock) -> Config {
        Config {
            iox,
            instance: None,
            mode,
            clock,
            render,
            python: None,
            host: None,
            vst3_dirs: Vec::new(),
            ui: None,
            evaluator: None,
            plugins: None,
            roots: Vec::new(),
            load: None,
            demo_base: None,
            requirements: None,
            register: None,
        }
    }
}

/// A booted goofi: the state, and the shutdown on drop, in the state's one order.
pub struct Manager {
    pub state: AppState,
}

impl std::ops::Deref for Manager {
    type Target = AppState;
    fn deref(&self) -> &AppState {
        &self.state
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        self.state.shutdown();
    }
}

/// Boot: the engines, the plugins, the requirements, the scan, then the workers. An error on the
/// way shuts down what was started.
pub fn boot(config: Config) -> Result<Manager, String> {
    let Config { iox, instance, mode, clock, render, python, host, vst3_dirs, ui, evaluator, plugins, roots, load, demo_base, requirements, register } = config;
    report("Starting signal, audio and graphics engines");
    let instance = match instance {
        Some(instance) => instance,
        None => goofi_supervisor::session::fresh_id()?,
    };
    let mut state = AppState::with_instance(iox, instance, mode, clock, render)?;
    state.load = load;
    state.demo_base = demo_base;
    let mut manager = Manager { state };
    if let (Some(home), Some(python)) = (&plugins, &python) {
        report("Preparing plugins");
        if let Err(error) = crate::plugins::Plugins::load(&mut manager.state, home, std::path::Path::new(python)) {
            let _ = goofi_supervisor::log::terminal_line(&format!("Could not load plugins: {error}"));
        }
    }
    manager.state.roots.extend(roots);
    if let Some(check) = requirements {
        report("Checking node package requirements");
        // Every root the scan reads, the private library included: a node saved there may name
        // packages exactly as a bundle's does.
        let scanned: Vec<PathBuf> = manager.state.node_roots().into_iter().map(|(d, _)| d).collect();
        check(&scanned)?;
    }
    {
        let mut g = manager.state.graph.lock();
        if let Some(evaluator) = evaluator {
            g.set_evaluator(evaluator);
        }
        if let Some(register) = register {
            register(&mut g);
        }
        // Handed to the engines before anything scans, so the boot scan and every rescan share it.
        if let Some(python) = python {
            crate::signal_engine(&mut g).set_python(goofi_signal::Python::new(python.clone()));
            if let Some(graphics) = crate::try_graphics_engine(&mut g) {
                graphics.set_python(goofi_signal::Python::new(python));
            }
        }
        if let Some(host) = &host {
            crate::signal_engine(&mut g).set_host(host.clone());
            if let Some(graphics) = crate::try_graphics_engine(&mut g) {
                graphics.set_host(host.clone());
            }
        }
        // A demo registers no audio engine, so there is nothing to hand it.
        if !mode.demo {
            let audio = crate::audio_engine(&mut g);
            if let Some(host) = host {
                audio.set_vst3(host, vst3_dirs);
            }
            audio.set_ui(ui.clone());
        }
        // One screen for both engines: a plugin's editor and a `Window` node are the same thread.
        if let Some(graphics) = crate::try_graphics_engine(&mut g) {
            graphics.set_ui(ui);
        }
    }
    let patch = manager.state.mount();
    report("Preparing native nodes (cached builds are reused)");
    crate::prebuild(&manager.state, &patch);
    report("Indexing the node library");
    let found = {
        let mut g = manager.state.graph.lock();
        let found = crate::rescan(&manager.state, &mut g, &patch).1;
        g.boot_done();
        found
    };
    let (mut n_native, mut n_in, mut n_sub, mut n_shader, mut n_bad) = (0u32, 0u32, 0u32, 0u32, 0u32);
    for t in found {
        match t.outcome {
            Scanned::Registered { isolation, replaced } => {
                // The boot registry starts empty, so a replacement is two files claiming one name.
                if replaced {
                    eprintln!("warning: two node files claim the type name `{}`; the later one wins", t.type_name);
                }
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
    note(format!("Node library: {total} available{bad}"));
    note(format!("{n_native} native · {n_in} in-process · {n_sub} subprocess · {n_shader} shaders"));
    crate::spawn_workers(&manager.state);
    Ok(manager)
}
