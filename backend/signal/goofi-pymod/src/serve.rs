//! `goofi.serve()` — the subprocess child loop (wheel only; `extension-module`): read the node
//! source and the exchange base from the environment, run the node over the same [`crate::exec`]
//! seam the in-process tier uses, and answer `[entry][now]` calls over iceoryx2.

use std::collections::HashSet;
use std::time::Duration;

use goofi_codec::rpc::{decode_request, encode_error_response, encode_options_response, encode_response, split_call, Emitted, Entry, Request};
use goofi_core::SrcDtype;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::exec::SlotIn;
use crate::loader::{find_node_class, module_from_source};


/// The subprocess entry point (`import goofi; goofi.serve()`); returns only on a fatal error.
#[pyfunction]
pub fn serve(py: Python<'_>) -> PyResult<()> {
    // FIRST, before the user module is even compiled, so a child orphaned during a slow import
    // still stops instead of reaching the poll loop.
    goofi_supervisor::child::watch_parent()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("parent-liveness watcher: {e}")))?;

    // The source comes on stdin rather than in the environment, which Windows caps as a block.
    let mut source = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut source)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("no node source on stdin: {e}")))?;
    let base = env("GOOFI_IOX_BASE")?;

    let module = module_from_source(py, "goofi_node_main", &source)?;
    let instance = find_node_class(py, &module)?.call0()?;
    let out_slots = slot_names(&instance, "OUTPUTS")?;
    let in_slots: Vec<(String, bool)> =
        crate::introspect::slots(&instance.getattr("INPUTS")?)?.into_iter().map(|s| (s.name, s.multi)).collect();
    let out_refs: Vec<&str> = out_slots.iter().map(|s| s.as_str()).collect();

    run_loop(py, &instance, &in_slots, &out_refs, &base).map_err(pyo3::exceptions::PyRuntimeError::new_err)
}

/// The slot names one declaration constant holds, in declaration order.
fn slot_names(instance: &Bound<'_, PyAny>, constant: &str) -> PyResult<Vec<String>> {
    instance.getattr(constant)?.cast::<PyDict>()?.iter().map(|(k, _)| k.extract()).collect()
}

/// Read a required env var, or a clean Python error naming it.
fn env(key: &str) -> PyResult<String> {
    std::env::var(key).map_err(|_| pyo3::exceptions::PyRuntimeError::new_err(format!("{key} unset")))
}

/// How long one wait for a call lasts; the parent's liveness is watched by its own thread.
const SLICE: Duration = Duration::from_millis(100);

/// Open the child's end of the exchange and answer calls until the parent says stop.
fn run_loop(
    py: Python<'_>,
    instance: &Bound<'_, PyAny>,
    in_slots: &[(String, bool)],
    out_slots: &[&str],
    base: &str,
) -> Result<(), String> {
    // The parent's session, joined through `GOOFI_SESSION`: the same root, prefix and limits.
    let mut served = goofi_transport::Served::open(&goofi_transport::Iox::from_env()?, base)?;
    let mut warned: HashSet<SrcDtype> = HashSet::new();
    // The live params, seeded by the setup and moved one at a time: a tick carries none.
    let mut params = crate::exec::Groups::new();
    loop {
        // DETACHED: holding the GIL over the wait would starve a node's own Python threads, and
        // a receiver thread started in `setup()` is this tier's canonical shape.
        let Some((seq, body)) = py.detach(|| served.wait(SLICE))? else { continue };
        let (entry, now, request) = split_call(&body)?;
        let resp = handle(py, instance, in_slots, out_slots, &mut warned, &mut params, entry, now, request)
            .map_err(|e| format!("node process: {e}"))?;
        served.answer(seq, &resp)?;
        if entry == Entry::Stop {
            return Ok(());
        }
    }
}

/// Decode one call → run the node → encode the response. A MALFORMED request is fatal; a node
/// raise is a per-call error response, which the parent surfaces without respawning the child.
#[allow(clippy::too_many_arguments)]
fn handle(
    py: Python<'_>,
    instance: &Bound<'_, PyAny>,
    in_slots: &[(String, bool)],
    out_slots: &[&str],
    warned: &mut HashSet<SrcDtype>,
    params: &mut crate::exec::Groups,
    entry: Entry,
    now: f64,
    body: &[u8],
) -> PyResult<Vec<u8>> {
    if entry == Entry::Stop {
        crate::exec::run_stop(instance);
        return Ok(response(&[], &[]));
    }
    let arrived = match decode_request(&[body]).map_err(pyo3::exceptions::PyValueError::new_err)? {
        // Setup is a call of its own, with the params it seeds; a raise is the engine's to retry.
        Request::Setup { params: seeded } => {
            *params = seeded;
            return Ok(match crate::exec::run_setup(py, instance, params, now) {
                Ok(()) => response(&[], &[]),
                Err(e) => encode_error_response(&e.to_string()),
            });
        }
        Request::Param { group, name, value } => {
            params.entry(group).or_default().insert(name, value);
            return Ok(response(&[], &[]));
        }
        Request::Process { slots } => slots,
        Request::Refresh { group, name } => {
            let options = crate::exec::run_refresh(py, instance, params, &group, &name);
            return Ok(encode_options_response(&options).unwrap_or_else(|e| encode_error_response(&e.to_string())));
        }
        Request::Pulse { group, name } => {
            return Ok(match crate::exec::run_pulse(py, instance, params, &group, &name) {
                None => response(&[], &[]),
                Some(raised) => encode_error_response(&raised),
            });
        }
    };
    // The wire carries only the frames that arrived; widen it back to every declared slot — a
    // `multi` slot gathers every entry under its name in order, a single one takes the last.
    let inputs: Vec<(&str, SlotIn<'_>)> = in_slots
        .iter()
        .map(|(name, multi)| {
            let slot = if *multi {
                SlotIn::Multi(arrived.iter().filter(|(n, _, _)| n == name).map(|(_, s, d)| (s.as_str(), d)).collect())
            } else {
                SlotIn::Single(arrived.iter().rev().find(|(n, _, _)| n == name).map(|(_, _, d)| d))
            };
            (name.as_str(), slot)
        })
        .collect();
    match crate::exec::run_process(py, instance, params, &inputs, out_slots, warned, now) {
        Ok(result) => {
            let slots: Vec<(&str, Emitted<'_>)> = result.outputs.iter().map(|(n, d)| (n.as_str(), Emitted::Frame(d))).collect();
            Ok(response(&slots, &result.clear_inputs))
        }
        Err(e) => Ok(encode_error_response(&e.to_string())),
    }
}

fn response(outputs: &[(&str, Emitted<'_>)], clears: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    match encode_response(outputs, clears, &mut out) {
        Ok(()) => out,
        Err(e) => encode_error_response(&e.to_string()),
    }
}
