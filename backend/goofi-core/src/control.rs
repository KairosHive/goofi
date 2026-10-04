//! Control data: a variable, a widget's value, a param's literal and a machine's attribute are
//! one `Data`, an array or a string, read into a param's declared kind by the one rule here.

use serde::de::Error as _;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Data, Meta, Param, Value};

/// A literal as a document spells it; the shape of a list is the shape of the array it reads as.
#[derive(Deserialize)]
#[serde(untagged)]
enum Literal {
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Literal>),
}

impl Literal {
    /// Flatten into `out`, answering the shape below this level; a ragged list has none.
    fn flatten(self, out: &mut Vec<f64>) -> Result<Vec<usize>, String> {
        match self {
            Literal::Bool(b) => out.push(if b { 1.0 } else { 0.0 }),
            Literal::Num(v) => out.push(v),
            Literal::Str(_) => return Err("a list holds numbers, not strings".into()),
            Literal::List(items) => {
                let len = items.len();
                let mut inner: Option<Vec<usize>> = None;
                for item in items {
                    let shape = item.flatten(out)?;
                    if inner.get_or_insert_with(|| shape.clone()) != &shape {
                        return Err("a ragged list has no shape".into());
                    }
                }
                let mut shape = vec![len];
                shape.extend(inner.unwrap_or_default());
                return Ok(shape);
            }
        }
        Ok(Vec::new())
    }
}

/// The literal of an array or a string: a `[1]` array is a number, a wider one nested lists, a
/// string a string. A texture or a table has no literal and writes as `null`.
impl Serialize for Data {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.value() {
            Value::Str(text) => s.serialize_str(text),
            Value::Array(a) => {
                let values: Vec<f32> = a.values().collect();
                match a.shape() {
                    [1] => number(values[0]).serialize(s),
                    shape => Nested(shape, &values).serialize(s),
                }
            }
            _ => s.serialize_none(),
        }
    }
}

/// One level of a literal's nesting: the values of `shape`, written as lists of lists.
struct Nested<'a>(&'a [usize], &'a [f32]);

impl Serialize for Nested<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let Nested(shape, values) = *self;
        let mut seq = s.serialize_seq(Some(shape.first().copied().unwrap_or(0)))?;
        match shape {
            [_] | [] => {
                for v in values {
                    seq.serialize_element(&number(*v))?;
                }
            }
            [n, rest @ ..] => {
                for chunk in values.chunks((values.len() / n.max(&1)).max(1)) {
                    seq.serialize_element(&Nested(rest, chunk))?;
                }
            }
        }
        seq.end()
    }
}

/// A whole number written without a fraction, so a document reads as it was typed.
fn number(v: f32) -> serde_json::Value {
    let v = precise(v);
    match v.fract() == 0.0 && v.abs() < 9.007_199_254_740_992e15 {
        true => serde_json::Value::from(v as i64),
        false => serde_json::Value::from(v),
    }
}

/// An f32 as the f64 its shortest decimal denotes: `0.97`, not the widened `0.9700000286102295`.
/// Every read of a number goes through here, so a literal reads back as it was typed.
fn precise(v: f32) -> f64 {
    v.to_string().parse().unwrap_or_else(|_| f64::from(v))
}

impl<'de> Deserialize<'de> for Data {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Data, D::Error> {
        match Literal::deserialize(d)? {
            Literal::Str(text) => Ok(Data::text(text)),
            literal => {
                let mut values = Vec::new();
                let shape = literal.flatten(&mut values).map_err(D::Error::custom)?;
                let bytes = values.iter().flat_map(|v| (*v as f32).to_le_bytes()).collect();
                Data::array_f32(shape, bytes, Meta::default()).map_err(D::Error::custom)
            }
        }
    }
}

