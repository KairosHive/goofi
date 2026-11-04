//! The phrase layer: ONE parser from a command line to an op call, and one renderer from a
//! result to text. Every transport that speaks lines shares it, so the surfaces cannot drift.

use serde_json::{json, Map, Value};

use crate::ops::{ArgDecl, Entry, Row, TREE};
use crate::text::{self, Role, Style};

/// A refusal that teaches: `why`, and what answers it in the shape [`help`] answers — the op
/// whose arguments were wrong (`{op}`), the ops under the words that matched (`{group}`), or the
/// index — laid out by [`Refusal::render`] for the reader's style.
#[derive(Debug)]
pub struct Refusal {
    pub why: String,
    pub help: Option<Value>,
}

impl From<String> for Refusal {
    fn from(why: String) -> Refusal {
        Refusal { why, help: None }
    }
}

/// The compact line: what a transport with no style of its own says.
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render(&Style::COMPACT))
    }
}

impl Refusal {
    fn teaching(why: String, help: Value) -> Refusal {
        Refusal { why, help: Some(help) }
    }

    /// The refusal as text. Compact is one line: why, then what answers it after a dash. Human
    /// parts them: the why reflowed, the answer laid out as help is.
    pub fn render(&self, style: &Style) -> String {
        let Some(h) = &self.help else {
            return match style.human {
                true => text::reflow(&self.why, style.width),
                false => self.why.clone(),
            };
        };
        if !style.human {
            let tail = if let Some(op) = h.get("op") {
                usage_words(&text::cell(&op["op"]), &strings(&op["args"]))
            } else if let Some(g) = h.get("group") {
                let names: Vec<String> = g["ops"].as_array().into_iter().flatten().map(|o| text::cell(&o["op"])).collect();
                format!("under `{}`: {}", text::cell(&g["word"]), names.join(", "))
            } else {
                "`op list` answers every op this server speaks".to_string()
            };
            return format!("{} — {tail}", self.why);
        }
        let answer = match h.get("op") {
            Some(op) => text::table(&[vec![text::cell(&op["op"]), strings(&op["args"]).join(" "), text::cell(&op["doc"])]], 2, style, op_row),
            None => render_help(h, style),
        };
        format!("{}\n\n{answer}", text::painted(&self.why, Role::Alert, style))
    }
}

/// Where a word-by-word descent of the phrase tree ended — the one state machine under
/// [`resolve`] and [`complete`].
enum Stop {
    /// The words named a leaf: its full phrase.
    Op(String),
    /// The words ran out inside a group: what can come next, and the phrase so far.
    Children(&'static [Entry], String),
    /// A word matched nothing at its level; the phrase walked before the miss.
    Unknown(String),
}

fn walk(words: &[String]) -> Stop {
    let mut children = TREE;
    let mut prefix = String::new();
    for word in words {
        match children.iter().find(|e| e.word() == word) {
            Some(Entry::Group(w, _, kids)) => {
                prefix.push_str(w);
                prefix.push(' ');
                children = kids;
            }
            Some(Entry::Leaf(op)) => return Stop::Op(op.name.to_string()),
            None => return Stop::Unknown(prefix),
        }
    }
    Stop::Children(children, prefix)
}

/// The op a word sequence names among the served rows, and how many words it took. A group word
/// alone is its `list` op where it has one (`goofi guide` is `guide list`). A refusal names the
/// served phrases under the words that matched, or the group a mode withholds.
pub fn resolve<'a>(ops: &'a [Row], words: &[String]) -> Result<(&'a Row, usize), Refusal> {
    let line = words.join(" ");
    if let Some(op) = ops.iter().find(|op| {
        let name: Vec<_> = op.name.split(' ').collect();
        words.len() >= name.len() && name.iter().zip(words).all(|(a, b)| *a == b)
    }) {
        return Ok((op, op.name.split(' ').count()));
    }
    let prefix = match walk(words) {
        Stop::Op(name) => name,
        Stop::Children(kids, prefix) => {
            let listing = format!("{prefix}list");
            if let Some(op) = kids.iter().any(|k| k.word() == "list").then(|| ops.iter().find(|o| o.name == listing)).flatten() {
                return Ok((op, words.len()));
            }
            prefix
        }
        Stop::Unknown(prefix) => prefix,
    };
    let near: Vec<&Row> = match prefix.is_empty() {
        true => Vec::new(),
        false => ops.iter().filter(|o| o.name.starts_with(&prefix)).collect(),
    };
    Err(match (near.is_empty(), prefix.is_empty()) {
        // The tree spells the phrase and no row serves it: a subtree this server withholds.
        (true, false) => {
            let group = prefix.split(' ').next().unwrap_or(&prefix);
            format!("unknown op `{line}` — this server does not serve the `{group}` ops").into()
        }
        (true, true) => Refusal::teaching(format!("unknown op `{line}`"), json!({ "index": index(ops) })),
        _ => Refusal::teaching(format!("unknown op `{line}`"), json!({ "group": group_value(prefix.trim_end(), &near, &[]) })),
    })
}

