//! Text for a reader: the STYLE a door asks for — compact, every byte to the point, for an agent
//! or a pipe; human, parted and aligned to a terminal's width, painted where the terminal shows
//! color — and the wrap, table and paint under it.

use serde_json::Value;

/// How a result reads. `width` is the column to wrap at; 0 wraps nothing. `color` paints the
/// human layout's roles in ANSI — for a terminal, never for a pipe, a panel or an agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub human: bool,
    pub width: usize,
    pub color: bool,
}

impl Style {
    pub const COMPACT: Style = Style { human: false, width: 0, color: false };

    /// The style an `/exec` body asks for: `style: "human"`, `width` and `color`; absent is compact.
    pub fn from_request(body: &Value) -> Style {
        let human = body["style"].as_str() == Some("human");
        Style {
            human,
            width: body["width"].as_u64().unwrap_or(0) as usize,
            color: human && body["color"].as_bool().unwrap_or(false),
        }
    }

    /// `s` in its role's color when this style paints; verbatim otherwise.
    pub fn paint(&self, role: Role, s: &str) -> String {
        if !self.color || s.is_empty() {
            return s.to_string();
        }
        let ansi = role.ansi();
        format!("{}{s}{}", ansi.render(), ansi.render_reset())
    }
}

/// What a span of the human layout IS, so attention lands in order: an alert before everything,
/// a title over the names under it, a name over its arguments, prose last and unpainted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Why something was refused.
    Alert,
    /// A section, a group, a label.
    Title,
    /// An op, a key, the first column of a table.
    Name,
    /// Arguments and their types.
    Arg,
    /// A table's header row.
    Header,
}

impl Role {
    fn ansi(self) -> anstyle::Style {
        use anstyle::{AnsiColor, Style};
        match self {
            Role::Alert => Style::new().bold().fg_color(Some(AnsiColor::Red.into())),
            Role::Title => Style::new().bold(),
            Role::Name => Style::new().fg_color(Some(AnsiColor::Cyan.into())),
            Role::Arg => Style::new().dimmed(),
            Role::Header => Style::new().dimmed().underline(),
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

/// Prose reflowed and painted whole in one role, line by line, so the wrap never sees an escape.
pub fn painted(text: &str, role: Role, style: &Style) -> String {
    reflow(text, style.width).lines().map(|l| style.paint(role, l)).collect::<Vec<_>>().join("\n")
}

/// Rows of cells in aligned columns, two spaces apart, `indent` columns in, each cell painted in
/// the role `role(row, column)` names. The last column takes what the width leaves and wraps
/// there; when that is too narrow, it goes on its own lines under the row, four columns further in.
pub fn table(rows: &[Vec<String>], indent: usize, style: &Style, role: impl Fn(usize, usize) -> Option<Role>) -> String {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }
    fn cell(row: &[String], j: usize) -> &str {
        row.get(j).map(String::as_str).unwrap_or("")
    }
    let paint = |i: usize, j: usize, s: &str| match role(i, j) {
        Some(r) => style.paint(r, s),
        None => s.to_string(),
    };
    let width = style.width;
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
    for (i, row) in rows.iter().enumerate() {
        let mut head = pad.clone();
        for (j, w) in widths[..cols - 1].iter().enumerate() {
            let text = cell(row, j);
            head.push_str(&paint(i, j, text));
            head.push_str(&" ".repeat(w - text.chars().count() + 2));
        }
        let tail: Vec<String> = wrap(cell(row, cols - 1), last).iter().map(|l| paint(i, cols - 1, l)).collect();
        if stacked {
            out.push(head.trim_end().to_string());
            out.extend(tail.iter().filter(|l| !l.is_empty()).map(|l| format!("{pad}    {l}")));
            continue;
        }
        for (k, l) in tail.iter().enumerate() {
            let line = if k == 0 { format!("{head}{l}") } else { format!("{pad}{}{l}", " ".repeat(lead)) };
            out.push(line.trim_end().to_string());
        }
    }
    out.join("\n")
}

/// A value as a table cell: a string bare, null empty, a list joined by commas, a record as
/// `key=value` pairs, the rest as JSON spells it.
pub fn cell(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Array(a) => a.iter().map(cell).collect::<Vec<_>>().join(", "),
        Value::Object(m) => m.iter().map(|(k, x)| format!("{k}={}", cell(x))).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}
