//! `PyExprEvaluator` — the pyo3 expression evaluator the engines' workers call into.

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicU64, Ordering};
use goofi_supervisor::sync::Mutex;

use goofi_core::{Data, Meta, Value};
use goofi_node::{BindingId, Compiled, DeterministicCompiled, EvalCtx, ExprError, ExprEvaluator, Local};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyModule, PyString, PyTuple};

/// The Python harness. The graph has already rewritten every `nd(..)` and `variables.*` term into a
/// generated variable, so an expression compiles ONCE into a function of `t` and those variables,
/// over a module scope with `np`, `math`'s whole namespace and `time()` simply there. `lfo()` and
/// `noi()` read the call's time and range off a thread local, so no namespace is rebuilt per call.
/// A result leaves as a string, or as the shape and bytes of an f32 array.
const EVAL_SRC: &str = r#"
import numpy as np
import ast
import threading
from math import *

__goofi_call = threading.local()

def time():
    return __goofi_call.t

# Deterministic expressions have the same language and evaluator, with pure capabilities only.
__goofi_math = set('acos acosh asin asinh atan atan2 atanh ceil comb copysign cos cosh degrees dist erf erfc exp expm1 fabs factorial floor fmod frexp fsum gamma gcd hypot isclose isfinite isinf isnan ldexp lgamma log log10 log1p log2 modf nextafter perm pow prod radians remainder sin sinh sqrt tan tanh trunc ulp'.split())
__goofi_builtin = set('abs all any bool float int len max min round sorted str sum'.split())
__goofi_constants = set('e inf nan pi tau True False None'.split())
__goofi_numpy = {
    **dict.fromkeys('abs absolute acos arccos arcsin arctan ceil cos cosh exp expm1 floor isfinite isinf isnan log log10 log1p log2 negative sign sin sinh sqrt square tan tanh trunc'.split(), 1),
    **dict.fromkeys('add arctan2 divide equal greater greater_equal hypot less less_equal logical_and logical_or maximum minimum multiply not_equal power remainder subtract'.split(), 2),
    **dict.fromkeys('all any array asarray concatenate cumsum diff expand_dims flatten full max mean median min ones prod ravel repeat reshape squeeze stack std sum tile transpose var zeros'.split(), 1),
    'clip': 3, 'where': 3, 'arange': 3, 'linspace': 3, 'full': 2,
    'expand_dims': 2, 'repeat': 2, 'reshape': 2, 'tile': 2,
    'float32': 1, 'float64': 1, 'int32': 1, 'int64': 1,
}
__goofi_methods = dict.fromkeys('all any flatten item max mean min prod ravel std sum tolist var'.split(), 0)
__goofi_methods.update({'clip': 2, 'reshape': 2, 'repeat': 1, 'transpose': 2, 'astype': 1})
__goofi_syntax = (ast.Expression, ast.Constant, ast.Name, ast.Load, ast.BinOp, ast.UnaryOp,
    ast.BoolOp, ast.Compare, ast.IfExp, ast.Call, ast.keyword, ast.Attribute, ast.Subscript,
    ast.Slice, ast.Tuple, ast.List, ast.Add, ast.Sub, ast.Mult, ast.Div, ast.FloorDiv,
    ast.Mod, ast.Pow, ast.MatMult, ast.UAdd, ast.USub, ast.Not, ast.Invert, ast.And,
    ast.Or, ast.BitAnd, ast.BitOr, ast.BitXor, ast.LShift, ast.RShift, ast.Eq, ast.NotEq,
    ast.Lt, ast.LtE, ast.Gt, ast.GtE, ast.In, ast.NotIn, ast.Is, ast.IsNot)

