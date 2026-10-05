//! State machines: the model the document carries, and the stepping that moves each playhead
//! through it and answers what its variables hold now.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use goofi_core::control;
use goofi_core::ease::Curve;
use goofi_core::time::{seconds, ticks, Tick};
use goofi_core::variables::{is_valid_identifier, Control, ControlKind, Entries};
use goofi_core::{Data, Meta, Value};
use goofi_node::{EvalCtx, ExprEvaluator, Local, RetainedExpression};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::expr_rewrite::{self, Target};
use crate::variable_projection::ReadFault;

mod output;

/// The elements of a playhead's group that are the machine's own, and no attribute's name.
pub const OWN: [&str; 5] = ["state", "prev", "progress", "transition", "arrived"];

/// What a name has to be, said once.
pub const NAME_RULE: &str = "a letter or underscore then letters, digits or underscores, and not a Python keyword";

fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct Machine {
    #[serde(skip)]
    #[ts(skip)]
    pub identity: goofi_core::identity::Identity,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub attributes: IndexMap<String, Attribute>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub states: IndexMap<String, State>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub transitions: IndexMap<String, Transition>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub playheads: IndexMap<String, Playhead>,
    /// What the random draws start from; absent is 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// What an attribute is, as a node's param is: a number — one, a vector, or a colour — a bool, or a
/// text, maybe one of some options. The inspector draws it with the param widget of that kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AttributeKind {
    Num {
        vmin: f64,
        vmax: f64,
        #[serde(default)]
        int: bool,
        #[serde(default)]
        color: bool,
    },
    Bool,
    #[serde(rename = "string")]
    Str {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        options: Option<Vec<String>>,
    },
}

impl AttributeKind {
    /// The kind a bare default implies: a number in 0..1, or a text.
    pub fn of(value: &Data) -> Self {
        match value.value() {
            Value::Str(_) => AttributeKind::Str { options: None },
            _ => AttributeKind::Num { vmin: 0.0, vmax: 1.0, int: false, color: false },
        }
    }

    /// Whether `value` is one this kind holds: numbers in one dimension (four for a colour), a bool
    /// as one number, a text (one of the options, where there are some).
    pub fn fits(&self, value: &Data) -> bool {
        match (self, value.value()) {
            (AttributeKind::Num { color: true, .. }, Value::Array(a)) => a.shape() == [4],
            (AttributeKind::Num { .. }, Value::Array(a)) => a.shape().len() == 1,
            (AttributeKind::Bool, Value::Array(a)) => a.shape() == [1],
            (AttributeKind::Str { options: Some(o) }, Value::Str(s)) => o.iter().any(|x| x.as_str() == s.as_ref()),
            (AttributeKind::Str { .. }, Value::Str(_)) => true,
            _ => false,
        }
    }

    pub fn mismatch(&self, value: &Data) -> String {
        let kind = match self {
            AttributeKind::Num { color: true, .. } => "colour",
            AttributeKind::Num { .. } => "num",
            AttributeKind::Bool => "bool",
            AttributeKind::Str { options: Some(_) } => "string of the options",
            AttributeKind::Str { .. } => "string",
        };
        format!("a `{kind}` attribute cannot hold {}", control::form(value))
    }

    /// The control-panel widget a playhead's variable of this kind wears, by the default's shape.
    pub fn control(&self, default: &Data) -> Control {
        let dims = match default.value() {
            Value::Array(a) => a.shape().first().copied().unwrap_or(1),
            _ => 1,
        };
        let (kind, range, options) = match self {
            AttributeKind::Num { color: true, .. } => (ControlKind::Color, None, Vec::new()),
            AttributeKind::Num { vmin, vmax, int, .. } => {
                let range = Some((*vmin, *vmax, if *int { 1.0 } else { 0.01 }));
                (if dims > 1 { ControlKind::Vector } else { ControlKind::Knob }, range, Vec::new())
            }
            AttributeKind::Bool => (ControlKind::Toggle, None, Vec::new()),
            AttributeKind::Str { options: Some(o) } => (ControlKind::Dropdown, None, o.clone()),
            AttributeKind::Str { options: None } => (ControlKind::Text, None, Vec::new()),
        };
        let (w, h) = kind.born_box();
        Control {
            kind,
            min: range.map(|r| r.0),
            max: range.map(|r| r.1),
            step: range.map(|r| r.2),
            options,
            resolution: None,
            x: 0.0,
            y: 0.0,
            w,
            h,
        }
    }
}

/// An attribute: what it is, and the default every state starts from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Attribute {
    #[serde(skip)]
    #[ts(skip)]
    pub identity: goofi_core::identity::Identity,
    #[ts(type = "Literal")]
    pub default: Data,
    pub kind: AttributeKind,
}

