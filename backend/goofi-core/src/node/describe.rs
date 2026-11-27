//! A manifest as plain data: what a probe or a `describe()` symbol answers, leaked back into the
//! `'static` manifest every engine reads — and the rule that names a type after its file.

use std::path::{Path, PathBuf};

use crate::probe;
use crate::SlotType;

use crate::node::{NodeManifest, OutputDecl, ParamDecl, ParamSpec, SlotDecl, Tag};

/// The type a node file names: a `.py` stem CamelCased, an `.rs` stem as written. `None` for a
/// file that is not a node file, one hidden by a `_` prefix, or a stem outside the name rule.
pub fn type_name_of(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    if stem.starts_with('_') {
        return None;
    }
    let name = match path.extension()?.to_str()? {
        "py" => camel(stem),
        "rs" | "wgsl" => stem.to_string(),
        _ => return None,
    };
    crate::variables::is_valid_name(&name).then_some(name)
}

/// The folder under a patch's workspace that holds `engine`'s authored files.
pub fn folder_of(engine: &str) -> String {
    format!("nodes_{engine}")
}

/// The engine a workspace folder is named for, `nodes_<engine>`: where a file that cannot say
/// its engine is filed.
pub fn engine_of_folder(dir: &Path) -> Option<&str> {
    dir.file_name()?.to_str()?.strip_prefix("nodes_").filter(|e| !e.is_empty())
}

/// Every node file in `dir`, sorted: its path, the type it names, and the stamp a rescan diffs.
pub fn node_files(dir: &Path) -> Vec<(PathBuf, String, Option<crate::node::Stamp>)> {
    let mut paths: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(e) => {
            goofi_supervisor::log::record(goofi_supervisor::log::Source::component("library"), goofi_supervisor::log::Level::Error, None, format!("failed to read {}: {e}", dir.display()));
            return Vec::new();
        }
    };
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let name = type_name_of(&p)?;
            let stamp = std::fs::metadata(&p).ok().and_then(|m| Some((m.len(), m.modified().ok()?)));
            Some((p, name, stamp))
        })
        .collect()
}