/// One line, parsed against the registry: the phrase, then every argument as a flag the op's own
/// schema types. Answers the op and the payload the socket envelope would carry.
pub fn parse<'a>(ops: &'a [Row], line: &str) -> Result<(&'a Row, Value), Refusal> {
    let words = shell_words::split(line).map_err(|e| Refusal::from(e.to_string()))?;
    if words.is_empty() {
        return Err("empty command".to_string().into());
    }
    let (op, used) = resolve(ops, &words)?;
    let payload = parse_flags(op, &words[used..])?;
    Ok((op, payload))
}

/// The arguments an op offers, one word each: leading positionals as `<name>`, then
/// `--name <type>`, `!` marking required.
fn spelled(op: &Row) -> Vec<String> {
    op.args()
        .iter()
        .enumerate()
        .map(|(i, a)| match (i < op.positional, a.ty.as_str(), a.required) {
            (true, _, true) => format!("<{}>", a.name),
            (true, _, false) => format!("[{}]", a.name),
            (false, "bool", _) => format!("--[no-]{}", a.name),
            (false, ty, true) => format!("--{} <{ty}>!", a.name),
            (false, ty, false) => format!("--{} <{ty}>", a.name),
        })
        .collect()
}

/// An op's usage from its spelled arguments: what a refusal that teaches ends with.
fn usage_words(name: &str, spelled: &[String]) -> String {
    match spelled.is_empty() {
        true => format!("`{name}` takes no arguments"),
        false => format!("`{name}` takes: {}", spelled.join(" ")),
    }
}

