//! A SECTION in a header's `params`: a run of params drawn apart from the rest of their group and
//! stored as `section_name`, or with `repeat` a LIST of them, repeated by a count param and
//! stored as one param per slot, `name_<slot>`. A `widget` names what a list is to its reader
//! and declares its members itself; a `show` on a section shows every param of it.

use std::collections::HashMap;

use goofi_core::probe::Widget;
use serde_json::{json, Value};

/// The most slots a list declares: a shader keeps one uniform field per slot.
pub const MAX_SLOTS: i64 = 64;

/// The entries with every section flattened into the params it stands for. A list without
/// sections is handed back as it came, so an engine's own flat description keeps its indices.
pub fn expand(entries: Vec<Value>) -> Result<Vec<Value>, String> {
    if !entries.iter().any(is_section) {
        return Ok(entries);
    }
    let mut out = Vec::new();
    let mut runs: HashMap<String, Run> = HashMap::new();
    for mut entry in entries {
        if !is_section(&entry) {
            let group = text(&entry, "group").ok_or("a param names its group")?.to_string();
            let index = runs.entry(group).or_default().next(false);
            entry["section"] = json!(index);
            out.push(entry);
            continue;
        }
        let name = text(&entry, "section").ok_or("a section names itself")?.to_string();
        if !goofi_core::variables::is_valid_name(&name) {
            return Err(format!("section `{name}`: {}", goofi_core::variables::NAME_RULE));
        }
        let group = text(&entry, "group").ok_or_else(|| format!("section `{name}` names its group"))?.to_string();
        let index = runs.entry(group.clone()).or_default().next(true);
        let widget = entry
            .get("widget")
            .map(|w| serde_json::from_value::<Widget>(w.clone()).map_err(|_| format!("section `{name}`: widget `{w}` is not `colormap`")))
            .transpose()?;
        let shows = rules(entry.get("show"));
        let members = match (widget, entry.get_mut("params").and_then(Value::as_array_mut)) {
            (Some(_), Some(_)) => return Err(format!("section `{name}` is a widget, which declares its own params")),
            (Some(Widget::Colormap), None) => colormap_members(),
            (None, Some(m)) => std::mem::take(m),
            (None, None) => return Err(format!("section `{name}` lists its params")),
        };
        let members: Vec<Value> = members.into_iter().map(|m| shown_by(m, &shows)).collect();
        let repeat = entry.get("repeat").map(|r| Repeat::parse(r, &name)).transpose()?;
        let Some(repeat) = repeat else {
            if widget.is_some() {
                return Err(format!("section `{name}` is a widget, which repeats"));
            }
            for mut m in members {
                let base = text(&m, "name").ok_or_else(|| format!("a param of section `{name}` has a name"))?.to_string();
                if m.get("defaults").is_some() {
                    return Err(format!("`{base}` in section `{name}` has defaults per slot, which only a repeated section has"));
                }
                m["group"] = json!(group);
                m["name"] = json!(format!("{name}_{base}"));
                m["section"] = json!(index);
                m["role"] = json!({ "as": "member", "section": name, "base": base });
                out.push(m);
            }
            continue;
        };
        let mut count = json!({
            "group": group, "name": name, "kind": "num", "int": true, "section": index,
            "default": repeat.default, "min": repeat.min, "max": repeat.max,
            "role": { "as": "count", "section": name, "widget": widget },
        });
        if let Some(doc) = entry.get("doc") {
            count["doc"] = doc.clone();
        }
        out.push(shown_by(count, &shows));
        let bases: Vec<String> = members.iter().filter_map(|m| text(m, "name").map(str::to_string)).collect();
        for slot in 0..repeat.max {
            for m in &members {
                let base = text(m, "name").ok_or_else(|| format!("a param of section `{name}` has a name"))?.to_string();
                let mut m = m.clone();
                m["group"] = json!(group);
                m["name"] = json!(format!("{base}_{slot}"));
                m["section"] = json!(index);
                m["role"] = json!({ "as": "member", "section": name, "base": base, "slot": slot });
                // `defaults` gives each slot its own; past its end the last one carries on.
                if let Some(list) = m.get("defaults").and_then(Value::as_array).cloned() {
                    let at = (slot as usize).min(list.len().saturating_sub(1));
                    let one = list.get(at).cloned().ok_or_else(|| format!("`{base}` in section `{name}` has an empty `defaults`"))?;
                    m["default"] = one;
                    m.as_object_mut().expect("a param object").remove("defaults");
                }
                // A member shown by another member of the list is shown by the one in ITS slot.
                let rules: Vec<&mut Value> = match m.get_mut("show") {
                    Some(Value::Array(rules)) => rules.iter_mut().collect(),
                    Some(rule) => vec![rule],
                    None => Vec::new(),
                };
                for rule in rules {
                    if let Some(controller) = text(rule, "param").map(str::to_string) {
                        if bases.contains(&controller) {
                            rule["param"] = json!(format!("{controller}_{slot}"));
                        }
                    }
                }
                out.push(m);
            }
        }
    }
    Ok(out)
}

