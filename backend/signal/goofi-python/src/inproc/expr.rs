//! `PyExprEvaluator` — the pyo3 expression evaluator the engines' workers call into.

use std::collections::{HashMap, VecDeque};
use std::ffi::CString;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use goofi_supervisor::sync::Mutex;

use goofi_core::{Data, Meta, Value};
use goofi_node::{BindingId, Compiled, EvalCtx, ExprError, ExprEvaluator, Local};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyModule, PyString};

/// The Python harness. The graph has already rewritten every `nd(..)` and `variables.*` term into a
/// generated variable, so an expression compiles ONCE into a function of `t` and those variables,
/// over a module scope with `np`, `math`'s whole namespace and `time()` simply there. `lfo()` and
/// `noi()` read the call's time and range off a thread local, so no namespace is rebuilt per call;
/// `qtz()` snaps through [`qtz_core`], so its scales are `goofi_core::scale`'s.
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

def qtz(v, scale="major", root="C"):
    if np.ndim(v) == 0:
        return __goofi_qtz(float(v), scale, root)
    return np.vectorize(lambda x: __goofi_qtz(float(x), scale, root), otypes=[float])(v)

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
type Loaded = (HashMap<u64, Arc<Py<PyAny>>>, VecDeque<u64>);

/// One compiled source: the function, its marshaled code, and how many bindings hold it.
struct Code {
    fn_: Arc<Py<PyAny>>,
    marshaled: Arc<[u8]>,
    holds: usize,
}

/// The compiled functions by [`BindingId`], and the id each source text compiled to.
#[derive(Default)]
struct Table {
    codes: HashMap<u64, Code>,
    by_source: HashMap<String, u64>,
}

/// The pyo3 evaluator: the harness functions, the compiled functions keyed by [`BindingId`] and
/// shared by source text, and the functionals loaded from their code.
///
/// Its locks are taken DETACHED and never held across a Python call. A free-threaded collection
/// stops the world until every attached thread parks; one attached thread blocked on a mutex
/// whose holder is itself waiting to attach never parks, and the process is dead.
pub struct PyExprEvaluator {
    compile_fn: Py<PyAny>,
    load_fn: Py<PyAny>,
    eval_fn: Py<PyAny>,
    table: Mutex<Table>,
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
            m.add_function(wrap_pyfunction!(qtz_core, &m)?)?;
            Ok(PyExprEvaluator {
                compile_fn: m.getattr("__goofi_compile")?.unbind(),
                load_fn: m.getattr("__goofi_load")?.unbind(),
                eval_fn: m.getattr("__goofi_eval")?.unbind(),
                table: Mutex::new(Table::default()),
                loaded: Mutex::new((HashMap::new(), VecDeque::new())),
                next: AtomicU64::new(1),
            })
        })
    }
}

/// A `qtz()` root: a note name, or semitones above C.
#[derive(FromPyObject)]
enum Root {
    Note(String),
    Semitones(f64),
}

