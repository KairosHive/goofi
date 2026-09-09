//! Python text streams retain the source of the node thread that writes them.
use std::cell::RefCell;
use std::sync::OnceLock;
use pyo3::prelude::*;
use goofi_core::log::{self, Level};

thread_local! {
    static LINES: RefCell<[String; 2]> = const { RefCell::new([String::new(), String::new()]) };
}

#[pyclass]
struct Stream { error: bool, original: Option<Py<PyAny>> }

#[pymethods]
impl Stream {
    fn write(&self, text: &str) -> usize {
        LINES.with(|lines| {
            let mut lines = lines.borrow_mut();
            let pending = &mut lines[usize::from(self.error)];
            for c in text.chars() {
                if c == '\n' { self.emit(std::mem::take(pending)); }
                else {
                    pending.push(c);
                    if pending.len() >= log::MAX_TEXT { self.emit(std::mem::take(pending)); }
                }
            }
        });
        text.chars().count()
    }
    fn flush(&self) {
        LINES.with(|lines| {
            let pending = &mut lines.borrow_mut()[usize::from(self.error)];
            if !pending.is_empty() { self.emit(std::mem::take(pending)); }
        });
    }
    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        self.original.as_ref().ok_or_else(|| pyo3::exceptions::PyAttributeError::new_err(name.to_string()))?
            .bind(py).getattr(name).map(Bound::unbind)
    }
    fn isatty(&self) -> bool { false }
    fn fileno(&self) -> i32 { if self.error { 2 } else { 1 } }
    #[getter]
    fn encoding(&self) -> &'static str { "utf-8" }
    #[getter]
    fn errors(&self) -> &'static str { "replace" }
    #[getter]
    fn closed(&self) -> bool { false }
    fn writable(&self) -> bool { true }
}

impl Stream {
    fn emit(&self, text: String) {
        log::record(log::source(), if self.error { Level::Error } else { Level::Info },
            Some(if self.error { "stderr" } else { "stdout" }), text);
    }
}

pub fn install(py: Python<'_>) -> PyResult<()> {
    static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let run = || -> PyResult<()> {
            let sys = py.import("sys")?;
            sys.setattr("stdout", Py::new(py, Stream { error: false, original: Some(sys.getattr("stdout")?.unbind()) })?)?;
            sys.setattr("stderr", Py::new(py, Stream { error: true, original: Some(sys.getattr("stderr")?.unbind()) })?)?;
            Ok(())
        };
        run().map_err(|e| e.to_string())
    }).clone().map_err(pyo3::exceptions::PyRuntimeError::new_err)
}

pub fn flush() {
    Stream { error: false, original: None }.flush();
    Stream { error: true, original: None }.flush();
}
