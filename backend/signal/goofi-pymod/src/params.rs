//! The Python-facing param + slot-type descriptors a node states in its `PARAMS` and
//! `INPUTS` constants.

use pyo3::prelude::*;

/// `goofi.DataType.ARRAY|STRING|TABLE` — the slot element type; `.value` is the wire name.
/// The variants are the Python-facing API, so they stay screaming-case despite the acronym lint.
#[pyclass(eq, eq_int)]
#[derive(PartialEq)]
#[allow(clippy::upper_case_acronyms)]
pub enum DataType {
    ARRAY,
    STRING,
    TABLE,
    TEXTURE,
}

#[pymethods]
impl DataType {
    #[getter]
    fn value(&self) -> &'static str {
        match self {
            DataType::ARRAY => "ARRAY",
            DataType::STRING => "STRING",
            DataType::TABLE => "TABLE",
            DataType::TEXTURE => "TEXTURE",
        }
    }
}

/// `goofi.InputSlot(dtype, required=False, trigger=True, multi=False)` — the per-slot options a
/// bare `goofi.DataType` has nowhere to put; the defaults are the bare form's behaviour. A `multi`
/// slot reaches `process` as `list[tuple[str, Data]]`, one entry per wire, in wire order.
#[pyclass]
pub struct InputSlot {
    #[pyo3(get)]
    pub dtype: Py<DataType>,
    #[pyo3(get)]
    pub required: bool,
    #[pyo3(get)]
    pub trigger: bool,
    #[pyo3(get)]
    pub multi: bool,
}

#[pymethods]
impl InputSlot {
    #[new]
    #[pyo3(signature = (dtype, required=false, trigger=true, multi=false))]
    fn new(dtype: Py<DataType>, required: bool, trigger: bool, multi: bool) -> InputSlot {
        InputSlot { dtype, required, trigger, multi }
    }
}

/// `show=("mode", ["iir", "fir"])`: the inspector shows the param only while the param `mode`
/// (or `group.mode`) holds one of the values, compared as text.
pub type Show = Option<(String, Vec<String>)>;

/// A `show=` value as Python writes it: `True` and `4` compare as `"true"` and `"4"`.
#[derive(FromPyObject)]
enum ShowValue {
    Bool(bool),
    Int(i64),
    Str(String),
}

type ShowArg = Option<(String, Vec<ShowValue>)>;

fn show_text(show: ShowArg) -> Show {
    let text = |v: ShowValue| match v {
        ShowValue::Bool(b) => b.to_string(),
        ShowValue::Int(i) => i.to_string(),
        ShowValue::Str(s) => s,
    };
    show.map(|(param, values)| (param, values.into_iter().map(text).collect()))
}

/// A number as Python writes it: one, or one per dimension.
#[derive(FromPyObject)]
pub enum NumDefault {
    One(f64),
    Many(Vec<f64>),
}

impl NumDefault {
    fn values(self) -> Vec<f64> {
        match self {
            NumDefault::One(v) => vec![v],
            NumDefault::Many(v) => v,
        }
    }
}

/// `goofi.NumParam(default, min, max, int=False, options=None, …)` — a number, or with a sequence
/// as `default` a vector of them, inside bounds they all share. `int=True` rounds every value.
#[pyclass]
pub struct NumParam {
    #[pyo3(get)]
    pub default: Vec<f64>,
    #[pyo3(get)]
    pub min: f64,
    #[pyo3(get)]
    pub max: f64,
    #[pyo3(get)]
    pub int: bool,
    #[pyo3(get)]
    pub options: Vec<i64>,
    #[pyo3(get)]
    pub color: bool,
    #[pyo3(get)]
    pub doc: Option<String>,
    #[pyo3(get)]
    pub expression: Option<String>,
    #[pyo3(get)]
    pub show: Show,
}

#[pymethods]
impl NumParam {
    #[new]
    #[pyo3(signature = (default, min, max, int=false, options=Vec::new(), doc=None, expression=None, show=None))]
    // The arguments ARE the Python signature, each a keyword an author writes.
    #[allow(clippy::too_many_arguments)]
    fn new(default: NumDefault, min: f64, max: f64, int: bool, options: Vec<i64>, doc: Option<String>, expression: Option<String>, show: ShowArg) -> PyResult<NumParam> {
        let default = default.values();
        if default.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err("a NumParam default is a number or a sequence of them"));
        }
        Ok(NumParam { default, min, max, int, options, color: false, doc, expression, show: show_text(show) })
    }
}

/// `goofi.ColorParam(default=(1, 1, 1, 1), …)` — an RGBA colour: a four-dimensional number from
/// 0 to 1 that the inspector picks.
#[pyclass]
pub struct ColorParam {
    #[pyo3(get)]
    pub default: Vec<f64>,
    #[pyo3(get)]
    pub doc: Option<String>,
    #[pyo3(get)]
    pub expression: Option<String>,
    #[pyo3(get)]
    pub show: Show,
}

#[pymethods]
impl ColorParam {
    #[new]
    #[pyo3(signature = (default=vec![1.0, 1.0, 1.0, 1.0], doc=None, expression=None, show=None))]
    fn new(default: Vec<f64>, doc: Option<String>, expression: Option<String>, show: ShowArg) -> PyResult<ColorParam> {
        if default.len() != 4 {
            return Err(pyo3::exceptions::PyValueError::new_err("a ColorParam default is four numbers, RGBA"));
        }
        Ok(ColorParam { default, doc, expression, show: show_text(show) })
    }
}

#[pyclass]
pub struct BoolParam {
    #[pyo3(get)]
    pub default: bool,
    #[pyo3(get)]
    pub doc: Option<String>,
    #[pyo3(get)]
    pub expression: Option<String>,
    #[pyo3(get)]
    pub show: Show,
}

#[pymethods]
impl BoolParam {
    #[new]
    #[pyo3(signature = (default, doc=None, expression=None, show=None))]
    fn new(default: bool, doc: Option<String>, expression: Option<String>, show: ShowArg) -> BoolParam {
        BoolParam { default, doc, expression, show: show_text(show) }
    }
}

#[pyclass]
pub struct StringParam {
    #[pyo3(get)]
    pub default: String,
    #[pyo3(get)]
    pub options: Vec<String>,
    #[pyo3(get)]
    pub refresh: bool,
    #[pyo3(get)]
    pub doc: Option<String>,
    #[pyo3(get)]
    pub expression: Option<String>,
    #[pyo3(get)]
    pub show: Show,
}

#[pymethods]
impl StringParam {
    #[new]
    #[pyo3(signature = (default, options=None, refresh=false, doc=None, expression=None, show=None))]
    fn new(default: String, options: Option<Vec<String>>, refresh: bool, doc: Option<String>, expression: Option<String>, show: ShowArg) -> StringParam {
        StringParam { default, options: options.unwrap_or_default(), refresh, doc, expression, show: show_text(show) }
    }
}

/// `goofi.PulseParam(doc=None)` — a request with no value; `pulse_<group>_<name>(self)` answers it.
#[pyclass]
pub struct PulseParam {
    #[pyo3(get)]
    pub doc: Option<String>,
    #[pyo3(get)]
    pub show: Show,
}

#[pymethods]
impl PulseParam {
    #[new]
    #[pyo3(signature = (doc=None, show=None))]
    fn new(doc: Option<String>, show: ShowArg) -> PulseParam {
        PulseParam { doc, show: show_text(show) }
    }
}