def __goofi_validate(tree, names):
    allowed = set(names) | __goofi_math | __goofi_builtin | __goofi_constants | {'t', 'time', 'lfo', 'noi', 'np'}
    parents = {child: parent for parent in ast.walk(tree) for child in ast.iter_child_nodes(parent)}
    observes = False
    for node in ast.walk(tree):
        if not isinstance(node, __goofi_syntax):
            raise ValueError('deterministic expression does not allow ' + type(node).__name__)
        if isinstance(node, ast.Name):
            if node.id not in allowed:
                raise ValueError('deterministic expression does not allow ' + node.id)
            observes |= node.id == 't'
            parent = parents.get(node)
            if node.id == 'np' and not (isinstance(parent, ast.Attribute) and parent.value is node):
                raise ValueError('deterministic expression requires a named numpy capability')
            if node.id in __goofi_math | __goofi_builtin | {'time', 'lfo', 'noi'} and not (isinstance(parent, ast.Call) and parent.func is node):
                raise ValueError('deterministic expression functions must be called directly')
        if isinstance(node, ast.Attribute):
            numpy = isinstance(node.value, ast.Name) and node.value.id == 'np'
            if node.attr not in (__goofi_numpy if numpy else __goofi_methods) and not (numpy and node.attr in __goofi_constants):
                raise ValueError('deterministic expression does not allow attribute ' + node.attr)
            parent = parents.get(node)
            if not (numpy and node.attr in __goofi_constants) and not (isinstance(parent, ast.Call) and parent.func is node):
                raise ValueError('deterministic expression functions must be called directly')
        if isinstance(node, ast.Call):
            fn = node.func
            if isinstance(fn, ast.Name):
                if fn.id not in __goofi_math | __goofi_builtin | {'time', 'lfo', 'noi'}:
                    raise ValueError('deterministic expression requires a pure function')
                if fn.id == 'time':
                    observes = True
                if fn.id in ('lfo', 'noi'):
                    src = next((k.value for k in node.keywords if k.arg == 'src'), None)
                    observes |= src is None or not isinstance(src, ast.Constant) or src.value is None
            elif isinstance(fn, ast.Attribute):
                numpy = isinstance(fn.value, ast.Name) and fn.value.id == 'np'
                limit = (__goofi_numpy if numpy else __goofi_methods).get(fn.attr)
                if limit is None or len(node.args) > limit:
                    raise ValueError('deterministic expression requires a pure call signature')
            else:
                raise ValueError('deterministic expression requires a named pure function')
            if any(k.arg is None or k.arg in ('out', 'overwrite_input', 'where') for k in node.keywords):
                raise ValueError('deterministic expression does not allow output mutation or keyword expansion')
    return observes

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

def __goofi_compile(source, names, deterministic):
    tree = ast.parse('(\n' + source + '\n)', mode='eval')
    error = None
    try:
        observes = __goofi_validate(tree, names)
    except ValueError as why:
        if deterministic:
            raise
        observes, error = False, str(why)
    # The source on a line of its own, so a trailing comment ends before the closing parenthesis.
    fn = eval(compile("lambda t" + "".join(", " + n for n in names) + ": (\n" + source + "\n)", "<goofi-expr>", "eval"))
    return fn, observes, error

def __goofi_run(fn, t, lo, hi, locals_):
    c = __goofi_call
    c.t, c.lo, c.hi = t, lo, hi
    return fn(t, *locals_)

def __goofi_array(out):
    a = np.asarray(out, dtype=np.float32)
    return a.reshape(1) if a.ndim == 0 else a

def __goofi_eval(fn, t, lo, hi, locals_):
    out = __goofi_run(fn, t, lo, hi, locals_)
    if isinstance(out, str):
        return out
    a = __goofi_array(out)
    return a.shape, np.ascontiguousarray(a).tobytes()

def __goofi_eval_sampled(fn, times, lo, hi, locals_, offsets):
    columns = []
    shape = None
    for i, t in enumerate(times):
        locals_at = [value if offset is None else value[:, offset + i] for value, offset in zip(locals_, offsets)]
        out = __goofi_run(fn, t, lo, hi, locals_at)
        if isinstance(out, str):
            raise ValueError('sampled expression must yield numbers')
        a = __goofi_array(out)
        if a.size == 0:
            raise ValueError('sampled expression yields no channels')
        if shape is not None and a.shape != shape:
            raise ValueError('sampled expression changes its result shape')
        shape = a.shape
        columns.append(a.reshape(-1))
    result = np.stack(columns, axis=1)
    return result.shape, np.ascontiguousarray(result).tobytes()
"#;

/// The pyo3 evaluator: the harness functions plus the compiled functions keyed by [`BindingId`].
pub struct PyExprEvaluator {
    compile_fn: Py<PyAny>,
    eval_fn: Py<PyAny>,
    sampled_fn: Py<PyAny>,
    codes: Mutex<HashMap<u64, Code>>,
    next: AtomicU64,
}

