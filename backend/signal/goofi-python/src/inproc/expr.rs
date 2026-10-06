//! `PyExprEvaluator` — the pyo3 expression evaluator the engines' workers call into.

use std::collections::{HashMap, VecDeque};
use std::ffi::CString;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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
import marshal
import numpy as np
import threading
import types
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
    fn = eval(compile("lambda t" + "".join(", " + n for n in names) + ": (\n" + source + "\n)", "<goofi-expr>", "eval"))
    return fn, marshal.dumps(fn.__code__)

def __goofi_load(code):
    return types.FunctionType(marshal.loads(code), globals())

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

/// How many functionals stay loaded: each departure of a machine mints one, so the loaded set is
/// a window over the recent ones, not a record of them all.
const LOADED: usize = 256;

/// The functionals loaded by the hash of their code, and the order they came in.
type Loaded = (HashMap<u64, Py<PyAny>>, VecDeque<u64>);

/// One compiled source: the function, its marshaled code, and how many bindings hold it.
struct Code {
    fn_: Py<PyAny>,
    marshaled: Arc<[u8]>,
    holds: usize,
}

/// The pyo3 evaluator: the harness functions, the compiled functions keyed by [`BindingId`] and
/// shared by source text, and the functionals loaded from their code.
pub struct PyExprEvaluator {
    compile_fn: Py<PyAny>,
    load_fn: Py<PyAny>,
    eval_fn: Py<PyAny>,
    codes: Mutex<HashMap<u64, Code>>,
    by_source: Mutex<HashMap<String, u64>>,
    loaded: Mutex<Loaded>,
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
                load_fn: m.getattr("__goofi_load")?.unbind(),
                eval_fn: m.getattr("__goofi_eval")?.unbind(),
                codes: Mutex::new(HashMap::new()),
                by_source: Mutex::new(HashMap::new()),
                loaded: Mutex::new((HashMap::new(), VecDeque::new())),
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
        Value::Table(_) | Value::Texture(_) | Value::Functional(_) => Ok(py.None()),
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

impl PyExprEvaluator {
    /// Call one compiled function at `ctx`, its result read back as a frame.
    fn call(&self, py: Python<'_>, fn_: &Bound<'_, PyAny>, ctx: &EvalCtx<'_>) -> Result<Data, ExprError> {
            let locals = PyDict::new(py);
            for (name, local) in ctx.locals {
                let val: Py<PyAny> = match local {
                    // A functional among the locals is read at this evaluation's instant.
                    Local::Frame(d) if d.as_functional().is_some() => {
                        let now = goofi_node::at(self, d, ctx.t, ctx.range)?;
                        data_to_py(py, &now).map_err(|e| ExprError(e.to_string()))?
                    }
                    Local::Frame(d) => data_to_py(py, d).map_err(|e| ExprError(e.to_string()))?,
                    Local::Value(p) => goofi_pymod::exec::param_to_py(py, p).map(Bound::unbind).map_err(|e| ExprError(e.to_string()))?,
                };
                locals.set_item(name.as_str(), val).map_err(|e| ExprError(e.to_string()))?;
            }
            let result = self
                .eval_fn
                .bind(py)
                .call1((fn_, ctx.t, ctx.range.0, ctx.range.1, &locals))
                .map_err(|e| ExprError(e.to_string()))?;
            if let Ok(text) = result.extract::<String>() {
                return Ok(Data::text(text));
            }
            let (shape, bytes) = result
                .extract::<(Vec<usize>, Bound<'_, PyBytes>)>()
                .map_err(|_| ExprError("expression result is not a number, a sequence of numbers or a string".into()))?;
            Data::array_f32(shape, bytes.as_bytes().to_vec(), Meta::default()).map_err(|e| ExprError(e.to_string()))
    }
}

impl ExprEvaluator for PyExprEvaluator {
    fn compile(&self, source: &str) -> Result<Compiled, ExprError> {
        let mut by_source = self.by_source.lock();
        let mut codes = self.codes.lock();
        if let Some(code) = by_source.get(source).and_then(|id| codes.get_mut(id).map(|c| (*id, c))) {
            code.1.holds += 1;
            return Ok(Compiled { id: code.0, code: code.1.marshaled.clone() });
        }
        crate::attach(|py| -> Result<Compiled, ExprError> {
            let (fn_, marshaled): (Bound<'_, PyAny>, Bound<'_, PyBytes>) = self
                .compile_fn
                .bind(py)
                .call1((source, names_of(source)))
                .and_then(|r| r.extract())
                .map_err(|e| ExprError(e.to_string()))?;
            let marshaled: Arc<[u8]> = Arc::from(marshaled.as_bytes());
            let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
            codes.insert(id, Code { fn_: fn_.unbind(), marshaled: marshaled.clone(), holds: 1 });
            by_source.insert(source.to_string(), id);
            Ok(Compiled { id, code: marshaled })
        })
    }

    fn eval(&self, id: BindingId, ctx: &EvalCtx<'_>) -> Result<Data, ExprError> {
        crate::attach(|py| -> Result<Data, ExprError> {
            let fn_ = self
                .codes
                .lock()
                .get(&id)
                .map(|c| c.fn_.clone_ref(py))
                .ok_or_else(|| ExprError("expression not compiled".into()))?;
            self.call(py, fn_.bind(py), ctx)
        })
    }

    fn release(&self, id: BindingId) {
        let mut codes = self.codes.lock();
        let Some(code) = codes.get_mut(&id) else { return };
        code.holds -= 1;
        if code.holds == 0 {
            codes.remove(&id);
            self.by_source.lock().retain(|_, held| *held != id);
        }
    }

    fn run(&self, code: &[u8], ctx: &EvalCtx<'_>) -> Result<Data, ExprError> {
        let mut h = DefaultHasher::new();
        code.hash(&mut h);
        let key = h.finish();
        crate::attach(|py| -> Result<Data, ExprError> {
            let fn_ = {
                let mut loaded = self.loaded.lock();
                match loaded.0.get(&key) {
                    Some(fn_) => fn_.clone_ref(py),
                    None => {
                        let fn_ = self.load_fn.bind(py).call1((PyBytes::new(py, code),)).map_err(|e| ExprError(e.to_string()))?;
                        if loaded.1.len() >= LOADED {
                            if let Some(old) = loaded.1.pop_front() {
                                loaded.0.remove(&old);
                            }
                        }
                        loaded.1.push_back(key);
                        loaded.0.insert(key, fn_.clone().unbind());
                        fn_.unbind()
                    }
                }
            };
            self.call(py, fn_.bind(py), ctx)
        })
    }
}
