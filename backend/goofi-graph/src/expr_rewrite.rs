//! The expression rewrite (spec §5.3): `nd('lfo').out.sig.mean() * variables.gain` becomes
//! `__v0.mean() * __v1` plus the variable map the graph resolves. Slots live behind `.out`,
//! params behind `.params`, a bare `nd('x')` is the single output, and `me` is this node.

use goofi_node::expr::is_ident;
use goofi_node::ExprError;

/// One variable of a rewritten expression, before the graph resolves it to a service or a value.
#[derive(Clone, Debug, PartialEq)]
pub struct VarRef {
    pub var: String,
    pub target: Target,
}

/// What a term refers to. Two terms with equal targets share one variable, and one mailbox.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// `nd('name').out.slot`, or bare `nd('name')` for a single-output node.
    Node { name: String, slot: Option<String> },
    /// `nd('name').params.group.param` — a node's param, read where the binding is derived.
    NodeParam { name: String, group: String, param: String },
    /// `me.out.slot`, or bare `me` for a node with one output.
    MeOut { slot: Option<String> },
    /// `me.params.group.param` — this node's own param.
    MeParam { group: String, param: String },
    Variable { key: String },
}

/// One span of the source to replace, and what it refers to.
struct Term {
    start: usize,
    end: usize,
    target: Target,
}

/// The path a reference names: nothing (the single output), a slot, or a param.
enum Path {
    Bare { end: usize },
    Out { slot: String, end: usize },
    Params { group: String, param: String, end: usize },
}

const SPELLING: &str = "read `.out.<slot>` for an output or `.params.<group>.<param>` for a param";

/// Parse the attribute path after a reference head. No path is the single-output shorthand; a
/// method call (`.mean()`) belongs to the value and counts as none.
fn path_after(source: &str, end: usize, head: &str) -> Result<Path, ExprError> {
    let Some((ns, at)) = ident_after(source, end) else {
        return Ok(Path::Bare { end });
    };
    match ns {
        "out" => {
            let (slot, end) = ident_after(source, at).ok_or_else(|| {
                ExprError(format!("`{head}.out` needs a slot: `{head}.out.<slot>`"))
            })?;
            Ok(Path::Out { slot: slot.to_string(), end })
        }
        "params" => {
            let missing =
                || ExprError(format!("`{head}.params` needs a path: `{head}.params.<group>.<param>`"));
            let (group, at) = ident_after(source, at).ok_or_else(missing)?;
            let (param, end) = ident_after(source, at).ok_or_else(missing)?;
            Ok(Path::Params { group: group.to_string(), param: param.to_string(), end })
        }
        other => Err(ExprError(format!("`{head}.{other}` is not a reference: {SPELLING}"))),
    }
}

/// Rewrite every `nd(..)`, `me` and `variables.*` term into a generated variable, answering the
/// rewritten source and the variables it names, in first-seen order.
pub fn rewrite(source: &str) -> Result<(String, Vec<VarRef>), ExprError> {
    let mut terms: Vec<Term> = Vec::new();
    for call in goofi_node::expr::scan_nd_calls(source) {
        // `nd('')` names nothing however it is spelled, so this precedes the unclosed-call check.
        if call.name.is_empty() {
            return Err(ExprError("nd() needs a node name".to_string()));
        }
        // An unclosed call is left verbatim, so the binding reports a NameError rather than being
        // quietly rewired.
        let Some(end) = call.end else { continue };
        let head = format!("nd('{}')", call.name);
        let target = match path_after(source, end, &head)? {
            Path::Bare { end: at } => (at, Target::Node { name: call.name.to_string(), slot: None }),
            Path::Out { slot, end: at } => {
                (at, Target::Node { name: call.name.to_string(), slot: Some(slot) })
            }
            Path::Params { group, param, end: at } => {
                (at, Target::NodeParam { name: call.name.to_string(), group, param })
            }
        };
        terms.push(Term { start: call.start, end: target.0, target: target.1 });
    }
    for (start, end) in goofi_node::expr::scan_me(source) {
        let target = match path_after(source, end, "me")? {
            Path::Bare { end: at } => (at, Target::MeOut { slot: None }),
            Path::Out { slot, end: at } => (at, Target::MeOut { slot: Some(slot) }),
            Path::Params { group, param, end: at } => (at, Target::MeParam { group, param }),
        };
        terms.push(Term { start, end: target.0, target: target.1 });
    }
    for read in goofi_node::expr::scan_variables(source) {
        terms.push(Term {
            start: read.start,
            end: read.end,
            target: Target::Variable { key: read.name.to_string() },
        });
    }
    let terms = merge(terms);

    let mut vars: Vec<VarRef> = Vec::new();
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for term in terms {
        // Two spellings of one reference share ONE variable: `nd('a').out.x + nd('a').out.x`
        // subscribes once.
        let var = match vars.iter().find(|v| v.target == term.target) {
            Some(v) => v.var.clone(),
            None => {
                let var = format!("__v{}", vars.len());
                vars.push(VarRef { var: var.clone(), target: term.target });
                var
            }
        };
        out.push_str(&source[cursor..term.start]);
        out.push_str(&var);
        cursor = term.end;
    }
    out.push_str(&source[cursor..]);
    Ok((out, vars))
}

