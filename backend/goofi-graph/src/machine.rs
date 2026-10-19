//! State machines: the model the document carries, and the stepping that moves each playhead
//! through it and answers what its variables hold now.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use goofi_core::control;
use goofi_core::ease::Curve;
use goofi_core::variables::{is_valid_identifier, Control, ControlKind, Entries};
use goofi_core::{Data, Face, Meta, Param, Value};
use goofi_node::{EvalCtx, ExprEvaluator, Local};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::expr_rewrite::{self, Target};

/// The elements of a playhead's group that are the machine's own, and no attribute's name.
pub const OWN: [&str; 4] = ["state", "prev", "progress", "arrived"];

/// How many instant transitions the playheads take at one instant before the chain is an error.
const HOPS: usize = 64;

/// What a name has to be, said once.
pub const NAME_RULE: &str = "a letter or underscore then letters, digits or underscores, and not a Python keyword";

fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct Machine {
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

/// What an attribute is, as a node's param is: a number — one, a vector, a colour or an envelope —
/// a bool, or a text, maybe one of some options. The inspector draws it with that param's widget.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AttributeKind {
    Num {
        vmin: f64,
        vmax: f64,
        #[serde(default)]
        int: bool,
        #[serde(default)]
        face: Face,
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
            _ => AttributeKind::Num { vmin: 0.0, vmax: 1.0, int: false, face: Face::Plain },
        }
    }

    /// Whether `value` is one this kind holds: numbers in one dimension (four for a colour or an
    /// envelope), a bool as one number, a text (one of the options, where there are some).
    pub fn fits(&self, value: &Data) -> bool {
        match (self, value.value()) {
            (AttributeKind::Num { face, .. }, Value::Array(a)) => face.dims().is_none_or(|d| a.shape() == [d]) && a.shape().len() == 1,
            (AttributeKind::Bool, Value::Array(a)) => a.shape() == [1],
            (AttributeKind::Str { options: Some(o) }, Value::Str(s)) => o.iter().any(|x| x.as_str() == s.as_ref()),
            (AttributeKind::Str { .. }, Value::Str(_)) => true,
            _ => false,
        }
    }

    /// `value` read into this kind as a param reads a frame ([`control::read`]), as many numbers
    /// as `default` holds; a value the kind still cannot hold is `default`.
    pub fn read(&self, value: &Data, default: &Data) -> Data {
        let target = match self {
            AttributeKind::Num { vmin, vmax, int, face } => {
                let dims = control::numbers(default).count().max(1);
                Param::Num { value: vec![0.0; dims], vmin: *vmin, vmax: *vmax, int: *int, options: Vec::new(), face: *face }
            }
            AttributeKind::Bool => Param::Bool { value: false },
            AttributeKind::Str { options } => Param::Str { value: String::new(), options: options.clone(), refresh: false },
        };
        control::data_of(&control::read(value, &target)).filter(|d| self.fits(d)).unwrap_or_else(|| default.clone())
    }

    pub fn mismatch(&self, value: &Data) -> String {
        let kind = match self {
            AttributeKind::Num { face: Face::Plain, .. } => "num",
            AttributeKind::Num { face, .. } => face.as_str(),
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
            AttributeKind::Num { face: Face::Color, .. } => (ControlKind::Color, None, Vec::new()),
            AttributeKind::Num { face: Face::Envelope, vmin, vmax, .. } => (ControlKind::Envelope, Some((*vmin, *vmax, 0.01)), Vec::new()),
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
    #[ts(type = "Literal")]
    pub default: Data,
    pub kind: AttributeKind,
}

/// A state: where its card sits, the attributes it sets (one left out is kept on entry) and the
/// envelopes they land by, and how a playhead resting in it picks a transition out, once per `interval`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct State {
    #[serde(default)]
    pub pos: [f64; 2],
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[ts(type = "Record<string, Literal>")]
    pub values: IndexMap<String, Data>,
    /// Per attribute set here, the ADSR its value lands by: attack, decay and release in seconds,
    /// sustain in 0..1 of the way from what the playhead held to the value.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[ts(type = "Record<string, [number, number, number, number]>")]
    pub envelopes: IndexMap<String, [f64; 4]>,
    /// Seconds between decisions, from arrival; 0 decides once, on arrival.
    #[serde(default = "one")]
    pub interval: f64,
    #[serde(default)]
    pub mode: Decision,
    /// The weight of a decision that waits one more interval, in `random`.
    #[serde(default)]
    pub noop: f64,
    /// Per transition out, by id, its weight in `random`; one left out weighs 1, so a new state
    /// draws evenly.
    #[serde(default)]
    pub weights: IndexMap<String, f64>,
    /// In `expression`: the index of the transition out to take; a negative one waits.
    #[serde(default)]
    pub expression: String,
}

impl Default for State {
    fn default() -> Self {
        State { pos: [0.0; 2], values: IndexMap::new(), envelopes: IndexMap::new(), interval: 1.0, mode: Decision::default(), noop: 0.0, weights: IndexMap::new(), expression: String::new() }
    }
}

