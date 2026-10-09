//! Select — keep part of one axis: `keep` names what stays, then `drop` names what goes, each a
//! list of positions, numpy slices, labels and EEG groups.

use goofi_core::{resolve_axis, Coord, Data, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};

#[derive(Default)]
struct Select;

/// A glob with `*` standing for any run of characters.
fn matches(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let Some(first) = parts.next() else { return false };
    if !name.starts_with(first) {
        return false;
    }
    let mut at = first.len();
    let mut last = None;
    for part in parts {
        last = Some(part);
        if part.is_empty() {
            continue;
        }
        match name[at..].find(part) {
            Some(found) => at += found + part.len(),
            None => return false,
        }
    }
    match last {
        // No `*` at all: the whole name had to be the pattern.
        None => name.len() == at,
        Some(tail) => name.ends_with(tail) && name.len() >= at,
    }
}

/// The labels of `axis`, as the strings a pattern is matched against.
fn names(d: &Data, axis: usize, len: usize) -> Vec<String> {
    match d.meta().channels().get(axis).and_then(|a| a.coords.clone()) {
        Some(coords) => coords
            .iter()
            .map(|c| match c {
                Coord::Str(s) => s.to_string(),
                Coord::Num(n) => n.to_string(),
            })
            .collect(),
        None => (0..len).map(|i| i.to_string()).collect(),
    }
}

/// An EEG label as the 10-20 system writes it: `EEG Fp1-REF` and `fp1` are both `FP1`.
fn electrode(label: &str) -> String {
    let label = label.trim();
    let label = label.strip_prefix("EEG").or_else(|| label.strip_prefix("eeg")).unwrap_or(label).trim();
    label.split(['-', ' ', ':']).next().unwrap_or("").to_ascii_uppercase()
}

/// Whether an electrode belongs to the EEG group `<eeg-…>` names: a lobe by the letters before
/// its number, a hemisphere by the number's parity, the midline (`z`) in neither hemisphere.
fn in_group(group: &str, electrode: &str) -> Option<bool> {
    let letters: String = electrode.chars().take_while(|c| c.is_ascii_alphabetic() && *c != 'Z').collect();
    let number: Option<u32> = electrode[letters.len()..].parse().ok();
    let lobe = |prefixes: &[&str]| prefixes.contains(&letters.as_str());
    Some(match group {
        "frontal" => lobe(&["FP", "AF", "F"]),
        "central" => lobe(&["FC", "C", "CP"]),
        "parietal" => lobe(&["P", "PO", "TP"]),
        "occipital" => lobe(&["O", "I"]),
        "left" => number.is_some_and(|n| n % 2 == 1),
        "right" => number.is_some_and(|n| n > 0 && n % 2 == 0),
        _ => return None,
    })
}

/// A position on an axis of `len`, negative from the end, as numpy reads one.
fn position(part: &str, len: usize) -> Result<usize, String> {
    let i: i64 = part.parse().map_err(|_| format!("`{part}` is not a position"))?;
    let at = if i < 0 { i + len as i64 } else { i };
    (0..len as i64).contains(&at).then_some(at as usize).ok_or_else(|| format!("{i} is out of range for {len}"))
}

/// `start:stop:step` as a numpy slice reads it: ends left out, negatives from the end, any step.
fn slice(part: &str, len: usize) -> Result<Vec<usize>, String> {
    let fields: Vec<&str> = part.split(':').map(str::trim).collect();
    let num = |s: &str| -> Result<Option<i64>, String> {
        if s.is_empty() { Ok(None) } else { s.parse().map(Some).map_err(|_| format!("`{part}` is not a slice")) }
    };
    let step = num(fields.get(2).copied().unwrap_or(""))?.unwrap_or(1);
    if step == 0 || fields.len() > 3 {
        return Err(format!("`{part}` is not a slice"));
    }
    let n = len as i64;
    let bound = |v: Option<i64>, forward: i64, backward: i64| match v {
        None => if step > 0 { forward } else { backward },
        Some(v) if v < 0 => (v + n).max(if step > 0 { 0 } else { -1 }),
        Some(v) => v.min(if step > 0 { n } else { n - 1 }),
    };
    let (mut i, stop) = (bound(num(fields[0])?, 0, n - 1), bound(num(fields[1])?, n, -1));
    let mut out = Vec::new();
    while (step > 0 && i < stop) || (step < 0 && i > stop) {
        out.push(i as usize);
        i += step;
    }
    Ok(out)
}

/// Whether an element reads as a numpy index: digits, a sign, and colons, nothing else.
fn is_index(part: &str) -> bool {
    !part.is_empty() && part.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | ':' | ' '))
}