/// A state: where its card sits, and the attributes it sets; one left out is kept on entry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct State {
    #[serde(skip)]
    #[ts(skip)]
    pub identity: goofi_core::identity::Identity,
    #[serde(default)]
    pub pos: [f64; 2],
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[ts(type = "Record<string, Literal>")]
    pub values: IndexMap<String, Data>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[ts(type = "Record<string, Literal>")]
    pub exit_values: IndexMap<String, Data>,
    #[serde(default)]
    pub selection: Selection,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Selection {
    #[default]
    Ordered,
    Weighted,
    Uniform,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Transition {
    /// A state name, or `*` for any state.
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<Trigger>,
    /// Sampled once at departure; zero is instant.
    #[serde(default)]
    pub duration: Seconds,
    #[serde(default)]
    pub curve: Curve,
    #[serde(default = "one")]
    pub chance: f64,
    #[serde(default = "one")]
    pub weight: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<String>,
    #[serde(default)]
    pub internal: bool,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[ts(type = "Record<string, Literal>")]
    pub values: IndexMap<String, Data>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Trigger {
    Manual,
    After {
        seconds: Seconds,
    },
    When {
        expression: String,
        #[serde(default)]
        edge: Edge,
    },
    Event { name: String },
    Always,
    Meet {
        #[serde(default)]
        policy: Policy,
    },
    Alone,
}

/// A dwell: a number of seconds, or an expression over the variables read on entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum Seconds {
    Fixed(f64),
    Expression(String),
    Uniform { min: f64, max: f64 },
}

impl Default for Seconds {
    fn default() -> Self { Self::Fixed(0.0) }
}

impl Seconds {
    fn expression(&self) -> Option<&str> {
        match self { Self::Expression(text) => Some(text), _ => None }
    }

    fn check(&self) -> Result<(), String> {
        match self {
            Self::Fixed(value) if ticks(*value).is_none() => Err("seconds must be finite and at least 0".into()),
            Self::Uniform { min, max } if ticks(*min).is_none() || ticks(*max).is_none() || min > max => Err("random range must be finite, nonnegative and ordered".into()),
            Self::Expression(text) => check_expression(text),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    #[default]
    Rising,
    Falling,
    Change,
    Level,
}

/// Who takes a `meet` transition: the longest resident, the newest arrival, or every resident.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    #[default]
    Fifo,
    Lifo,
    All,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Playhead {
    #[serde(skip)]
    #[ts(skip)]
    pub identity: goofi_core::identity::Identity,
    #[serde(default)]
    pub color: String,
    pub start: String,
}

/// Why an expression cannot drive a machine: it names what is not a variable.
pub fn check_expression(expression: &str) -> Result<(), String> {
    let (_, refs) = expr_rewrite::rewrite(expression).map_err(|e| e.0)?;
    match refs.iter().find(|r| !matches!(r.target, Target::Variable { .. })) {
        Some(_) => Err(format!("`{expression}` reads a node; a machine reads variables alone — follow the output in a variable")),
        None => Ok(()),
    }
}

impl Machine {
    /// Every expression the machine evaluates.
    pub fn expressions(&self) -> impl Iterator<Item = &str> {
        self.expression_sites().map(|(_, _, text)| text)
    }

    fn expression_sites(&self) -> impl Iterator<Item = (&str, ExpressionSurface, &str)> {
        self.transitions.iter().flat_map(|(id, t)| {
            t.guard.as_deref().map(|text| (id.as_str(), ExpressionSurface::Guard, text)).into_iter()
                .chain(t.duration.expression().map(|text| (id.as_str(), ExpressionSurface::Duration, text)))
                .chain(t.triggers.iter().enumerate().filter_map(move |(index, trigger)| match trigger {
                    Trigger::When { expression, .. } => Some((id.as_str(), ExpressionSurface::When { index }, expression.as_str())),
                    Trigger::After { seconds } => seconds.expression().map(|text| (id.as_str(), ExpressionSurface::After { index }, text)),
                    _ => None,
                }))
        })
    }

    fn site(&self, playhead: &str, transition: &str, surface: ExpressionSurface) -> ExpressionSite {
        ExpressionSite { machine: self.identity.generation(), playhead: self.playheads[playhead].identity.generation(), transition: transition.into(), surface }
    }

    fn expression_at(&self, site: &ExpressionSite) -> Option<&str> {
        if self.identity.generation() != site.machine || !self.playheads.values().any(|head| head.identity.generation() == site.playhead) { return None; }
        self.expression_sites().find(|(transition, surface, _)| *transition == site.transition && *surface == site.surface).map(|(_, _, text)| text)
    }

    /// The effective priority order, without duplicating transition membership.
    pub fn outgoing(&self, state: &str) -> Vec<&str> {
        let preferred = self.states.get(state).map(|s| s.order.as_slice()).unwrap_or_default();
        preferred.iter().map(String::as_str).chain(self.transitions.iter()
            .filter(|(id, t)| leaves(t, state) && !preferred.contains(id))
            .map(|(id, _)| id.as_str())).collect()
    }

    /// Remove priorities invalidated by a transition endpoint edit or deletion.
    pub fn prune_order(&mut self) {
        for (state, record) in &mut self.states {
            record.order.retain(|id| self.transitions.get(id).is_some_and(|t| leaves(t, state)));
        }
    }

    fn check_values(&self, values: &IndexMap<String, Data>) -> Result<(), String> {
        for (name, value) in values {
            let attr = self.attributes.get(name).ok_or_else(|| format!("no attribute `{name}` in the machine"))?;
            if !attr.kind.fits(value) { return Err(format!("attribute `{name}`: {}", attr.kind.mismatch(value))); }
        }
        Ok(())
    }

    pub fn rewrite_expressions(&mut self, edit: impl Fn(&str) -> Option<String>) {
        let seconds = |value: &mut Seconds| {
            if let Seconds::Expression(text) = value {
                if let Some(next) = edit(text) { *text = next; }
            }
        };
        for transition in self.transitions.values_mut() {
            if let Some(guard) = &mut transition.guard {
                if let Some(next) = edit(guard) { *guard = next; }
            }
            seconds(&mut transition.duration);
            for trigger in &mut transition.triggers {
                match trigger {
                    Trigger::After { seconds: value } => seconds(value),
                    Trigger::When { expression, .. } => if let Some(next) = edit(expression) { *expression = next; },
                    _ => {}
                }
            }
        }
    }

    /// Whether the record holds together: every name legal, every reference to something it holds.
    pub fn check(&self) -> Result<(), String> {
        for (name, attribute) in &self.attributes {
            if !is_valid_identifier(name) {
                return Err(format!("invalid attribute name `{name}`: {NAME_RULE}"));
            }
            if OWN.contains(&name.as_str()) {
                return Err(format!("`{name}` is the machine's own element of a playhead; an attribute cannot take it"));
            }
            if !attribute.kind.fits(&attribute.default) {
                return Err(format!("attribute `{name}`: {}", attribute.kind.mismatch(&attribute.default)));
            }
            if let AttributeKind::Num { vmin, vmax, .. } = attribute.kind {
                if !vmin.is_finite() || !vmax.is_finite() || vmin > vmax {
                    return Err(format!("attribute `{name}`: range must be finite and ordered"));
                }
            }
        }
        for (name, state) in &self.states {
            if !is_valid_identifier(name) {
                return Err(format!("invalid state name `{name}`: {NAME_RULE}"));
            }
            if state.pos.iter().any(|x| !x.is_finite()) {
                return Err(format!("state `{name}`: position must be finite"));
            }
            self.check_values(&state.values).map_err(|why| format!("state `{name}`: {why}"))?;
            self.check_values(&state.exit_values).map_err(|why| format!("state `{name}` exit: {why}"))?;
            let mut seen = HashSet::new();
            for id in &state.order {
                if !seen.insert(id) || !self.transitions.get(id).is_some_and(|t| leaves(t, name)) {
                    return Err(format!("state `{name}`: order must contain unique outgoing transition IDs"));
                }
            }
        }
        for (id, t) in &self.transitions {
            if t.from != "*" && !self.states.contains_key(&t.from) {
                return Err(format!("transition `{id}` leaves `{}`, which is no state of the machine", t.from));
            }
            if !self.states.contains_key(&t.to) {
                return Err(format!("transition `{id}` enters `{}`, which is no state of the machine", t.to));
            }
            t.duration.check().map_err(|why| format!("transition `{id}` duration: {why}"))?;
            if !t.chance.is_finite() || !(0.0..=1.0).contains(&t.chance) {
                return Err(format!("transition `{id}`: chance must be in 0..1"));
            }
            if !t.weight.is_finite() || t.weight < 0.0 {
                return Err(format!("transition `{id}`: weight must be finite and at least 0"));
            }
            if t.internal && (t.from != t.to || t.duration != Seconds::Fixed(0.0)) {
                return Err(format!("transition `{id}`: internal requires a self-transition with zero duration"));
            }
            self.check_values(&t.values).map_err(|why| format!("transition `{id}`: {why}"))?;
            if let Some(guard) = &t.guard { check_expression(guard).map_err(|why| format!("transition `{id}` guard: {why}"))?; }
            for trigger in &t.triggers {
                match trigger {
                    Trigger::After { seconds } => seconds.check().map_err(|why| format!("transition `{id}` after: {why}"))?,
                    Trigger::When { expression: e, .. } => {
                        check_expression(e).map_err(|why| format!("transition `{id}`: {why}"))?;
                    }
                    Trigger::Event { name } if name.trim().is_empty() => return Err(format!("transition `{id}`: event name cannot be empty")),
                    _ => {}
                }
            }
        }
        for (name, p) in &self.playheads {
            if !is_valid_identifier(name) {
                return Err(format!("invalid playhead name `{name}`: {NAME_RULE}"));
            }
            if !self.states.contains_key(&p.start) {
                return Err(format!("playhead `{name}` starts in `{}`, which is no state of the machine", p.start));
            }
        }
        Ok(())
    }

    /// The variables a playhead's group holds: runtime facts, then one per attribute,
    /// each born as the start state has it — at rest there, before the machine has moved it.
    pub fn playhead_entries(&self, playhead: &str) -> Entries {
        let Some(p) = self.playheads.get(playhead) else { return Vec::new() };
        born(self, &p.start, 0, &mut Events::default()).entries(playhead, 0).into_iter()
            .map(|(key, value)| {
                let control = key.split_once('.').and_then(|(_, name)| self.attributes.get(name))
                    .map(|attr| attr.kind.control(&attr.default));
                (key, value, control)
            }).collect()
    }
}

/// One expression as the machines run it: compiled once per text, its variables by name.
struct Expr {
    rewritten: String,
    refs: Vec<(String, String)>,
    code: Option<RetainedExpression>,
    error: Option<String>,
}

#[derive(Clone, PartialEq)]
struct Flight {
    id: String,
    from: String,
    start: Tick,
    end: Tick,
    curve: Curve,
    target: IndexMap<String, Data>,
}

impl Flight {
    fn progress(&self, at: Tick) -> f64 {
        (at.saturating_sub(self.start) as f64 / (self.end - self.start) as f64).min(1.0)
    }
}

/// An independent trigger, with the source that armed it. Edits replace its armed state.
#[derive(Clone, PartialEq)]
struct Armed {
    trigger: Trigger,
    due: Option<Tick>,
    gate: Option<bool>,
    interval: Option<Tick>,
}

/// One playhead's run: where it is, what it holds, and what is armed for it.
#[derive(Clone, PartialEq)]
struct Head {
    state: String,
    /// At rest, the held values; in flight, the values at departure.
    held: IndexMap<String, Data>,
    arrived: Tick,
    flight: Option<Flight>,
    armed: IndexMap<(String, usize), Armed>,
    arm_pending: Option<bool>,
    stopped: Option<String>,
}

/// Runtime failures, projected into the control document without changing the patch.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct Health {
    pub expressions: Vec<ExpressionIssue>,
    pub playheads: IndexMap<String, String>,
}

/// The authored surface that evaluates an expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpressionSurface {
    Guard,
    Duration,
    When { index: usize },
    After { index: usize },
}

/// One standing expression failure, projected from its runtime owner and current model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ExpressionIssue {
    pub playhead: Option<String>,
    pub transition: String,
    pub surface: ExpressionSurface,
    pub expression: String,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ExpressionSite {
    machine: u64,
    playhead: u64,
    transition: String,
    surface: ExpressionSurface,
}

impl Head {
    fn values(&self, at: Tick) -> IndexMap<String, Data> {
        let Some(flight) = &self.flight else { return self.held.clone() };
        let t = flight.curve.at(flight.progress(at));
        self.held.iter().map(|(a, v)| {
            (a.clone(), flight.target.get(a).map_or_else(|| v.clone(), |target| blend(v, target, t)))
        }).collect()
    }

    fn entries(&self, ph: &str, at: Tick) -> Vec<(String, Data)> {
        let flight = self.flight.as_ref();
        let mut out = vec![
            (format!("{ph}.state"), Data::text(self.state.as_str())),
            (format!("{ph}.prev"), Data::text(flight.map_or("", |f| f.from.as_str()))),
            (format!("{ph}.progress"), Data::number(flight.map_or(1.0, |f| f.progress(at)))),
            (format!("{ph}.transition"), Data::text(flight.map_or("", |f| f.id.as_str()))),
            (format!("{ph}.arrived"), Data::text(self.arrived.to_string())),
        ];
        out.extend(self.values(at).into_iter().map(|(a, v)| (format!("{ph}.{a}"), v)));
        out
    }

}

#[derive(Clone, PartialEq)]
struct Run {
    heads: IndexMap<String, Head>,
    rng: u64,
    events: Events,
}

/// What a tick found: who arrived where, and who left what.
#[derive(Clone, Default, PartialEq)]
struct Events {
    arrived: Vec<String>,
    left: Vec<String>,
    named: Vec<(String, Option<String>)>,
}

pub enum Effect {
    Fire { machine: String, playhead: String, transition: String },
    Jump { machine: String, playhead: String, state: String },
    Reset { machine: Option<String> },
    Event { machine: String, name: String, playhead: Option<String> },
}

/// The machines as they run: the model, each playhead's run, and the expressions compiled for
/// them. Pure but for the evaluator: clocked by the time it is handed, reading the frames it is
/// handed, answering the writes it owes.
#[derive(Default)]
pub struct Machines {
    outputs: output::Outputs,
    projection: Arc<crate::variable_projection::VariableProjection>,
    config: IndexMap<String, Machine>,
    runs: IndexMap<String, Run>,
    evaluator: Option<Arc<dyn ExprEvaluator>>,
    compiled: HashMap<String, Expr>,
    requests: Vec<(Tick, Effect)>,
    reconcile: Option<Tick>,
    expression_errors: IndexMap<ExpressionSite, String>,
    inputs: IndexMap<String, Data>,
    input_errors: IndexMap<String, String>,
    producers: IndexMap<String, u64>,
    at: Option<Tick>,
    round: Option<DecisionRound>,
}

impl Machines {
    pub fn matches_config(&self, config: &IndexMap<String, Machine>) -> bool { &self.config == config }
    pub fn input_error(&mut self, key: String, error: Option<String>) {
        match error { Some(error) => { self.input_errors.insert(key, error); }, None => { self.input_errors.shift_remove(&key); } }
    }
    pub fn logical_time(&self) -> Option<Tick> { self.at }
    pub fn pending_time(&self) -> Option<Tick> { self.round.as_ref().filter(|round| !round.pending.is_empty()).map(|round| round.at) }
    pub fn pending_inputs(&self) -> Vec<(String, Tick)> { self.round.as_ref().map(|round| round.pending.iter().map(|(source, at)| (source.clone(), *at)).collect()).unwrap_or_default() }
    pub fn matches_projection(&self, graph: &crate::Graph) -> bool { graph.matches_variable_projection(&self.projection) }
    pub fn is_empty(&self) -> bool {
        self.config.is_empty()
    }

    /// Replace the model. A playhead still named keeps its place and what it holds; one whose
    /// state is gone starts over. Takes effect on the next advance.
    pub fn configure(&mut self, at: Tick, config: IndexMap<String, Machine>, evaluator: Option<Arc<dyn ExprEvaluator>>, producers: IndexMap<String, u64>, projection: Arc<crate::variable_projection::VariableProjection>) {
        // The old model owns every instant before this edit, for every caller.
        if !self.config.is_empty() && self.at.is_none_or(|previous| previous < at)
            && self.reconcile.is_none_or(|pending| pending < at)
        {
            let inputs = self.inputs.clone();
            let _ = self.advance(at.saturating_sub(1), &|key| inputs.get(key).cloned());
        }
        if self.config == config && self.producers == producers && self.projection.same_config(&projection) && evaluator.as_ref().map(Arc::as_ptr) == self.evaluator.as_ref().map(Arc::as_ptr) {
            return;
        }
        self.round = None;
        let moved = self.preserve_producers(&producers, &projection);
        self.projection = projection;
        self.expression_errors.retain(|site, _| {
            let Some(text) = self.config.values().find_map(|machine| machine.expression_at(site)) else { return false };
            let next = expr_rewrite::rename_variables(text, |key| moved.get(key).filter(|to| *to != key).cloned());
            config.values().find_map(|machine| machine.expression_at(site)) == Some(next.as_deref().unwrap_or(text))
        });
        self.producers = producers;
        self.preserve(&config, &moved);
        let wanted: HashSet<String> = config.values().flat_map(Machine::expressions).map(str::to_string).collect();
        if evaluator.as_ref().map(Arc::as_ptr) != self.evaluator.as_ref().map(Arc::as_ptr) {
            self.compiled.clear();
        }
        self.evaluator = evaluator;
        self.compiled.retain(|text, _| wanted.contains(text));
        for text in wanted {
            if !self.compiled.contains_key(&text) {
                let expr = self.compile(&text);
                self.compiled.insert(text, expr);
            }
        }
        self.config = config;
        let keys: HashSet<_> = self.keys().into_iter().collect();
        self.input_errors.retain(|key, _| keys.contains(key));
        self.reconcile = Some(at);
    }

    /// A renamed producer keeps its timers and condition baselines; a replaced one rearms them.
    fn preserve_producers(&mut self, producers: &IndexMap<String, u64>, projection: &crate::variable_projection::VariableProjection) -> HashMap<String, String> {
        let names: HashMap<_, _> = producers.iter().map(|(name, birth)| (*birth, name)).collect();
        let moved: HashMap<_, _> = self.producers.iter().filter_map(|(from, birth)| {
            names.get(birth).map(|to| (from.clone(), (*to).clone()))
        }).collect();
        let previous = &self.producers;
        for run in self.runs.values_mut() {
            for head in run.heads.values_mut() {
                head.armed.retain(|_, armed| {
                    let text = match &mut armed.trigger {
                        Trigger::After { seconds: Seconds::Expression(text) } | Trigger::When { expression: text, .. } => text,
                        _ => return true,
                    };
                    let Ok((_, refs)) = expr_rewrite::rewrite(text) else { return true };
                    let same = refs.iter().all(|reference| match &reference.target {
                        Target::Variable { key } => {
                            let next = moved.get(key).unwrap_or(key);
                            previous.get(key) == producers.get(next) && self.projection.equivalent(key, projection, next)
                        }
                        _ => true,
                    });
                    if same {
                        if let Some(next) = expr_rewrite::rename_variables(text, |key| moved.get(key).filter(|to| *to != key).cloned()) { *text = next; }
                    }
                    same
                });
            }
        }
        moved
    }

    fn preserve(&mut self, config: &IndexMap<String, Machine>, moved: &HashMap<String, String>) {
        let mut previous = std::mem::take(&mut self.runs);
        for (name, machine) in config {
            let Some((old_name, old)) = self.config.iter().find(|(_, old)| old.identity == machine.identity) else { continue };
            let Some(mut run) = previous.shift_remove(old_name) else { continue };
            let states = renamed(&old.states, &machine.states, |record| &record.identity);
            let attributes = renamed(&old.attributes, &machine.attributes, |record| &record.identity);
            let playheads = renamed(&old.playheads, &machine.playheads, |record| &record.identity);
            if old.seed.unwrap_or(0) != machine.seed.unwrap_or(0) { run.rng = seed_state(machine.seed.unwrap_or(0)); }
            let mut prior = old.clone();
            prior.rewrite_expressions(|text| expr_rewrite::rename_variables(text, |key| moved.get(key).filter(|to| *to != key).cloned()));
            prior.states = rekey(prior.states, &states);
            prior.attributes = rekey(prior.attributes, &attributes);
            prior.playheads = rekey(prior.playheads, &playheads);
            for state in prior.states.values_mut() {
                state.values = rekey(std::mem::take(&mut state.values), &attributes);
                state.exit_values = rekey(std::mem::take(&mut state.exit_values), &attributes);
            }
            for transition in prior.transitions.values_mut() {
                if let Some(name) = states.get(&transition.from) { transition.from = name.clone(); }
                if let Some(name) = states.get(&transition.to) { transition.to = name.clone(); }
                transition.values = rekey(std::mem::take(&mut transition.values), &attributes);
            }
            for playhead in prior.playheads.values_mut() {
                if let Some(name) = states.get(&playhead.start) { playhead.start = name.clone(); }
            }
            let cosmetic = |mut model: Machine| {
                for state in model.states.values_mut() { state.pos = [0.0, 0.0]; }
                for playhead in model.playheads.values_mut() { playhead.color.clear(); }
                model
            };
            let changed = cosmetic(prior) != cosmetic(machine.clone());
            run.heads = rekey(run.heads, &playheads);
            for head in run.heads.values_mut() {
                if changed { head.stopped = None; }
                head.state = states.get(&head.state).cloned().unwrap_or_default();
                head.held = rekey(std::mem::take(&mut head.held), &attributes);
                if let Some(flight) = &mut head.flight {
                    if let Some(from) = states.get(&flight.from) { flight.from = from.clone(); }
                    flight.target = rekey(std::mem::take(&mut flight.target), &attributes);
                }
            }
            for state in &mut run.events.arrived { if let Some(next) = states.get(state) { *state = next.clone(); } }
            for state in &mut run.events.left { if let Some(next) = states.get(state) { *state = next.clone(); } }
            self.runs.insert(name.clone(), run);
        }
    }

    fn compile(&self, text: &str) -> Expr {
        let none = |error: String| Expr { rewritten: String::new(), refs: Vec::new(), code: None, error: Some(error) };
        let (rewritten, refs) = match expr_rewrite::rewrite(text) {
            Ok(r) => r,
            Err(e) => return none(e.0),
        };
        let refs = refs
            .into_iter()
            .map(|r| match r.target {
                Target::Variable { key } => (r.var, key),
                _ => (r.var, String::new()),
            })
            .collect();
        if expr_rewrite::is_bare(&rewritten) {
            return Expr { rewritten, refs, code: None, error: None };
        }
        match &self.evaluator {
            None => none("no expression evaluator available".to_string()),
            Some(ev) => match RetainedExpression::compile(ev.clone(), &rewritten) {
                Ok(code) => Expr { rewritten, refs, code: Some(code), error: None },
                Err(e) => none(e.0),
            },
        }
    }

    /// Take an explicit transition at its effective patch time. Validate before drawing.
    pub fn fire(&mut self, at: Tick, machine: &str, playhead: &str, transition: &str, read: &dyn Fn(&str) -> Option<Data>) -> Result<(), String> {
        self.check_time(at)?;
        let _ = self.advance_to(at, read, false);
        let accepted = self.fire_at(at, machine, playhead, transition, read).map_err(|why| why.to_string());
        self.settle(at);
        accepted
    }

    fn fire_at(&mut self, at: Tick, machine: &str, playhead: &str, transition: &str, read: &dyn Fn(&str) -> Option<Data>) -> Result<(), ReadFault> {
        let snapshot: IndexMap<String, Data> = self.runs.values().flat_map(|run| run.heads.iter().flat_map(|(ph, head)| head.entries(ph, at))).collect();
        let read = |key: &str| snapshot.get(key).cloned().or_else(|| read(key));
        let ctx = Ctx { compiled: &self.compiled, projection: &self.projection, input_errors: &self.input_errors, read: &read, now: at, outcomes: std::cell::RefCell::new(IndexMap::new()), pending: std::cell::RefCell::new(IndexMap::new()) };
        let model = self.config.get(machine).ok_or_else(|| format!("no machine `{machine}`"))?;
        let t = model.transitions.get(transition).ok_or_else(|| format!("no transition `{transition}` in machine `{machine}`"))?;
        let run = self.runs.get_mut(machine).ok_or_else(|| format!("no running machine `{machine}`"))?;
        let head = run.heads.get_mut(playhead).ok_or_else(|| format!("no running playhead `{playhead}` in machine `{machine}`"))?;
        if t.internal && head.flight.is_some() {
            return Err(format!("internal transition `{transition}` requires a resident playhead").into());
        }
        if !leaves(t, &head.state) {
            return Err(format!("transition `{transition}` leaves `{}`, but playhead `{playhead}` is in `{}`", t.from, head.state).into());
        }
        if let Some(guard) = &t.guard {
            let result = ctx.gate_result(guard, model.site(playhead, transition, ExpressionSurface::Guard)).map_err(|why| match why {
                ReadFault::Failed(why) => ReadFault::Failed(format!("transition `{transition}` guard: {why}")),
                pending => pending,
            });
            record_evaluations(&mut self.expression_errors, std::mem::take(&mut *ctx.outcomes.borrow_mut()));
            if !result? { return Err(format!("transition `{transition}` guard is false").into()); }
        }
        let duration = ctx.dwell_result(&t.duration, model.site(playhead, transition, ExpressionSurface::Duration), &mut run.rng);
        record_evaluations(&mut self.expression_errors, std::mem::take(&mut *ctx.outcomes.borrow_mut()));
        depart_ready(head, model, transition, at, &mut run.events, duration?);
        head.stopped = None;
        let owner = model.playheads[playhead].identity.generation();
        self.expression_errors.retain(|site, _| site.machine != model.identity.generation() || site.playhead != owner);
        record_evaluations(&mut self.expression_errors, ctx.outcomes.into_inner());
        self.record_output(at);
        Ok(())
    }

    /// Put a playhead in a state on the next advance: instant, no easing.
    pub fn jump(&mut self, at: Tick, machine: &str, playhead: &str, state: &str) -> Result<(), String> {
        self.check_time(at)?;
        let model = self.config.get(machine).ok_or_else(|| format!("no machine `{machine}`"))?;
        if !model.playheads.contains_key(playhead) { return Err(format!("no playhead `{playhead}` in machine `{machine}`")); }
        if !model.states.contains_key(state) { return Err(format!("no state `{state}` in machine `{machine}`")); }
        self.requests.push((at, Effect::Jump { machine: machine.into(), playhead: playhead.into(), state: state.into() }));
        Ok(())
    }

    /// Every playhead of a machine, or of all, back to its start state on the next advance.
    pub fn reset(&mut self, at: Tick, machine: Option<&str>) -> Result<(), String> {
        self.check_time(at)?;
        if let Some(machine) = machine.filter(|name| !self.config.contains_key(*name)) { return Err(format!("no machine `{machine}`")); }
        self.requests.push((at, Effect::Reset { machine: machine.map(str::to_string) }));
        Ok(())
    }

    /// Offer one named event to the resident playheads as a settled batch.
    pub fn event(&mut self, at: Tick, machine: &str, name: &str, playhead: Option<&str>) -> Result<(), String> {
        self.check_time(at)?;
        let model = self.config.get(machine).ok_or_else(|| format!("no machine `{machine}`"))?;
        if name.trim().is_empty() { return Err("event name cannot be empty".into()); }
        if let Some(ph) = playhead.filter(|ph| !model.playheads.contains_key(*ph)) { return Err(format!("no playhead `{ph}` in machine `{machine}`")); }
        self.requests.push((at, Effect::Event { machine: machine.into(), name: name.into(), playhead: playhead.map(str::to_string) }));
        Ok(())
    }

    fn check_time(&self, at: Tick) -> Result<(), String> {
        if self.at.is_some_and(|previous| at < previous) {
            Err("an effect cannot change an instant already processed".into())
        } else {
            Ok(())
        }
    }

    /// Apply effects in order. An uncovered fire stays with its caller at the same instant.
    pub fn apply_effects(&mut self, at: Tick, effects: &mut VecDeque<Effect>, read: &dyn Fn(&str) -> Option<Data>) -> (Vec<Result<(), String>>, Option<ReadFault>) {
        if let Err(why) = self.check_time(at) {
            return (effects.drain(..).map(|_| Err(why.clone())).collect(), None);
        }
        let _ = self.advance_to(at, read, false);
        let mut answers = Vec::new();
        while let Some(effect) = effects.front() {
            let accepted = match effect {
                Effect::Fire { machine, playhead, transition } => self.fire_at(at, machine, playhead, transition, read),
                Effect::Jump { machine, playhead, state } => self.jump(at, machine, playhead, state).map_err(ReadFault::Failed),
                Effect::Reset { machine } => self.reset(at, machine.as_deref()).map_err(ReadFault::Failed),
                Effect::Event { machine, name, playhead } => self.event(at, machine, name, playhead.as_deref()).map_err(ReadFault::Failed),
            };
            if matches!(accepted, Err(ReadFault::Pending { .. })) { return (answers, accepted.err()); }
            effects.pop_front();
            self.apply_requests(at);
            answers.push(accepted.map_err(|why| why.to_string()));
        }
        self.settle(at);
        (answers, None)
    }

    /// The owner that produced this playhead value. A replaced group refuses its writes.
    pub fn owner(&self, variable: &str) -> Option<&goofi_core::identity::Identity> {
        let (group, _) = variable.split_once('.')?;
        self.config.values().find_map(|machine| machine.playheads.get(group).map(|head| &head.identity))
    }

    /// Every variable the machines' expressions read — what an advance wants handed to it.
    pub fn keys(&self) -> Vec<String> {
        let mut leaves = HashSet::new();
        for (_, key) in self.compiled.values().flat_map(|e| &e.refs) { self.projection.leaves(key, &mut leaves); }
        let mut keys: Vec<String> = leaves.into_iter().collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// Standing failures from their runtime owners, with the model that produced them.
    pub fn health(&self) -> Vec<(String, Machine, Health)> {
        self.config.iter().map(|(name, machine)| {
            let mut expressions: Vec<_> = machine.expression_sites().filter_map(|(transition, surface, text)| {
                let error = self.compiled.get(text)?.error.as_ref()?;
                Some(ExpressionIssue { playhead: None, transition: transition.into(), surface, expression: text.into(), error: error.clone() })
            }).collect();
            expressions.extend(self.expression_errors.iter().filter_map(|(site, error)| {
                let text = machine.expression_at(site)?;
                if self.compiled.get(text).is_some_and(|expr| expr.error.is_some()) { return None; }
                let playhead = machine.playheads.iter().find(|(_, head)| head.identity.generation() == site.playhead)?.0.clone();
                Some(ExpressionIssue { playhead: Some(playhead), transition: site.transition.clone(), surface: site.surface, expression: text.into(), error: error.clone() })
            }));
            let playheads = self.runs.get(name).into_iter().flat_map(|run| run.heads.iter()).filter_map(|(ph, head)| Some((ph.clone(), head.stopped.clone()?))).collect();
            (name.clone(), machine.clone(), Health { expressions, playheads })
        }).collect()
    }
    pub fn next_deadline(&self) -> Option<Tick> {
        let scheduled = self.runs.values().filter_map(next_deadline).min();
        let observation = self.at.filter(|_| self.needs_observation()).map(|at| {
            let sampled = self.inputs.values().filter_map(goofi_core::samples::SampleSpan::of).filter_map(|span| {
                let presentation = at.checked_add(goofi_core::samples::CONTROL_DELAY)?;
                let sample = span.clock.index(presentation);
                let sample = sample.checked_add(u64::from(span.clock.at(sample) <= presentation))?;
                Some(span.clock.at(sample).saturating_sub(goofi_core::samples::CONTROL_DELAY))
            });
            sampled.chain(std::iter::once(goofi_core::time::next_control_tick(at))).min().unwrap_or(at)
        });
        scheduled.into_iter().chain(observation).min()
    }

    fn needs_observation(&self) -> bool {
        let varies = |text: &str| self.compiled.get(text).is_some_and(|expr| {
            expr.code.as_ref().is_some_and(|code| code.observes_time) || expr.refs.iter().any(|(_, key)| self.projection.observes(key, &|key| {
                if self.inputs.get(key).is_some_and(|frame| goofi_core::samples::SampleSpan::of(frame).is_some()) { return true; }
                let Some((ph, _)) = key.split_once('.') else { return false };
                self.runs.values().any(|run| run.heads.get(ph).is_some_and(|head| head.flight.is_some()))
            }))
        });
        self.runs.iter().any(|(name, run)| run.heads.values().filter(|head| head.flight.is_none() && head.stopped.is_none()).any(|head| {
            head.armed.iter().any(|((id, _), armed)| {
                let Some(transition) = self.config[name].transitions.get(id) else { return false };
                match &armed.trigger {
                    Trigger::When { expression, edge } => varies(expression)
                        || (*edge == Edge::Level && (transition.guard.as_deref().is_some_and(varies)
                            || transition.duration.expression().is_some_and(varies))),
                    Trigger::Always => transition.guard.as_deref().is_some_and(varies)
                        || transition.duration.expression().is_some_and(varies),
                    Trigger::After { seconds: Seconds::Expression(text) } => armed.interval.is_none() && varies(text),
                    _ => false,
                }
            })
        }))
    }

    /// Advance through scheduled events. A changed input is effective at the supplied time.
    pub fn advance(&mut self, now: Tick, read: &dyn Fn(&str) -> Option<Data>) -> Vec<(String, Data)> {
        self.advance_to(now, read, true)
    }

    fn advance_to(&mut self, now: Tick, read: &dyn Fn(&str) -> Option<Data>, select: bool) -> Vec<(String, Data)> {
        let now = now.max(self.at.unwrap_or(0));
        if self.reconcile.is_some_and(|at| at > now) {
            return self.runs.values().flat_map(|run| run.heads.iter().flat_map(|(ph, head)| head.entries(ph, now))).collect();
        }
        let inputs: IndexMap<String, Data> = self.keys().into_iter().filter(|key| self.owner(key).is_none()).filter_map(|key| Some((key.clone(), read(&key)?))).collect();
        let changed = self.inputs != inputs;
        let initial = self.at.is_none();
        if let Some(at) = self.round.as_ref().map(|round| round.at) {
            self.inputs = inputs.clone();
            if !self.settle(at) { return Vec::new(); }
        }
        if initial { self.inputs = inputs.clone(); }
        if let Some(at) = self.reconcile.take() {
            self.inputs = inputs.clone();
            self.runs.retain(|name, _| self.config.contains_key(name));
            for (name, machine) in &self.config {
                let run = self.runs.entry(name.clone()).or_insert_with(|| Run {
                    heads: IndexMap::new(), rng: seed_state(machine.seed.unwrap_or(0)),
                    events: Events::default(),
                });
                reconcile(machine, run, at);
            }
            if at < now && !self.settle(at) { return Vec::new(); }
        }
        // Process requests with deadlines, before the new input becomes effective.
        self.requests.sort_by_key(|(at, _)| *at);
        loop {
            let requested = self.requests.first().map(|(at, _)| *at);
            let due = self.next_deadline().into_iter().chain(requested).min();
            let Some(at) = due.filter(|at| *at < now) else { break };
            self.apply_requests(at);
            if !self.settle(at) { return Vec::new(); }
        }
        self.inputs = inputs;
        let requested = self.apply_requests(now);
        if select && (changed || initial || requested || self.runs.values().any(|run| run.heads.values().any(|head| head.arm_pending.is_some())) || self.next_deadline().is_some_and(|at| at <= now))
            && !self.settle(now) { return Vec::new(); }
        self.at = Some(now);
        self.record_output(now);
        self.runs.values().flat_map(|run| run.heads.iter().flat_map(|(ph, head)| head.entries(ph, now))).collect()
    }

    /// Apply every control request at one instant before selecting automatic moves.
    fn apply_requests(&mut self, at: Tick) -> bool {
        self.land(at);
        let count = self.requests.iter().take_while(|(due, _)| *due <= at).count();
        let requests: Vec<_> = self.requests.drain(..count).collect();
        for (_, request) in requests {
            for (name, run) in &mut self.runs {
                let machine = &self.config[name];
                match &request {
                    Effect::Jump { machine: m, playhead, state } if m == name => {
                        if let Some(head) = run.heads.get_mut(playhead).filter(|_| machine.states.contains_key(state)) {
                            relocate(head, machine, state, at, &mut run.events);
                            let owner = machine.playheads[playhead].identity.generation();
                            self.expression_errors.retain(|site, _| site.machine != machine.identity.generation() || site.playhead != owner);
                        }
                    }
                    Effect::Reset { machine: m } if m.as_deref().is_none_or(|m| m == name) => {
                        run.rng = seed_state(machine.seed.unwrap_or(0));
                        for (ph, head) in &mut run.heads { *head = born(machine, &machine.playheads[ph].start, at, &mut run.events); }
                        self.expression_errors.retain(|site, _| site.machine != machine.identity.generation());
                    }
                    Effect::Event { machine: m, name: event, playhead } if m == name => run.events.named.push((event.clone(), playhead.clone())),
                    _ => {}
                }
            }
        }
        self.record_output(at);
        count > 0
    }

    /// Land the full arrival batch before effects or automatic selection at this instant.
    fn land(&mut self, at: Tick) {
        for (name, run) in &mut self.runs {
            for head in run.heads.values_mut() {
                if head.flight.as_ref().is_some_and(|flight| flight.end <= at) {
                    let state = head.state.clone();
                    arrive(head, &self.config[name], &state, at, &mut run.events);
                }
            }
        }
    }

    /// Retry an incomplete microstep from its settled state and RNG at the same logical instant.
    fn settle(&mut self, at: Tick) -> bool {
        if self.round.is_none() { self.land(at); }
        let mut round = self.round.take().unwrap_or_else(|| DecisionRound::new(at));
        loop {
            let decision = Arc::new(std::mem::take(&mut self.runs));
            let mut tentative = (*decision).clone();
            let snapshot: IndexMap<String, Data> = decision.values().flat_map(|run| run.heads.iter().flat_map(|(ph, head)| head.entries(ph, round.at))).collect();
            let read = |key: &str| snapshot.get(key).cloned().or_else(|| self.inputs.get(key).cloned());
            let ctx = Ctx { compiled: &self.compiled, projection: &self.projection, input_errors: &self.input_errors,
                read: &read, now: round.at, outcomes: std::cell::RefCell::new(IndexMap::new()), pending: std::cell::RefCell::new(IndexMap::new()) };
            let moved = step(&self.config, &mut tentative, &mut round, &decision, &ctx);
            round.pending = ctx.pending.into_inner();
            record_evaluations(&mut self.expression_errors, ctx.outcomes.into_inner());
            let Some(moved) = moved else {
                self.runs = Arc::unwrap_or_clone(decision);
                self.round = Some(round);
                return false;
            };
            self.runs = tentative;
            self.record_output(round.at);
            if !moved { break; }
            round.rounds += 1;
            if round.rounds == 4096 {
                for run in self.runs.values_mut() {
                    for head in run.heads.values_mut() {
                        if head.flight.is_none() && (head.arm_pending.is_some() || !head.armed.is_empty()) {
                            head.armed.clear();
                            head.arm_pending = None;
                            head.stopped = Some("zero-time chain exceeds 4096 decisions".to_string());
                        }
                    }
                }
                break;
            }
        }
        self.at = Some(round.at);
        self.record_output(round.at);
        true
    }

}

fn renamed<T>(old: &IndexMap<String, T>, new: &IndexMap<String, T>, identity: impl Fn(&T) -> &goofi_core::identity::Identity) -> HashMap<String, String> {
    old.iter().filter_map(|(from, record)| new.iter().find(|(_, next)| identity(record) == identity(next)).map(|(to, _)| (from.clone(), to.clone()))).collect()
}

fn rekey<T>(values: IndexMap<String, T>, names: &HashMap<String, String>) -> IndexMap<String, T> {
    values.into_iter().filter_map(|(name, value)| names.get(&name).map(|next| (next.clone(), value))).collect()
}

/// What a step reads and evaluates against.
struct Ctx<'a> {
    compiled: &'a HashMap<String, Expr>,
    projection: &'a crate::variable_projection::VariableProjection,
    input_errors: &'a IndexMap<String, String>,
    read: &'a dyn Fn(&str) -> Option<Data>,
    now: Tick,
    outcomes: std::cell::RefCell<IndexMap<ExpressionSite, Option<String>>>,
    pending: std::cell::RefCell<IndexMap<String, Tick>>,
}

impl<'a> Ctx<'a> {
    fn has_pending(&self) -> bool { !self.pending.borrow().is_empty() }

    fn evaluate(&self, text: &str) -> Result<Data, crate::variable_projection::ReadFault> {
        use crate::variable_projection::ReadFault;
        let Some(e) = self.compiled.get(text) else { return Err(format!("`{text}` is not compiled").into()) };
        if let Some(error) = &e.error { return Err(error.clone().into()); }
        let mut locals: Vec<(String, Local)> = Vec::with_capacity(e.refs.len());
        for (var, key) in &e.refs {
            let frame = self.projection.resolve(key, self.now, self.read, self.input_errors)?;
            locals.push((var.clone(), Local::Frame(frame)));
        }
        if expr_rewrite::is_bare(&e.rewritten) {
            let (var, index) = goofi_node::mailbox::split_index(e.rewritten.trim())?;
            let local = locals.iter().find(|(v, _)| v == var).map(|(_, l)| l).ok_or("no local")?;
            return goofi_node::mailbox::pick(local, index).map_err(ReadFault::Failed);
        }
        e.code.as_ref().ok_or("no expression evaluator available")?
            .eval(&EvalCtx { locals: &locals, t: seconds(self.now), range: (0.0, 1.0) }).map_err(|error| ReadFault::Failed(error.0))
    }

    fn record<T>(&self, site: ExpressionSite, result: &Result<T, crate::variable_projection::ReadFault>) {
        use crate::variable_projection::ReadFault;
        match result {
            Err(ReadFault::Pending { source, at }) => { self.pending.borrow_mut().insert(source.clone(), *at); },
            Err(ReadFault::Failed(why)) => { self.outcomes.borrow_mut().insert(site, Some(why.clone())); },
            Ok(_) => { self.outcomes.borrow_mut().insert(site, None); },
        }
    }

    fn gate_result(&self, text: &str, site: ExpressionSite) -> Result<bool, crate::variable_projection::ReadFault> {
        let result = self.evaluate(text).and_then(|frame| {
            if let Value::Array(array) = frame.value() {
                if array.values().any(|value| !value.is_finite()) { return Err("condition must yield finite values".into()); }
            }
            Ok(control::truth(&frame))
        });
        self.record(site, &result);
        result
    }

    fn dwell_result(&self, dwell: &Seconds, site: ExpressionSite, rng: &mut u64) -> Result<Tick, crate::variable_projection::ReadFault> {
        let value = match dwell {
            Seconds::Fixed(value) => Ok(*value),
            Seconds::Expression(text) => self.evaluate(text).and_then(|frame| control::numbers(&frame).next().ok_or_else(|| "duration expression must yield a number".into())),
            Seconds::Uniform { min, max } => Ok(if min == max { *min } else { min + (max - min) * draw(rng) }),
        };
        let result = value.and_then(|value| ticks(value).ok_or_else(|| "dwell must be finite and at least 0".into()));
        if matches!(dwell, Seconds::Expression(_)) { self.record(site, &result); }
        result
    }

}

fn record_evaluations(errors: &mut IndexMap<ExpressionSite, String>, outcomes: IndexMap<ExpressionSite, Option<String>>) {
    for (site, error) in outcomes {
        match error {
            Some(error) => { errors.insert(site, error); }
            None => { errors.shift_remove(&site); }
        }
    }
}

fn seed_state(seed: u64) -> u64 {
    seed ^ 0x9E37_79B9_7F4A_7C15 | 1
}

/// The next draw in 0..1, xorshift64*.
fn draw(rng: &mut u64) -> f64 {
    let mut x = *rng;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *rng = x;
    (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
}

/// Whether `t` leaves `state`.
fn leaves(t: &Transition, state: &str) -> bool {
    t.from == "*" || t.from == state
}

/// A head at its start state, with its arrival included in the settled batch.
fn born(machine: &Machine, state: &str, at: Tick, events: &mut Events) -> Head {
    let mut head = Head {
        state: String::new(),
        held: machine.attributes.iter().map(|(a, attr)| (a.clone(), attr.default.clone())).collect(),
        arrived: at, flight: None, armed: IndexMap::new(), arm_pending: Some(true),
        stopped: None,
    };
    arrive(&mut head, machine, state, at, events);
    head
}

fn arrive(head: &mut Head, machine: &Machine, state: &str, at: Tick, events: &mut Events) {
    head.stopped = None;
    if let Some(flight) = head.flight.take() {
        head.held.extend(flight.target);
    }
    head.state = state.to_string();
    head.arrived = at;
    if let Some(state) = machine.states.get(state) {
        for (attr, value) in &state.values {
            if head.held.contains_key(attr) {
                head.held.insert(attr.clone(), value.clone());
            }
        }
    }
    head.armed.clear();
    head.arm_pending = Some(true);
    events.arrived.push(head.state.clone());
}

fn exit(head: &mut Head, machine: &Machine, at: Tick, events: &mut Events) {
    head.held = head.values(at);
    if head.flight.is_none() {
        events.left.push(head.state.clone());
        if let Some(source) = machine.states.get(&head.state) { head.held.extend(source.exit_values.clone()); }
    }
    head.flight = None;
}

fn relocate(head: &mut Head, machine: &Machine, state: &str, at: Tick, events: &mut Events) {
    exit(head, machine, at, events);
    arrive(head, machine, state, at, events);
}

fn reconcile(machine: &Machine, run: &mut Run, at: Tick) {
    run.heads.retain(|ph, _| machine.playheads.contains_key(ph));
    for (ph, playhead) in &machine.playheads {
        match run.heads.get_mut(ph) {
            None => {
                let head = born(machine, &playhead.start, at, &mut run.events);
                run.heads.insert(ph.clone(), head);
            }
            Some(head) => {
                head.held.retain(|a, _| machine.attributes.contains_key(a));
                for (a, attr) in &machine.attributes {
                    let value = head.held.entry(a.clone()).or_insert_with(|| attr.default.clone());
                    if !attr.kind.fits(value) { *value = attr.default.clone(); }
                }
                if let Some(flight) = &mut head.flight {
                    flight.target.retain(|a, _| machine.attributes.contains_key(a));
                    for (a, value) in &mut flight.target {
                        let attr = &machine.attributes[a];
                        if !attr.kind.fits(value) { *value = attr.default.clone(); }
                    }
                }
                if !machine.states.contains_key(&head.state) {
                    relocate(head, machine, &playhead.start, at, &mut run.events);
                } else if head.stopped.is_none() {
                    head.arm_pending = Some(false);
                }
            }
        }
    }
}

/// `a` toward `b` by `t`: elementwise for arrays of one shape; anything else switches on arrival.
fn blend(a: &Data, b: &Data, t: f64) -> Data {
    match (a.value(), b.value()) {
        (Value::Array(x), Value::Array(y)) if x.shape() == y.shape() => {
            let bytes: Vec<u8> = x
                .values()
                .zip(y.values())
                .flat_map(|(p, q)| control::mix(p, q, t).to_le_bytes())
                .collect();
            Data::array_f32(x.shape().to_vec(), bytes, Meta::default()).unwrap_or_else(|_| b.clone())
        }
        _ if t >= 1.0 => b.clone(),
        _ => a.clone(),
    }
}

fn depart_ready(head: &mut Head, machine: &Machine, id: &str, at: Tick, events: &mut Events, duration: Tick) {
    let transition = &machine.transitions[id];
    if transition.internal {
        head.held.extend(transition.values.clone());
        return;
    }
    let from = head.state.clone();
    exit(head, machine, at, events);
    head.held.extend(transition.values.clone());
    if duration == 0 {
        arrive(head, machine, &transition.to, at, events);
        return;
    }
    head.state = transition.to.clone();
    head.armed.clear();
    let target = machine.states[&transition.to].values.iter().filter(|(a, _)| head.held.contains_key(*a)).map(|(a, v)| (a.clone(), v.clone())).collect();
    head.flight = Some(Flight {
        id: id.to_string(), from, start: at, end: at.saturating_add(duration),
        curve: transition.curve, target,
    });
}

fn next_deadline(run: &Run) -> Option<Tick> {
    run.heads.values().filter_map(|head| match &head.flight {
        Some(flight) => Some(flight.end),
        None => head.armed.values().filter_map(|armed| armed.due).min(),
    }).min()
}

#[derive(PartialEq)]
struct ChoicePoint {
    transition: String,
    runtime: Arc<IndexMap<String, Run>>,
}

type HeadKey = (String, String);

/// Successful zero-time decisions share one instant; incomplete attempts commit nothing.
struct DecisionRound {
    at: Tick,
    pending: IndexMap<String, Tick>,
    visited: HashMap<HeadKey, Vec<ChoicePoint>>,
    rounds: usize,
}

impl DecisionRound {
    fn new(at: Tick) -> Self {
        Self { at, pending: IndexMap::new(), visited: HashMap::new(), rounds: 0 }
    }
}

fn arm(head: &mut Head, machine: &Machine, ph: &str, ctx: &Ctx, rng: &mut u64) {
    let Some(fresh) = head.arm_pending.take() else { return };
    let mut previous = std::mem::take(&mut head.armed);
    if head.flight.is_some() { return; }
    for id in machine.outgoing(&head.state) {
        for (index, trigger) in machine.transitions[id].triggers.iter().enumerate() {
            let key = (id.to_string(), index);
            let prior = (!fresh).then(|| previous.iter().find(|(key, armed)| key.0 == id && armed.trigger == *trigger).map(|(key, _)| key.clone())).flatten();
            let armed = prior.and_then(|key| previous.shift_remove(&key)).unwrap_or_else(|| {
                let interval = match trigger {
                    Trigger::After { seconds } => ctx.dwell_result(seconds, machine.site(ph, id, ExpressionSurface::After { index }), rng).ok(),
                    _ => None,
                };
                let gate = match trigger {
                    Trigger::When { expression, .. } => ctx.gate_result(expression, machine.site(ph, id, ExpressionSurface::When { index })).ok(),
                    _ => None,
                };
                Armed { trigger: trigger.clone(), interval, gate,
                    due: interval.and_then(|interval| (if fresh { head.arrived } else { ctx.now }).checked_add(interval)) }
            });
            head.armed.insert(key, armed);
        }
    }
}

/// Offers read the resident/event batch and update only the tentative trigger state.
fn gather(config: &IndexMap<String, Machine>, runs: &mut IndexMap<String, Run>, ctx: &Ctx) -> IndexMap<HeadKey, HashSet<String>> {
    let mut candidates: IndexMap<HeadKey, HashSet<String>> = IndexMap::new();
    for (name, run) in runs {
        let machine = &config[name];
        for (ph, head) in &mut run.heads {
            if head.flight.is_some() || head.stopped.is_some() { continue; }
            arm(head, machine, ph, ctx, &mut run.rng);
            for ((id, index), armed) in &mut head.armed {
                if armed.interval.is_none() {
                    if let Trigger::After { seconds: Seconds::Expression(text) } = &armed.trigger {
                        armed.interval = ctx.dwell_result(&Seconds::Expression(text.clone()), machine.site(ph, id, ExpressionSurface::After { index: *index }), &mut run.rng).ok();
                        armed.due = armed.interval.and_then(|interval| ctx.now.checked_add(interval));
                    }
                }
                let fired = match &armed.trigger {
                    Trigger::After { .. } if armed.due.is_some_and(|at| at <= ctx.now) => {
                        armed.due = armed.interval.filter(|period| *period > 0).and_then(|period| armed.due?.checked_add(period));
                        true
                    }
                    Trigger::When { expression, edge } => {
                        let Ok(current) = ctx.gate_result(expression, machine.site(ph, id, ExpressionSurface::When { index: *index })) else { continue };
                        let fired = match edge {
                            Edge::Rising => armed.gate == Some(false) && current,
                            Edge::Falling => armed.gate == Some(true) && !current,
                            Edge::Change => armed.gate.is_some_and(|previous| previous != current),
                            Edge::Level => current,
                        };
                        armed.gate = Some(current);
                        fired
                    }
                    Trigger::Always => true,
                    Trigger::Event { name } => run.events.named.iter().any(|(event, target)| event == name && target.as_ref().is_none_or(|target| target == ph)),
                    _ => false,
                };
                if fired { candidates.entry((name.clone(), ph.clone())).or_default().insert(id.clone()); }
            }
        }
        for state in machine.states.keys() {
            let arrived = run.events.arrived.contains(state);
            let left = run.events.left.contains(state);
            if !arrived && !left { continue; }
            let residents: Vec<_> = run.heads.iter().filter(|(_, head)| head.flight.is_none() && head.stopped.is_none() && head.state == *state).map(|(ph, head)| (ph, head.arrived)).collect();
            for id in machine.outgoing(state) {
                for trigger in &machine.transitions[id].triggers {
                    let chosen: Vec<&String> = match trigger {
                        Trigger::Meet { policy } if arrived && residents.len() >= 2 => match policy {
                            Policy::All => residents.iter().map(|(ph, _)| *ph).collect(),
                            Policy::Fifo => residents.iter().min_by_key(|(_, at)| *at).map(|(ph, _)| *ph).into_iter().collect(),
                            Policy::Lifo => residents.iter().rev().max_by_key(|(_, at)| *at).map(|(ph, _)| *ph).into_iter().collect(),
                        },
                        Trigger::Alone if left && residents.len() == 1 => vec![residents[0].0],
                        _ => Vec::new(),
                    };
                    for ph in chosen { candidates.entry((name.clone(), ph.clone())).or_default().insert(id.to_string()); }
                }
            }
        }
    }
    candidates
}

/// Select and resolve all departures before the tentative batch changes any resident.
fn step(config: &IndexMap<String, Machine>, runs: &mut IndexMap<String, Run>, round: &mut DecisionRound, decision: &Arc<IndexMap<String, Run>>, ctx: &Ctx) -> Option<bool> {
    use crate::variable_projection::ReadFault;
    let candidates = gather(config, runs, ctx);
    if ctx.has_pending() { return None; }
    let mut selected = Vec::new();
    for (name, run) in runs.iter_mut() {
        let machine = &config[name];
        for (ph, head) in &run.heads {
            let Some(fired) = candidates.get(&(name.clone(), ph.clone())) else { continue };
            let selection = machine.states[&head.state].selection;
            let mut eligible = Vec::new();
            for id in machine.outgoing(&head.state).into_iter().filter(|id| fired.contains(*id)) {
                let transition = &machine.transitions[id];
                if selection == Selection::Weighted && transition.weight == 0.0 { continue; }
                if let Some(guard) = &transition.guard {
                    match ctx.gate_result(guard, machine.site(ph, id, ExpressionSurface::Guard)) {
                        Ok(true) => {}, Err(ReadFault::Pending { .. }) => return None, _ => continue,
                    }
                }
                if !chance(transition.chance, &mut run.rng) { continue; }
                eligible.push((id.to_string(), if selection == Selection::Weighted { transition.weight } else { 1.0 }));
                if selection == Selection::Ordered { break; }
            }
            let Some(id) = pick(&eligible, &mut run.rng) else { continue };
            selected.push((name.clone(), ph.clone(), id, None));
        }
    }
    for (name, ph, id, duration) in &mut selected {
        let machine = &config[name.as_str()];
        *duration = match ctx.dwell_result(&machine.transitions[id.as_str()].duration, machine.site(ph, id, ExpressionSurface::Duration), &mut runs[name.as_str()].rng) {
            Ok(duration) => Some(duration), Err(ReadFault::Pending { .. }) => return None, Err(ReadFault::Failed(_)) => None,
        };
    }
    for run in runs.values_mut() { run.events = Events::default(); }
    let mut moved = false;
    for (name, ph, id, duration) in selected {
        let point = ChoicePoint { transition: id.clone(), runtime: decision.clone() };
        let prior = round.visited.entry((name.clone(), ph.clone())).or_default();
        let run = &mut runs[&name];
        let head = &mut run.heads[&ph];
        if prior.contains(&point) {
            head.armed.clear();
            head.stopped = Some(format!("zero-time transition cycle in `{}`", head.state));
        } else {
            prior.push(point);
            if let Some(duration) = duration {
                depart_ready(head, &config[&name], &id, ctx.now, &mut run.events, duration);
                moved = true;
            }
        }
    }
    Some(moved)
}

fn chance(value: f64, rng: &mut u64) -> bool {
    value > 0.0 && (value == 1.0 || draw(rng) < value)
}

/// A stable weighted draw. Scaling prevents overflow from large finite weights.
fn pick(fired: &[(String, f64)], rng: &mut u64) -> Option<String> {
    if fired.len() == 1 { return Some(fired[0].0.clone()); }
    let scale = fired.iter().map(|(_, w)| *w).fold(0.0, f64::max);
    if scale == 0.0 { return None; }
    let total: f64 = fired.iter().map(|(_, w)| w / scale).sum();
    let mut r = draw(rng) * total;
    for (id, w) in fired {
        if r < w / scale { return Some(id.clone()); }
        r -= w / scale;
    }
    fired.last().map(|(id, _)| id.clone())
}