/// How a state decides: the next transition out in turn, a weighted draw, or an expression.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Sequence,
    #[default]
    Random,
    Expression,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Transition {
    /// A state name, or `*` for any state.
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<Trigger>,
    /// Seconds; 0 is instant.
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub curve: Curve,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Trigger {
    Manual,
    /// `weight` is its share of the draw among the triggers that fire in one tick; 0 is never drawn.
    When {
        expression: String,
        #[serde(default = "one")]
        weight: f64,
    },
    Meet {
        #[serde(default)]
        policy: Policy,
    },
    Alone,
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
        let whens = self.transitions.values().flat_map(|t| &t.triggers).filter_map(|tr| match tr {
            Trigger::When { expression, .. } => Some(expression.as_str()),
            _ => None,
        });
        whens.chain(self.states.values().filter(|s| s.mode == Decision::Expression && !s.expression.is_empty()).map(|s| s.expression.as_str()))
    }

    /// The transitions out of `state`, in authored order: what a decision picks among.
    pub fn outgoing<'a>(&'a self, state: &'a str) -> impl Iterator<Item = &'a String> + 'a {
        self.transitions.iter().filter(move |(_, t)| leaves(t, state)).map(|(id, _)| id)
    }

    /// Drop every state's weight for a transition that no longer leaves it.
    pub fn prune(&mut self) {
        let outgoing: Vec<HashSet<String>> = self.states.keys().map(|s| self.outgoing(s).cloned().collect()).collect();
        for (s, out) in self.states.values_mut().zip(outgoing) {
            s.weights.retain(|id, _| out.contains(id));
        }
    }

    /// Whether the record holds together: every name legal, every reference to something it holds.
    pub fn check(&self) -> Result<(), String> {
        for name in self.attributes.keys() {
            if !is_valid_identifier(name) {
                return Err(format!("invalid attribute name `{name}`: {NAME_RULE}"));
            }
            if OWN.contains(&name.as_str()) {
                return Err(format!("`{name}` is the machine's own element of a playhead; an attribute cannot take it"));
            }
        }
        for (name, state) in &self.states {
            if !is_valid_identifier(name) {
                return Err(format!("invalid state name `{name}`: {NAME_RULE}"));
            }
            if let Some(attr) = state.values.keys().find(|a| !self.attributes.contains_key(*a)) {
                return Err(format!("state `{name}` sets `{attr}`, which is no attribute of the machine"));
            }
            for (attr, e) in &state.envelopes {
                if !state.values.contains_key(attr) {
                    return Err(format!("state `{name}` shapes `{attr}` with an envelope but does not set it"));
                }
                if !matches!(self.attributes.get(attr).map(|a| &a.kind), Some(AttributeKind::Num { .. })) {
                    return Err(format!("state `{name}`: `{attr}` is no number; an envelope shapes numbers alone"));
                }
                if e.iter().any(|v| !(v.is_finite() && *v >= 0.0)) || e[2] > 1.0 {
                    return Err(format!("state `{name}`: `{attr}`'s envelope is three times of at least 0 and a sustain in 0..1"));
                }
            }
            if !(state.interval.is_finite() && state.interval >= 0.0) {
                return Err(format!("state `{name}`: interval is seconds of at least 0"));
            }
            if !(state.noop.is_finite() && state.noop >= 0.0) || state.weights.values().any(|w| !(w.is_finite() && *w >= 0.0)) {
                return Err(format!("state `{name}`: a weight is at least 0"));
            }
            if let Some(id) = state.weights.keys().find(|id| !self.transitions.get(*id).is_some_and(|t| leaves(t, name))) {
                return Err(format!("state `{name}` weighs `{id}`, which is no transition out of it"));
            }
            if !state.expression.is_empty() {
                check_expression(&state.expression).map_err(|why| format!("state `{name}`: {why}"))?;
            }
        }
        for (id, t) in &self.transitions {
            if t.from != "*" && !self.states.contains_key(&t.from) {
                return Err(format!("transition `{id}` leaves `{}`, which is no state of the machine", t.from));
            }
            if !self.states.contains_key(&t.to) {
                return Err(format!("transition `{id}` enters `{}`, which is no state of the machine", t.to));
            }
            if t.duration.is_nan() || t.duration < 0.0 {
                return Err(format!("transition `{id}`: duration is at least 0"));
            }
            for trigger in &t.triggers {
                if let Trigger::When { expression, weight } = trigger {
                    if weight.is_nan() || *weight < 0.0 {
                        return Err(format!("transition `{id}`: a trigger's weight is at least 0"));
                    }
                    check_expression(expression).map_err(|why| format!("transition `{id}`: {why}"))?;
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

    /// The variables a playhead's group holds: the machine's own four, then one per attribute,
    /// each born as the start state has it — at rest there, before the machine has moved it.
    pub fn playhead_entries(&self, playhead: &str) -> Entries {
        let Some(p) = self.playheads.get(playhead) else { return Vec::new() };
        let start = self.states.get(&p.start);
        let own = [(Data::text(p.start.as_str()), None), (Data::text(""), None), (Data::number(1.0), None), (Data::number(0.0), None)];
        let held = |a: &str, attr: &Attribute| start.and_then(|s| s.values.get(a)).unwrap_or(&attr.default).clone();
        OWN.iter()
            .zip(own)
            .map(|(e, (v, c))| (format!("{playhead}.{e}"), v, c))
            .chain(self.attributes.iter().map(|(a, attr)| (format!("{playhead}.{a}"), held(a, attr), Some(attr.kind.control(&attr.default)))))
            .collect()
    }
}

/// One expression as the machines run it: compiled once per text, its variables by name.
struct Expr {
    rewritten: String,
    refs: Vec<(String, String)>,
    id: Option<goofi_node::BindingId>,
    error: Option<String>,
}

/// A playhead's travel along a transition: when it left and lands, for `progress`, and the values
/// that cannot blend, which switch on arrival.
struct Flight {
    start: f64,
    end: f64,
    landing: IndexMap<String, Data>,
}

/// One leg of an attribute's motion: `from` toward `to` over `start..end`, by `curve`.
struct Leg {
    start: f64,
    end: f64,
    from: Data,
    to: Data,
    curve: Curve,
}

/// An attribute on the move: its legs in order, then what it rests at.
struct Motion {
    legs: Vec<Leg>,
    rest: Data,
}

impl Motion {
    /// Where the motion stands at `t`: its origin before it, the blend inside a leg, its rest after.
    fn at(&self, t: f64) -> Data {
        for leg in &self.legs {
            if t < leg.start {
                return leg.from.clone();
            }
            if t < leg.end {
                return lerp(&leg.from, &leg.to, leg.curve.at((t - leg.start) / (leg.end - leg.start))).unwrap_or_else(|| leg.from.clone());
            }
        }
        self.rest.clone()
    }

    /// When the last leg lands.
    fn end(&self) -> Option<f64> {
        self.legs.last().map(|l| l.end)
    }

    /// The whole motion as Python over `t`: each leg while it lasts, then the rest.
    fn source(&self) -> Option<String> {
        let mut out = String::new();
        for leg in &self.legs {
            let (a, b) = (py_list(&leg.from)?, py_list(&leg.to)?);
            let x = format!("min(max((t - {:?}) / {:?}, 0.0), 1.0)", leg.start, leg.end - leg.start);
            out.push_str(&format!("(lambda x: np.array({a}) + (np.array({b}) - np.array({a})) * ({}))({x}) if t < {:?} else ", curve_source(leg.curve), leg.end));
        }
        out.push_str(&format!("np.array({})", py_list(&self.rest)?));
        Some(out)
    }
}

/// A decision armed: when it is due, and the interval it was armed with.
#[derive(Clone, Copy)]
struct Due {
    at: f64,
    interval: f64,
}

/// One playhead's run: at rest in `state` since `arrived`, or in flight toward it; what it holds,
/// and the attributes still on their way.
struct Head {
    state: String,
    prev: String,
    arrived: f64,
    held: IndexMap<String, Data>,
    motions: IndexMap<String, Motion>,
    flight: Option<Flight>,
    /// The state's next decision, at infinity once a 0 interval has decided; none in a state with
    /// no transition out.
    due: Option<Due>,
    /// Per state, where `sequence` takes up again: the index of the next transition out.
    cursor: HashMap<String, usize>,
    /// Per `when` transition out of the state, the gate as last read.
    gates: HashMap<String, bool>,
    fire: Option<String>,
}

struct Run {
    heads: IndexMap<String, Head>,
    rng: u64,
    seed: u64,
}

/// What a step found: who arrived where, and who left what.
#[derive(Default)]
struct Events {
    arrived: Vec<String>,
    left: Vec<String>,
}

enum Request {
    Fire { machine: String, playhead: String, transition: String },
    Jump { machine: String, playhead: String, state: String },
    Reset { machine: Option<String> },
}

/// The machines as they run: the model, each playhead's run, and the expressions compiled for
/// them. Pure but for the evaluator: clocked by the instants it is handed, reading the frames it
/// is handed, answering the writes it owes. Every deadline is settled at its own instant.
#[derive(Default)]
pub struct Machines {
    config: IndexMap<String, Machine>,
    runs: IndexMap<String, Run>,
    evaluator: Option<Arc<dyn ExprEvaluator>>,
    compiled: HashMap<String, Expr>,
    requests: Vec<(f64, Request)>,
    reconcile: bool,
    errors: Vec<String>,
    /// The functionals the last advance handed out, by source: a flight's stay the same for
    /// its whole duration, and each compile is a Python call.
    functionals: HashMap<String, Arc<[u8]>>,
}

impl Machines {
    pub fn is_empty(&self) -> bool {
        self.config.is_empty()
    }

    /// Replace the model. A playhead still named keeps its place and what it holds; one whose
    /// state is gone starts over. Takes effect on the next advance.
    pub fn configure(&mut self, config: IndexMap<String, Machine>, evaluator: Option<Arc<dyn ExprEvaluator>>) {
        let wanted: HashSet<String> = config.values().flat_map(Machine::expressions).map(str::to_string).collect();
        if evaluator.as_ref().map(Arc::as_ptr) != self.evaluator.as_ref().map(Arc::as_ptr) {
            self.release_all();
        }
        self.evaluator = evaluator;
        let gone: Vec<String> = self.compiled.keys().filter(|k| !wanted.contains(*k)).cloned().collect();
        for text in gone {
            if let (Some(ev), Some(id)) = (&self.evaluator, self.compiled.remove(&text).and_then(|e| e.id)) {
                ev.release(id);
            }
        }
        for text in wanted {
            if !self.compiled.contains_key(&text) {
                let expr = self.compile(&text);
                self.compiled.insert(text, expr);
            }
        }
        self.config = config;
        self.reconcile = true;
    }

    fn release_all(&mut self) {
        for (_, e) in self.compiled.drain() {
            if let (Some(ev), Some(id)) = (&self.evaluator, e.id) {
                ev.release(id);
            }
        }
    }

    fn compile(&self, text: &str) -> Expr {
        let none = |error: String| Expr { rewritten: String::new(), refs: Vec::new(), id: None, error: Some(error) };
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
            return Expr { rewritten, refs, id: None, error: None };
        }
        match &self.evaluator {
            None => none("no expression evaluator available".to_string()),
            Some(ev) => match ev.compile(&rewritten) {
                Ok(c) => Expr { rewritten, refs, id: Some(c.id), error: None },
                Err(e) => none(e.0),
            },
        }
    }

    /// Take a transition for a playhead at `at`, the way a tap on it does.
    pub fn fire(&mut self, at: f64, machine: &str, playhead: &str, transition: &str) {
        self.requests.push((at, Request::Fire { machine: machine.into(), playhead: playhead.into(), transition: transition.into() }));
    }

    /// Put a playhead in a state at `at`: instant, no easing.
    pub fn jump(&mut self, at: f64, machine: &str, playhead: &str, state: &str) {
        self.requests.push((at, Request::Jump { machine: machine.into(), playhead: playhead.into(), state: state.into() }));
    }

    /// Every playhead of a machine, or of all, back to its start state at `at`.
    pub fn reset(&mut self, at: f64, machine: Option<&str>) {
        self.requests.push((at, Request::Reset { machine: machine.map(str::to_string) }));
    }

    /// Every variable the machines' expressions read — what an advance wants handed to it.
    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.compiled.values().flat_map(|e| e.refs.iter().map(|(_, k)| k.clone())).collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// What went wrong since the last ask, each once.
    pub fn take_errors(&mut self) -> Vec<String> {
        let mut out = std::mem::take(&mut self.errors);
        out.sort();
        out.dedup();
        out
    }

    /// The next instant something is owed: a landing, a motion's rest, a decision due, or a
    /// request — what the thread sleeps toward. None while nothing is armed.
    pub fn next_deadline(&self) -> Option<f64> {
        let heads = self.runs.values().flat_map(|r| r.heads.values());
        let owed = heads.flat_map(|h| {
            let due = h.flight.as_ref().map(|f| f.end).or_else(|| h.due.map(|d| d.at).filter(|at| at.is_finite()));
            due.into_iter().chain(h.motions.values().filter_map(Motion::end))
        });
        owed.chain(self.requests.iter().map(|(at, _)| *at)).fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.min(t))))
    }

    /// Step every machine to `now`: every landing, due and request before it is settled at its
    /// own instant, in order, then the instant itself. Answers every playhead variable as it
    /// stands, a functional for an attribute in flight.
    pub fn advance(&mut self, now: f64, read: &dyn Fn(&str) -> Option<Data>) -> Vec<(String, Data)> {
        if std::mem::take(&mut self.reconcile) {
            let ctx = Ctx::new(&self.compiled, &self.evaluator, read, now);
            self.runs.retain(|name, _| self.config.contains_key(name));
            for (name, machine) in &self.config {
                let seed = machine.seed.unwrap_or(0);
                let run = self.runs.entry(name.clone()).or_insert_with(|| Run { heads: IndexMap::new(), rng: seed_state(seed), seed });
                if run.seed != seed {
                    run.rng = seed_state(seed);
                    run.seed = seed;
                }
                reconcile(machine, run, &ctx);
            }
            self.errors.extend(ctx.errors.into_inner());
        }
        self.requests.sort_by(|a, b| a.0.total_cmp(&b.0));
        while let Some(at) = self.next_deadline().filter(|at| *at < now) {
            self.settle(at, read);
        }
        self.settle(now, read);
        let mut out = Vec::new();
        let mut used = HashMap::new();
        let mut functional = |source: String| {
            let code = match self.functionals.remove(&source).or_else(|| used.get(&source).cloned()) {
                Some(code) => code,
                None => compile(&self.evaluator, &source),
            };
            used.insert(source, code.clone());
            Data::functional(code, Meta::default())
        };
        for run in self.runs.values() {
            for (ph, head) in &run.heads {
                out.push((format!("{ph}.state"), Data::text(head.state.as_str())));
                out.push((format!("{ph}.prev"), Data::text(head.prev.as_str())));
                out.push((format!("{ph}.arrived"), Data::number(head.arrived)));
                let progress = match &head.flight {
                    None => Data::number(1.0),
                    Some(f) => functional(progress_source(f)),
                };
                out.push((format!("{ph}.progress"), progress));
                for (a, held) in &head.held {
                    let value = match head.motions.get(a).and_then(Motion::source) {
                        Some(source) => functional(source),
                        None => held.clone(),
                    };
                    out.push((format!("{ph}.{a}"), value));
                }
            }
        }
        self.functionals = used;
        out
    }

    /// Everything owed at `at`: landings, requests, then the moves that follow.
    fn settle(&mut self, at: f64, read: &dyn Fn(&str) -> Option<Data>) {
        let ctx = Ctx::new(&self.compiled, &self.evaluator, read, at);
        let requests: Vec<Request> = {
            let split = self.requests.iter().position(|(t, _)| *t > at).unwrap_or(self.requests.len());
            self.requests.drain(..split).map(|(_, r)| r).collect()
        };
        for (name, machine) in &self.config {
            let Some(run) = self.runs.get_mut(name) else { continue };
            let mut events = Events::default();
            for r in &requests {
                match r {
                    Request::Fire { machine: m, playhead, transition } if m == name => {
                        if let Some(head) = run.heads.get_mut(playhead) {
                            head.fire = Some(transition.clone());
                        }
                    }
                    Request::Jump { machine: m, playhead, state } if m == name => {
                        if let (Some(head), true) = (run.heads.get_mut(playhead), machine.states.contains_key(state)) {
                            relocate(head, machine, state, &ctx, &mut events);
                        }
                    }
                    Request::Reset { machine: m } if m.as_deref().is_none_or(|m| m == name) => {
                        for (ph, head) in run.heads.iter_mut() {
                            head.cursor.clear();
                            relocate(head, machine, &machine.playheads[ph].start.clone(), &ctx, &mut events);
                        }
                    }
                    _ => {}
                }
            }
            step(name, machine, run, &ctx, events);
        }
        self.errors.extend(ctx.errors.into_inner());
    }

}

