//! The one lexical scan of an expression's source. Every reader of `nd(..)`, `sm(..)`, `me` and
//! `vars.*` walks these tokens, so a string, an f-string or a `#` comment hides a term from all of
//! them alike.

/// What a token is; the source between tokens is whitespace or a comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A word: an identifier or a keyword.
    Ident,
    /// A number.
    Number,
    /// A string literal with its prefix and quotes; an f-string's braces are part of it.
    Str,
    /// One punctuation byte.
    Punct(u8),
}

/// One token: its kind and its byte span in the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
}

pub fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Tokenize `source`. An unterminated string or comment runs to the end; nothing fails, since the
/// evaluator reports the syntax and a scan only needs to know what is text.
pub fn tokens(source: &str) -> Vec<Token> {
    let b = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'#' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c.is_ascii_whitespace() || c == b'\\' {
            i += 1;
        } else if c == b'\'' || c == b'"' {
            let end = string_end(b, i);
            out.push(Token { kind: Kind::Str, start: i, end });
            i = end;
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let start = i;
            while i < b.len() && (is_ident(b[i]) || b[i] == b'.') {
                i += 1;
            }
            out.push(Token { kind: Kind::Number, start, end: i });
        } else if is_ident(c) {
            let start = i;
            while i < b.len() && is_ident(b[i]) {
                i += 1;
            }
            // A string prefix (`f`, `rb`, `u`...) belongs to the string that follows it.
            let prefix = source[start..i].bytes().all(|p| matches!(p.to_ascii_lowercase(), b'r' | b'b' | b'f' | b'u'));
            if prefix && i - start <= 2 && b.get(i).is_some_and(|q| *q == b'\'' || *q == b'"') {
                let end = string_end(b, i);
                out.push(Token { kind: Kind::Str, start, end });
                i = end;
            } else {
                out.push(Token { kind: Kind::Ident, start, end: i });
            }
        } else {
            out.push(Token { kind: Kind::Punct(c), start: i, end: i + 1 });
            i += 1;
        }
    }
    out
}

/// One past the closing quote of the string opening at `at`, triple quotes and escapes honoured.
fn string_end(b: &[u8], at: usize) -> usize {
    let q = b[at];
    let triple = b.get(at + 1) == Some(&q) && b.get(at + 2) == Some(&q);
    let mut i = at + if triple { 3 } else { 1 };
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
        } else if triple && b[i] == q && b.get(i + 1) == Some(&q) && b.get(i + 2) == Some(&q) {
            return i + 3;
        } else if !triple && (b[i] == q || b[i] == b'\n') {
            return i + 1;
        } else {
            i += 1;
        }
    }
    b.len()
}

/// One `nd(..)` or `sm(..)` call, with both spans its consumers need: the name literal a rename
/// replaces, and the whole term a rewrite replaces.
pub struct Call<'a> {
    pub start: usize,
    pub name_start: usize,
    pub name_end: usize,
    /// One past the closing `)`, or `None` when the call does not close cleanly — a rewrite leaves
    /// those verbatim, so the failure shows up as an eval error.
    pub end: Option<usize>,
    pub name: &'a str,
}

/// Every `callee('name')` call in `source`, in source order: `nd` names a node, `sm` a state or a
/// playhead of a machine.
pub fn scan_calls<'a>(source: &'a str, callee: &str) -> Vec<Call<'a>> {
    let toks = tokens(source);
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.kind != Kind::Ident || &source[t.start..t.end] != callee {
            continue;
        }
        if toks.get(i + 1).is_none_or(|o| o.kind != Kind::Punct(b'(')) {
            continue;
        }
        let Some(lit) = toks.get(i + 2).filter(|l| l.kind == Kind::Str) else { continue };
        let Some((name_start, name_end)) = plain_string_body(source, lit) else { continue };
        let end = toks.get(i + 3).filter(|c| c.kind == Kind::Punct(b')')).map(|c| c.end);
        out.push(Call { start: t.start, name_start, name_end, end, name: &source[name_start..name_end] });
    }
    out
}

/// The body of a plain single-quoted string token: no prefix, no triple quote, no escape. A node
/// name is spelled plainly or it is not a name.
fn plain_string_body(source: &str, lit: &Token) -> Option<(usize, usize)> {
    let b = source.as_bytes();
    let q = b[lit.start];
    if q != b'\'' && q != b'"' || lit.end - lit.start < 2 || b[lit.end - 1] != q {
        return None;
    }
    let body = &source[lit.start + 1..lit.end - 1];
    (!body.contains(['\\', '\n']) && !body.starts_with(q as char)).then_some((lit.start + 1, lit.end - 1))
}