fn is_section(entry: &Value) -> bool {
    entry.get("params").is_some() || entry.get("widget").is_some()
}

/// A `show` as its list of rules: one rule bare, a list of them, or none.
fn rules(show: Option<&Value>) -> Vec<Value> {
    match show {
        Some(Value::Array(list)) => list.clone(),
        Some(rule) => vec![rule.clone()],
        None => Vec::new(),
    }
}

/// `param` shown by the section's rules as well as its own.
fn shown_by(mut param: Value, shows: &[Value]) -> Value {
    if !shows.is_empty() {
        let mut all = rules(param.get("show"));
        all.extend(shows.iter().cloned());
        param["show"] = Value::Array(all);
    }
    param
}

/// The slots of a colormap: a stop's place on the map, its colour, and the way to the next stop.
fn colormap_members() -> Vec<Value> {
    vec![
        json!({ "name": "at", "kind": "num", "defaults": [0.0, 1.0], "min": 0.0, "max": 1.0,
                "doc": "where the stop sits on the map, from 0 to 1" }),
        json!({ "name": "colour", "kind": "color", "defaults": [[0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]],
                "doc": "the colour at the stop" }),
        json!({ "name": "blend", "kind": "str", "default": "linear", "options": ["linear", "smooth", "step"],
                "doc": "how the colour runs to the next stop: straight, eased in and out, or held" }),
    ]
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

/// One group's section indices as the entries hand them out: a section takes a fresh index, and
/// so does a plain param following one, so the inspector parts the two.
#[derive(Default)]
struct Run {
    next: u8,
    seen: bool,
    in_section: bool,
}

impl Run {
    fn next(&mut self, section: bool) -> u8 {
        if self.seen && (section || self.in_section) {
            self.next = self.next.saturating_add(1);
        }
        self.seen = true;
        self.in_section = section;
        self.next
    }
}

/// How often a list section repeats: the slots it keeps, and how many are open at birth.
struct Repeat {
    min: i64,
    max: i64,
    default: i64,
}

impl Repeat {
    fn parse(v: &Value, section: &str) -> Result<Repeat, String> {
        let int = |key: &str| v.get(key).and_then(Value::as_i64).ok_or_else(|| format!("section `{section}` repeats with `{key}` as a whole number"));
        let r = Repeat { min: int("min")?, max: int("max")?, default: int("default")? };
        if r.min < 0 || r.max < 1 || r.max > MAX_SLOTS || r.min > r.max || r.default < r.min || r.default > r.max {
            return Err(format!("section `{section}` repeats {} to {} times with {} at birth; 0 ≤ min ≤ default ≤ max ≤ {MAX_SLOTS}", r.min, r.max, r.default));
        }
        Ok(r)
    }
}