/// The code of a functional over `source`, compiled through the evaluator; with none, an empty
/// one that any reader reports.
fn compile(evaluator: &Option<Arc<dyn ExprEvaluator>>, source: &str) -> Arc<[u8]> {
    match evaluator {
        Some(ev) => match ev.compile(source) {
            Ok(c) => {
                ev.release(c.id);
                c.code
            }
            Err(_) => Arc::from(&b""[..]),
        },
        None => Arc::from(&b""[..]),
    }
}

/// A Python literal of a frame's numbers, for a functional's source.
fn py_list(d: &Data) -> Option<String> {
    match d.value() {
        Value::Array(a) => Some(format!("[{}]", a.values().map(|v| format!("{:?}", f64::from(v))).collect::<Vec<_>>().join(", "))),
        _ => None,
    }
}

/// The curve's blend of `x` in 0..1 as Python.
fn curve_source(curve: Curve) -> &'static str {
    match curve {
        Curve::Step => "(1.0 if x >= 1.0 else 0.0)",
        Curve::Linear => "x",
        Curve::In => "x * x",
        Curve::Out => "1.0 - (1.0 - x) * (1.0 - x)",
        Curve::InOut => "(2.0 * x * x if x < 0.5 else 1.0 - 2.0 * (1.0 - x) * (1.0 - x))",
        Curve::Smooth => "x * x * (3.0 - 2.0 * x)",
    }
}

