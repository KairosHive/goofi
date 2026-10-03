//! A SECTION in a header's `params`: a run of params drawn apart from the rest of their group and
//! stored as `section_name`, or with `repeat` a LIST of them, repeated by a count param and
//! stored as one param per slot, `name_<slot>`.

use std::collections::HashMap;

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
        let members = match entry.get_mut("params").and_then(Value::as_array_mut) {
            Some(m) => std::mem::take(m),
            None => return Err(format!("section `{name}` lists its params")),
        };
        let repeat = entry.get("repeat").map(|r| Repeat::parse(r, &name)).transpose()?;
        let Some(repeat) = repeat else {
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
            "role": { "as": "count", "section": name },
        });
        if let Some(doc) = entry.get("doc") {
            count["doc"] = doc.clone();
        }
        out.push(count);
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
                if let Some(controller) = m.get("show").and_then(|s| text(s, "param")).map(str::to_string) {
                    if bases.contains(&controller) {
                        m["show"]["param"] = json!(format!("{controller}_{slot}"));
                    }
                }
                out.push(m);
            }
        }
    }
    Ok(out)
}

fn is_section(entry: &Value) -> bool {
    entry.get("params").is_some()
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
