//! Text for a reader: the STYLE a door asks for — compact, every byte to the point, for an agent
//! or a pipe; human, parted and aligned to a terminal's width — and the wrap and table under it.

use serde_json::Value;

/// How a result reads. `width` is the column to wrap at; 0 wraps nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub human: bool,
    pub width: usize,
}

impl Style {
    pub const COMPACT: Style = Style { human: false, width: 0 };

    /// The style an `/exec` body asks for: `style: "human"` and `width`; absent is compact.
    pub fn from_request(body: &Value) -> Style {
        Style {
            human: body["style"].as_str() == Some("human"),
            width: body["width"].as_u64().unwrap_or(0) as usize,
        }
    }
}

/// The narrowest column a wrapped cell is worth; under it the cell goes on lines of its own.
const NARROW: usize = 24;

/// Greedy word wrap of one line; a width of 0 keeps it whole, and one word never splits.
pub fn wrap(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![line.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in line.split_whitespace() {
        let fits = cur.is_empty() || cur.chars().count() + 1 + word.chars().count() <= width;
        if !fits {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

/// Prose reflowed to `width`: blank lines part paragraphs, an indented line keeps its indent
/// and wraps on its own, and the rest of a paragraph flows as one.
pub fn reflow(text: &str, width: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut Vec<String>| {
        if !run.is_empty() {
            out.extend(wrap(run, width));
            run.clear();
        }
    };
    for line in text.lines() {
        if line.trim().is_empty() {
            flush(&mut run, &mut out);
            out.push(String::new());
        } else if line.starts_with(char::is_whitespace) {
            flush(&mut run, &mut out);
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            let inner = width.saturating_sub(indent.chars().count());
            out.extend(wrap(line.trim_start(), if inner < NARROW { 0 } else { inner }).into_iter().map(|l| format!("{indent}{l}")));
        } else {
            if !run.is_empty() {
                run.push(' ');
            }
            run.push_str(line.trim());
        }
    }
    flush(&mut run, &mut out);
    out.join("\n")
}

/// Rows of cells in aligned columns, two spaces apart, `indent` columns in. The last column
/// takes what the width leaves and wraps there; when that is too narrow, it goes on its own
/// lines under the row, four columns further in.
pub fn table(rows: &[Vec<String>], indent: usize, width: usize) -> String {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }
    fn cell(row: &[String], j: usize) -> &str {
        row.get(j).map(String::as_str).unwrap_or("")
    }
    let widths: Vec<usize> = (0..cols).map(|j| rows.iter().map(|r| cell(r, j).chars().count()).max().unwrap_or(0)).collect();
    let lead: usize = widths[..cols - 1].iter().sum::<usize>() + 2 * (cols - 1);
    let last = match width {
        0 => 0,
        w if w.saturating_sub(indent + lead) >= widths[cols - 1] => 0,
        w if w.saturating_sub(indent + lead) >= NARROW => w - indent - lead,
        w => w.saturating_sub(indent + 4).max(NARROW),
    };
    let stacked = width != 0 && last != 0 && width.saturating_sub(indent + lead) < NARROW;
    let pad = " ".repeat(indent);
    let mut out = Vec::new();
    for row in rows {
        let mut head = pad.clone();
        for (j, w) in widths[..cols - 1].iter().enumerate() {
            head.push_str(&format!("{:<w$}  ", cell(row, j)));
        }
        let tail = wrap(cell(row, cols - 1), last);
        if stacked {
            out.push(head.trim_end().to_string());
            out.extend(tail.iter().filter(|l| !l.is_empty()).map(|l| format!("{pad}    {l}")));
            continue;
        }
        for (i, l) in tail.iter().enumerate() {
            let line = if i == 0 { format!("{head}{l}") } else { format!("{pad}{}{l}", " ".repeat(lead)) };
            out.push(line.trim_end().to_string());
        }
    }
    out.join("\n")
}

/// A scalar as a table cell: a string bare, null empty, the rest as JSON spells it.
pub fn cell(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