/// `snake_case` file stem → `CamelCase` palette type name.
fn camel(stem: &str) -> String {
    stem.split('_')
        .filter(|s| !s.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Parse the introspection JSON. A `params` entry may be a SECTION rather than a param: it names
/// its members' group, and with `repeat` it is a list, declared once and stored per slot. A
/// param of kind `color` is a four-dimensional `num` from 0 to 1, white unless it says; one of kind
/// `envelope` is a `num` of four ADSR values, its times up to `max` (10 unless it says).
pub fn parse_introspection(json: &str) -> Result<probe::Introspection, String> {
    let mut value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if let Some(entries) = value.get_mut("params").and_then(|p| p.as_array_mut()) {
        let mut flat = crate::node::sections::expand(std::mem::take(entries))?;
        for p in flat.iter_mut() {
            let (face, max, born) = match p.get("kind").and_then(|k| k.as_str()) {
                Some("color") => ("color", 1.0, [1.0, 1.0, 1.0, 1.0]),
                Some("envelope") => ("envelope", 10.0, crate::ENVELOPE_BORN),
                _ => continue,
            };
            p["kind"] = serde_json::json!("num");
            p["face"] = serde_json::json!(face);
            p["min"] = serde_json::json!(0.0);
            if p.get("max").is_none() || face == "color" {
                p["max"] = serde_json::json!(max);
            }
            if p.get("default").is_none() {
                p["default"] = serde_json::json!(born);
            }
        }
        *entries = flat;
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// A manifest as the probe schema — the one description every out-of-crate node answers, as JSON.
pub fn describe(
    engine: &str,
    tags: &[Tag],
    doc: &str,
    inputs: &[SlotDecl],
    outputs: &[OutputDecl],
    params: &[ParamDecl],
    producer: bool,
) -> String {
    let intro = probe::Introspection {
        engine: engine.to_string(),
        gil_safe: true,
        doc: doc.to_string(),
        tags: tags.iter().map(|t| t.as_str().to_string()).collect(),
        producer,
        feedback: false,
        window: false,
        state: Vec::new(),
        inputs: inputs
            .iter()
            .map(|s| probe::Slot {
                name: s.name.to_string(),
                kind: s.kind.name().to_string(),
                trigger: s.trigger_process,
                multi: s.multi,
                required: s.required,
            })
            .collect(),
        outputs: outputs
            .iter()
            .map(|o| probe::OutSlot { name: o.name.to_string(), kind: o.kind.name().to_string() })
            .collect(),
        params: params
            .iter()
            .map(|p| probe::Param {
                group: p.group.to_string(),
                name: p.name.to_string(),
                doc: p.doc.map(str::to_string),
                expression: p.expression.map(|e| e.source.to_string()),
                section: p.section,
                show: p.show.iter().map(|s| probe::Show {
                    param: s.param.to_string(),
                    any_of: s.any_of.iter().map(|v| v.to_string()).collect(),
                }).collect(),
                role: p.role.map(|r| match r {
                    crate::node::Role::Count { section, widget } => probe::Role::Count { section: section.to_string(), widget },
                    crate::node::Role::Member { section, base, slot } => {
                        probe::Role::Member { section: section.to_string(), base: base.to_string(), slot }
                    }
                    crate::node::Role::Feed { slot, group } => probe::Role::Feed { slot: slot.to_string(), group: group.to_string() },
                    crate::node::Role::Held => probe::Role::Held,
                }),
                spec: match p.spec {
                    ParamSpec::Num { default, min, max, int, options, face } => {
                        probe::ParamSpec::Num { default: default.to_vec(), min, max, int, options: options.to_vec(), face }
                    }
                    ParamSpec::Bool { default } => probe::ParamSpec::Bool { default },
                    ParamSpec::Str { default, options, refresh } => probe::ParamSpec::Str {
                        default: default.to_string(),
                        options: options.iter().map(|s| s.to_string()).collect(),
                        refresh,
                    },
                    ParamSpec::Pulse => probe::ParamSpec::Pulse {},
                },
            })
            .collect(),
    };
    serde_json::to_string(&intro).expect("a manifest serializes")
}

/// The first slot name the name rule refuses, phrased for the palette — a reference spells
/// `node.slot` and an expression reads a slot as an attribute, so a bad name never registers.
fn illegal_slot(intro: &probe::Introspection) -> Option<String> {
    intro
        .inputs
        .iter()
        .map(|s| &s.name)
        .chain(intro.outputs.iter().map(|s| &s.name))
        .find(|n| !crate::variables::is_valid_name(n))
        .map(|bad| format!("slot `{bad}` is not a legal name: {}", crate::variables::NAME_RULE))
}

/// The first OUTPUT whose kind one engine alone makes, when the node declares another engine,
/// phrased for the palette. Such a kind on an INPUT is a crossing, and every engine routes a
/// foreign arrival as a frame off the wire.
fn foreign_output(intro: &probe::Introspection) -> Option<String> {
    intro.outputs.iter().find_map(|s| {
        let owner = crate::SlotType::from_name(&s.kind)?.engine_local().filter(|owner| *owner != intro.engine)?;
        Some(format!("output `{}` is {}, which only a node of the {owner} engine makes: a Rust file against `goofi_sdk::{owner}`", s.name, s.kind))
    })
}

/// [`leak_manifest`] of an introspection whose slot names are legal and whose outputs are its
/// engine's to make.
pub fn manifest_of(type_name: &str, intro: &probe::Introspection) -> Result<&'static NodeManifest, String> {
    if let Some(reason) = illegal_slot(intro).or_else(|| foreign_output(intro)) {
        return Err(reason);
    }
    leak_manifest(type_name.to_string(), intro)
}

/// The first param declaration the inspector cannot draw, phrased for the palette: a name declared
/// twice in a group, or a `show` rule whose controller is missing, has no fixed values, or loops.
pub fn illegal_param(params: &[ParamDecl]) -> Option<String> {
    let find = |group: &str, name: &str| params.iter().find(|d| d.group == group && d.name == name);
    params.iter().enumerate().find_map(|(i, d)| {
        let at = format!("param `{}.{}`", d.group, d.name);
        if params[..i].iter().any(|e| e.group == d.group && e.name == d.name) {
            return Some(format!("{at} is declared twice"));
        }
        for show in d.show {
            let (group, name) = show.controller(d.group);
            let Some(controller) = find(group, name) else {
                return Some(format!("{at} shows by `{}`, which this node does not declare", show.param));
            };
            let Some(choices) = choices(controller.spec) else {
                return Some(format!("{at} shows by `{}`, which has no fixed set of values", show.param));
            };
            if show.any_of.is_empty() {
                return Some(format!("{at} shows for no value of `{}`", show.param));
            }
            if let Some(v) = show.any_of.iter().find(|v| !choices.iter().any(|c| c == *v)) {
                return Some(format!("{at} shows for `{v}`, which `{}` does not offer: {}", show.param, choices.join(", ")));
            }
        }
        // Every chain of controllers out of `d`, walked to its end or back to `d`.
        let mut frontier = vec![(d, 0usize)];
        while let Some((cur, depth)) = frontier.pop() {
            if depth > params.len() {
                break;
            }
            for s in cur.show {
                let (g, n) = s.controller(cur.group);
                if (g, n) == (d.group, d.name) {
                    return Some(format!("{at} shows by a chain of params that leads back to it"));
                }
                if let Some(next) = find(g, n) {
                    frontier.push((next, depth + 1));
                }
            }
        }
        None
    })
}

/// The values a param can hold as `show` compares them, or `None` where the set is not fixed.
fn choices(spec: ParamSpec) -> Option<Vec<String>> {
    match spec {
        ParamSpec::Str { options, refresh: false, .. } if !options.is_empty() => {
            Some(options.iter().map(|o| o.to_string()).collect())
        }
        ParamSpec::Num { int: true, options, .. } if !options.is_empty() => Some(options.iter().map(i64::to_string).collect()),
        ParamSpec::Bool { .. } => Some(vec!["true".into(), "false".into()]),
        _ => None,
    }
}

/// Leak a `'static &str` for the catalog's lifetime.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// The one copy of a manifest per type and declaration: a rescan that reads the same declaration
/// again is handed the copy already leaked, and a changed one gets a new copy beside it.
pub fn interned(type_name: &str, digest: u64, build: impl FnOnce() -> NodeManifest) -> &'static NodeManifest {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static INTERNED: OnceLock<Mutex<HashMap<(String, u64), &'static NodeManifest>>> = OnceLock::new();
    let mut held = INTERNED.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    held.entry((type_name.to_string(), digest)).or_insert_with(|| Box::leak(Box::new(build())))
}

/// A digest of what a manifest is made of, for [`interned`].
pub fn digest_of(what: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    what.hash(&mut hasher);
    hasher.finish()
}

/// The `'static NodeManifest` of an introspection, interned by type and declaration; a tag
/// outside the vocabulary or an [`illegal_param`] refuses it.
pub fn leak_manifest(
    type_name: String,
    intro: &probe::Introspection,
) -> Result<&'static NodeManifest, String> {
    let digest = digest_of(&serde_json::to_string(intro).map_err(|e| e.to_string())?);
    let tags = intro
        .tags
        .iter()
        .map(|t| Tag::parse(t).ok_or_else(|| format!("unknown tag `{t}`; the vocabulary is {}", Tag::vocabulary())))
        .collect::<Result<Vec<Tag>, String>>()?;
    let inputs: Vec<SlotDecl> = intro
        .inputs
        .iter()
        .map(|s| SlotDecl {
            name: leak_str(&s.name),
            kind: SlotType::from_name(&s.kind).unwrap_or(SlotType::Array),
            trigger_process: s.trigger,
            multi: s.multi,
            required: s.required,
        })
        .collect();
    let outputs: Vec<OutputDecl> = intro
        .outputs
        .iter()
        .map(|s| OutputDecl {
            name: leak_str(&s.name),
            kind: SlotType::from_name(&s.kind).unwrap_or(SlotType::Array),
        })
        .collect();
    let params: Vec<ParamDecl> = intro.params.iter().map(param_decl).collect();
    if let Some(reason) = illegal_param(&params) {
        return Err(reason);
    }

    Ok(interned(&type_name, digest, || NodeManifest {
        type_name: leak_str(&type_name),
        tags: Box::leak(tags.into_boxed_slice()),
        doc: leak_str(&intro.doc),
        inputs: Box::leak(inputs.into_boxed_slice()),
        outputs: Box::leak(outputs.into_boxed_slice()),
        params: Box::leak(params.into_boxed_slice()),
        producer: intro.producer,
    }))
}

fn param_decl(p: &probe::Param) -> ParamDecl {
    let spec = match &p.spec {
        probe::ParamSpec::Num { default, min, max, int, options, face } => ParamSpec::Num {
            default: Box::leak(default.clone().into_boxed_slice()),
            min: *min,
            max: *max,
            int: *int,
            options: Box::leak(options.clone().into_boxed_slice()),
            face: *face,
        },
        probe::ParamSpec::Bool { default } => ParamSpec::Bool { default: *default },
        probe::ParamSpec::Str { default, options, refresh } => {
            let opts: Vec<&'static str> = options.iter().map(|s| leak_str(s)).collect();
            ParamSpec::Str {
                default: leak_str(default),
                options: Box::leak(opts.into_boxed_slice()),
                refresh: *refresh,
            }
        }
        probe::ParamSpec::Pulse {} => ParamSpec::Pulse,
    };
    ParamDecl {
        group: leak_str(&p.group),
        name: leak_str(&p.name),
        spec,
        expression: p.expression.as_deref().map(|src| crate::node::ExprDecl {
            source: leak_str(src),
            mode: crate::node::ExprMode::On,
        }),
        doc: p.doc.as_deref().map(leak_str),
        section: p.section,
        show: Box::leak(
            p.show
                .iter()
                .map(|s| crate::node::Show {
                    param: leak_str(&s.param),
                    any_of: Box::leak(s.any_of.iter().map(|v| leak_str(v)).collect::<Vec<_>>().into_boxed_slice()),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        role: p.role.as_ref().map(|r| match r {
            probe::Role::Count { section, widget } => crate::node::Role::Count { section: leak_str(section), widget: *widget },
            probe::Role::Member { section, base, slot } => {
                crate::node::Role::Member { section: leak_str(section), base: leak_str(base), slot: *slot }
            }
            probe::Role::Feed { slot, group } => crate::node::Role::Feed { slot: leak_str(slot), group: leak_str(group) },
            probe::Role::Held => crate::node::Role::Held,
        }),
    }
}
