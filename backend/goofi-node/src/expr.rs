//! The one lexical scan of an expression's source. Every reader of `nd(..)`, `me` and
//! `variables.*` walks these tokens, so a string, an f-string or a `#` comment hides a term from
//! all of them alike.

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

/// One `nd(..)` call, with both spans its consumers need: the name literal a rename replaces, and
/// the whole term a rewrite replaces.
pub struct NdCall<'a> {
    pub start: usize,
    pub name_start: usize,
    pub name_end: usize,
    /// One past the closing `)`, or `None` when the call does not close cleanly — a rewrite leaves
    /// those verbatim, so the failure shows up as an eval error.
    pub end: Option<usize>,
    pub name: &'a str,
}

/// Every `nd('name')` call in `source`, in source order.
pub fn scan_nd_calls(source: &str) -> Vec<NdCall<'_>> {
    let toks = tokens(source);
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.kind != Kind::Ident || &source[t.start..t.end] != "nd" {
            continue;
        }
        if toks.get(i + 1).is_none_or(|o| o.kind != Kind::Punct(b'(')) {
            continue;
        }
        let Some(lit) = toks.get(i + 2).filter(|l| l.kind == Kind::Str) else { continue };
        let Some((name_start, name_end)) = plain_string_body(source, lit) else { continue };
        let end = toks.get(i + 3).filter(|c| c.kind == Kind::Punct(b')')).map(|c| c.end);
        out.push(NdCall { start: t.start, name_start, name_end, end, name: &source[name_start..name_end] });
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

/// One `variables.<group>.<element>` read [`scan_variables`] found; the span covers the prefix too.
pub struct VariableRead<'a> {
    pub start: usize,
    pub end: usize,
    pub name: &'a str,
}

/// Every `variables.<group>.<element>` read in `source`. Every variable is `group.element`, so
/// one identifier alone names nothing.
pub fn scan_variables(source: &str) -> Vec<VariableRead<'_>> {
    let toks = tokens(source);
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.kind != Kind::Ident || &source[t.start..t.end] != "variables" {
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
