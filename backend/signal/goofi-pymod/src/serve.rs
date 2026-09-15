//! `goofi.serve()` — the subprocess child loop (wheel only; `extension-module`): read the node
//! source and service names from the environment, run the node over the same [`crate::exec`]
//! seam the in-process tier uses, and speak `goofi_codec` frames over iceoryx2.

use std::collections::HashSet;
use std::time::Duration;

use goofi_codec::{decode_request, encode_error_response, encode_options_response, encode_response, Request};
use goofi_core::{Data as CoreData, SrcDtype};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::exec::SlotIn;
use crate::loader::{find_node_class, module_from_source};


/// The subprocess entry point (`import goofi; goofi.serve()`); returns only on a fatal error.
#[pyfunction]
pub fn serve(py: Python<'_>) -> PyResult<()> {
    // FIRST, before the user module is even compiled, so a child orphaned during a slow import
    // still stops instead of reaching the poll loop.
    goofi_core::child::watch_parent()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("parent-liveness watcher: {e}")))?;

    // The source comes on stdin rather than in the environment, which Windows caps as a block.
    let mut source = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut source)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(format!("no node source on stdin: {e}")))?;
    let req_name = env("GOOFI_IOX_REQ")?;
    let resp_name = env("GOOFI_IOX_RESP")?;

    let module = module_from_source(py, "goofi_node_main", &source)?;
    let instance = find_node_class(py, &module)?.call0()?;
    let out_slots = slot_names(&instance, "OUTPUTS")?;
    let in_slots: Vec<(String, bool)> =
        crate::introspect::slots(&instance.getattr("INPUTS")?)?.into_iter().map(|s| (s.name, s.multi)).collect();
    let out_refs: Vec<&str> = out_slots.iter().map(|s| s.as_str()).collect();

    run_loop(py, &instance, &in_slots, &out_refs, &req_name, &resp_name)
        .map_err(pyo3::exceptions::PyRuntimeError::new_err)
}

/// The slot names one declaration constant holds, in declaration order.
fn slot_names(instance: &Bound<'_, PyAny>, constant: &str) -> PyResult<Vec<String>> {
    instance.getattr(constant)?.cast::<PyDict>()?.iter().map(|(k, _)| k.extract()).collect()
}

/// Read a required env var, or a clean Python error naming it.
fn env(key: &str) -> PyResult<String> {
    std::env::var(key).map_err(|_| pyo3::exceptions::PyRuntimeError::new_err(format!("{key} unset")))
}

/// Open the iceoryx2 ports (the mirror of the parent's) and run the request→process→response loop.
fn run_loop(
    py: Python<'_>,
    instance: &Bound<'_, PyAny>,
    in_slots: &[(String, bool)],
    out_slots: &[&str],
    req_name: &str,
    resp_name: &str,
) -> Result<(), String> {
    // The parent's session, joined through `GOOFI_SESSION`: the same root, prefix and limits.
    let mut served = goofi_transport::Served::open(req_name, resp_name)?;
    let mut warned: HashSet<SrcDtype> = HashSet::new();
    let mut did_setup = false;
    loop {
        let Some((seq, body)) = served.request()? else {
            // DETACHED: holding the GIL over the idle poll would starve a node's own Python
            // threads, and a receiver thread started in `setup()` is this tier's canonical shape.
            py.detach(|| std::thread::sleep(Duration::from_micros(500)));
            continue;
        };
        let resp = handle(py, instance, in_slots, out_slots, &mut warned, &mut did_setup, &body)
            .map_err(|e| format!("node process: {e}"))?;
        served.answer(seq, &resp)?;
    }
}

/// Decode one request → run the node → encode the response. A MALFORMED request is fatal; a node
/// raise is a per-tick error response, which the parent surfaces without respawning the child.
fn handle(
    py: Python<'_>,
    instance: &Bound<'_, PyAny>,
    in_slots: &[(String, bool)],
    out_slots: &[&str],
    warned: &mut HashSet<SrcDtype>,
    did_setup: &mut bool,
    body: &[u8],
) -> PyResult<Vec<u8>> {
    let (params, arrived) = match decode_request(body).map_err(pyo3::exceptions::PyValueError::new_err)? {
        Request::Process { params, slots } => (params, slots),
        Request::Refresh { params, group, name } => {
            return Ok(encode_options_response(&crate::exec::run_refresh(py, instance, &params, &group, &name)));
        }
        Request::Pulse { params, group, name } => {
            return Ok(match crate::exec::run_pulse(py, instance, &params, &group, &name) {
                None => encode_response(&[], &[]),
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
    match run_node(py, instance, &params, &inputs, out_slots, warned, did_setup) {
        Ok(result) => {
            let slots: Vec<(&str, &CoreData)> = result.outputs.iter().map(|(n, d)| (n.as_str(), d)).collect();
            Ok(encode_response(&slots, &result.clear_inputs))
        }
        Err(e) => Ok(encode_error_response(&e.to_string())),
    }
}

/// Run `setup()` until it SUCCEEDS, then `process()`; a setup that raised is retried on the next
/// request, and `process()` never runs after a failed one.
fn run_node(
    py: Python<'_>,
    instance: &Bound<'_, PyAny>,
    params: &crate::exec::Groups,
    inputs: &[(&str, SlotIn<'_>)],
    out_slots: &[&str],
    warned: &mut HashSet<SrcDtype>,
    did_setup: &mut bool,
) -> PyResult<goofi_codec::ProcessOutput> {
    if !*did_setup {
        crate::exec::run_setup(py, instance, params)?;
        *did_setup = true;
    }
    crate::exec::run_process(py, instance, params, inputs, out_slots, warned)
}