impl Data {
    /// A `[1]` array.
    pub fn number(v: f64) -> Data {
        Data::numbers([v])
    }
    /// A `[n]` array of `values`.
    pub fn numbers(values: impl IntoIterator<Item = f64>) -> Data {
        let bytes: Vec<u8> = values.into_iter().flat_map(|v| (v as f32).to_le_bytes()).collect();
        let shape = vec![bytes.len() / 4];
        Data::array_f32(shape, bytes, Meta::default()).expect("a flat array")
    }
    /// A string with no stamps.
    pub fn text(s: impl Into<std::sync::Arc<str>>) -> Data {
        Data::string(s, Meta::default())
    }
}

/// The numbers a frame reads as: an array's elements, or a string parsed as a JSON number or
/// list, or as bare numbers set apart by commas or spaces. A string that is none of these is empty.
pub fn numbers(d: &Data) -> Box<dyn Iterator<Item = f64> + '_> {
    match d.value() {
        Value::Array(a) => Box::new(a.values().map(precise)),
        Value::Str(s) => {
            let text = s.trim();
            let parsed: Vec<f64> = match serde_json::from_str::<Data>(text) {
                Ok(parsed) if !matches!(parsed.value(), Value::Str(_)) => numbers(&parsed).collect(),
                _ => text
                    .trim_matches(|c| c == '[' || c == ']' || c == '(' || c == ')')
                    .split([',', ' '])
                    .filter_map(|t| t.trim().parse::<f64>().ok())
                    .collect(),
            };
            Box::new(parsed.into_iter())
        }
        _ => Box::new(std::iter::empty()),
    }
}

/// The one number a frame reads as: its first, or 0 where it has none.
pub fn number_of(d: &Data) -> f64 {
    numbers(d).next().unwrap_or(0.0)
}

/// Whether a frame reads as true: a non-empty string, or an array with any element above zero.
pub fn truth(d: &Data) -> bool {
    match d.value() {
        Value::Str(s) => !s.is_empty(),
        Value::Array(a) => a.values().any(|v| v > 0.0),
        _ => false,
    }
}

/// The text a frame reads as: the string, or the array's literal printed.
pub fn text(d: &Data) -> String {
    match d.value() {
        Value::Str(s) => s.to_string(),
        _ => serde_json::to_string(d).unwrap_or_default(),
    }
}

/// What a frame is, in the words a refusal uses.
pub fn form(d: &Data) -> String {
    match d.value() {
        Value::Str(_) => "a string".to_string(),
        Value::Array(a) => format!("an array of shape {:?}", a.shape()),
        other => format!("a {}", crate::DTYPE_NAMES[other.dtype_tag() as usize].to_lowercase()),
    }
}

/// `d` read into `target`'s declared kind: a number or vector is the numbers, filled or cut to
/// the target's dimensions and rounded for an int; a bool and a pulse's gate are the truth; a
/// string is the text. The one conversion from the data plane to a param.
pub fn read(d: &Data, target: &Param) -> Param {
    match target {
        Param::Num { value, vmin, vmax, int, options, color } => {
            // One element fills every dimension; fewer than the dimensions fill with the last.
            let got: Vec<f64> = numbers(d).take(value.len().max(1)).collect();
            let fill = got.last().copied().unwrap_or(0.0);
            let value: Vec<f64> = (0..value.len().max(1))
                .map(|i| if got.len() == 1 { fill } else { got.get(i).copied().unwrap_or(fill) })
                .map(|v| if *int { v.round() } else { v })
                .collect();
            Param::Num { value, vmin: *vmin, vmax: *vmax, int: *int, options: options.clone(), color: *color }
        }
        Param::Bool { .. } | Param::Pulse => Param::Bool { value: truth(d) },
        Param::Str { options, refresh, .. } => Param::Str { value: text(d), options: options.clone(), refresh: *refresh },
    }
}

/// The inverse of [`read`]: what a param holds, as the frame it is. A pulse holds nothing.
pub fn data_of(p: &Param) -> Option<Data> {
    match p {
        Param::Num { value, .. } => Some(Data::numbers(value.iter().copied())),
        Param::Bool { value } => Some(Data::number(if *value { 1.0 } else { 0.0 })),
        Param::Str { value, .. } => Some(Data::text(value.as_str())),
        Param::Pulse => None,
    }
}
