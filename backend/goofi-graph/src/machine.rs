//! State machines: the model the document carries, and the stepping that moves each playhead
//! through it and answers what its variables hold now.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use goofi_core::control;
use goofi_core::ease::Curve;
use goofi_core::variables::{is_valid_identifier, Control, ControlKind, Entries};
use goofi_core::{Data, Meta, Value};
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
    #[ts(type = "Literal")]
    pub default: Data,
    pub kind: AttributeKind,
}

/// A state: where its card sits, and the attributes it sets; one left out is kept on entry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct State {
    #[serde(default)]
    pub pos: [f64; 2],
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[ts(type = "Record<string, Literal>")]
    pub values: IndexMap<String, Data>,
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
    After {
        seconds: Seconds,
        #[serde(default = "one")]
        weight: f64,
    },
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

/// A dwell: a number of seconds, or an expression over the variables read on entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum Seconds {
    Fixed(f64),
    Expression(String),
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
        self.transitions.values().flat_map(|t| &t.triggers).filter_map(|tr| match tr {
            Trigger::When { expression, .. } => Some(expression.as_str()),
            Trigger::After { seconds: Seconds::Expression(e), .. } => Some(e.as_str()),
            _ => None,
        })
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
                match trigger {
                    Trigger::After { weight, .. } | Trigger::When { weight, .. } if weight.is_nan() || *weight < 0.0 => {
                        return Err(format!("transition `{id}`: a trigger's weight is at least 0"));
                    }
                    Trigger::After { seconds: Seconds::Fixed(s), .. } if s.is_nan() || *s < 0.0 => {
                        return Err(format!("transition `{id}`: `after` takes seconds of at least 0"));
                    }
                    Trigger::After { seconds: Seconds::Expression(e), .. } | Trigger::When { expression: e, .. } => {
                        check_expression(e).map_err(|why| format!("transition `{id}`: {why}"))?;
                    }
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

/// A move under way: when it left and when it lands, and what it blends between.
struct Flight {
    start: f64,
    end: f64,
    curve: Curve,
    origin: IndexMap<String, Data>,
    target: IndexMap<String, Data>,
}

/// A dwell armed: when it is due, and the interval it was armed with, which a retry keeps.
#[derive(Clone, Copy)]
struct Due {
    at: f64,
    interval: f64,
}

/// One playhead's run: at rest in `state` since `arrived`, or in flight toward it.
struct Head {
    state: String,
    prev: String,
    arrived: f64,
    held: IndexMap<String, Data>,
    flight: Option<Flight>,
    /// Per `after` transition out of the state, its dwell.
    due: HashMap<String, Due>,
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

    /// The next instant something is owed: a landing, a dwell's due, or a request — what the
    /// thread sleeps toward. None while nothing is armed.
    pub fn next_deadline(&self) -> Option<f64> {
        let heads = self.runs.values().flat_map(|r| r.heads.values());
        let owed = heads.flat_map(|h| h.flight.as_ref().map(|f| f.end).into_iter().chain(h.flight.is_none().then(|| h.due.values().map(|d| d.at)).into_iter().flatten()));
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
        for run in self.runs.values() {
            for (ph, head) in &run.heads {
                out.push((format!("{ph}.state"), Data::text(head.state.as_str())));
                out.push((format!("{ph}.prev"), Data::text(head.prev.as_str())));
                out.push((format!("{ph}.arrived"), Data::number(head.arrived)));
                match &head.flight {
                    None => {
                        out.push((format!("{ph}.progress"), Data::number(1.0)));
                        out.extend(head.held.iter().map(|(a, v)| (format!("{ph}.{a}"), v.clone())));
                    }
                    Some(f) => {
                        out.push((format!("{ph}.progress"), self.functional(&progress_source(f))));
                        for (a, origin) in &f.origin {
                            let value = match f.target.get(a).and_then(|target| blend_source(origin, target, f)) {
                                Some(source) => self.functional(&source),
                                None => origin.clone(),
                            };
                            out.push((format!("{ph}.{a}"), value));
                        }
                    }
                }
            }
        }
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

    /// A functional over `source`, compiled through the evaluator; with none, an empty one that
    /// any reader reports.
    fn functional(&self, source: &str) -> Data {
        let code = match &self.evaluator {
            Some(ev) => match ev.compile(source) {
                Ok(c) => {
                    ev.release(c.id);
                    c.code
                }
                Err(_) => Arc::from(&b""[..]),
            },
            None => Arc::from(&b""[..]),
        };
        Data::functional(code, Meta::default())
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

/// `origin` toward `target` along the flight's curve, elementwise for arrays of one shape; past
/// the end it is the target exactly. None for what switches on arrival instead.
fn blend_source(origin: &Data, target: &Data, f: &Flight) -> Option<String> {
    if origin.as_array().ok()?.shape() != target.as_array().ok()?.shape() {
        return None;
    }
    let (a, b) = (py_list(origin)?, py_list(target)?);
    Some(format!(
        "(lambda x: np.array({a}) + (np.array({b}) - np.array({a})) * ({}))({})",
        curve_source(f.curve),
        progress_source(f)
    ))
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

    fn seconds(&self, s: &Seconds) -> f64 {
        match s {
            Seconds::Fixed(s) => *s,
            Seconds::Expression(text) => match self.eval(text) {
                Ok(frame) => control::number_of(&frame).max(0.0),
                Err(why) => {
                    self.errors.borrow_mut().push(format!("`{text}`: {why}"));
                    0.0
                }
            },
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
        flight: None,
        due: HashMap::new(),
        gates: HashMap::new(),
        fire: None,
    };
    arrive(&mut head, machine, state, ctx.now, ctx, events);
    head
}

/// Put `head` in `state` at `at`: the state's values land, the dwell starts, its triggers arm.
fn arrive(head: &mut Head, machine: &Machine, state: &str, at: f64, ctx: &Ctx, events: &mut Events) {
    head.state = state.to_string();
    head.prev.clear();
    head.flight = None;
    head.arrived = at;
    head.fire = None;
    if let Some(s) = machine.states.get(state) {
        for (a, v) in &s.values {
            if head.held.contains_key(a) {
                head.held.insert(a.clone(), v.clone());
            }
        }
    }
    arm(head, machine, ctx, true);
    events.arrived.push(state.to_string());
}

/// A jump: the head leaves where it rests and arrives at once, both seen by `meet` and `alone`.
fn relocate(head: &mut Head, machine: &Machine, state: &str, ctx: &Ctx, events: &mut Events) {
    if head.flight.is_none() {
        events.left.push(head.state.clone());
    }
    arrive(head, machine, state, ctx.now, ctx, events);
}

/// Arm what fires out of the head's state: a dwell's due from its arrival, a gate's baseline.
/// With `fresh`, everything is re-armed; without, only what was not armed yet, so a model edit
/// keeps a dwell.
fn arm(head: &mut Head, machine: &Machine, ctx: &Ctx, fresh: bool) {
    if fresh {
        head.due.clear();
        head.gates.clear();
    }
    let state = head.state.clone();
    head.due.retain(|id, _| machine.transitions.get(id).is_some_and(|t| leaves(t, &state)));
    head.gates.retain(|id, _| machine.transitions.get(id).is_some_and(|t| leaves(t, &state)));
    for (id, t) in machine.transitions.iter().filter(|(_, t)| leaves(t, &state)) {
        for trigger in &t.triggers {
            match trigger {
                Trigger::After { seconds, .. } if !head.due.contains_key(id) => {
                    let interval = ctx.seconds(seconds);
                    head.due.insert(id.clone(), Due { at: head.arrived + interval, interval });
                }
                Trigger::When { expression, .. } if !head.gates.contains_key(id) => {
                    let gate = ctx.gate(expression);
                    head.gates.insert(id.clone(), gate);
                }
                _ => {}
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
                for (a, attr) in &machine.attributes {
                    if !head.held.contains_key(a) {
                        head.held.insert(a.clone(), attr.default.clone());
                    }
                }
                if let Some(f) = &mut head.flight {
                    f.origin.retain(|a, _| machine.attributes.contains_key(a));
                    f.target.retain(|a, _| machine.attributes.contains_key(a));
                }
                if !machine.states.contains_key(&head.state) {
                    arrive(head, machine, &p.start, ctx.now, ctx, &mut events);
                } else {
                    arm(head, machine, ctx, false);
                }
            }
        }
    }
}

/// Where a flight stands at `t`, for a redirection: the blend of its origin and target, so the
/// next flight leaves from here and is continuous.
fn sample(f: &Flight, t: f64) -> IndexMap<String, Data> {
    let b = f.curve.at(((t - f.start) / (f.end - f.start)).clamp(0.0, 1.0));
    f.origin
        .iter()
        .map(|(a, origin)| {
            let value = match (origin.value(), f.target.get(a).map(Data::value)) {
                (Value::Array(x), Some(Value::Array(y))) if x.shape() == y.shape() => {
                    let bytes: Vec<u8> = x
                        .values()
                        .zip(y.values())
                        .flat_map(|(p, q)| ((f64::from(p) + (f64::from(q) - f64::from(p)) * b) as f32).to_le_bytes())
                        .collect();
                    Data::array_f32(x.shape().to_vec(), bytes, Meta::default()).unwrap_or_else(|_| origin.clone())
                }
                _ => origin.clone(),
            };
            (a.clone(), value)
        })
        .collect()
}

/// Take `t` out of the head's state at `ctx.now`: instant when it has no duration, else a flight
/// from what the head holds now, so a redirection mid-flight is continuous.
fn depart(head: &mut Head, machine: &Machine, id: &str, ctx: &Ctx, events: &mut Events) {
    let t = &machine.transitions[id];
    let prev = head.state.clone();
    events.left.push(prev.clone());
    if let Some(f) = &head.flight {
        head.held = sample(f, ctx.now);
    }
    let target: IndexMap<String, Data> =
        machine.states.get(&t.to).map(|s| s.values.iter().filter(|(a, _)| head.held.contains_key(*a)).map(|(a, v)| (a.clone(), v.clone())).collect()).unwrap_or_default();
    head.fire = None;
    if t.duration <= 0.0 {
        return arrive(head, machine, &t.to, ctx.now, ctx, events);
    }
    head.prev = prev;
    head.state = t.to.clone();
    head.due.clear();
    head.gates.clear();
    head.flight = Some(Flight { start: ctx.now, end: ctx.now + t.duration, curve: t.curve, origin: head.held.clone(), target });
}

/// Land the flights that are done by this instant, then let what fires out of each rest state
/// take its playhead, a bounded number of hops; a chain still going past the bound is an error.
fn step(name: &str, machine: &Machine, run: &mut Run, ctx: &Ctx, mut events: Events) {
    for head in run.heads.values_mut() {
        let Some(f) = &head.flight else { continue };
        if f.end <= ctx.now {
            let (state, end) = (head.state.clone(), f.end);
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
            // Every trigger firing at this instant, across the transitions out of here, is one draw
            // by weight. A dwell that fires is re-armed from its due, drawn or not.
            let mut fired: Vec<(String, f64)> = Vec::new();
            for (id, t) in machine.transitions.iter().filter(|(_, t)| leaves(t, &head.state)) {
                for trigger in &t.triggers {
                    match trigger {
                        Trigger::After { weight, .. } => {
                            if let Some(due) = head.due.get_mut(id).filter(|d| d.at <= ctx.now) {
                                fired.push((id.clone(), *weight));
                                if due.interval > 0.0 {
                                    due.at += due.interval;
                                } else {
                                    head.due.remove(id);
                                }
                            }
                        }
                        Trigger::When { expression, weight } => {
                            let gate = ctx.gate(expression);
                            if gate && !head.gates.get(id).copied().unwrap_or(false) {
                                fired.push((id.clone(), *weight));
                            }
                            head.gates.insert(id.clone(), gate);
                        }
                        _ => {}
                    }
                }
            }
            if let Some(id) = pick(&fired, &mut run.rng) {
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
