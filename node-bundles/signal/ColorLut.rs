//! ColorLut — a colour transfer as a 3D lookup table. The table moves the colours of `source`
//! toward those of `reference`, and Lut3D applies it to every texel on the GPU: the transfer is
//! fitted on a small sample, and a frame of any size costs eight texel reads per pixel.
//!
//! A global transfer is a function from one colour to another, the same at every pixel, so a
//! table holds it exactly up to its resolution. That includes ModFlows, whose flow runs in RGB on
//! a pixel's own colour: Arcana bakes it offline to a `.cube` file, which `file` reads.

use goofi_core::{Data, Meta, SlotType};
use goofi_signal_sdk::{Face, Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, Show,
    SlotDecl, Tag,
};
use std::time::SystemTime;

const G: &str = "lut";
const FROM_IMAGES: Show = Show { param: "method", any_of: &["reinhard", "mkl", "idt", "histogram"] };
const IDT: Show = Show { param: "method", any_of: &["idt"] };
const FILE: Show = Show { param: "method", any_of: &["file"] };

/// The most pixels an input is read at. A sample this size fixes means and covariances to well
/// under a level of 8-bit colour; GraphicsIn's `size` makes the frames smaller still.
const SAMPLE: usize = 16384;

type Rgb = [f32; 3];
type M3 = [[f32; 3]; 3];

#[derive(Default)]
struct ColorLut {
    /// The last table written and its side, which `smooth` eases from.
    last: Vec<f32>,
    last_side: usize,
    /// A `.cube` file as last read: its path, when it was modified, its side and its entries.
    cube: Option<(String, SystemTime, usize, Vec<Rgb>)>,
}

impl Node for ColorLut {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let method = p.str(G, "method").unwrap_or("mkl");
        let (n, entries) = if method == "file" {
            self.read_cube(p.str(G, "path").unwrap_or(""))?
        } else {
            let n = p.i64(G, "size").unwrap_or(33).clamp(2, 65) as usize;
            let lab = p.str(G, "space").unwrap_or("lab") == "lab";
            let into = |c: Rgb| if lab { rgb_to_lab(c) } else { c };
            let source: Vec<Rgb> = colours(inp.get("source").ok_or("`source` is required: the frame whose colours move")?)?
                .into_iter()
                .map(into)
                .collect();
            let reference: Vec<Rgb> = colours(inp.get("reference").ok_or("`reference` is required: the colours they move toward")?)?
                .into_iter()
                .map(into)
                .collect();
            let mut lattice: Vec<Rgb> = lattice(n).into_iter().map(into).collect();
            match method {
                "reinhard" => reinhard(&source, &reference, &mut lattice),
                "mkl" => mkl(&source, &reference, &mut lattice),
                "histogram" => histogram(&source, &reference, &mut lattice),
                "idt" => idt(source, &reference, &mut lattice, p.i64(G, "iterations").unwrap_or(12).clamp(1, 50) as usize),
                other => return Err(format!("unknown method `{other}`").into()),
            }
            let back = |c: Rgb| if lab { lab_to_rgb(c) } else { c };
            (n, lattice.into_iter().map(|c| back(c).map(|v| v.clamp(0.0, 1.0))).collect())
        };

        let mut table: Vec<f32> = entries.iter().flat_map(|c| [c[0], c[1], c[2], 1.0]).collect();
        let smooth = p.f64("follow", "smooth").unwrap_or(0.0).clamp(0.0, 0.99) as f32;
        if smooth > 0.0 && self.last_side == n && self.last.len() == table.len() {
            for (t, l) in table.iter_mut().zip(&self.last) {
                *t = smooth * l + (1.0 - smooth) * *t;
            }
        }
        self.last.clone_from(&table);
        self.last_side = n;
        let bytes: Vec<u8> = table.iter().flat_map(|v| v.to_le_bytes()).collect();
        out.set("lut", Data::array_f32(vec![n, n * n, 4], bytes, Meta::new()).map_err(|e| e.to_string())?);
        Ok(())
    }
}

