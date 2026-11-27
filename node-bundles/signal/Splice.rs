//! Splice — two frames of one shape cut together. Each cell of the first two axes takes `a`, `b`
//! or a mix of the two, by a fractal map grown at `scale` cells, or by a wired `mask`.
//!
//! The frames are `[rows, columns, ...]`: the token grids of `DINO` nodes on their way to
//! `DinoDecode`, where a cut at 2 cells gives a collage of small fragments and a cut at 9 two
//! textures meeting along organic seams, or pictures, where it is a pixel patchwork. The map is
//! value noise over three octaves, thresholded at the quantile that gives `a` its `share` of the
//! cells, with a ramp `edge` cells wide across each boundary. `drift` moves the map through a third
//! axis of the noise, and the weights follow it continuously: one unit grows a new map, and at
//! `edge` 1 the largest step of a weight measured 0.056 for 0.001 of drift (24 x 24 cells, `scale`
//! 6), so about 0.05 a second at 30 frames a second makes the pieces glide.

use goofi_core::{Data, SlotType};
use goofi_signal_sdk::{Face, Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};

const G: &str = "splice";
const OCTAVES: i32 = 3;

#[derive(Default)]
struct Splice;

/// A value in 0..1 for each lattice point of the noise, from a 64-bit mix of its coordinates.
fn lattice(x: i64, y: i64, z: i64, seed: u64) -> f32 {
    let mut h = seed
        ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Value noise at a point: the eight lattice values around it, eased so the field has no creases.
fn noise(x: f64, y: f64, z: f64, seed: u64) -> f32 {
    let (x0, y0, z0) = (x.floor(), y.floor(), z.floor());
    let ease = |t: f64| (t * t * (3.0 - 2.0 * t)) as f32;
    let (fx, fy, fz) = (ease(x - x0), ease(y - y0), ease(z - z0));
    let (ix, iy, iz) = (x0 as i64, y0 as i64, z0 as i64);
    let v = |dx: i64, dy: i64, dz: i64| lattice(ix + dx, iy + dy, iz + dz, seed);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let near = lerp(lerp(v(0, 0, 0), v(1, 0, 0), fx), lerp(v(0, 1, 0), v(1, 1, 0), fx), fy);
    let far = lerp(lerp(v(0, 0, 1), v(1, 0, 1), fx), lerp(v(0, 1, 1), v(1, 1, 1), fx), fy);
    lerp(near, far, fz)
}

/// The fractal map over a `rows` x `cols` grid: blobs about `scale` cells across, and two finer
/// octaves at half and a quarter of the weight.
pub fn growth(rows: usize, cols: usize, scale: f64, seed: u64, drift: f64) -> Vec<f32> {
    let mut f = vec![0f32; rows * cols];
    for k in 0..OCTAVES {
        let freq = 2f64.powi(k) / scale.max(0.5);
        let amp = 0.5f32.powi(k);
        let octave_seed = seed.wrapping_add(k as u64 * 0x9E37);
        for r in 0..rows {
            for c in 0..cols {
                let n = noise((c as f64 + 0.5) * freq, (r as f64 + 0.5) * freq, drift, octave_seed);
                f[r * cols + c] += amp * n;
            }
        }
    }
    f
}

/// The weights of `a` from the map: 1 above the quantile that leaves `a` its `share` of the cells,
/// 0 below, and a ramp across the boundary that spans about `edge` cells. The ramp is set in the
/// map's own units from its mean slope, so the weights move continuously as the map does.
pub fn cut(f: &[f32], rows: usize, cols: usize, share: f32, edge: f32) -> Vec<f32> {
    if share <= 0.0 {
        return vec![0.0; f.len()];
    }
    if share >= 1.0 {
        return vec![1.0; f.len()];
    }
    let mut sorted = f.to_vec();
    sorted.sort_by(f32::total_cmp);
    let at = ((1.0 - share) * f.len() as f32).clamp(0.0, (f.len() - 1) as f32);
    let (lo, t) = (at.floor() as usize, at.fract());
    let q = sorted[lo] + (sorted[(lo + 1).min(f.len() - 1)] - sorted[lo]) * t;
    if edge <= 0.0 {
        return f.iter().map(|&v| if v > q { 1.0 } else { 0.0 }).collect();
    }
    let at = |r: usize, c: usize| f[r.min(rows - 1) * cols + c.min(cols - 1)];
    let mut slope = 0f32;
    for r in 0..rows {
        for c in 0..cols {
            let dx = at(r, c + 1) - at(r, c.saturating_sub(1));
            let dy = at(r + 1, c) - at(r.saturating_sub(1), c);
            slope += 0.5 * (dx * dx + dy * dy).sqrt();
        }
    }
    let half = (0.5 * edge * slope / f.len() as f32).max(1e-6);
    f.iter()
        .map(|&v| {
            let t = ((v - q) / half * 0.5 + 0.5).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        })
        .collect()
}

/// A mask `[h, w]` (or `[h, w, ...]`, its first channel) read at the centre of each grid cell,
/// bilinear, clamped to 0..1.
pub fn resample(mask: &[f32], shape: &[usize], rows: usize, cols: usize) -> Result<Vec<f32>, String> {
    if shape.len() < 2 || shape[0] == 0 || shape[1] == 0 {
        return Err(format!("`mask` is a `[height, width]` field; this frame is {shape:?}"));
    }
    let (h, w) = (shape[0], shape[1]);
    let stride = shape[2..].iter().product::<usize>().max(1);
    let px = |y: usize, x: usize| mask[(y * w + x) * stride];
    let mut out = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        let y = ((r as f32 + 0.5) * h as f32 / rows as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, ty) = (y.floor() as usize, y.fract());
        let y1 = (y0 + 1).min(h - 1);
        for c in 0..cols {
            let x = ((c as f32 + 0.5) * w as f32 / cols as f32 - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, tx) = (x.floor() as usize, x.fract());
            let x1 = (x0 + 1).min(w - 1);
            let top = px(y0, x0) + (px(y0, x1) - px(y0, x0)) * tx;
            let bottom = px(y1, x0) + (px(y1, x1) - px(y1, x0)) * tx;
            out.push((top + (bottom - top) * ty).clamp(0.0, 1.0));
        }
    }
    Ok(out)
}