/// An op's usage line for a person, wrapped: the name painted as a title, its arguments after it.
fn usage_lines(name: &str, args: &[String], style: &Style) -> String {
    let lines = text::wrap(&format!("{name} {}", args.join(" ")), style.width);
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| match i {
            0 => format!("{}{}", style.paint(Role::Title, name), style.paint(Role::Arg, &l[name.len()..])),
            _ => style.paint(Role::Arg, l),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One raw flag value, typed by the schema. A list type `T[]` is typed by its item.
fn typed(op: &Row, key: &str, ty: &str, raw: String) -> Result<Value, Refusal> {
    match ty {
        "int" => raw.parse::<i64>().map(Value::from).map_err(|_| {
            format!("{}: `--{key}` takes an integer, not `{raw}`", op.name).into()
        }),
        "float" => raw.parse::<f64>().map(Value::from).map_err(|_| {
            format!("{}: `--{key}` takes a number, not `{raw}`", op.name).into()
        }),
        "float2" => {
            let parsed = raw.split_once(',').and_then(|(x, y)| {
                Some(json!([x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?]))
            });
            parsed.ok_or_else(|| format!("{}: `--{key}` takes `x,y`, not `{raw}`", op.name).into())
        }
        "json" => serde_json::from_str(&raw)
            .map_err(|e| format!("{}: `--{key}` takes JSON — {e}", op.name).into()),
        // `any`: JSON when it parses, otherwise the bare string — `--value 2.5` and `--value hi`.
        "any" => Ok(serde_json::from_str(&raw).unwrap_or(Value::String(raw))),
        // uid, string, param_addr and the rest ride as the string they are.
        _ => Ok(Value::String(raw)),
    }
}

/// Where a scan of an op's argument words rests: what the NEXT word may be. The one grammar
/// under [`parse_flags`] and [`complete_args`].
enum Expect<'a> {
    /// `--name` was given and its value is pending.
    Value(&'a str, &'a str),
    /// Between words: a flag, or this declared arg as a positional while they are still open.
    Open(Option<&'a ArgDecl>),
}

/// Step through `words` against the op's args, emitting each `(decl name, typed value)`. Leading
/// bare words fill the positionals (a list one is variadic) until the first flag.
fn scan<'a>(
    op: &Row,
    decls: &'a [ArgDecl],
    words: &[String],
    emit: &mut dyn FnMut(&'a str, Value) -> Result<(), Refusal>,
) -> Result<Expect<'a>, Refusal> {
    let fail = |msg: String| Refusal::teaching(format!("{}: {msg}", op.name), json!({ "op": op_entry(op) }));
    let (mut pos, mut open, mut i) = (0, true, 0);
    while i < words.len() {
        let word = &words[i];
        i += 1;
        let Some(flag) = word.strip_prefix("--") else {
            let Some(a) = decls.get(pos).filter(|_| open && pos < op.positional) else {
                return Err(fail(format!("unexpected `{word}`")));
            };
            emit(&a.name, typed(op, &a.name, a.item(), word.clone())?)?;
            pos += usize::from(!a.is_list());
            continue;
        };
        open = false;
        let (mut key, inline) = match flag.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (flag, None),
        };
        let mut negated = false;
        if let Some(bare) = key.strip_prefix("no-") {
            // `--no-x` only where `x` is a declared bool: an op's own `no-…` name stays reachable.
            if decls.iter().any(|a| a.name == bare && a.is_bool()) {
                (key, negated) = (bare, true);
            }
        }
        let Some(a) = decls.iter().find(|a| a.name == key) else {
            return Err(fail(format!("no flag `--{key}`")));
        };
        match (a.ty.as_str(), inline, words.get(i)) {
            ("bool", Some(_), _) => {
                return Err(format!("{}: `--{key}` is a flag and takes no value", op.name).into())
            }
            ("bool", None, _) => emit(&a.name, Value::Bool(!negated))?,
            (_, Some(v), _) => emit(&a.name, typed(op, key, a.item(), v)?)?,
            (_, None, Some(next)) => {
                i += 1;
                emit(&a.name, typed(op, key, a.item(), next.clone())?)?;
            }
            (ty, None, None) => return Ok(Expect::Value(&a.name, ty)),
        }
    }
    Ok(Expect::Open(decls.get(pos).filter(|_| open && pos < op.positional)))
}

/// A command line's argument words as the payload the socket envelope would carry.
fn parse_flags(op: &Row, words: &[String]) -> Result<Value, Refusal> {
    let decls = op.args();
    let mut payload = Map::new();
    let mut put = |name: &str, v: Value| {
        if decls.iter().any(|a| a.name == name && a.is_list()) {
            match payload.entry(name.to_string()).or_insert_with(|| json!([])) {
                Value::Array(list) => list.push(v),
                _ => unreachable!("a list arg only ever inserts an array"),
            }
        } else if payload.insert(name.to_string(), v).is_some() {
            return Err(format!("{}: `{name}` was given twice", op.name).into());
        }
        Ok(())
    };
    let teach = |msg: String| Refusal::teaching(format!("{}: {msg}", op.name), json!({ "op": op_entry(op) }));
    if let Expect::Value(name, _) = scan(op, &decls, words, &mut put)? {
        return Err(teach(format!("`--{name}` needs a value")));
    }
    for a in decls.iter().filter(|a| a.required) {
        if !payload.contains_key(&a.name) {
            return Err(teach(format!("`--{}` is required", a.name)));
        }
    }
    Ok(Value::Object(payload))
}