/// The positions `spec` names on an axis of `labels`, in the order given, each once. An element
/// is a position or a slice when it reads as one, an EEG group as `<eeg-left>`, a quoted `"3"`
/// the label itself, and anything else a label with `*` for any run of characters.
fn select(spec: &str, labels: &[String]) -> Result<Vec<usize>, String> {
    let len = labels.len();
    let mut picked: Vec<usize> = Vec::new();
    let mut take = |i: usize| {
        if !picked.contains(&i) {
            picked.push(i);
        }
    };
    let spec = spec.trim().trim_start_matches('[').trim_end_matches(']');
    for part in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(group) = part.strip_prefix("<eeg-").and_then(|g| g.strip_suffix('>')) {
            for (i, label) in labels.iter().enumerate() {
                match in_group(group, &electrode(label)) {
                    Some(true) => take(i),
                    Some(false) => {}
                    None => return Err(format!("`{part}` is no EEG group: frontal, central, parietal, occipital, left or right")),
                }
            }
        } else if let Some(text) = part.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
            labels.iter().enumerate().filter(|(_, l)| matches(text, l)).for_each(|(i, _)| take(i));
        } else if part.contains(':') && is_index(part) {
            slice(part, len)?.into_iter().for_each(&mut take);
        } else if is_index(part) {
            take(position(part, len)?);
        } else {
            labels.iter().enumerate().filter(|(_, l)| matches(part, l)).for_each(|(i, _)| take(i));
        }
    }
    Ok(picked)
}

impl Node for Select {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let a = d.assert_ndims().at_least(1)?;
        let shape = a.shape();
        let axis = resolve_axis(p.i64("select", "axis").unwrap_or(0), shape.len())?;
        let len = shape[axis];
        let keep = p.str("select", "keep").unwrap_or("");
        let drop = p.str("select", "drop").unwrap_or("");

        let labels = names(d, axis, len);
        let kept = if keep.trim().is_empty() { (0..len).collect() } else { select(keep, &labels)? };
        let gone = select(drop, &labels)?;
        let kept: Vec<usize> = kept.into_iter().filter(|i| !gone.contains(i)).collect();
        if kept.is_empty() {
            return Err("nothing is left after the selection".to_string().into());
        }

        let outer: usize = shape[..axis].iter().product();
        let inner: usize = shape[axis + 1..].iter().product();
        let src = a.as_bytes();
        let block = inner * 4;
        let mut buf = Vec::with_capacity(outer * kept.len() * block);
        for o in 0..outer {
            for &k in &kept {
                let from = (o * len + k) * block;
                buf.extend_from_slice(&src[from..from + block]);
            }
        }

        let mut shape_out = shape.to_vec();
        shape_out[axis] = kept.len();
        let mut meta = d.meta().keep(axis, &kept);
        if kept.len() == 1 && p.bool("select", "squeeze").unwrap_or(false) {
            shape_out.remove(axis);
            meta = d.meta().keep(axis, &kept).drop_axis(axis, shape.len());
        }
        out.set("out", Data::array_f32(shape_out, buf, meta).map_err(|e| e.to_string())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "select",
        name: "keep",
        spec: ParamSpec::Str { default: "", options: &[], refresh: false },
        expression: None,
        doc: Some(
            "What to keep, separated by commas, in the order given: positions and numpy slices \
             like `-1` or `5:7`, labels with `*` for anything, `\"3\"` for the label rather than \
             the position, and `<eeg-frontal>`, `<eeg-central>`, `<eeg-parietal>`, \
             `<eeg-occipital>`, `<eeg-left>` or `<eeg-right>` for the channels of an EEG group. \
             Empty keeps everything.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "select",
        name: "drop",
        spec: ParamSpec::Str { default: "", options: &[], refresh: false },
        expression: None,
        doc: Some("What to drop from what was kept, written the same way. Empty drops nothing."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "select",
        name: "axis",
        spec: ParamSpec::Num { default: &[0.0], min: -8.0, max: 7.0, int: true, options: &[-2, -1, 0, 1, 2], color: false },
        expression: None,
        doc: Some("Which axis to cut, negative from the end. 0 is channels on a `[channels, time]` frame."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "select",
        name: "squeeze",
        spec: ParamSpec::Bool { default: true },
        expression: None,
        doc: Some("When one entry is left, remove the axis instead of leaving it one long."),
        section: 0,
        show: &[],
        role: None,
    },
];
static INPUTS: &[SlotDecl] = &[SlotDecl {
    name: "input",
    kind: SlotType::Array,
    trigger_process: true,
    multi: false,
    required: true,
}];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "Keep part of one axis.\n\
          By position, numpy slice, label or EEG group: `keep` takes, then `drop` deletes.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Select, MANIFEST);