/// The two scans' terms as ONE ascending, non-overlapping list — the only shape the splice can
/// consume. An unsorted or nested term slices backwards, which panics under the graph mutex.
fn merge(mut terms: Vec<Term>) -> Vec<Term> {
    terms.sort_by_key(|t| t.start);
    let mut merged: Vec<Term> = Vec::with_capacity(terms.len());
    for term in terms {
        if merged.last().is_none_or(|prev| prev.end <= term.start) {
            merged.push(term);
        }
    }
    merged
}

/// Rewrite the `nd('name').out.slot` terms `rename` answers for — a replacement name, a
/// replacement slot, or neither — leaving every other byte alone. Both positions, because a
/// display name is read in both: a node's own, and, when that node is a boundary port, the slot
/// label its facade wears. A `.params` path offers no slot: a param never renames.
pub fn rename_refs(
    source: &str,
    rename: impl Fn(&str, Option<&str>) -> (Option<String>, Option<String>),
) -> Option<String> {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for call in goofi_node::expr::scan_nd_calls(source) {
        let slot = call
            .end
            .and_then(|end| path_after(source, end, "").ok())
            .and_then(|p| match p {
                Path::Out { slot, end } => Some((slot, end)),
                Path::Bare { .. } | Path::Params { .. } => None,
            });
        let (name, label) = rename(call.name, slot.as_ref().map(|(s, _)| s.as_str()));
        if let Some(name) = name {
            edits.push((call.name_start, call.name_end, name));
        }
        if let (Some((was, at)), Some(label)) = (slot, label) {
            edits.push((at - was.len(), at, label));
        }
    }
    splice(source, edits)
}

/// Rewrite the `variables.<group>.<element>` terms `rename` answers for, leaving every other byte
/// alone.
pub fn rename_variables(source: &str, rename: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for read in goofi_node::expr::scan_variables(source) {
        if let Some(to) = rename(read.name) {
            edits.push((read.end - read.name.len(), read.end, to));
        }
    }
    splice(source, edits)
}

/// Apply `edits` right-to-left, so earlier byte offsets stay valid; `None` when there are none.
fn splice(source: &str, mut edits: Vec<(usize, usize, String)>) -> Option<String> {
    if edits.is_empty() {
        return None;
    }
    let mut out = source.to_string();
    edits.sort_by_key(|(start, _, _)| *start);
    for (start, end, repl) in edits.into_iter().rev() {
        out.replace_range(start..end, &repl);
    }
    Some(out)
}

/// Whether a rewritten expression is BARE: exactly one target with an optional flat index, which
/// an engine reads in Rust and never hands the interpreter. The one owner of that predicate.
pub fn is_bare(rewritten: &str) -> bool {
    let Ok((var, _)) = goofi_node::mailbox::split_index(rewritten.trim()) else { return false };
    var.strip_prefix("__v").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The identifier after a `.` at `end`, and where it stops. `None` when the next thing is a
/// method call, or not an attribute at all.
fn ident_after(source: &str, end: usize) -> Option<(&str, usize)> {
    let b = source.as_bytes();
    if b.get(end) != Some(&b'.') {
        return None;
    }
    let start = end + 1;
    let mut at = start;
    while at < b.len() && is_ident(b[at]) {
        at += 1;
    }
    if at == start || b[start].is_ascii_digit() {
        return None;
    }
    // `.mean()` is the evaluator's to run; whitespace counts, since `nd('x').out.mean ()` is a call.
    let mut after = at;
    while after < b.len() && (b[after] as char).is_whitespace() {
        after += 1;
    }
    if b.get(after) == Some(&b'(') {
        return None;
    }
    Some((&source[start..at], at))
}