/// `a` where the weight is 1, `b` where it is 0, every value of a cell's trailing axes alike.
pub fn mix(a: &[f32], b: &[f32], weights: &[f32]) -> Vec<f32> {
    let depth = a.len() / weights.len().max(1);
    let mut out = Vec::with_capacity(a.len());
    for (cell, &wa) in weights.iter().enumerate() {
        let span = cell * depth..(cell + 1) * depth;
        out.extend(a[span.clone()].iter().zip(&b[span]).map(|(x, y)| y + wa * (x - y)));
    }
    out
}

impl Node for Splice {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, _c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        // Not required: the node runs on its own clock, and before both frames arrive there is nothing to cut.
        let (Some(da), Some(db)) = (inp.get("a"), inp.get("b")) else {
            return Ok(());
        };
        let (a, b) = (da.as_array()?, db.as_array()?);
        let shape = a.shape().to_vec();
        if b.shape() != shape.as_slice() {
            return Err(format!("`b` is {:?} where `a` is {shape:?}", b.shape()).into());
        }
        if shape.len() < 2 || shape[0] == 0 || shape[1] == 0 {
            return Err(format!("`a` and `b` are `[rows, columns, ...]` frames; these are {shape:?}").into());
        }
        let (rows, cols) = (shape[0], shape[1]);

        let weights = match inp.get("mask") {
            Some(m) => {
                let m = m.as_array()?;
                resample(&m.values().collect::<Vec<_>>(), m.shape(), rows, cols)?
            }
            None => {
                let scale = p.f64(G, "scale").unwrap_or(6.0).clamp(1.0, 64.0);
                let seed = p.i64(G, "seed").unwrap_or(0).max(0) as u64;
                let drift = p.f64(G, "drift").unwrap_or(0.0);
                if !drift.is_finite() {
                    return Err(format!("`drift` is not a number: {drift}").into());
                }
                let share = p.f64(G, "share").unwrap_or(0.5).clamp(0.0, 1.0) as f32;
                let edge = p.f64(G, "edge").unwrap_or(1.0).clamp(0.0, 16.0) as f32;
                cut(&growth(rows, cols, scale, seed, drift), rows, cols, share, edge)
            }
        };
        let a_values: Vec<f32> = a.values().collect();
        let b_values: Vec<f32> = b.values().collect();
        let bytes = mix(&a_values, &b_values, &weights).iter().flat_map(|v| v.to_le_bytes()).collect();
        out.set("out", Data::array_f32(shape, bytes, da.meta().clone()).map_err(|e| e.to_string())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: G,
        name: "scale",
        spec: ParamSpec::Num { default: &[6.0], min: 1.0, max: 64.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("The size of the pieces, in cells of the grid: 2 cuts a collage of fragments, 9 lets each frame keep its forms."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "share",
        spec: ParamSpec::Num { default: &[0.5], min: 0.0, max: 1.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("The part of the cells taken from `a`; the rest come from `b`."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "edge",
        spec: ParamSpec::Num { default: &[1.0], min: 0.0, max: 16.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("How many cells a boundary takes to pass from one frame to the other; 0 cuts hard."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "seed",
        spec: ParamSpec::Num { default: &[0.0], min: 0.0, max: 9999.0, int: true, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Which cut: another seed grows another map."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "drift",
        spec: ParamSpec::Num { default: &[0.0], min: 0.0, max: 16.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Where the map is along its third axis: one unit grows a new map, and about 0.05 a second makes the pieces glide."),
        section: 0,
        show: &[],
        role: None,
    },
    // On by default: the frames are held, and a moving `drift` cuts them again between their updates.
    ParamDecl {
        group: "common",
        name: "autotrigger",
        spec: ParamSpec::Bool { default: true },
        expression: None,
        doc: Some("Run on the node's own clock, so a moving `drift` recuts the held frames."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "common",
        name: "max_frequency",
        spec: ParamSpec::Num { default: &[30.0], min: 0.1, max: 240.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Rate cap: frames a second at most."),
        section: 0,
        show: &[],
        role: None,
    },
];

static INPUTS: &[SlotDecl] = &[
    SlotDecl { name: "a", kind: SlotType::Array, trigger_process: true, multi: false, required: false },
    SlotDecl { name: "b", kind: SlotType::Array, trigger_process: true, multi: false, required: false },
    SlotDecl { name: "mask", kind: SlotType::Array, trigger_process: true, multi: false, required: false },
];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "Two frames of one shape cut together along a fractal map.\n\
          `a` and `b` are `[rows, columns, ...]` frames, such as the token grids of two `DINO` nodes on \
          their way to `DinoDecode`. A wired `mask` (`[height, width]` in 0..1, 1 for `a`) is the cut \
          instead of the map, and `scale`, `share`, `edge`, `seed` and `drift` then have no effect.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Splice, MANIFEST);