/// The flight's progress in 0..1 at `t`, clamped at both ends: before the start it is 0, past the
/// end it is 1.
fn progress_source(f: &Flight) -> String {
    format!("min(max((t - {:?}) / {:?}, 0.0), 1.0)", f.start, f.end - f.start)
}

/// `a` toward `b` by `x` in 0..1, elementwise; None where the two are not arrays of one shape.
fn lerp(a: &Data, b: &Data, x: f64) -> Option<Data> {
    match (a.value(), b.value()) {
        (Value::Array(p), Value::Array(q)) if p.shape() == q.shape() => {
            let bytes: Vec<u8> = p.values().zip(q.values()).flat_map(|(p, q)| ((f64::from(p) + (f64::from(q) - f64::from(p)) * x) as f32).to_le_bytes()).collect();
            Data::array_f32(p.shape().to_vec(), bytes, Meta::default()).ok()
        }
        _ => None,
    }
}

/// `cur` toward `target` over `duration` by `curve` from `now`: no leg for an instant move. None
/// for what cannot blend, which switches on arrival instead.
fn ramp(now: f64, duration: f64, cur: &Data, target: &Data, curve: Curve) -> Option<Motion> {
    lerp(cur, target, 0.0)?;
    let legs = if duration > 0.0 { vec![Leg { start: now, end: now + duration, from: cur.clone(), to: target.clone(), curve }] } else { Vec::new() };
    Some(Motion { legs, rest: target.clone() })
}

