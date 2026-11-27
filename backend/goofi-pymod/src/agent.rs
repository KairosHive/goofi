//! `goofi.agent` — the session's harness plane from a node, on either tier: the agents the
//! config offers, the instances running, and the spawn, the entry and the stop of one.

use std::sync::OnceLock;

use goofi_agent::{Agents, Launch};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

fn plane() -> PyResult<&'static Agents> {
    static PLANE: OnceLock<Result<Agents, String>> = OnceLock::new();
    PLANE.get_or_init(Agents::current).as_ref().map_err(|e| PyRuntimeError::new_err(e.clone()))
}

fn refused(e: String) -> PyErr {
    PyRuntimeError::new_err(e)
}

/// The agents the config offers to launch: `[{name, command, model, effort}]`, each flag a
/// template with `{}` for the value, `None` where the harness takes no such option.
#[pyfunction]
fn agents(py: Python<'_>) -> PyResult<Py<PyAny>> {
    let (offered, _) = Agents::offered();
    pythonize::pythonize(py, &offered).map(Bound::unbind).map_err(|e| refused(e.to_string()))
}

/// Every instance of the session, in spawn order: `[{id, harness, model, effort, state,
/// exit_code, pid, owner, port, born}]`.
#[pyfunction]
fn roster(py: Python<'_>) -> PyResult<Py<PyAny>> {
    let plane = plane()?;
    let records = py.detach(|| plane.roster());
    pythonize::pythonize(py, &records).map(Bound::unbind).map_err(|e| refused(e.to_string()))
}

/// Launch the config entry `name` in the patch workspace, with `model` and `effort` as its
/// flags where the entry has them; this node's process owns it. Answers the instance id.
#[pyfunction]
#[pyo3(signature = (name, model=None, effort=None))]
fn spawn(py: Python<'_>, name: String, model: Option<String>, effort: Option<String>) -> PyResult<String> {
    let plane = plane()?;
    py.detach(|| {
        let cwd = plane.workspace().ok_or_else(|| "the session has no patch workspace to run an agent in".to_string())?;
        plane.spawn(&name, Launch { model, effort }, &cwd, &goofi_agent::parent_env())
    })
    .map_err(refused)
}

/// Type `text` into a running instance as one entry, submitted with Enter.
#[pyfunction]
fn send(py: Python<'_>, instance: String, text: String) -> PyResult<()> {
    let plane = plane()?;
    py.detach(|| plane.send(&instance, &text)).map_err(refused)
}

/// Stop a running instance, or dismiss one that exited.
#[pyfunction]
fn stop(py: Python<'_>, instance: String) -> PyResult<()> {
    let plane = plane()?;
    py.detach(|| plane.stop(&instance)).map_err(refused)
}

pub fn module(py: Python<'_>) -> PyResult<Bound<'_, PyModule>> {
    let m = PyModule::new(py, "agent")?;
    m.add_function(wrap_pyfunction!(agents, &m)?)?;
    m.add_function(wrap_pyfunction!(roster, &m)?)?;
    m.add_function(wrap_pyfunction!(spawn, &m)?)?;
    m.add_function(wrap_pyfunction!(send, &m)?)?;
    m.add_function(wrap_pyfunction!(stop, &m)?)?;
    Ok(m)
}