impl ColorLut {
    /// A `.cube` file's table, in Lut3D's order. Read again only when the file changes.
    fn read_cube(&mut self, path: &str) -> Result<(usize, Vec<Rgb>), String> {
        if path.is_empty() {
            return Err("`path` names no file: give a `.cube` table".into());
        }
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).map_err(|e| format!("`{path}`: {e}"))?;
        if let Some((p, m, n, e)) = &self.cube {
            if p == path && *m == modified {
                return Ok((*n, e.clone()));
            }
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("`{path}`: {e}"))?;
        let (n, cube_order) = parse_cube(&text)?;
        // A `.cube` lists red fastest, then green, then blue; Lut3D reads green by row, then blue.
        let mut entries = Vec::with_capacity(n * n * n);
        for g in 0..n {
            for b in 0..n {
                for r in 0..n {
                    entries.push(cube_order[r + n * g + n * n * b]);
                }
            }
        }
        self.cube = Some((path.to_string(), modified, n, entries.clone()));
        Ok((n, entries))
    }
}

/// The entries of a `.cube` file, red fastest. Only the 0..1 domain is read, the one Lut3D samples.
fn parse_cube(text: &str) -> Result<(usize, Vec<Rgb>), String> {
    let mut n = 0usize;
    let mut entries = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("LUT_3D_SIZE") => {
                n = words.next().and_then(|w| w.parse().ok()).ok_or("`LUT_3D_SIZE` takes a whole number")?;
            }
            Some("DOMAIN_MIN") | Some("DOMAIN_MAX") => {
                let want = if line.starts_with("DOMAIN_MIN") { 0.0 } else { 1.0 };
                if words.any(|w| w.parse::<f32>().map_or(true, |v| (v - want).abs() > 1e-6)) {
                    return Err("the table's domain is not 0..1, the only one Lut3D samples".into());
                }
            }
            Some("LUT_1D_SIZE") => return Err("a 1D table; Lut3D takes a 3D one".into()),
            Some(first) if first.parse::<f32>().is_ok() => {
                let v: Vec<f32> = line.split_whitespace().filter_map(|w| w.parse().ok()).collect();
                if v.len() != 3 {
                    return Err(format!("a table row holds three numbers: `{line}`"));
                }
                entries.push([v[0], v[1], v[2]]);
            }
            _ => {} // TITLE and other keywords
        }
    }
    if !(2..=129).contains(&n) {
        return Err("no `LUT_3D_SIZE` between 2 and 129".into());
    }
    if entries.len() != n * n * n {
        return Err(format!("`LUT_3D_SIZE {n}` wants {} rows, the file has {}", n * n * n, entries.len()));
    }
    Ok((n, entries))
}

/// An `[H, W, C]` frame's colours, C of 3 or more, as at most SAMPLE pixels in 0..1. A frame of
/// 0..255 values, as an 8-bit image file arrives, is brought to 0..1.
fn colours(d: &Data) -> Result<Vec<Rgb>, String> {
    let a = d.assert_ndims().at_least(3)?;
    let shape = a.shape();
    let c = shape[shape.len() - 1];
    if c < 3 {
        return Err(format!("a frame of colours has 3 or 4 channels last, not {shape:?}"));
    }
    let values: Vec<f32> = a.values().collect();
    let pixels = values.len() / c;
    if pixels == 0 {
        return Err("the frame is empty".into());
    }
    let step = pixels.div_ceil(SAMPLE);
    let high = values.iter().fold(0.0f32, |m, v| m.max(*v));
    let unit = if high > 1.5 { 1.0 / 255.0 } else { 1.0 };
    Ok((0..pixels).step_by(step).map(|i| [values[i * c] * unit, values[i * c + 1] * unit, values[i * c + 2] * unit]).collect())
}