/// `cur` toward `target` by an ADSR from `now`: up to the target over the attack, down to the
/// sustain over the decay, at rest there. The release plays on departure.
fn adsr(now: f64, cur: &Data, target: &Data, [a, d, s, _]: [f64; 4]) -> Option<Motion> {
    let sustain = if s >= 1.0 { target.clone() } else { lerp(cur, target, s)? };
    lerp(cur, target, 0.0)?;
    let mut legs = Vec::new();
    if a > 0.0 {
        legs.push(Leg { start: now, end: now + a, from: cur.clone(), to: target.clone(), curve: Curve::Linear });
    }
    if d > 0.0 && s < 1.0 {
        legs.push(Leg { start: now + a, end: now + a + d, from: target.clone(), to: sustain.clone(), curve: Curve::Linear });
    }
    Some(Motion { legs, rest: sustain })
}

/// Settle the head's motions to `now`: a leg that ended is behind it, and a motion done rests.
fn fold(head: &mut Head, now: f64) {
    let Head { held, motions, .. } = head;
    motions.retain(|a, m| {
        while m.legs.first().is_some_and(|l| l.end <= now) {
            m.legs.remove(0);
        }
        if m.legs.is_empty() {
            held.insert(a.clone(), m.rest.clone());
        }
        !m.legs.is_empty()
    });
}

