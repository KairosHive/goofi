//! Immutable expression read plans from the graph's existing variable bindings.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use goofi_core::{Data, Param};
use goofi_core::time::{seconds, Tick};
use goofi_node::{EvalCtx, Local, RetainedExpression, Uid};
use indexmap::IndexMap;

use crate::{expr_rewrite, BoundVar, Graph, Target};

/// The raw stream leaf, independent of an alias's name or completed worker result.
pub fn stream_key(producer: Uid, generation: u64, slot: &str) -> String {
    format!("\0stream/{}/{generation}/{slot}", producer.to_hex())
}

#[derive(Clone, PartialEq)]
enum Input {
    Variable { key: String },
    Stream { producer: Uid, generation: u64, slot: &'static str },
    Value(Param),
    Missing(String),
}

struct Alias {
    binding: u64,
    rewritten: String,
    inputs: Vec<(String, Input)>,
    code: Option<RetainedExpression>,
    error: Option<String>,
}

/// One-way read projection. Handles retain the exact functions owned by the graph bindings.
#[derive(Default)]
pub struct VariableProjection {
    aliases: IndexMap<String, Alias>,
    evaluator: Option<Arc<dyn goofi_node::ExprEvaluator>>,
}

/// A source can finish an exact logical instant, or report why it cannot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadFault {
    Pending { source: String, at: Tick },
    Failed(String),
}

impl std::fmt::Display for ReadFault {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Pending { source, at } => write!(out, "input `{source}` has no coverage at {}", seconds(*at)), Self::Failed(why) => out.write_str(why) }
    }
}

impl From<String> for ReadFault {
    fn from(why: String) -> Self { Self::Failed(why) }
}
impl From<&str> for ReadFault {
    fn from(why: &str) -> Self { Self::Failed(why.into()) }
}

fn point(frame: &Data, key: &str, at: Tick) -> Result<Data, ReadFault> {
    goofi_node::samples::at(frame, at).map_err(|why| ReadFault::Failed(why.0))?
        .ok_or_else(|| ReadFault::Pending { source: key.into(), at })
}

impl Graph {
    pub fn variable_projection(&self) -> Arc<VariableProjection> {
        let store = self.variables();
        let mut aliases = IndexMap::new();
        for (name, _) in store.entries().filter(|(_, variable)| variable.expression.is_some()) {
            let Some(binding) = self.runtime.variable_binds.get(name) else { continue };
            let inputs = binding.refs.iter().zip(&binding.vars).map(|(reference, bound)| {
                let input = match (&reference.target, bound) {
                    (Target::Variable { key }, _) => Input::Variable { key: key.clone() },
                    (_, BoundVar::Stream { producer, generation, slot, .. }) => Input::Stream { producer: *producer, generation: *generation, slot },
                    (_, BoundVar::Value { value, .. }) => Input::Value(value.clone()),
                    (_, BoundVar::Missing { reason, .. }) => Input::Missing(reason.clone()),
                };
                (reference.var.clone(), input)
            }).collect();
            let mut error = binding.bind_error.clone();
            let code = match (binding.id, self.runtime.evaluator.clone()) {
                (Some(id), Some(evaluator)) => match RetainedExpression::new(evaluator, id) {
                    Ok(code) => Some(code),
                    Err(why) => { error = Some(why.0); None },
                },
                _ => None,
            };
            aliases.insert(name.to_string(), Alias { binding: store.binding(name).expect("existing variable").generation(),
                rewritten: binding.rewritten.clone(), inputs, code, error });
        }
        Arc::new(VariableProjection { aliases, evaluator: self.runtime.evaluator.clone() })
    }

    /// A source edit cannot accept decisions made with an earlier binding plan.
    pub fn matches_variable_projection(&self, projection: &VariableProjection) -> bool {
        let store = self.variables();
        let count = store.entries().filter(|(_, variable)| variable.expression.is_some()).count();
        self.runtime.evaluator.as_ref().map(Arc::as_ptr) == projection.evaluator.as_ref().map(Arc::as_ptr)
            && count == projection.aliases.len() && projection.aliases.iter().all(|(name, alias)|
            store.binding(name).is_some_and(|binding| binding.generation() == alias.binding))
    }
}

impl VariableProjection {
    pub fn stream_inputs(&self) -> Vec<(String, Uid, u64, &'static str)> {
        self.aliases.values().flat_map(|alias| alias.inputs.iter().filter_map(|(_, input)| match input {
            Input::Stream { producer, generation, slot } => Some((stream_key(*producer, *generation, slot), *producer, *generation, *slot)),
            _ => None,
        })).collect()
    }

    pub fn same_config(&self, next: &Self) -> bool {
        self.evaluator.as_ref().map(Arc::as_ptr) == next.evaluator.as_ref().map(Arc::as_ptr)
            && self.aliases.len() == next.aliases.len() && self.aliases.iter().all(|(name, old)| next.aliases.get(name).is_some_and(|new| old.binding == new.binding && old.error == new.error))
    }

