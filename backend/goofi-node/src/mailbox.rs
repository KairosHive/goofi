//! A bound param's latest-wins mailboxes and the ONE rule for reading them — shared by every
//! engine that holds a binding on a thread of its own.

use goofi_core::{Data, Param};
use indexmap::IndexMap;

use crate::{BindingId, Local};

/// One variable's cell. It holds a [`Local`] because that is what the evaluator's locals channel
/// takes: a `variables.*` term is a scalar, an `nd()` term a whole frame.
#[derive(Clone, Debug, Default)]
pub struct Mailbox {
    value: Option<Local>,
    /// Why the graph could not resolve this variable; an arrival clears it.
    unresolved: Option<String>,
}

impl Mailbox {
    /// A variable awaiting its first arrival. Empty is not an error: the literal stands.
    pub fn empty() -> Mailbox {
        Mailbox::default()
    }
    /// A variable the graph resolved and delivered inline (a `variables.*` read).
    pub fn seeded(value: Param) -> Mailbox {
        Mailbox { value: Some(Local::Value(value)), unresolved: None }
    }
    /// A variable the graph could not resolve.
    pub fn missing(reason: impl Into<String>) -> Mailbox {
        Mailbox { value: None, unresolved: Some(reason.into()) }
    }
    pub fn put(&mut self, value: Local) {
        self.value = Some(value);
        self.unresolved = None;
    }
    pub fn value(&self) -> Option<&Local> {
        self.value.as_ref()
    }
    pub fn unresolved(&self) -> Option<&str> {
        self.unresolved.as_deref()
    }
}

/// One resolved variable as a node's thread receives it: a producer's service to subscribe, a
/// value delivered inline, or the reason the graph could not resolve it. `held` is the door of a
/// producer that keeps its last frame for a late subscriber, rung once the subscription is open.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Var {
    Stream { service: String, held: Option<String> },
    Value(Param),
    Missing(String),
}

/// Split an optional flat array index from a bare source.
pub fn split_index(source: &str) -> Result<(&str, Option<usize>), String> {
    let Some((base, tail)) = source.split_once('[') else {
        return Ok((source, None));
    };
    let index = tail.strip_suffix(']')
        .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|text| text.parse::<usize>().ok())
        .ok_or_else(|| format!("invalid index in `{source}`"))?;
    Ok((base, Some(index)))
}

/// One arrival as the frame it is: the element `index` names, or the frame whole. The one rule a
/// bare source copies by, so a tap and a binding read a producer alike.
pub fn pick(local: &Local, index: Option<usize>) -> Result<Data, String> {
    let frame = match local {
        Local::Value(value) => goofi_core::control::data_of(value).ok_or_else(|| format!("`{value:?}` holds nothing"))?,
        Local::Frame(frame) => frame.clone(),
    };
    match (frame.value(), index) {
        (goofi_core::Value::Texture(_), _) => Err("a bare source cannot read an unrendered texture submission".into()),
        (goofi_core::Value::Table(_), _) => Err("a bare source cannot read a TABLE output".into()),
        (goofi_core::Value::Array(a), Some(at)) => {
            let x = a.values().nth(at).ok_or_else(|| format!("index {at} is outside frame {:?}", a.shape()))?;
            Ok(Data::number(f64::from(x)))
        }
        (goofi_core::Value::Str(_), Some(_)) => Err("a STRING output has no element to index".into()),
        (_, None) => Ok(frame),
    }
}

/// A bound param's expression as a thread holds it: the rewritten source, the evaluator's handle
/// for it, and a mailbox per variable it names.
#[derive(Clone, Debug)]
pub struct Expression {
    pub source: String,
    /// `None` for a BARE source — one target, an optional index — which needs no evaluator.
    pub id: Option<BindingId>,
    pub vars: IndexMap<String, Mailbox>,
}

impl Expression {
    pub fn new(source: impl Into<String>, id: Option<BindingId>, vars: impl IntoIterator<Item = (String, Var)>) -> Expression {
        let vars = vars
            .into_iter()
            .map(|(name, v)| {
                let mailbox = match v {
                    Var::Stream { .. } => Mailbox::empty(),
                    Var::Value(value) => Mailbox::seeded(value),
                    Var::Missing(reason) => Mailbox::missing(reason),
                };
                (name, mailbox)
            })
            .collect();
        Expression { source: source.into(), id, vars }
    }

    /// Keep what a surviving variable already holds — for the variables `same` names, which is the
    /// caller's "still the same producer": a variable re-pointed at another stream starts empty,
    /// or a silent producer would stand in for the one it replaced.
    pub fn carry(&mut self, previous: &Expression, same: impl Fn(&str) -> bool) {
        for (name, mailbox) in &mut self.vars {
            if mailbox.value().is_some() || mailbox.unresolved().is_some() || !same(name) {
                continue;
            }
            if let Some(held) = previous.vars.get(name).and_then(Mailbox::value) {
                mailbox.put(held.clone());
            }
        }
    }

    /// Land a producer's frame in the variable named `var`.
    pub fn deliver(&mut self, var: &str, frame: Data) {
        if let Some(mailbox) = self.vars.get_mut(var) {
            mailbox.put(Local::Frame(frame));
        }
    }

    /// What the expression reads now: `Ok(None)` while a variable has not arrived (the literal
    /// stands), `Err` when one cannot be resolved. A bare source is its one arrival, picked in
    /// Rust; a computed one is every arrival, for the evaluator.
    pub fn inputs(&self) -> Result<Option<Inputs>, String> {
        if let Some(reason) = self.vars.values().find_map(Mailbox::unresolved) {
            return Err(reason.to_string());
        }
        if self.vars.values().any(|m| m.value().is_none()) {
            return Ok(None);
        }
        if self.id.is_some() {
            let locals = self.vars.iter().map(|(n, m)| (n.clone(), m.value().cloned().expect("arrived"))).collect();
            return Ok(Some(Inputs::Computed(locals)));
        }
        let (variable, index) = split_index(self.source.trim())?;
        let held = self.vars.get(variable).and_then(Mailbox::value).ok_or_else(|| format!("`{}` is not one target", self.source))?;
        pick(held, index).map(|frame| Some(Inputs::Bare(frame)))
    }
}

/// What an expression has to read: the frame a bare source is, or the locals a computed one takes.
pub enum Inputs {
    Bare(Data),
    Computed(Vec<(String, Local)>),
}