/// What a step reads and evaluates against.
struct Ctx<'a> {
    compiled: &'a HashMap<String, Expr>,
    evaluator: &'a Option<Arc<dyn ExprEvaluator>>,
    read: &'a dyn Fn(&str) -> Option<Data>,
    now: f64,
    errors: std::cell::RefCell<Vec<String>>,
}

impl<'a> Ctx<'a> {
    fn new(compiled: &'a HashMap<String, Expr>, evaluator: &'a Option<Arc<dyn ExprEvaluator>>, read: &'a dyn Fn(&str) -> Option<Data>, now: f64) -> Ctx<'a> {
        Ctx { compiled, evaluator, read, now, errors: std::cell::RefCell::new(Vec::new()) }
    }

    fn eval(&self, text: &str) -> Result<Data, String> {
        let Some(e) = self.compiled.get(text) else { return Err(format!("`{text}` is not compiled")) };
        if let Some(error) = &e.error {
            return Err(error.clone());
        }
        let mut locals: Vec<(String, Local)> = Vec::with_capacity(e.refs.len());
        for (var, key) in &e.refs {
            let frame = (self.read)(key).ok_or_else(|| format!("variable `{key}` is not defined"))?;
            // A variable holding a functional is read at this instant, as any reader reads it.
            let frame = match (frame.as_functional().is_some(), self.evaluator) {
                (true, Some(ev)) => goofi_node::at(&**ev, &frame, self.now, (0.0, 1.0)).map_err(|e| e.0)?,
                _ => frame,
            };
            locals.push((var.clone(), Local::Frame(frame)));
        }
        if expr_rewrite::is_bare(&e.rewritten) {
            let (var, index) = goofi_node::mailbox::split_index(e.rewritten.trim())?;
            let local = locals.iter().find(|(v, _)| v == var).map(|(_, l)| l).ok_or("no local")?;
            return goofi_node::mailbox::pick(local, index);
        }
        match (self.evaluator, e.id) {
            (Some(ev), Some(id)) => ev.eval(id, &EvalCtx { locals: &locals, t: self.now, range: (0.0, 1.0) }).map_err(|e| e.0),
            _ => Err("no expression evaluator available".to_string()),
        }
    }

    /// The gate an expression holds now; an expression that cannot be read is a closed gate.
    fn gate(&self, text: &str) -> bool {
        match self.eval(text) {
            Ok(frame) => control::truth(&frame),
            Err(why) => {
                self.errors.borrow_mut().push(format!("`{text}`: {why}"));
                false
            }
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

/// A head born at the attribute defaults, placed in `state`.
fn born(machine: &Machine, state: &str, ctx: &Ctx, events: &mut Events) -> Head {
    let mut head = Head {
        state: String::new(),
        prev: String::new(),
        arrived: ctx.now,
        held: machine.attributes.iter().map(|(a, attr)| (a.clone(), attr.default.clone())).collect(),
        motions: IndexMap::new(),
        flight: None,
        due: None,
        cursor: HashMap::new(),
        gates: HashMap::new(),
        fire: None,
    };
    place(&mut head, machine, state);
    arrive(&mut head, machine, state, ctx.now, ctx, events);
    head
}

/// `state`'s values land at once, no easing: what a birth, a jump and a reset do.
fn place(head: &mut Head, machine: &Machine, state: &str) {
    if let Some(s) = machine.states.get(state) {
        for (a, v) in &s.values {
            if head.held.contains_key(a) {
                head.held.insert(a.clone(), v.clone());
                head.motions.shift_remove(a);
            }
        }
    }
}

/// Put `head` at rest in `state` at `at`: its decisions and triggers arm.
fn arrive(head: &mut Head, machine: &Machine, state: &str, at: f64, ctx: &Ctx, events: &mut Events) {
    head.state = state.to_string();
    head.prev.clear();
    head.flight = None;
    head.arrived = at;
    head.fire = None;
    arm(head, machine, ctx, true);
    events.arrived.push(state.to_string());
}

/// A jump: the head leaves where it rests and arrives at once, both seen by `meet` and `alone`.
fn relocate(head: &mut Head, machine: &Machine, state: &str, ctx: &Ctx, events: &mut Events) {
    if head.flight.is_none() {
        events.left.push(head.state.clone());
    }
    place(head, machine, state);
    arrive(head, machine, state, ctx.now, ctx, events);
}

/// Arm what fires out of the head's state: its decisions from its arrival, a gate's baseline.
/// With `fresh`, everything is re-armed; without, a model edit keeps the decision's beat, moved to
/// a changed interval from the last one.
fn arm(head: &mut Head, machine: &Machine, ctx: &Ctx, fresh: bool) {
    if fresh {
        head.due = None;
        head.gates.clear();
    }
    let state = head.state.clone();
    head.gates.retain(|id, _| machine.transitions.get(id).is_some_and(|t| leaves(t, &state)));
    let interval = machine.states.get(&state).map_or(0.0, |s| s.interval);
    let from = if fresh { head.arrived } else { ctx.now };
    head.due = match (machine.outgoing(&state).next(), head.due) {
        (None, _) => None,
        (Some(_), None) => Some(Due { at: from + interval, interval }),
        (Some(_), Some(d)) if d.interval == interval => Some(d),
        (Some(_), Some(d)) if d.at.is_infinite() => Some(Due { at: ctx.now + interval, interval }),
        (Some(_), Some(d)) => Some(Due { at: (d.at - d.interval + interval).max(ctx.now), interval }),
    };
    for (id, t) in machine.transitions.iter().filter(|(_, t)| leaves(t, &state)) {
        for trigger in &t.triggers {
            if let Trigger::When { expression, .. } = trigger {
                if !head.gates.contains_key(id) {
                    let gate = ctx.gate(expression);
                    head.gates.insert(id.clone(), gate);
                }
            }
        }
    }
}

/// Bring a run up to a replaced model.
fn reconcile(machine: &Machine, run: &mut Run, ctx: &Ctx) {
    let mut events = Events::default();
    run.heads.retain(|ph, _| machine.playheads.contains_key(ph));
    for (ph, p) in &machine.playheads {
        match run.heads.get_mut(ph) {
            None => {
                let head = born(machine, &p.start, ctx, &mut events);
                run.heads.insert(ph.clone(), head);
            }
            Some(head) => {
                head.held.retain(|a, _| machine.attributes.contains_key(a));
                head.motions.retain(|a, _| machine.attributes.contains_key(a));
                for (a, attr) in &machine.attributes {
                    if !head.held.contains_key(a) {
                        head.held.insert(a.clone(), attr.default.clone());
                    }
                }
                if let Some(f) = &mut head.flight {
                    f.landing.retain(|a, _| machine.attributes.contains_key(a));
                }
                if !machine.states.contains_key(&head.state) {
                    place(head, machine, &p.start);
                    arrive(head, machine, &p.start, ctx.now, ctx, &mut events);
                } else {
                    arm(head, machine, ctx, false);
                }
            }
        }
    }
}

/// Take `t` out of the head's state at `ctx.now`. Each attribute the next state sets moves from
/// where it stands now, so a redirection is continuous: by the state's envelope where it has one;
/// else by the state left behind's release, once its envelope had come to rest; else by the
/// transition's own duration and curve. One the next state leaves out goes on untouched.
fn depart(head: &mut Head, machine: &Machine, id: &str, ctx: &Ctx, events: &mut Events) {
    let t = &machine.transitions[id];
    let prev = head.state.clone();
    events.left.push(prev.clone());
    fold(head, ctx.now);
    let leaving = machine.states.get(&prev);
    let entering = machine.states.get(&t.to);
    let mut landing = IndexMap::new();
    let attrs: Vec<String> = head.held.keys().cloned().collect();
    for a in attrs {
        let Some(target) = entering.and_then(|s| s.values.get(&a)) else { continue };
        let cur = head.motions.get(&a).map_or_else(|| head.held[&a].clone(), |m| m.at(ctx.now));
        let unfinished = head.motions.contains_key(&a);
        let envelope = entering.and_then(|s| s.envelopes.get(&a)).copied();
        let release = leaving.and_then(|s| s.envelopes.get(&a)).map(|e| e[3]).filter(|_| !unfinished);
        let motion = match (envelope, release) {
            (Some(e), _) => adsr(ctx.now, &cur, target, e),
            (None, Some(r)) => ramp(ctx.now, r, &cur, target, Curve::Linear),
            (None, None) => ramp(ctx.now, t.duration, &cur, target, t.curve),
        };
        match motion {
            Some(m) if m.legs.is_empty() => {
                head.held.insert(a.clone(), m.rest);
                head.motions.shift_remove(&a);
            }
            Some(m) => {
                head.held.insert(a.clone(), cur);
                head.motions.insert(a, m);
            }
            None => {
                landing.insert(a, target.clone());
            }
        }
    }
    head.fire = None;
    if t.duration <= 0.0 {
        head.held.extend(landing);
        return arrive(head, machine, &t.to, ctx.now, ctx, events);
    }
    head.prev = prev;
    head.state = t.to.clone();
    head.due = None;
    head.gates.clear();
    head.flight = Some(Flight { start: ctx.now, end: ctx.now + t.duration, landing });
}

/// Land the flights that are done by this instant, then let what fires out of each rest state
/// take its playhead, a bounded number of hops; a chain still going past the bound is an error.
fn step(name: &str, machine: &Machine, run: &mut Run, ctx: &Ctx, mut events: Events) {
    for head in run.heads.values_mut() {
        fold(head, ctx.now);
        let Some(f) = &head.flight else { continue };
        if f.end <= ctx.now {
            let (state, end) = (head.state.clone(), f.end);
            let landing = std::mem::take(&mut head.flight).map(|f| f.landing).unwrap_or_default();
            head.held.extend(landing);
            arrive(head, machine, &state, end, ctx, &mut events);
        }
    }
    for hop in 0..=HOPS {
        let mut takes: Vec<(String, String)> = Vec::new();
        for (ph, head) in run.heads.iter_mut() {
            // A manual fire redirects a flight; nothing else reaches a playhead in the air.
            if let Some(id) = head.fire.take() {
                if machine.transitions.get(&id).is_some_and(|t| leaves(t, &head.state)) {
                    takes.push((ph.clone(), id));
                    continue;
                }
            }
            if head.flight.is_some() {
                continue;
            }
            // A decision due now is taken, and re-armed from its due; a `when` rising at this
            // instant goes first, the draw among them by weight.
            let decided = match head.due.filter(|d| d.at <= ctx.now) {
                Some(d) => {
                    let at = if d.interval > 0.0 { d.at + d.interval } else { f64::INFINITY };
                    head.due = Some(Due { at, interval: d.interval });
                    decide(machine, head, ctx, &mut run.rng)
                }
                None => None,
            };
            let mut fired: Vec<(String, f64)> = Vec::new();
            for (id, t) in machine.transitions.iter().filter(|(_, t)| leaves(t, &head.state)) {
                for trigger in &t.triggers {
                    if let Trigger::When { expression, weight } = trigger {
                        let gate = ctx.gate(expression);
                        if gate && !head.gates.get(id).copied().unwrap_or(false) {
                            fired.push((id.clone(), *weight));
                        }
                        head.gates.insert(id.clone(), gate);
                    }
                }
            }
            if let Some(id) = pick(&fired, &mut run.rng).or(decided) {
                takes.push((ph.clone(), id));
            }
        }
        // A meeting or a lone stay, from what this hop's arrivals and departures left behind.
        let arrivals = std::mem::take(&mut events.arrived);
        let departures = std::mem::take(&mut events.left);
        for state in arrivals.iter().chain(&departures) {
            let residents: Vec<(String, f64)> =
                run.heads.iter().filter(|(_, h)| h.flight.is_none() && h.state == *state).map(|(ph, h)| (ph.clone(), h.arrived)).collect();
            for (id, t) in machine.transitions.iter().filter(|(_, t)| leaves(t, state)) {
                for trigger in &t.triggers {
                    let chosen: Vec<String> = match trigger {
                        Trigger::Meet { policy } if arrivals.contains(state) && residents.len() >= 2 => match policy {
                            Policy::All => residents.iter().map(|(ph, _)| ph.clone()).collect(),
                            Policy::Fifo => residents.iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|(ph, _)| ph.clone()).into_iter().collect(),
                            Policy::Lifo => residents.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|(ph, _)| ph.clone()).into_iter().collect(),
                        },
                        Trigger::Alone if departures.contains(state) && residents.len() == 1 => vec![residents[0].0.clone()],
                        _ => Vec::new(),
                    };
                    for ph in chosen {
                        if !takes.iter().any(|(p, _)| *p == ph) {
                            takes.push((ph, id.clone()));
                        }
                    }
                }
            }
        }
        if takes.is_empty() {
            break;
        }
        if hop == HOPS {
            ctx.errors.borrow_mut().push(format!("machine `{name}`: {HOPS} instant moves at one instant and still going; the chain was cut"));
            break;
        }
        for (ph, id) in takes {
            if let Some(head) = run.heads.get_mut(&ph) {
                depart(head, machine, &id, ctx, &mut events);
            }
        }
    }
}

/// The transition out of the head's state its decision takes, or none to wait.
fn decide(machine: &Machine, head: &mut Head, ctx: &Ctx, rng: &mut u64) -> Option<String> {
    let s = machine.states.get(&head.state)?;
    let outgoing: Vec<&String> = machine.outgoing(&head.state).collect();
    if outgoing.is_empty() {
        return None;
    }
    match s.mode {
        Decision::Sequence => {
            let next = head.cursor.entry(head.state.clone()).or_insert(0);
            let id = outgoing[*next % outgoing.len()].clone();
            *next = (*next + 1) % outgoing.len();
            Some(id)
        }
        Decision::Random => {
            let mut options: Vec<(String, f64)> = vec![(String::new(), s.noop)];
            options.extend(outgoing.iter().map(|id| ((*id).clone(), s.weights.get(*id).copied().unwrap_or(1.0))));
            pick(&options, rng).filter(|id| !id.is_empty())
        }
        Decision::Expression if s.expression.is_empty() => None,
        Decision::Expression => {
            let index = match ctx.eval(&s.expression) {
                Ok(frame) => control::number_of(&frame),
                Err(why) => {
                    ctx.errors.borrow_mut().push(format!("`{}`: {why}", s.expression));
                    return None;
                }
            };
            if index < 0.0 || index.is_nan() {
                return None;
            }
            let id = outgoing.get(index as usize);
            if id.is_none() {
                ctx.errors.borrow_mut().push(format!("state `{}`: `{}` is {index}, past the last of {} transitions out", head.state, s.expression, outgoing.len()));
            }
            id.map(|id| (*id).clone())
        }
    }
}

/// One of the triggers that fired, drawn by weight; the transition it is on.
fn pick(fired: &[(String, f64)], rng: &mut u64) -> Option<String> {
    let total: f64 = fired.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return None;
    }
    let mut r = draw(rng) * total;
    for (id, w) in fired {
        if r < *w {
            return Some(id.clone());
        }
        r -= w;
    }
    fired.last().map(|(id, _)| id.clone())
}
