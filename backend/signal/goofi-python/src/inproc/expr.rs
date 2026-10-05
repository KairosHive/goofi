//! `PyExprEvaluator` — the pyo3 expression evaluator the engines' workers call into.

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicU64, Ordering};
use goofi_supervisor::sync::Mutex;

use goofi_core::{Data, Meta, Value};
use goofi_node::{BindingId, Compiled, EvalCtx, ExprError, ExprEvaluator, Local};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule, PyString};

/// The Python harness. The graph has already rewritten every `nd(..)` and `variables.*` term into a
/// generated variable, so an expression compiles ONCE into a function of `t` and those variables,
/// over a module scope with `np`, `math`'s whole namespace and `time()` simply there. `lfo()` and
/// `noi()` read the call's time and range off a thread local, so no namespace is rebuilt per call.
/// A result leaves as a string, or as the shape and bytes of an f32 array.
const EVAL_SRC: &str = r#"
import numpy as np
import threading
from math import *
from time import time

__goofi_call = threading.local()

def __goofi_noise(src):
    # Hash integer coordinates, then interpolate with zero slope at each endpoint.
    x = floor(src)
    def sample(i):
        i = (i ^ 0x9e3779b9) & 0xffffffff
        i = ((i ^ (i >> 16)) * 0x7feb352d) & 0xffffffff
        i = ((i ^ (i >> 15)) * 0x846ca68b) & 0xffffffff
        return (i ^ (i >> 16)) / 0xffffffff
    f = src - x
    blend = f * f * f * (f * (f * 6 - 15) + 10)
    return sample(x) * (1 - blend) + sample(x + 1) * blend

def lfo(*, freq=1, src=None, vmin=None, vmax=None):
    c = __goofi_call
    src = c.t if src is None else src
    vmin = c.lo if vmin is None else vmin
    vmax = c.hi if vmax is None else vmax
    return vmin + (vmax - vmin) * (sin(tau * freq * src) + 1) / 2

def noi(*, freq=1, src=None, vmin=None, vmax=None):
    c = __goofi_call
    src = c.t if src is None else src
    vmin = c.lo if vmin is None else vmin
    vmax = c.hi if vmax is None else vmax
    return vmin + (vmax - vmin) * __goofi_noise(freq * src)

def __goofi_compile(source, names):
    # The source on a line of its own, so a trailing comment ends before the closing parenthesis.
    return eval(compile("lambda t" + "".join(", " + n for n in names) + ": (\n" + source + "\n)", "<goofi-expr>", "eval"))

def __goofi_eval(fn, t, lo, hi, locals_):
    c = __goofi_call
    c.t, c.lo, c.hi = t, lo, hi
    out = fn(t, **locals_)
    if isinstance(out, str):
        return out
    a = np.asarray(out, dtype=np.float32)
    if a.ndim == 0:
        a = a.reshape(1)
    return a.shape, np.ascontiguousarray(a).tobytes()
"#;

/// The pyo3 evaluator: the harness functions plus the compiled functions keyed by [`BindingId`].
pub struct PyExprEvaluator {
    compile_fn: Py<PyAny>,
    eval_fn: Py<PyAny>,
    codes: Mutex<HashMap<u64, Py<PyAny>>>,
    next: AtomicU64,
}

impl PyExprEvaluator {
    pub fn new() -> PyResult<PyExprEvaluator> {
        crate::attach(|py| {
            let m = PyModule::from_code(
                py,
                CString::new(EVAL_SRC)?.as_c_str(),
                c"goofi_expr.py",
                c"goofi_expr",
            )?;
            Ok(PyExprEvaluator {
                compile_fn: m.getattr("__goofi_compile")?.unbind(),
                eval_fn: m.getattr("__goofi_eval")?.unbind(),
                codes: Mutex::new(HashMap::new()),
                next: AtomicU64::new(1),
            })
        })
    }
}

/// A frame as the expression sees it: an array as a numpy view over its bytes, a string as itself;
/// a table or a texture is `None`.
fn data_to_py(py: Python<'_>, d: &Data) -> PyResult<Py<PyAny>> {
    match d.value() {
        Value::Array(s) => Ok(goofi_pymod::numpy_f32(py, s.shape(), s.as_bytes())?.unbind()),
        Value::Str(st) => Ok(PyString::new(py, st.as_ref()).into_any().unbind()),
        Value::Table(_) | Value::Texture(_) => Ok(py.None()),
    }
}

/// The names an expression's variables wear, in the order the rewrite minted them: `__v0`, `__v1`
/// and so on, which is the order the harness declares them in.
fn names_of(source: &str) -> Vec<String> {
    let mut names: Vec<String> = goofi_node::expr::tokens(source)
        .iter()
        .filter(|t| t.kind == goofi_node::expr::Kind::Ident)
        .map(|t| &source[t.start..t.end])
        .filter(|w| w.strip_prefix("__v").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
        .map(str::to_string)
        .collect();
    names.sort();
    names.dedup();
    names
}

impl ExprEvaluator for PyExprEvaluator {
    fn compile(&self, source: &str) -> Result<Compiled, ExprError> {
        crate::attach(|py| -> Result<Compiled, ExprError> {
            let code = self
                .compile_fn
                .bind(py)
                .call1((source, names_of(source)))
                .map_err(|e| ExprError(e.to_string()))?;
            let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
            self.codes.lock().insert(id, code.unbind());
            Ok(Compiled { id })
        })
    }

    fn eval(&self, id: BindingId, ctx: &EvalCtx<'_>) -> Result<Data, ExprError> {
        crate::attach(|py| -> Result<Data, ExprError> {
            let code = self
                .codes
                .lock()
                .get(&id)
                .map(|c| c.clone_ref(py))
                .ok_or_else(|| ExprError("expression not compiled".into()))?;
            let locals = PyDict::new(py);
            for (name, local) in ctx.locals {
                let val: Py<PyAny> = match local {
                    Local::Frame(d) => data_to_py(py, d).map_err(|e| ExprError(e.to_string()))?,
                    Local::Value(p) => goofi_pymod::exec::param_to_py(py, p).map(Bound::unbind).map_err(|e| ExprError(e.to_string()))?,
                };
                locals.set_item(name.as_str(), val).map_err(|e| ExprError(e.to_string()))?;
            }
            let result = self
                .eval_fn
                .bind(py)
                .call1((code.bind(py), ctx.t, ctx.range.0, ctx.range.1, &locals))
                .map_err(|e| ExprError(e.to_string()))?;
            if let Ok(text) = result.extract::<String>() {
                return Ok(Data::text(text));
            }
            let (shape, bytes) = result
                .extract::<(Vec<usize>, Bound<'_, PyBytes>)>()
                .map_err(|_| ExprError("expression result is not a number, a sequence of numbers or a string".into()))?;
            Data::array_f32(shape, bytes.as_bytes().to_vec(), Meta::default()).map_err(|e| ExprError(e.to_string()))
        })
    }

    fn release(&self, id: BindingId) {
        self.codes.lock().remove(&id);
    }
}