struct Code {
    function: Py<PyAny>,
    names: std::sync::Arc<[String]>,
    determinism: Result<bool, String>,
    references: usize,
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
                sampled_fn: m.getattr("__goofi_eval_sampled")?.unbind(),
                codes: Mutex::new(HashMap::new()),
                next: AtomicU64::new(1),
            })
        })
    }

    fn compile_with(&self, source: &str, deterministic: bool) -> Result<DeterministicCompiled, ExprError> {
        crate::attach(|py| -> Result<DeterministicCompiled, ExprError> {
            let names = names_of(source);
            let (code, observes_time, error) = self.compile_fn.bind(py)
                .call1((source, &names, deterministic))
                .and_then(|value| value.extract::<(Py<PyAny>, bool, Option<String>)>())
                .map_err(|e| ExprError(e.to_string()))?;
            let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
            self.codes.lock().insert(id, Code { function: code, names: names.into(), determinism: error.map_or(Ok(observes_time), Err), references: 1 });
            Ok(DeterministicCompiled { compiled: Compiled { id }, observes_time })
        })
    }

    fn evaluate(&self, id: BindingId, ctx: &EvalCtx<'_>, span: Option<goofi_core::samples::SampleSpan>) -> Result<Data, ExprError> {
        crate::attach(|py| -> Result<Data, ExprError> {
            let (code, names) = self.codes.lock().get(&id)
                .map(|c| (c.function.clone_ref(py), c.names.clone()))
                .ok_or_else(|| ExprError("expression not compiled".into()))?;
            let mut locals = Vec::with_capacity(names.len());
            let mut offsets = Vec::with_capacity(if span.is_some() { names.len() } else { 0 });
            for name in names.iter() {
                let local = ctx.locals.iter().find(|(n, _)| n == name).map(|(_, local)| local)
                    .ok_or_else(|| ExprError(format!("expression input `{name}` is missing")))?;
                let (value, offset) = match local {
                    Local::Frame(frame) => {
                        let offset = span.and_then(|span| goofi_core::samples::SampleSpan::of(frame).map(|input| span.first - input.first));
                        (data_to_py(py, frame).map_err(|e| ExprError(e.to_string()))?, offset)
                    }
                    Local::Value(value) => (goofi_pymod::exec::param_to_py(py, value).map(Bound::unbind)
                        .map_err(|e| ExprError(e.to_string()))?, None),
                };
                locals.push(value);
                if span.is_some() { offsets.push(offset); }
            }
            let locals = PyTuple::new(py, locals).map_err(|e| ExprError(e.to_string()))?;
            let result = match span {
                Some(span) => {
                    let times: Vec<_> = (0..span.length).map(|i| goofi_core::time::seconds(
                        span.clock.at(span.first + i as u64).saturating_sub(goofi_core::samples::CONTROL_DELAY))).collect();
                    self.sampled_fn.bind(py).call1((code.bind(py), times, ctx.range.0, ctx.range.1, locals, offsets))
                }
                None => self.eval_fn.bind(py).call1((code.bind(py), ctx.t, ctx.range.0, ctx.range.1, locals)),
            }.map_err(|e| ExprError(e.to_string()))?;
            if span.is_none() {
                if let Ok(text) = result.extract::<String>() { return Ok(Data::text(text)); }
            }
            let (shape, bytes) = result.extract::<(Vec<usize>, Bound<'_, PyBytes>)>()
                .map_err(|error| match span {
                    Some(_) => ExprError(error.to_string()),
                    None => ExprError("expression result is not a number, a sequence of numbers or a string".into()),
                })?;
            Data::array_f32(shape, bytes.as_bytes().to_vec(), span.map_or_else(Meta::default, |span| span.meta()))
                .map_err(|e| ExprError(e.to_string()))
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
        self.compile_with(source, false).map(|result| result.compiled)
    }

    fn compile_deterministic(&self, source: &str) -> Result<DeterministicCompiled, ExprError> {
        self.compile_with(source, true)
    }

    fn retain_deterministic(&self, id: BindingId) -> Result<bool, ExprError> {
        let mut codes = self.codes.lock();
        let code = codes.get_mut(&id).ok_or_else(|| ExprError("expression not compiled".into()))?;
        let observes = code.determinism.clone().map_err(ExprError)?;
        code.references = code.references.checked_add(1).ok_or_else(|| ExprError("expression reference count overflows".into()))?;
        Ok(observes)
    }

    fn eval(&self, id: BindingId, ctx: &EvalCtx<'_>) -> Result<Data, ExprError> {
        self.evaluate(id, ctx, None)
    }

    fn eval_sampled(&self, id: BindingId, ctx: &EvalCtx<'_>, span: goofi_core::samples::SampleSpan) -> Result<Data, ExprError> {
        goofi_node::samples::validate(ctx.locals, span)?;
        self.evaluate(id, ctx, Some(span))
    }

    fn release(&self, id: BindingId) {
        let mut codes = self.codes.lock();
        if let Some(code) = codes.get_mut(&id) {
            code.references -= 1;
            if code.references == 0 { codes.remove(&id); }
        }
    }
}