/// A value one table cell holds whole: a scalar, a list of scalars, or a record of scalars.
fn cellable(v: &Value) -> bool {
    let scalar = |x: &Value| !matches!(x, Value::Array(_) | Value::Object(_));
    match v {
        Value::Array(a) => a.iter().all(scalar),
        Value::Object(m) => m.values().all(scalar),
        _ => true,
    }
}

/// A non-empty list of records — objects whose every value a cell holds — what the human style
/// draws as a table.
fn records(rows: &[Value]) -> bool {
    !rows.is_empty() && rows.iter().all(|r| r.as_object().is_some_and(|m| m.values().all(cellable)))
}

/// An object for a person: its cell-sized values as key and value, then each list of records
/// as a table under its key, and anything else as JSON under its key.
fn sections(o: &Map<String, Value>, style: &Style) -> String {
    let mut pairs: Vec<Vec<String>> = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    for (k, v) in o {
        match v {
            v if cellable(v) => pairs.push(vec![k.clone(), text::cell(v)]),
            Value::Array(rows) if records(rows) => parts.push(format!("{}\n{}", style.paint(Role::Title, k), records_table(rows, style))),
            v => parts.push(format!("{}\n{}", style.paint(Role::Title, k), pretty(v).lines().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n"))),
        }
    }
    let mut out = Vec::new();
    if !pairs.is_empty() {
        out.push(text::table(&pairs, 0, style, key_value));
    }
    out.extend(parts);
    out.join("\n\n")
}

/// Records as a table: a header of every key in first-seen order, then one row per record.
fn records_table(rows: &[Value], style: &Style) -> String {
    let mut keys: Vec<&str> = Vec::new();
    for r in rows {
        for k in r.as_object().expect("a record").keys() {
            if !keys.contains(&k.as_str()) {
                keys.push(k);
            }
        }
    }
    // The widest column goes last: it is the prose, and the last column is the one that wraps.
    let widest = keys.iter().enumerate().max_by_key(|(_, k)| rows.iter().map(|r| text::cell(&r[**k]).chars().count()).max()).map(|(i, _)| i);
    if let Some(i) = widest {
        let k = keys.remove(i);
        keys.push(k);
    }
    let mut table = vec![keys.iter().map(|k| k.to_string()).collect::<Vec<String>>()];
    table.extend(rows.iter().map(|r| keys.iter().map(|k| text::cell(&r[*k])).collect()));
    text::table(&table, 0, style, |i, j| match (i, j) {
        (0, _) => Some(Role::Header),
        (_, 0) => Some(Role::Name),
        _ => None,
    })
}

/// The roles of an op's row in a listing: the op, then its arguments, then prose.
fn op_row(_: usize, column: usize) -> Option<Role> {
    match column {
        0 => Some(Role::Name),
        1 => Some(Role::Arg),
        _ => None,
    }
}

/// The roles of a key and value table: the key.
fn key_value(_: usize, column: usize) -> Option<Role> {
    (column == 0).then_some(Role::Name)
}

/// Whether a value fits on one line: a scalar, or a RECORD — an object with no container in it.
fn flat(v: &Value) -> bool {
    match v {
        Value::Array(_) => false,
        Value::Object(m) => m.values().all(|x| !matches!(x, Value::Array(_) | Value::Object(_))),
        _ => true,
    }
}

/// JSON as a caller reads it, with ONE departure from a plain pretty print: a list of flat
/// records puts one record per line, the shape every catalog read answers.
pub fn pretty(v: &Value) -> String {
    fn go(v: &Value, pad: usize) -> String {
        let (inner, close) = (" ".repeat(pad + 2), " ".repeat(pad));
        let wrap = |open: char, body: Vec<String>, shut: char| match body.is_empty() {
            true => format!("{open}{shut}"),
            false => format!("{open}\n{}\n{close}{shut}", body.join(",\n")),
        };
        match v {
            Value::Array(a) if a.iter().all(flat) => {
                wrap('[', a.iter().map(|e| format!("{inner}{e}")).collect(), ']')
            }
            Value::Array(a) => {
                wrap('[', a.iter().map(|e| format!("{inner}{}", go(e, pad + 2))).collect(), ']')
            }
            Value::Object(m) => wrap(
                '{',
                m.iter()
                    .map(|(k, val)| format!("{inner}{}: {}", Value::String(k.clone()), go(val, pad + 2)))
                    .collect(),
                '}',
            ),
            other => other.to_string(),
        }
    }
    go(v, 0)
}

/// An op's answer as text: prose and bare strings verbatim, help laid out, everything else as
/// JSON — except that the human style draws a list of records as a table and an object as
/// sections. A result carrying NPY is DATA: the text form points at the pipe, and only the CLI
/// writes the bytes.
pub fn render(result: &Value, style: &Style) -> String {
    if let Value::String(s) = result {
        return s.clone();
    }
    if let Some(b64) = result.get("npy_b64").and_then(Value::as_str) {
        return format!(
            "an ARRAY frame as NPY — {} base64 bytes in `npy_b64`, beside its `meta`",
            b64.len()
        );
    }
    match result {
        Value::Object(o) if o.len() == 1 && o.contains_key("help") => render_help(&o["help"], style),
        Value::Object(o) if o.len() == 1 && o.get("text").is_some_and(Value::is_string) => text::cell(&o["text"]),
        Value::Array(rows) if style.human && records(rows) => records_table(rows, style),
        Value::Object(o) if style.human && !o.is_empty() => sections(o, style),
        _ => pretty(result),
    }
}

/// The command lines a `{commands: [...]}` body carries; a line that is not a string is empty.
pub fn command_lines(body: &Value) -> Vec<String> {
    body["commands"].as_array().map(|c| c.iter().map(|l| l.as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default()
}

/// One or several command lines, as every line transport runs them. A single line may ask for
/// help; several lines run as ONE batch, where a parse refusal names its command index.
pub fn exec_lines(
    state: &crate::AppState,
    lines: &[String],
    actor: &str,
) -> Result<Vec<Value>, Refusal> {
    let table = state.ops();
    if lines.is_empty() {
        return Err("`commands` is a non-empty list of command lines".to_string().into());
    }
    if let [line] = lines {
        let words = shell_words::split(line).map_err(|e| Refusal::from(e.to_string()))?;
        if let Some(help) = help(&table, &words) {
            return Ok(vec![help]);
        }
        let (op, payload) = parse(&table, line)?;
        return state.call(op.name, payload, actor).map(|r| vec![r]).map_err(Refusal::from);
    }
    let mut steps = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let (op, payload) = parse(&table, line).map_err(|mut r| {
            r.why = format!("command {i}: {}", r.why);
            r
        })?;
        steps.push(json!({ "op": op.name, "payload": payload }));
    }
    match state.call("compound", json!({ "ops": steps }), actor).map_err(Refusal::from)? {
        Value::Array(results) => Ok(results),
        other => Ok(vec![other]),
    }
}

/// The help door: `help [words…]` or `<words…> --help`. `None` when the line is not asking.
/// The answer is DATA under `help` — the index, a group, one op, or a miss with the index —
/// and [`render`] lays it out for the reader's style.
pub fn help(ops: &[Row], words: &[String]) -> Option<Value> {
    let target: Vec<String> = match words.first().map(String::as_str) {
        Some("help") => words[1..].to_vec(),
        _ if words.iter().any(|w| w == "--help") => {
            words.iter().filter(|w| *w != "--help").cloned().collect()
        }
        _ => return None,
    };
    if target.is_empty() {
        return Some(json!({ "help": { "index": index(ops) } }));
    }
    if let Ok((op, _)) = resolve(ops, &target) {
        return Some(json!({ "help": { "op": op_help(op) } }));
    }
    let first = target[0].as_str();
    let ops_under: Vec<&Row> = ops.iter().filter(|o| o.name.split(' ').next() == Some(first)).collect();
    let doors: Vec<&str> = crate::ops::RESERVED.iter().copied().filter(|r| r.split(' ').next() == Some(first) && r.contains(' ')).collect();
    if ops_under.is_empty() && doors.is_empty() {
        return Some(json!({ "help": { "missing": target.join(" "), "index": index(ops) } }));
    }
    Some(json!({ "help": { "group": group_value(first, &ops_under, &doors) } }))
}

/// A group as help carries it: its word and doc, the ops under it, and the client's own doors there.
fn group_value(word: &str, ops: &[&Row], doors: &[&str]) -> Value {
    let doc = TREE.iter().find_map(|e| match e {
        Entry::Group(w, doc, _) if *w == word => Some(*doc),
        _ => None,
    });
    let ops: Vec<Value> = ops.iter().map(|o| op_entry(o)).collect();
    json!({ "word": word, "doc": doc, "ops": ops, "doors": doors })
}

/// The top of the vocabulary: the served groups, the subjectless ops and the client's own words.
fn index(ops: &[Row]) -> Value {
    let mut groups: Vec<Value> = Vec::new();
    let mut bare: Vec<&str> = Vec::new();
    for e in TREE {
        match e {
            Entry::Group(word, doc, _) if served(ops, "", e) => groups.push(json!({ "word": word, "doc": doc })),
            Entry::Group(..) => {}
            Entry::Leaf(op) => bare.push(op.name),
        }
    }
    json!({ "groups": groups, "subjectless": bare, "reserved": crate::ops::RESERVED })
}

/// One op as a group listing carries it: the phrase, its argument words and its first sentence.
fn op_entry(op: &Row) -> Value {
    json!({ "op": op.name, "args": spelled(op), "doc": first_sentence(op.doc) })
}

/// The first sentence of a doc: up to the first full stop that ends a word, or its first line.
fn first_sentence(doc: &str) -> &str {
    let line = doc.lines().next().unwrap_or_default();
    match line.match_indices(". ").next() {
        Some((at, _)) => &line[..=at],
        None => line,
    }
}

fn op_help(op: &Row) -> Value {
    json!({ "op": op.name, "args": spelled(op), "doc": op.doc(), "answers": op.result })
}

/// The doors on the index, as the human style tables them.
const DOORS: &[(&str, &str)] = &[
    ("help <group>", "the ops of one group"),
    ("<phrase> --help", "one op, explained"),
    ("op list [--doc]", "the registry as data; `--doc` explains every op at once, the whole manual"),
    ("goofi -", "stdin lines, run as one batch"),
];

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(text::cell).collect()).unwrap_or_default()
}

/// A help value as text. Compact is one dense block; human parts it and aligns it.
fn render_help(h: &Value, style: &Style) -> String {
    if let Some(op) = h.get("op") {
        let (name, args, doc, answers) = (text::cell(&op["op"]), strings(&op["args"]), text::cell(&op["doc"]), text::cell(&op["answers"]));
        let usage = match args.is_empty() {
            true => format!("`{name}` takes no arguments"),
            false => format!("`{name}` takes: {}", args.join(" ")),
        };
        return match style.human {
            false => format!("{usage}\n\n{doc}\n\nanswers: {answers}"),
            true => format!(
                "{}\n\n{}\n\n{}\n{}",
                usage_lines(&name, &args, style),
                text::reflow(&doc, style.width),
                style.paint(Role::Title, "answers"),
                text::reflow(&format!("  {answers}"), style.width),
            ),
        };
    }
    if let Some(g) = h.get("group") {
        let ops: Vec<&Value> = g["ops"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
        let doors = strings(&g["doors"]);
        if !style.human {
            let lines: Vec<String> = ops
                .iter()
                .map(|o| {
                    let args = strings(&o["args"]);
                    match args.is_empty() {
                        true => format!("  `{}` takes no arguments", text::cell(&o["op"])),
                        false => format!("  `{}` takes: {}", text::cell(&o["op"]), args.join(" ")),
                    }
                })
                .chain(doors.iter().map(|d| format!("  `{d}` — the client's own door")))
                .collect();
            return format!("under `{}`:\n{}", text::cell(&g["word"]), lines.join("\n"));
        }
        let mut rows: Vec<Vec<String>> = ops
            .iter()
            .map(|o| vec![text::cell(&o["op"]), strings(&o["args"]).join(" "), text::cell(&o["doc"])])
            .collect();
        rows.extend(doors.iter().map(|d| vec![d.clone(), String::new(), "the client's own door".to_string()]));
        let word = text::cell(&g["word"]);
        let head = match g["doc"].as_str() {
            Some(doc) => format!("{word} — {doc}"),
            None => word.clone(),
        };
        // Painted after the wrap: the word leads the first line, and the rest is prose.
        let head = text::reflow(&head, style.width).replacen(&word, &style.paint(Role::Title, &word), 1);
        return format!("{head}\n\n{}", text::table(&rows, 2, style, op_row));
    }
    let index = &h["index"];
    let groups: Vec<&Value> = index["groups"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let (bare, reserved) = (strings(&index["subjectless"]), strings(&index["reserved"]));
    let missing = h.get("missing").map(text::cell);
    if !style.human {
        let top = format!(
            "goofi speaks noun-first phrases. `help <group>` lists one group, a group word\n\
             alone runs its `list`, `<phrase> --help` explains one op, and `op list` answers\n\
             the registry as data — `op list --doc` explains every op at once, the whole manual.\n\
             groups:\n{}\n\
             subjectless: {}.\n\
             reserved words: {} — and `goofi -` runs stdin lines as one batch.",
            groups.iter().map(|g| format!("  {} — {}", text::cell(&g["word"]), text::cell(&g["doc"]))).collect::<Vec<_>>().join("\n"),
            bare.join(", "),
            reserved.join(", "),
        );
        return match missing {
            Some(m) => format!("nothing under `{m}` — {top}"),
            None => top,
        };
    }
    let doors: Vec<Vec<String>> = DOORS.iter().map(|(d, doc)| vec![d.to_string(), doc.to_string()]).collect();
    let group_rows: Vec<Vec<String>> = groups.iter().map(|g| vec![text::cell(&g["word"]), text::cell(&g["doc"])]).collect();
    let words = vec![
        vec!["subjectless".to_string(), bare.join(", ")],
        vec!["reserved".to_string(), format!("{} — the client's own words", reserved.join(", "))],
    ];
    let mut out = String::new();
    if let Some(m) = missing {
        out.push_str(&format!("{}\n\n", style.paint(Role::Alert, &format!("nothing under `{m}`"))));
    }
    out.push_str(&format!(
        "goofi speaks noun-first phrases: `goofi <group> <op> [args]`; a group word alone runs its `list`\n\n{}\n\n{}\n{}\n\n{}",
        text::table(&doors, 2, style, key_value),
        style.paint(Role::Title, "groups"),
        text::table(&group_rows, 2, style, key_value),
        text::table(&words, 0, style, key_value),
    ));
    out
}

/// Whether anything under `entry` (at the phrase position `prefix`) is in the served set — how a
/// headless server's listings drop the layout group without a second spelling of the mode.
fn served(ops: &[Row], prefix: &str, entry: &Entry) -> bool {
    match entry {
        Entry::Leaf(op) => ops.iter().any(|o| o.name == op.name),
        Entry::Group(word, _, _) => {
            let below = format!("{prefix}{word} ");
            ops.iter().any(|o| o.name.starts_with(&below))
        }
    }
}

/// An entry's completion doc: a group's own line, an op's first line capped to a candidate row.
fn doc_line(entry: &Entry) -> String {
    let line = match entry {
        Entry::Group(_, doc, _) => doc,
        Entry::Leaf(op) => op.doc.lines().next().unwrap_or_default(),
    };
    match line.char_indices().nth(100) {
        Some((cut, _)) => {
            let end = line[..cut].rfind(' ').unwrap_or(cut);
            format!("{}…", &line[..end])
        }
        None => line.to_string(),
    }
}

/// What can come NEXT on a partial command line, as `(word, doc)` candidates; a trailing partial
/// word filters. `state` gives the LIVE candidates, and `None` still answers everything static.
pub fn complete(
    ops: &[Row],
    state: Option<&crate::AppState>,
    line: &str,
) -> Vec<(String, String)> {
    let Ok(words) = shell_words::split(line) else { return Vec::new() };
    let (done, partial) = match line.ends_with(char::is_whitespace) || words.is_empty() {
        true => (&words[..], String::new()),
        false => (&words[..words.len() - 1], words[words.len() - 1].clone()),
    };
    if let Ok((op, used)) = resolve(ops, done) {
        return complete_args(op, &done[used..], &partial, state);
    }
    let mut candidates = std::collections::BTreeMap::new();
    if let Stop::Children(children, prefix) = walk(done) {
        for entry in children {
            if entry.word().starts_with(&partial) && served(ops, &prefix, entry) {
                candidates.insert(entry.word().to_string(), doc_line(entry));
            }
        }
    }
    for op in ops {
        let words: Vec<_> = op.name.split(' ').collect();
        if words.len() > done.len() && words.iter().zip(done).all(|(a, b)| *a == b) {
            let word = words[done.len()];
            if word.starts_with(&partial) {
                candidates.entry(word.to_string()).or_insert_with(|| op.doc.lines().next().unwrap_or_default().to_string());
            }
        }
    }
    candidates.into_iter().collect()
}

fn complete_args(
    op: &Row,
    given: &[String],
    partial: &str,
    state: Option<&crate::AppState>,
) -> Vec<(String, String)> {
    let decls = op.args();
    let mut spent: Vec<String> = Vec::new();
    let mut note = |name: &str, _: Value| {
        spent.push(name.to_string());
        Ok(())
    };
    let positional = match scan(op, &decls, given, &mut note) {
        Ok(Expect::Value(_, ty)) => return values_for(ty, state, partial),
        Ok(Expect::Open(p)) => p.filter(|_| !partial.starts_with("--")),
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<(String, String)> =
        positional.map(|a| values_for(&a.ty, state, partial)).unwrap_or_default();
    for a in decls.iter() {
        if spent.contains(&a.name) && !a.is_bool() && !a.is_list() {
            continue;
        }
        let doc = match (a.is_list(), a.is_bool(), a.required) {
            (_, true, _) => "flag; --no- negates".to_string(),
            (true, _, true) => format!("<{}>, repeats, required", a.item()),
            (true, _, false) => format!("<{}>, repeats", a.item()),
            (false, _, true) => format!("<{}>, required", a.ty),
            (false, _, false) => format!("<{}>", a.ty),
        };
        let flag = format!("--{}", a.name);
        if flag.starts_with(partial) {
            out.push((flag, doc));
        }
    }
    out
}

/// The live values a typed position can offer. Closed vocabularies answer always; the patch's own
/// names need the server.
fn values_for(ty: &str, state: Option<&crate::AppState>, partial: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = match ty.trim_end_matches("[]") {
        "panel_type" => crate::vocab::PANEL_TYPES
            .iter()
            .map(|p| (p.id.to_string(), p.doc.to_string()))
            .collect(),
        "uid" | "endpoint" => {
            let Some(state) = state else { return Vec::new() };
            let doc = state.doc.lock().to_json();
            let Some(nodes) = doc.get("nodes").and_then(Value::as_object) else {
                return Vec::new();
            };
            // The NAME is the candidate — it is what a human types — and the uid rides in the
            // doc column; both spellings resolve.
            nodes
                .iter()
                .map(|(uid, n)| {
                    let name = n["name"].as_str().unwrap_or_default();
                    let kind = n["type"].as_str().unwrap_or_default();
                    (name.to_string(), format!("({kind}) {uid}"))
                })
                .collect()
        }
        _ => Vec::new(),
    };
    out.retain(|(w, _)| w.starts_with(partial));
    out.sort();
    out
}

