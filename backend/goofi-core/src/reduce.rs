//! Axis subsampling over a frame's f32 LE bytes: a strided copy of the entries a reader asked for,
//! nothing computed. Returns `None` when it would not shrink the axis.

use crate::{Coord, Data, Meta, MetaValue, Value};
use goofi_view::{MergedViewSpec, ViewSpec};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Evaluate a merged view plan against a frame: reduce each planned axis, co-reduce the coords,
/// and record `meta.reduced`. Fail-open — an unreconstructible result returns the source frame.
pub fn reduce_for_view(frame: &Data, plan: &MergedViewSpec) -> Data {
    let Value::Array(store) = frame.value() else {
        return frame.clone();
    };
    if plan.axes.is_empty() {
        return frame.clone();
    }
    // Borrowed until an axis actually shrinks: a plan whose axes all fit is the common path.
    let mut bytes = Cow::Borrowed(store.as_bytes());
    let mut shape = store.shape().to_vec();
    let mut axes = frame.meta().channels().clone();
    let mut reduced: BTreeMap<String, MetaValue> = BTreeMap::new();

    // Descending dim so a reduction never invalidates a not-yet-processed lower dim.
    let mut planned = plan.axes.clone();
    planned.sort_by_key(|ax| std::cmp::Reverse(ax.dim));
    for ax in &planned {
        let Some(r) = reduce_axis(&bytes, &shape, ax.dim, ax.max) else {
            continue;
        };
        let orig_len = shape[ax.dim];
        // Capture the ORIGINAL coords before slicing, so a small subsampled axis keeps exact labels.
        let verbatim = (orig_len <= 4096).then(|| axes.get(ax.dim).and_then(|a| a.coords.clone())).flatten();
        bytes = Cow::Owned(r.bytes);
        shape[ax.dim] = r.new_len;
        axes = axes.sliced(ax.dim, &r.centers);
        let mut entry = axis_record(orig_len);
        if let Some(coords) = verbatim {
            let list = coords
                .iter()
                .map(|c| match c {
                    Coord::Num(n) => MetaValue::Float(*n),
                    Coord::Str(s) => MetaValue::Str(s.to_string()),
                })
                .collect();
            entry.insert("orig_coord".to_string(), MetaValue::List(list));
        }
        reduced.insert(ax.dim.to_string(), MetaValue::Map(entry));
    }
    if reduced.is_empty() {
        return frame.clone();
    }
    let mut meta = frame.meta().clone();
    meta.set_channels(axes);
    meta.set_reduced(Some(MetaValue::Map(reduced)));
    Data::array_f32(shape, bytes.into_owned(), meta).unwrap_or_else(|_| frame.clone())
}

/// A table's arrays reduced entry by entry, to any depth, each planned against `specs` as a frame
/// of its own — a table has no axes to plan. Fail-open like [`reduce_for_view`].
pub fn reduce_table(frame: &Data, specs: &[ViewSpec]) -> Data {
    let Value::Table(map) = frame.value() else {
        return frame.clone();
    };
    let mut shrunk = false;
    let mut out = indexmap::IndexMap::with_capacity(map.len());
    for (key, entry) in map.iter() {
        let reduced = match entry.value() {
            Value::Table(_) => reduce_table(entry, specs),
            Value::Array(_) => reduce_for_view(entry, &goofi_view::plan(specs, entry)),
            _ => entry.clone(),
        };
        shrunk |= !std::sync::Arc::ptr_eq(&reduced.0, &entry.0);
        out.insert(key.clone(), reduced);
    }
    if shrunk { Data::table(out, frame.meta().clone()) } else { frame.clone() }
}

/// One axis's reduction, as `meta.reduced` records it. The ONE writer of that shape, so a
/// producer that reduced a frame itself says it the same way this module does.
fn axis_record(orig_len: usize) -> BTreeMap<String, MetaValue> {
    BTreeMap::from([("orig_len".to_string(), MetaValue::Uint(orig_len as u64))])
}

/// What `dim` measured before every reduction this meta already records, and `now` where it
/// records none — so a second reduction of one frame states the first one's origin and not its
/// own input, which is the length a reader would map a texel back through.
pub fn origin_of(meta: &Meta, dim: usize, now: usize) -> usize {
    let Some(MetaValue::Map(dims)) = meta.reduced() else { return now };
    match dims.get(&dim.to_string()) {
        Some(MetaValue::Map(entry)) => match entry.get("orig_len") {
            Some(MetaValue::Uint(n)) => *n as usize,
            _ => now,
        },
        _ => now,
    }
}