/// The table's input colours, in Lut3D's order: green by row, then blue slices, red across each.
fn lattice(n: usize) -> Vec<Rgb> {
    let at = |i: usize| i as f32 / (n - 1) as f32;
    let mut out = Vec::with_capacity(n * n * n);
    for g in 0..n {
        for b in 0..n {
            for r in 0..n {
                out.push([at(r), at(g), at(b)]);
            }
        }
    }
    out
}

// ─────────────────────────── the transfers ───────────────────────────

fn mean(xs: &[Rgb]) -> Rgb {
    let mut m = [0.0f64; 3];
    for x in xs {
        for k in 0..3 {
            m[k] += x[k] as f64;
        }
    }
    m.map(|v| (v / xs.len() as f64) as f32)
}

fn covariance(xs: &[Rgb], m: Rgb) -> M3 {
    let mut c = [[0.0f64; 3]; 3];
    for x in xs {
        for i in 0..3 {
            for j in 0..3 {
                c[i][j] += ((x[i] - m[i]) * (x[j] - m[j])) as f64;
            }
        }
    }
    c.map(|row| row.map(|v| (v / xs.len() as f64) as f32))
}

/// Reinhard et al. 2001: each channel's mean and spread matched on its own.
fn reinhard(source: &[Rgb], reference: &[Rgb], lattice: &mut [Rgb]) {
    let (ms, mr) = (mean(source), mean(reference));
    let (cs, cr) = (covariance(source, ms), covariance(reference, mr));
    let gain: Rgb = std::array::from_fn(|k| (cr[k][k].max(0.0).sqrt()) / cs[k][k].max(1e-12).sqrt());
    for x in lattice.iter_mut() {
        *x = std::array::from_fn(|k| (x[k] - ms[k]) * gain[k] + mr[k]);
    }
}

/// Pitié and Kokaram 2007, the linear Monge–Kantorovitch map: the one linear map carrying the
/// source's covariance onto the reference's while moving colours as little as possible.
fn mkl(source: &[Rgb], reference: &[Rgb], lattice: &mut [Rgb]) {
    let (ms, mr) = (mean(source), mean(reference));
    let (cs, cr) = (covariance(source, ms), covariance(reference, mr));
    let (s_half, s_inv_half) = (sym_power(cs, 0.5), sym_power(cs, -0.5));
    let middle = sym_power(mul(mul(s_half, cr), s_half), 0.5);
    let t = mul(mul(s_inv_half, middle), s_inv_half);
    for x in lattice.iter_mut() {
        let d = [x[0] - ms[0], x[1] - ms[1], x[2] - ms[2]];
        *x = std::array::from_fn(|i| t[i][0] * d[0] + t[i][1] * d[1] + t[i][2] * d[2] + mr[i]);
    }
}

/// Each channel's distribution matched on its own, quantile for quantile.
fn histogram(source: &[Rgb], reference: &[Rgb], lattice: &mut [Rgb]) {
    for k in 0..3 {
        let s = sorted(source.iter().map(|x| x[k]));
        let r = sorted(reference.iter().map(|x| x[k]));
        for x in lattice.iter_mut() {
            x[k] = matched(x[k], &s, &r);
        }
    }
}

/// Pitié, Kokaram and Dahyot 2005: the whole three-dimensional distribution, by matching it along
/// one rotated set of axes after another. The rotations come from a fixed sequence, so a source
/// that holds still gives a table that holds still.
fn idt(mut source: Vec<Rgb>, reference: &[Rgb], lattice: &mut [Rgb], iterations: usize) {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..iterations {
        let r = rotation(&mut seed);
        for axis in r {
            let along = |x: &Rgb| axis[0] * x[0] + axis[1] * x[1] + axis[2] * x[2];
            let s = sorted(source.iter().map(along));
            let t = sorted(reference.iter().map(along));
            let shift = |x: &mut Rgb| {
                let v = along(x);
                let d = matched(v, &s, &t) - v;
                for k in 0..3 {
                    x[k] += d * axis[k];
                }
            };
            source.iter_mut().for_each(shift);
            lattice.iter_mut().for_each(shift);
        }
    }
}