    pub fn resolve(&self, key: &str, at: Tick, read: &dyn Fn(&str) -> Option<Data>, errors: &IndexMap<String, String>) -> Result<Data, ReadFault> {
        self.resolve_inner(key, at, read, errors, &mut Vec::new(), &mut HashMap::new())
    }

    fn resolve_inner(&self, key: &str, at: Tick, read: &dyn Fn(&str) -> Option<Data>, errors: &IndexMap<String, String>, path: &mut Vec<String>, values: &mut HashMap<String, Data>) -> Result<Data, ReadFault> {
        if let Some(error) = errors.get(key) { return Err(ReadFault::Failed(error.clone())); }
        if let Some(value) = values.get(key) { return Ok(value.clone()); }
        let Some(alias) = self.aliases.get(key) else {
            let frame = read(key).ok_or_else(|| format!("input `{key}` is not defined"))?;
            return point(&frame, key, at);
        };
        if path.iter().any(|name| name == key) { return Err(format!("variable expression cycle: {} -> {key}", path.join(" -> ")).into()); }
        if let Some(error) = &alias.error { return Err(format!("variable `{key}`: {error}").into()); }
        path.push(key.into());
        let result = (|| {
            let locals = alias.inputs.iter().map(|(name, input)| {
                let local = match input {
                    Input::Variable { key, .. } => Local::Frame(self.resolve_inner(key, at, read, errors, path, values)?),
                    Input::Stream { producer, generation, slot } => {
                        let key = stream_key(*producer, *generation, slot);
                        if let Some(error) = errors.get(&key) { return Err(ReadFault::Failed(error.clone())); }
                        let frame = read(&key).ok_or_else(|| ReadFault::Pending { source: key.clone(), at })?;
                        Local::Frame(point(&frame, &key, at)?)
                    }
                    Input::Value(value) => Local::Value(value.clone()),
                    Input::Missing(why) => return Err(ReadFault::Failed(why.clone())),
                };
                Ok((name.clone(), local))
            }).collect::<Result<Vec<_>, ReadFault>>()?;
            if expr_rewrite::is_bare(&alias.rewritten) {
                let (name, index) = goofi_node::mailbox::split_index(alias.rewritten.trim())?;
                let local = locals.iter().find(|(n, _)| n == name).map(|(_, local)| local).ok_or("alias input is missing")?;
                goofi_node::mailbox::pick(local, index).map_err(ReadFault::Failed)
            } else {
                alias.code.as_ref().ok_or("alias has no deterministic compiled function")?.eval(&EvalCtx { locals: &locals, t: seconds(at), range: (0.0, 1.0) }).map_err(|why| ReadFault::Failed(why.0))
            }
        })();
        path.pop();
        if let Ok(value) = &result { values.insert(key.into(), value.clone()); }
        result
    }

    /// Expand an alias to the leaf frames the runtime must receive.
    pub fn leaves(&self, key: &str, out: &mut HashSet<String>) {
        for (key, alias) in self.dependencies(key) {
            match alias {
                None => { out.insert(key.into()); },
                Some(alias) => for (_, input) in &alias.inputs {
                    if let Input::Stream { producer, generation, slot } = input {
                        out.insert(stream_key(*producer, *generation, slot));
                    }
                },
            }
        }
    }

    pub fn observes(&self, key: &str, varying: &dyn Fn(&str) -> bool) -> bool {
        self.dependencies(key).any(|(key, alias)| match alias {
            None => varying(key),
            Some(alias) => alias.code.as_ref().is_some_and(|code| code.observes_time)
                || alias.inputs.iter().any(|(_, input)| match input {
                    Input::Stream { producer, generation, slot } => varying(&stream_key(*producer, *generation, slot)),
                    _ => false,
                }),
        })
    }

    /// Source edits change binding identities; renames retain them throughout the DAG.
    pub fn equivalent(&self, key: &str, next: &Self, to: &str) -> bool {
        self.evaluator.as_ref().map(Arc::as_ptr) == next.evaluator.as_ref().map(Arc::as_ptr)
            && self.bindings(key) == next.bindings(to)
    }

    fn bindings(&self, key: &str) -> HashSet<u64> {
        self.dependencies(key).filter_map(|(_, alias)| alias.map(|alias| alias.binding)).collect()
    }

    /// Visit each variable dependency once, including unresolved leaves and cycles.
    fn dependencies<'a>(&'a self, key: &'a str) -> impl Iterator<Item = (&'a str, Option<&'a Alias>)> {
        let mut pending = vec![key];
        let mut seen = HashSet::new();
        std::iter::from_fn(move || loop {
            let key = pending.pop()?;
            if !seen.insert(key) { continue; }
            let alias = self.aliases.get(key);
            if let Some(alias) = alias {
                pending.extend(alias.inputs.iter().filter_map(|(_, input)| match input {
                    Input::Variable { key } => Some(key.as_str()),
                    _ => None,
                }));
            }
            return Some((key, alias));
        })
    }
}