/// `qtz()`'s core: a pitch in volts per octave onto the nearest degree of the preset `scale`,
/// counted from `root`.
#[pyfunction]
#[pyo3(name = "__goofi_qtz")]
fn qtz_core(volts: f64, scale: &str, root: Root) -> PyResult<f64> {
    use goofi_core::scale::{Recipe, MAX_DEGREES, NOTES, OCTAVE, PRESET_NAMES};
    if !PRESET_NAMES.contains(&scale) {
        return Err(PyValueError::new_err(format!("qtz: no scale `{scale}`; one of {}", PRESET_NAMES.join(", "))));
    }
    let semitones = match root {
        Root::Semitones(s) => s,
        Root::Note(n) => NOTES.iter().position(|x| x.eq_ignore_ascii_case(&n)).ok_or_else(|| {
            PyValueError::new_err(format!("qtz: no root `{n}`; a note of {} or semitones above C", NOTES.join(" ")))
        })? as f64,
    };
    let recipe = Recipe::preset(scale);
    let mut degrees = [0.0; MAX_DEGREES];
    let n = recipe.degrees(&mut degrees);
    let root = semitones * OCTAVE / 12.0;
    let (cents, _) = recipe.snap(&degrees[..n], volts * OCTAVE - root);
    Ok((cents + root) / OCTAVE)
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
    /// The functionals among `ctx`'s locals, read at this evaluation's instant — DETACHED, since
    /// reading one loads its code under the `loaded` lock.
    fn functionals(&self, ctx: &EvalCtx<'_>) -> Result<Vec<Option<Data>>, ExprError> {
        ctx.locals
            .iter()
            .map(|(_, local)| match local {
                Local::Frame(d) if d.as_functional().is_some() => goofi_node::at(self, d, ctx.t, ctx.range).map(Some),
                _ => Ok(None),
            })
            .collect()
    }

    /// Call one compiled function at `ctx`, its result read back as a frame. `now` is
    /// [`Self::functionals`] for the same `ctx`.
    fn call(&self, py: Python<'_>, fn_: &Bound<'_, PyAny>, ctx: &EvalCtx<'_>, now: &[Option<Data>]) -> Result<Data, ExprError> {
            let locals = PyDict::new(py);
            for ((name, local), now) in ctx.locals.iter().zip(now) {
                let val: Py<PyAny> = match (local, now) {
                    (_, Some(now)) => data_to_py(py, now).map_err(|e| ExprError(e.to_string()))?,
                    (Local::Frame(d), None) => data_to_py(py, d).map_err(|e| ExprError(e.to_string()))?,
                    (Local::Value(p), None) => goofi_pymod::exec::param_to_py(py, p).map(Bound::unbind).map_err(|e| ExprError(e.to_string()))?,
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
        let held = |table: &mut Table, source: &str| {
            let id = *table.by_source.get(source)?;
            let code = table.codes.get_mut(&id)?;
            code.holds += 1;
            Some(Compiled { id, code: code.marshaled.clone() })
        };
        if let Some(hit) = held(&mut self.table.lock(), source) {
            return Ok(hit);
        }
        let (fn_, marshaled) = crate::attach(|py| -> Result<(Py<PyAny>, Arc<[u8]>), ExprError> {
            let (fn_, marshaled): (Bound<'_, PyAny>, Bound<'_, PyBytes>) = self
                .compile_fn
                .bind(py)
                .call1((source, names_of(source)))
                .and_then(|r| r.extract())
                .map_err(|e| ExprError(e.to_string()))?;
            Ok((fn_.unbind(), Arc::from(marshaled.as_bytes())))
        })?;
        let mut table = self.table.lock();
        // Another thread compiled the same text meanwhile: one source, one function.
        if let Some(hit) = held(&mut table, source) {
            drop(table);
            drop(fn_);
            return Ok(hit);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        table.codes.insert(id, Code { fn_: Arc::new(fn_), marshaled: marshaled.clone(), holds: 1 });
        table.by_source.insert(source.to_string(), id);
        Ok(Compiled { id, code: marshaled })
    }

    fn eval(&self, id: BindingId, ctx: &EvalCtx<'_>) -> Result<Data, ExprError> {
        let fn_ = self.table.lock().codes.get(&id).map(|c| c.fn_.clone()).ok_or_else(|| ExprError("expression not compiled".into()))?;
        let now = self.functionals(ctx)?;
        crate::attach(|py| self.call(py, fn_.bind(py), ctx, &now))
    }

    fn release(&self, id: BindingId) {
        let gone = {
            let mut table = self.table.lock();
            let Some(code) = table.codes.get_mut(&id) else { return };
            code.holds -= 1;
            if code.holds > 0 {
                return;
            }
            table.by_source.retain(|_, held| *held != id);
            table.codes.remove(&id)
        };
        // Dropped off the lock: the function's last reference may run Python.
        drop(gone);
    }

    fn run(&self, code: &[u8], ctx: &EvalCtx<'_>) -> Result<Data, ExprError> {
        let mut h = DefaultHasher::new();
        code.hash(&mut h);
        let key = h.finish();
        let known = self.loaded.lock().0.get(&key).cloned();
        let fn_ = match known {
            Some(fn_) => fn_,
            None => {
                let fn_ = crate::attach(|py| -> Result<Py<PyAny>, ExprError> {
                    Ok(self.load_fn.bind(py).call1((PyBytes::new(py, code),)).map_err(|e| ExprError(e.to_string()))?.unbind())
                })?;
                let fn_ = Arc::new(fn_);
                let mut loaded = self.loaded.lock();
                let evicted = (loaded.1.len() >= LOADED).then(|| loaded.1.pop_front()).flatten().and_then(|old| loaded.0.remove(&old));
                loaded.1.push_back(key);
                let fn_ = loaded.0.entry(key).or_insert(fn_).clone();
                drop(loaded);
                drop(evicted);
                fn_
            }
        };
        let now = self.functionals(ctx)?;
        crate::attach(|py| self.call(py, fn_.bind(py), ctx, &now))
    }
}