fn sorted(it: impl Iterator<Item = f32>) -> Vec<f32> {
    let mut v: Vec<f32> = it.filter(|x| x.is_finite()).collect();
    v.sort_by(f32::total_cmp);
    v
}

/// Where `v` falls in the sorted source, read off the sorted reference at the same quantile.
/// Positions between samples are interpolated, so a table built from it has no steps, and a
/// source matched to itself comes back unchanged.
fn matched(v: f32, s: &[f32], r: &[f32]) -> f32 {
    if s.len() < 2 || r.is_empty() {
        return v;
    }
    let below = s.partition_point(|x| *x < v);
    let upto = s.partition_point(|x| *x <= v);
    // v's position along the source, in samples: the middle of its run when many pixels share it.
    let at = if below < upto {
        (below + upto - 1) as f32 * 0.5
    } else if below == 0 {
        0.0
    } else if below == s.len() {
        (s.len() - 1) as f32
    } else {
        let (a, b) = (s[below - 1], s[below]);
        (below - 1) as f32 + (v - a) / (b - a)
    };
    let at = at / (s.len() - 1) as f32 * (r.len() - 1) as f32;
    let i = (at.floor() as usize).min(r.len() - 1);
    let j = (i + 1).min(r.len() - 1);
    r[i] + (r[j] - r[i]) * (at - i as f32)
}

/// A rotation drawn from `seed`: three random directions, made orthonormal.
fn rotation(seed: &mut u64) -> M3 {
    let mut next = || {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        (*seed >> 11) as f32 / (1u64 << 53) as f32 * 2.0 - 1.0
    };
    let mut r: M3 = std::array::from_fn(|_| std::array::from_fn(|_| next()));
    for i in 0..3 {
        for j in 0..i {
            let (d, rj) = (dot(r[i], r[j]), r[j]);
            for (x, y) in r[i].iter_mut().zip(rj) {
                *x -= d * y;
            }
        }
        let len = dot(r[i], r[i]).sqrt().max(1e-6);
        r[i] = r[i].map(|v| v / len);
    }
    r
}

fn dot(a: Rgb, b: Rgb) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn mul(a: M3, b: M3) -> M3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

/// A symmetric matrix raised to `power` through its eigenvalues, each floored so a flat channel
/// does not divide by zero.
fn sym_power(m: M3, power: f32) -> M3 {
    let (values, vectors) = jacobi(m);
    let d: Rgb = values.map(|v| v.max(1e-8).powf(power));
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| vectors[i][k] * d[k] * vectors[j][k]).sum()))
}

/// Eigenvalues and eigenvectors (as columns) of a symmetric 3×3 matrix, by cyclic Jacobi rotations.
fn jacobi(mut a: M3) -> (Rgb, M3) {
    let mut v: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off < 1e-12 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-15 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            // A ← Jᵀ A J, columns then rows, and V ← V J.
            let turn = |m: &mut M3| {
                for row in m.iter_mut() {
                    let (xp, xq) = (row[p], row[q]);
                    row[p] = c * xp - s * xq;
                    row[q] = s * xp + c * xq;
                }
            };
            turn(&mut a);
            let (rp, rq) = (a[p], a[q]);
            a[p] = std::array::from_fn(|k| c * rp[k] - s * rq[k]);
            a[q] = std::array::from_fn(|k| s * rp[k] + c * rq[k]);
            turn(&mut v);
        }
    }
    ([a[0][0], a[1][1], a[2][2]], v)
}

// ─────────────────────────── colour spaces ───────────────────────────

const WHITE: Rgb = [0.950_47, 1.0, 1.088_83];