/// One `vars.<group>.<element>` read [`scan_variables`] found; the span covers the prefix too.
pub struct VariableRead<'a> {
    pub start: usize,
    pub end: usize,
    pub name: &'a str,
}

/// Every `vars.<group>.<element>` read in `source`. Every variable is `group.element`, so one
/// identifier alone names nothing.
pub fn scan_variables(source: &str) -> Vec<VariableRead<'_>> {
    let toks = tokens(source);
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.kind != Kind::Ident || &source[t.start..t.end] != "vars" {
            continue;
        }
        let path = [1, 2, 3, 4].map(|k| toks.get(i + k));
        let [Some(d1), Some(group), Some(d2), Some(element)] = path else { continue };
        let dot = |t: &Token| t.kind == Kind::Punct(b'.');
        if dot(d1) && group.kind == Kind::Ident && dot(d2) && element.kind == Kind::Ident {
            out.push(VariableRead { start: t.start, end: element.end, name: &source[group.start..element.end] });
        }
    }
    out
}

/// Every bare `me` in `source`: a word of its own, not an attribute (`x.me`), never text.
pub fn scan_me(source: &str) -> Vec<(usize, usize)> {
    let toks = tokens(source);
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        let attribute = i > 0 && toks[i - 1].kind == Kind::Punct(b'.');
        if t.kind == Kind::Ident && &source[t.start..t.end] == "me" && !attribute {
            out.push((t.start, t.end));
        }
    }
    out
}

/// One argument of a builtin, as the editor documents it.
#[derive(Clone, Copy)]
pub struct Arg {
    pub name: &'static str,
    /// What it takes, in a word: `number`, `string`, `number or array`.
    pub kind: &'static str,
    /// The default, spelled as the user would; `None` is required.
    pub default: Option<&'static str>,
    /// The closed set of strings it takes, where there is one.
    pub options: &'static [&'static str],
    /// Keyword-only: it is spelled `name=value`.
    pub keyword: bool,
    pub doc: &'static str,
}

/// One name the evaluator injects into every expression, as the editor documents it. The harness
/// in `goofi_python::inproc::expr` defines each; the python situation holds the two together.
#[derive(Clone, Copy)]
pub struct Builtin {
    pub name: &'static str,
    /// `function`, `value` or `namespace`.
    pub kind: &'static str,
    /// Where it reads: `all`, `node` (a param's expression) or `machine`.
    pub scope: &'static str,
    /// The call or the path as the user spells it.
    pub signature: &'static str,
    pub doc: &'static str,
    pub args: &'static [Arg],
    /// What the one name argument picks from the live patch: `node`, `machine` or nothing.
    pub pick: Option<&'static str>,
}

const RANGE: [Arg; 4] = [
    Arg { name: "freq", kind: "number", default: Some("1"), options: &[], keyword: true, doc: "cycles per second of `src`" },
    Arg { name: "src", kind: "number or array", default: Some("t"), options: &[], keyword: true, doc: "what the phase runs over; the patch time unless given" },
    Arg { name: "vmin", kind: "number", default: Some("the param's minimum"), options: &[], keyword: true, doc: "the value at the bottom of the cycle" },
    Arg { name: "vmax", kind: "number", default: Some("the param's maximum"), options: &[], keyword: true, doc: "the value at the top of the cycle" },
];

