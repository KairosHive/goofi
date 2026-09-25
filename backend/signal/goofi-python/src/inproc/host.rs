use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use goofi_core::SrcDtype;
use goofi_node::{Isolation, IsolationCell, Params};
use goofi_host_sdk::{Inputs, Node, NodeCtx, NodeError, NodeResult, Outputs};
use pyo3::prelude::*;
use pyo3::types::PyModule;

use crate::attach;

/// A live `goofi.Node` subclass instance running in-process on the free-threaded interpreter.
pub struct PyNode {
    instance: Py<PyAny>,
    in_slots: Vec<(&'static str, bool)>,
    out_slots: Vec<&'static str>,
    /// Set when the GIL went on during one of THIS node's calls. A node that finds it already on
    /// did not cause it, and runs on, serialized with the rest.
    serialized: bool,
    /// Source dtypes already warned about (dedup for the ingest cast warning).
    cast_warned: HashSet<SrcDtype>,
    /// This node TYPE's tier. The tripwire writes it, and the next build reads it. `None` for a
    /// node built from a source string rather than discovered, which no registry routes.
    tier: Option<&'static IsolationCell>,
}

impl PyNode {
    /// Compile a node module from `source` and instantiate its `goofi.Node` subclass.
    pub fn from_source(
        source: &str,
        in_slots: Vec<(&'static str, bool)>,
        out_slots: Vec<&'static str>,
    ) -> PyResult<PyNode> {
        // Unique per instance: a shared name lets concurrent builds clobber each other's module.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let name = format!("goofi_user_{}", SEQ.fetch_add(1, Ordering::Relaxed));
        let (built, flipped) = watched(|py| -> PyResult<Py<PyAny>> {
            super::log::install(py)?;
            let module = goofi_pymod::loader::module_from_source(py, &name, source)?;
            let instance = goofi_pymod::loader::instantiate(py, &module)?;
            // The instance keeps its module alive through `__globals__`, so evicting the
            // `sys.modules` entry `from_code` inserted only bounds that map's growth.
            py.import("sys")?.getattr("modules")?.call_method1("pop", (&name, py.None()))?;
            Ok(instance.unbind())
        });
        let mut node =
            PyNode { instance: built?, in_slots, out_slots, serialized: false, cast_warned: HashSet::new(), tier: None };
        node.saw(flipped);
        Ok(node)
    }

    /// Let this node demote its own TYPE when the runtime GIL tripwire fires.
    pub fn routed_by(mut self, tier: &'static IsolationCell) -> PyNode {
        self.tier = Some(tier);
        self.saw(self.serialized);
        self
    }

    /// Whether the embedded interpreter currently has the GIL enabled.
    pub fn gil_enabled() -> PyResult<bool> {
        attach(gil_on)
    }

    /// The tripwire: a call that turned the GIL on marks this node, and demotes its TYPE so the
    /// next `restart_node` builds it in a subprocess.
    fn saw(&mut self, flipped: bool) {
        if !flipped {
            return;
        }
        self.serialized = true;
        if self.tier.is_some_and(|t| t.set(Isolation::Subprocess)) {
            goofi_core::log::record(goofi_core::log::source(), goofi_core::log::Level::Warning, None,
                "This node re-enabled the GIL. Restart it to move it to a subprocess.");
        }
    }
}

fn gil_on(py: Python<'_>) -> PyResult<bool> {
    PyModule::import(py, "sys")?.getattr("_is_gil_enabled")?.call0()?.extract()
}

/// The node calls running now, and how many have begun: a flip is a call's own when it ran alone.
static RUNNING: AtomicUsize = AtomicUsize::new(0);
static BEGUN: AtomicU64 = AtomicU64::new(0);

/// Run `f` attached, and say whether IT turned the GIL on: the flip came in this call, and this
/// thread printed CPython's notice of it, or no other call overlapped to have caused it instead.
fn watched<R>(f: impl FnOnce(Python<'_>) -> R) -> (R, bool) {
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            RUNNING.fetch_sub(1, Ordering::SeqCst);
        }
    }
    attach(|py| {
        let before = gil_on(py).unwrap_or(false);
        let begun = BEGUN.fetch_add(1, Ordering::SeqCst) + 1;
        let running = (RUNNING.fetch_add(1, Ordering::SeqCst) == 0, Running);
        let out = f(py);
        let alone = running.0 && BEGUN.load(Ordering::SeqCst) == begun;
        drop(running);
        let here = super::log::gil_warned_here();
        (out, !before && gil_on(py).unwrap_or(false) && (here || alone))
    })
}

/// Path to the free-threaded interpreter. `PYO3_PYTHON` comes first because, embedded,
/// `sys.executable` is the host binary rather than a python.
pub fn interpreter_path() -> Option<String> {
    if let Some(p) = option_env!("PYO3_PYTHON") {
        if !p.is_empty() {
            return Some(p.to_string());
        }
    }
    attach(|py| {
        PyModule::import(py, "sys").ok()?.getattr("executable").ok()?.extract::<String>().ok()
    })
}

impl Drop for PyNode {
    fn drop(&mut self) {
        attach(|py| goofi_pymod::exec::run_stop(self.instance.bind(py)));
        super::log::flush();
    }
}

impl Node for PyNode {
    fn setup(&mut self, _ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let (done, flipped) = watched(|py| goofi_pymod::exec::run_setup(py, self.instance.bind(py), p.groups()));
        self.saw(flipped);
        done.map_err(|e| NodeError(e.to_string()))
    }

    fn on_param_refreshed(&mut self, key: &goofi_node::ParamKey, p: &Params<'_>) -> Option<Vec<String>> {
        let (options, flipped) = watched(|py| {
            goofi_pymod::exec::run_refresh(py, self.instance.bind(py), p.groups(), &key.group, &key.name)
        });
        self.saw(flipped);
        options
    }

    fn on_pulse(&mut self, key: &goofi_node::ParamKey, p: &Params<'_>) -> NodeResult {
        let (raised, flipped) =
            watched(|py| goofi_pymod::exec::run_pulse(py, self.instance.bind(py), p.groups(), &key.group, &key.name));
        self.saw(flipped);
        raised.map_or(Ok(()), |e| Err(NodeError(e)))
    }

    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let inputs: Vec<(&str, goofi_pymod::exec::SlotIn<'_>)> = self
            .in_slots
            .iter()
            .map(|(name, multi)| {
                let slot = if *multi {
                    goofi_pymod::exec::SlotIn::Multi(inp.get_multi(name).iter().map(|(s, d)| (s.as_str(), d)).collect())
                } else {
                    goofi_pymod::exec::SlotIn::Single(inp.get(name))
                };
                (*name, slot)
            })
            .collect();

        let (outs, flipped) = watched(|py| {
            goofi_pymod::exec::run_process(py, self.instance.bind(py), p.groups(), &inputs, &self.out_slots, &mut self.cast_warned)
        });
        self.saw(flipped);
        let outs = outs.map_err(|e| e.to_string())?;
        if self.serialized {
            // Not latched: the condition is permanent, and the stats sweep samples state, so it
            // would miss a one-tick error.
            return Err("node re-enabled the GIL at runtime; restart it to move it to a subprocess".into());
        }
        for slot in outs.clear_inputs {
            ctx.clear_input(&slot);
        }
        for (slot, data) in outs.outputs {
            out.set(&slot, data);
        }
        Ok(())
    }
}