/// Say that `dims` were already reduced — for a producer that rendered the reduced size rather
/// than making a frame only to shrink it. Without this the frame would understate its own origin.
pub fn note_reduced(meta: &mut Meta, dims: &[(usize, usize)]) {
    let mut reduced: BTreeMap<String, MetaValue> = BTreeMap::new();
    for &(dim, orig_len) in dims {
        reduced.insert(dim.to_string(), MetaValue::Map(axis_record(orig_len)));
    }
    if !reduced.is_empty() {
        meta.set_reduced(Some(MetaValue::Map(reduced)));
    }
}

/// A frame as 8-bit texels for the browser hop: a quarter of the bytes. Three or four channels
/// quantize over `[0, 1]`, the colour convention; fewer over the frame's own finite range,
/// recorded as `reduced.depth = {lo, hi}` so a reader maps a texel back. `None` for anything but
/// a 2-D or 3-D array.
pub fn quantize_u8(frame: &Data) -> Option<(Vec<usize>, Vec<u8>, crate::Meta)> {
    let Value::Array(store) = frame.value() else {
        return None;
    };
    let shape = store.shape();
    if shape.len() != 2 && shape.len() != 3 {
        return None;
    }
    let values = || store.as_bytes().chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four bytes")));
    let colour = shape.len() == 3 && shape[2] >= 3;
    let (lo, hi) = if colour {
        (0.0, 1.0)
    } else {
        let (lo, hi) = values()
            .filter(|v| v.is_finite())
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
        // An all-NaN or empty frame leaves the seeds; a flat one spans a unit either way.
        if lo < hi {
            (lo, hi)
        } else {
            let lo = if lo.is_finite() { lo } else { 0.0 };
            (lo, lo + 1.0)
        }
    };
    let span = hi as f64 - lo as f64;
    let texels: Vec<u8> = values().map(|v| ((((v as f64) - lo as f64) / span).clamp(0.0, 1.0) * 255.0).round() as u8).collect();
    let mut meta = frame.meta().clone();
    note_depth(&mut meta, lo, hi);
    Some((shape.to_vec(), texels, meta))
}

/// Say what window a frame's texels span, so a reader maps one back to a value. ONE spelling,
/// whether the quantization happened here or on a GPU that wrote the texels directly.
pub fn note_depth(meta: &mut crate::Meta, lo: f32, hi: f32) {
    let mut reduced = match meta.reduced() {
        Some(MetaValue::Map(m)) => m.clone(),
        _ => BTreeMap::new(),
    };
    reduced.insert(
        "depth".to_string(),
        MetaValue::Map(BTreeMap::from([
            ("lo".to_string(), MetaValue::Float(lo as f64)),
            ("hi".to_string(), MetaValue::Float(hi as f64)),
        ])),
    );
    meta.set_reduced(Some(MetaValue::Map(reduced)));
}

/// `m` evenly-spaced indices into `0..n` (inclusive endpoints, like `np.linspace(0,n-1,m)`).
pub fn subsample_idx(n: usize, m: usize) -> Vec<usize> {
    if n == 0 || m == 0 {
        return Vec::new();
    }
    if m >= n {
        return (0..n).collect();
    }
    if m == 1 {
        return vec![0];
    }
    (0..m)
        .map(|i| ((i as f64) * (n - 1) as f64 / (m - 1) as f64).round() as usize)
        .collect()
}

/// One axis's reduction: new bytes, the new axis length, and the original index each entry maps to.
pub struct AxisReduction {
    pub bytes: Vec<u8>,
    pub new_len: usize,
    pub centers: Vec<usize>,
}

/// Row-major strides for dimension `dim`: (outer count, axis length, inner element count), so
/// element `(o, a, i)` sits at flat index `(o*axis + a)*inner + i`.
fn strides(shape: &[usize], dim: usize) -> (usize, usize, usize) {
    let outer: usize = shape[..dim].iter().product();
    let axis = shape[dim];
    let inner: usize = shape[dim + 1..].iter().product();
    (outer, axis, inner)
}

/// Keep at most `max` evenly spaced entries of one axis, each copied whole with everything inside
/// it; `None` when that would not shrink the axis.
pub fn reduce_axis(bytes: &[u8], shape: &[usize], dim: usize, max: usize) -> Option<AxisReduction> {
    let elements = shape.iter().try_fold(1usize, |n, &d| n.checked_mul(d))?;
    if elements == 0 || elements.checked_mul(4)? != bytes.len() || dim >= shape.len() || max == 0 {
        return None;
    }
    let (outer, axis, inner) = strides(shape, dim);
    let idx = subsample_idx(axis, max);
    if idx.len() >= axis {
        return None;
    }
    let block = inner * 4;
    let mut out = Vec::with_capacity(outer * idx.len() * block);
    for o in 0..outer {
        for &a in &idx {
            let start = (o * axis + a) * block;
            out.extend_from_slice(&bytes[start..start + block]);
        }
    }
    Some(AxisReduction { bytes: out, new_len: idx.len(), centers: idx })
}