/// Every name an expression may use beyond Python's own, `numpy` and `math`.
pub static BUILTINS: &[Builtin] = &[
    Builtin { name: "nd", kind: "function", scope: "all", signature: "nd('node')", pick: Some("node"), args: &[
        Arg { name: "name", kind: "string", default: None, options: &[], keyword: false, doc: "the node's display name" },
    ], doc: "A node's output: bare for a node with one output, `.out.<slot>` for one of several, `.params.<group>.<param>` for a param's current value." },
    Builtin { name: "sm", kind: "function", scope: "node", signature: "sm('playhead').element", pick: Some("machine"), args: &[
        Arg { name: "name", kind: "string", default: None, options: &[], keyword: false, doc: "a playhead's name" },
    ], doc: "A playhead of a state machine: `.state`, `.progress` or one of its attributes, as the machine publishes them." },
    Builtin { name: "sm", kind: "function", scope: "machine", signature: "sm('state' | 'playhead')", pick: Some("machine"), args: &[
        Arg { name: "name", kind: "string", default: None, options: &[], keyword: false, doc: "a state's or a playhead's name in this machine" },
    ], doc: "A state's census — `.residents`, `.playheads`, `.arrivals`, `.departures` — or another playhead's record, as `me` is." },
    Builtin { name: "me", kind: "value", scope: "node", signature: "me", pick: None, args: &[],
        doc: "This node: bare for its one output, `.out.<slot>` for one of several, `.params.<group>.<param>` for one of its params." },
    Builtin { name: "me", kind: "value", scope: "machine", signature: "me", pick: None, args: &[],
        doc: "The playhead this expression is read for: `.name`, `.state`, `.arrived` and one field per attribute." },
    Builtin { name: "vars", kind: "namespace", scope: "all", signature: "vars.group.element", pick: None, args: &[],
        doc: "The patch's variables: a control panel's widgets, a MIDI device's controls and the system's facts." },
    Builtin { name: "t", kind: "value", scope: "all", signature: "t", pick: None, args: &[],
        doc: "Patch seconds: the patch clock at this evaluation." },
    Builtin { name: "np", kind: "namespace", scope: "all", signature: "np", pick: None, args: &[],
        doc: "numpy, whole. A frame arrives as a float32 array; a text as a str." },
    Builtin { name: "lfo", kind: "function", scope: "all", signature: "lfo(freq=1, src=t, vmin=…, vmax=…, fn='sin')", pick: None, args: &[
        RANGE[0], RANGE[1], RANGE[2], RANGE[3],
        Arg { name: "fn", kind: "string", default: Some("'sin'"), options: &["sin", "cos", "tri", "sqr", "saw"], keyword: true, doc: "the waveform over one cycle" },
    ], doc: "A periodic wave between `vmin` and `vmax`, one cycle per `1/freq` of `src`." },
    Builtin { name: "nse", kind: "function", scope: "all", signature: "nse(freq=1, src=t, vmin=…, vmax=…)", pick: None, args: &[
        RANGE[0], RANGE[1], RANGE[2], RANGE[3],
    ], doc: "Smooth noise between `vmin` and `vmax`, new every `1/freq` of `src`." },
    Builtin { name: "gte", kind: "function", scope: "node", signature: "gte(x, attack=0, decay=0.1, sustain=1, release=0)", pick: None, args: &[
        Arg { name: "x", kind: "number, text or array", default: None, options: &[], keyword: false, doc: "what is watched: a number turning positive, a text changing, an array with an element turning positive" },
        Arg { name: "attack", kind: "number", default: Some("0"), options: &[], keyword: true, doc: "seconds from the edge up to 1" },
        Arg { name: "decay", kind: "number", default: Some("0.1"), options: &[], keyword: true, doc: "seconds from 1 down to the sustain: with the defaults, how long the gate stays open" },
        Arg { name: "sustain", kind: "number", default: Some("1"), options: &[], keyword: true, doc: "the level the decay lands on" },
        Arg { name: "release", kind: "number", default: Some("0"), options: &[], keyword: true, doc: "seconds from the sustain down to 0" },
    ], doc: "A one-shot envelope from each rising edge of `x`: 0 to 1 over the attack, to the sustain over the decay, to 0 over the release, in patch seconds from the edge. The defaults open a gate for the decay. The edge is read as the param's node runs; the binding keeps the last `x` and the edge's instant." },
    Builtin { name: "qtz", kind: "function", scope: "all", signature: "qtz(v, scale='major', root='C', mode='voct')", pick: None, args: &[
        Arg { name: "v", kind: "number or array", default: None, options: &[], keyword: false, doc: "the pitch to snap, elementwise over an array" },
        Arg { name: "scale", kind: "string", default: Some("'major'"), options: &goofi_core::scale::PRESET_NAMES, keyword: false, doc: "the scale whose degrees are kept" },
        Arg { name: "root", kind: "string or number", default: Some("'C'"), options: goofi_core::scale::NOTES, keyword: false, doc: "a note name, or semitones above C" },
        Arg { name: "mode", kind: "string", default: Some("'voct'"), options: &["voct", "hz"], keyword: false, doc: "what `v` is: volts per octave with C4 at 0 V, or hertz" },
    ], doc: "The nearest degree of `scale` from `root`, in the unit `v` came in." },
];

/// The names `from math import *` brings into every expression.
pub static MATH: &[&str] = &[
    "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "ceil", "comb", "copysign", "cos", "cosh", "degrees", "dist",
    "erf", "erfc", "exp", "expm1", "fabs", "factorial", "floor", "fmod", "frexp", "fsum", "gamma", "gcd", "hypot", "isclose",
    "isfinite", "isinf", "isnan", "isqrt", "lcm", "ldexp", "lgamma", "log", "log10", "log1p", "log2", "modf", "nextafter", "perm",
    "pow", "prod", "radians", "remainder", "sin", "sinh", "sqrt", "tan", "tanh", "trunc", "ulp",
];
/// The constants `from math import *` brings into every expression.
pub static MATH_CONSTANTS: &[&str] = &["pi", "e", "tau", "inf", "nan"];