fn linear(c: f32) -> f32 {
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn gamma(c: f32) -> f32 {
    let c = c.max(0.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB in 0..1 to CIELAB under D65: L in 0..100, a and b about -128..127.
fn rgb_to_lab(c: Rgb) -> Rgb {
    let [r, g, b] = c.map(linear);
    let xyz = [
        0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b,
        0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b,
        0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b,
    ];
    let f = |t: f32| if t > 0.008_856 { t.cbrt() } else { t / 0.128_418 + 4.0 / 29.0 };
    let [fx, fy, fz] = std::array::from_fn(|k| f(xyz[k] / WHITE[k]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn lab_to_rgb(c: Rgb) -> Rgb {
    let fy = (c[0] + 16.0) / 116.0;
    let (fx, fz) = (fy + c[1] / 500.0, fy - c[2] / 200.0);
    let inv = |t: f32| if t > 6.0 / 29.0 { t * t * t } else { 0.128_418 * (t - 4.0 / 29.0) };
    let [x, y, z] = [inv(fx) * WHITE[0], inv(fy) * WHITE[1], inv(fz) * WHITE[2]];
    [
        3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z,
        -0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z,
        0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z,
    ]
    .map(gamma)
}

// ─────────────────────────── declaration ───────────────────────────

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: G,
        name: "method",
        spec: ParamSpec::Str { default: "mkl", options: &["reinhard", "mkl", "idt", "histogram", "file"], refresh: false },
        expression: None,
        doc: Some("How colours move: `reinhard` matches each channel's mean and spread, `mkl` the whole \
                   covariance with the least movement, `idt` the full distribution, `histogram` each \
                   channel's quantiles; `file` reads a `.cube` table, such as a baked ModFlows transfer."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "size",
        spec: ParamSpec::Num { default: &[33.0], min: 2.0, max: 65.0, int: true, options: &[17, 33, 65], face: Face::Plain },
        expression: None,
        doc: Some("Entries along each side of the table: 33 is the usual grade, 65 finer and slower to fit."),
        section: 0,
        show: &[FROM_IMAGES],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "space",
        spec: ParamSpec::Str { default: "lab", options: &["lab", "rgb"], refresh: false },
        expression: None,
        doc: Some("Where the matching happens: `lab` separates lightness from colour, `rgb` keeps the camera's channels."),
        section: 0,
        show: &[FROM_IMAGES],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "iterations",
        spec: ParamSpec::Num { default: &[12.0], min: 1.0, max: 50.0, int: true, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Rotations `idt` matches along; more come closer to the reference's colours."),
        section: 0,
        show: &[IDT],
        role: None,
    },
    ParamDecl {
        group: G,
        name: "path",
        spec: ParamSpec::Str { default: "", options: &[], refresh: false },
        expression: None,
        doc: Some("A `.cube` file with a 3D table over the 0..1 domain; it is read again when it changes."),
        section: 0,
        show: &[FILE],
        role: None,
    },
    ParamDecl {
        group: "follow",
        name: "smooth",
        spec: ParamSpec::Num { default: &[0.0], min: 0.0, max: 0.99, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("How slowly the table follows a moving source: 0 takes each new frame at once, 0.9 drifts."),
        section: 0,
        show: &[],
        role: None,
    },
];

static INPUTS: &[SlotDecl] = &[
    SlotDecl { name: "source", kind: SlotType::Array, trigger_process: true, multi: false, required: false },
    SlotDecl { name: "reference", kind: SlotType::Array, trigger_process: true, multi: false, required: false },
];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "lut", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Image, Tag::Transform],
    doc: "A colour transfer as a 3D lookup table, for Lut3D.\n\
          `source` is the frame whose colours move and `reference` the colours they move toward, each an \
          `[H, W, 3]` or `[H, W, 4]` frame: GraphicsIn reads a texture back at a small `size`, ImageFile reads \
          a picture. The table is `[N, N*N, 4]`; `file` reads a `.cube` one instead, with no inputs.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(ColorLut, MANIFEST);
