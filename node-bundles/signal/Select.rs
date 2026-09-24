//! Select — keep part of one axis, named by label or by numpy index.

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

fn by_name(spec: &str, labels: &[String]) -> Vec<usize> {
    let mut picked = Vec::new();
    for pattern in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        for (i, name) in labels.iter().enumerate() {
            if matches(pattern, name) && !picked.contains(&i) {
                picked.push(i);
            }
        }
    }
    picked
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

/// A numpy index on one axis of `len`: positions and slices, comma-separated, in the order given,
/// repeats kept. Empty is the whole axis.
fn index(spec: &str, len: usize) -> Result<Vec<usize>, String> {
    let spec = spec.trim().trim_start_matches('[').trim_end_matches(']');
    if spec.trim().is_empty() {
        return Ok((0..len).collect());
    }
    let mut picked = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match part.contains(':') {
            true => picked.extend(slice(part, len)?),
            false => picked.push(position(part, len)?),
        }
    }
    Ok(picked)
}

/// `kept` with the positions `spec` names deleted, as `np.delete` does.
fn delete(kept: Vec<usize>, spec: &str) -> Result<Vec<usize>, String> {
    if spec.trim().is_empty() {
        return Ok(kept);
    }
    let gone = index(spec, kept.len())?;
    Ok(kept.into_iter().enumerate().filter(|(i, _)| !gone.contains(i)).map(|(_, k)| k).collect())
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
        let mode = p.str("select", "mode").unwrap_or("keep-drop");
        let keep = p.str("select", "keep").unwrap_or("");
        let drop = p.str("select", "drop").unwrap_or("");

        let kept = match mode {
            // Each select reads the frame the one before it left, as two numpy getitems in a row.
            "drop-keep" => {
                let rest = delete((0..len).collect(), drop)?;
                index(keep, rest.len())?.into_iter().map(|i| rest[i]).collect()
            }
            "name" => {
                let labels = names(d, axis, len);
                let dropped = by_name(drop, &labels);
                let kept = if keep.trim().is_empty() { (0..len).collect() } else { by_name(keep, &labels) };
                kept.into_iter().filter(|i| !dropped.contains(i)).collect::<Vec<_>>()
            }
            _ => delete(index(keep, len)?, drop)?,
        };
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
        name: "mode",
        spec: ParamSpec::Str { default: "keep-drop", options: &["keep-drop", "drop-keep", "name"], refresh: false },
        expression: None,
        doc: Some(
            "By numpy index — `keep-drop` takes `keep` and then deletes `drop` from what it took, \
             `drop-keep` deletes first and then takes — or by label.",
        ),
    },
    ParamDecl {
        group: "select",
        name: "keep",
        spec: ParamSpec::Str { default: "", options: &[], refresh: false },
        expression: None,
        doc: Some(
            "What to keep: labels separated by commas with `*` for anything, or a numpy index like \
             `-1`, `::2` or `[3, 0, 1:4]`, in the order given. Empty keeps everything.",
        ),
    },
    ParamDecl {
        group: "select",
        name: "drop",
        spec: ParamSpec::Str { default: "", options: &[], refresh: false },
        expression: None,
        doc: Some("What to drop, written the same way; an index is deleted as `np.delete` does. Empty drops nothing."),
    },
    ParamDecl {
        group: "select",
        name: "axis",
        spec: ParamSpec::Int { default: 0, min: -8, max: 7, options: &[-2, -1, 0, 1, 2] },
        expression: None,
        doc: Some("Which axis to cut, negative from the end. 0 is channels on a `[channels, time]` frame."),
    },
    ParamDecl {
        group: "select",
        name: "squeeze",
        spec: ParamSpec::Bool { default: false },
        expression: None,
        doc: Some("When one entry is left, remove the axis instead of leaving it one long."),
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
          Named by label, or by numpy index with keep and drop applied in either order.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Select, MANIFEST);
