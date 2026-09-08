/* goofi
{ "doc": "the plane divided, and the picture that divides it\nOne tessellation, in four geometries. Wallpaper folds the plane by one of the seventeen periodic symmetry groups. Kaleido folds it by a triangle of mirrors, and the three angles decide whether the tiling lives on a sphere, on a plane or in Escher's circle limit — sweep them and the curvature sweeps with them. Quasi lays n grids across each other: two or three of them cross into a lattice, and five never repeat. Mosaic hands the picture its own cells and moves them, one relaxation step a frame, until they sit where the edges, the colours or a wired guide say they should.\nEscher is the page that turns a division into a creature: a displacement field the group carries with it, so the tiles interlock however far they are pushed. Fit blends that field into the picture's own gradient, so the outline learns the image. Morph makes the field drift across the frame and the tiles change shape as they go, still interlocking, which is what a parquet deformation is. Gestalt is where the pattern stops being geometry: figure trades the two hands of a tile, flatten takes each tile's one colour, contour draws the seam and tint tells the tiles apart.\nAmount holds the tiling exact on one harmonic however far it is pushed. On several, past about three quarters it begins to fold tiles through each other, which is a look and not an error.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}, {"name": "guide", "kind": "TEXTURE"}],
  "state": ["sites"],
  "params": [
    {"group": "tile", "name": "kind", "kind": "str", "default": "wallpaper",
     "options": ["wallpaper", "kaleido", "quasi", "mosaic"]},
    {"group": "tile", "name": "source", "kind": "str", "default": "whole", "options": ["whole", "window"]},
    {"group": "tile", "name": "scale", "kind": "float", "default": 4.0, "min": 0.2, "max": 64.0},
    {"group": "tile", "name": "turn", "kind": "float", "default": 0.0, "min": -360.0, "max": 360.0},
    {"group": "tile", "name": "x", "kind": "float", "default": 0.0, "min": -2.0, "max": 2.0},
    {"group": "tile", "name": "y", "kind": "float", "default": 0.0, "min": -2.0, "max": 2.0},

    {"group": "wallpaper", "name": "group", "kind": "str", "default": "p4m",
     "options": ["p1", "p2", "pm", "pg", "cm", "pmm", "pmg", "pgg", "cmm",
                 "p4", "p4m", "p4g", "p3", "p3m1", "p31m", "p6", "p6m"]},
    {"group": "wallpaper", "name": "aspect", "kind": "float", "default": 1.0, "min": 0.2, "max": 5.0},
    {"group": "wallpaper", "name": "skew", "kind": "float", "default": 0.0, "min": -0.5, "max": 0.5},

    {"group": "kaleido", "name": "sides", "kind": "float", "default": 7.0, "min": 2.0, "max": 12.0},
    {"group": "kaleido", "name": "meet", "kind": "float", "default": 3.0, "min": 2.0, "max": 12.0},
    {"group": "kaleido", "name": "hinge", "kind": "float", "default": 2.0, "min": 2.0, "max": 12.0},

    {"group": "quasi", "name": "fold", "kind": "int", "default": 5, "min": 2, "max": 12},
    {"group": "quasi", "name": "shift", "kind": "float", "default": 0.2, "min": -4.0, "max": 4.0},
    {"group": "quasi", "name": "spread", "kind": "float", "default": 0.0, "min": -2.0, "max": 2.0},

    {"group": "mosaic", "name": "cells", "kind": "int", "default": 24, "min": 2, "max": 96},
    {"group": "mosaic", "name": "density", "kind": "str", "default": "edges",
     "options": ["flat", "luma", "dark", "edges", "guide"]},
    {"group": "mosaic", "name": "affinity", "kind": "float", "default": 0.0, "min": 0.0, "max": 4.0},
    {"group": "mosaic", "name": "rate", "kind": "float", "default": 0.25, "min": 0.0, "max": 1.0},

    {"group": "escher", "name": "amount", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0},
    {"group": "escher", "name": "harmonics", "kind": "int", "default": 2, "min": 1, "max": 4},
    {"group": "escher", "name": "phase", "kind": "float", "default": 0.0, "min": -12.0, "max": 12.0},
    {"group": "escher", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0},
    {"group": "escher", "name": "fit", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0},
    {"group": "escher", "name": "follow", "kind": "str", "default": "input", "options": ["input", "guide"]},

    {"group": "morph", "name": "drift", "kind": "float", "default": 0.0, "min": -4.0, "max": 4.0},
    {"group": "morph", "name": "swirl", "kind": "float", "default": 0.0, "min": -12.0, "max": 12.0},
    {"group": "morph", "name": "angle", "kind": "float", "default": 0.0, "min": -360.0, "max": 360.0},

    {"group": "gestalt", "name": "figure", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0},
    {"group": "gestalt", "name": "flatten", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0},
    {"group": "gestalt", "name": "contour", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0},
    {"group": "gestalt", "name": "tint", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0} ] }
*/
const TAU: f32 = 6.28318530718;
// Reflections a kaleido fold may take before the point is called unreachable, and how many of them
// are remembered so the tile's own seat can be replayed. Two bits a mirror fills the word exactly.
const KMAX: i32 = 32;
const REPLAY: u32 = 16u;
// Samples a side in the Lloyd quadrature, and the widest point group any of the seventeen has.
const QUAD: i32 = 10;
const OPS: i32 = 12;

// What a division answers about the point it was asked. Every geometry fills the same five, and
// everything after the fold reads these and never the geometry. A negative id is the one answer
// that is not a tile: the point lies outside a geometry that ENDS, which only the hyperbolic one does.
struct Cell {
    local: vec2f,
    seat: vec2f,
    edge: f32,
    parity: f32,
    id: f32,
}

fn hash21(v: vec2f) -> f32 {
    let q = vec2u(vec2i(floor(v)) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

fn hash11(x: f32) -> f32 {
    var h = bitcast<u32>(x + 3.7) * 2654435761u;
    h = (h ^ (h >> 15u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

fn rot(a: f32) -> mat2x2f {
    let s = sin(a);
    let c = cos(a);
    return mat2x2f(vec2f(c, s), vec2f(-s, c));
}

fn inv2(m: mat2x2f) -> mat2x2f {
    let det = m[0].x * m[1].y - m[1].x * m[0].y;
    let safe = select(det, 1e-5, abs(det) < 1e-5);
    return mat2x2f(vec2f(m[1].y, -m[0].y), vec2f(-m[1].x, m[0].x)) * (1.0 / safe);
}

fn asp() -> vec2f { return resolution / min(resolution.x, resolution.y); }

fn toPattern(uv: vec2f) -> vec2f {
    return (rot(-radians(p.turn)) * ((uv - vec2f(0.5)) * asp() - vec2f(p.x, p.y))) * p.scale;
}

fn toUv(x: vec2f) -> vec2f {
    return ((rot(radians(p.turn)) * (x / p.scale)) + vec2f(p.x, p.y)) / asp() + vec2f(0.5);
}

// The mosaic keeps the frame's own size, because its cells are counted and not scaled.
fn toMosaic(uv: vec2f) -> vec2f {
    return rot(-radians(p.turn)) * ((uv - vec2f(0.5)) * asp() - vec2f(p.x, p.y)) + vec2f(0.5);
}

fn mosaicToUv(m: vec2f) -> vec2f {
    return ((rot(radians(p.turn)) * (m - vec2f(0.5))) + vec2f(p.x, p.y)) / asp() + vec2f(0.5);
}

fn pic(uv: vec2f) -> vec4f { return textureSampleLevel(input, samp, uv, 0.0); }
fn hint(uv: vec2f) -> vec4f { return textureSampleLevel(guide, samp, uv, 0.0); }
fn luma(c: vec4f) -> f32 { return dot(c.rgb, vec3f(0.299, 0.587, 0.114)); }

// 0 oblique, 1 rectangular, 2 rhombic, 3 square, 4 hexagonal.
fn latKind() -> i32 {
    switch p.group {
        case 0u, 1u: { return 0; }
        case 2u, 3u, 5u, 6u, 7u: { return 1; }
        case 4u, 8u: { return 2; }
        case 9u, 10u, 11u: { return 3; }
        default: { return 4; }
    }
}

fn basis() -> mat2x2f {
    let h = p.aspect;
    switch latKind() {
        case 0: { return mat2x2f(vec2f(1.0, 0.0), vec2f(p.skew, h)); }
        case 1: { return mat2x2f(vec2f(1.0, 0.0), vec2f(0.0, h)); }
        case 2: { return mat2x2f(vec2f(0.5, h * 0.5), vec2f(0.5, -h * 0.5)); }
        case 3: { return mat2x2f(vec2f(1.0, 0.0), vec2f(0.0, 1.0)); }
        default: { return mat2x2f(vec2f(1.0, 0.0), vec2f(0.5, 0.86602540)); }
    }
}

fn spin() -> i32 {
    switch p.group {
        case 1u, 5u, 6u, 7u, 8u: { return 2; }
        case 12u, 13u, 14u: { return 3; }
        case 9u, 10u, 11u: { return 4; }
        case 15u, 16u: { return 6; }
        default: { return 1; }
    }
}

fn mirrored() -> bool {
    switch p.group {
        case 0u, 1u, 9u, 12u, 15u: { return false; }
        default: { return true; }
    }
}

// The mirror's own line, as the angle it makes with x. p3m1 lies along the lattice and p31m across
// it, which is the whole difference between the two.
fn mirrorAngle() -> f32 {
    switch p.group {
        case 2u, 3u, 4u, 6u: { return TAU * 0.25; }
        case 11u: { return -TAU * 0.125; }
        case 14u: { return TAU / 12.0; }
        default: { return 0.0; }
    }
}

// What the mirror carries with it. Along the line it is a glide; across it, an offset mirror.
fn glideStep() -> vec2f {
    let b = basis();
    switch p.group {
        case 3u: { return b[1] * 0.5; }
        case 6u: { return b[0] * 0.5; }
        case 7u, 11u: { return (b[0] + b[1]) * 0.5; }
        default: { return vec2f(0.0); }
    }
}

fn pointOrder() -> i32 { return spin() * select(1, 2, mirrored()); }

struct Op { m: mat2x2f, t: vec2f }

fn pointOp(k: i32) -> Op {
    let n = spin();
    let turn = rot(TAU * f32(k % n) / f32(n));
    var o: Op;
    o.m = turn;
    o.t = vec2f(0.0);
    if k >= n {
        let d = 2.0 * mirrorAngle();
        o.m = turn * mat2x2f(vec2f(cos(d), sin(d)), vec2f(sin(d), -cos(d)));
        o.t = turn * glideStep();
    }
    return o;
}

// One cell of the lattice sweeps the whole picture and comes back the way it went, so a field read
// off the picture stays continuous where two cells meet.
fn cellUv(c: vec2f) -> vec2f {
    let s = sin(TAU * 0.5 * c);
    return s * s;
}

fn steer(uv: vec2f) -> f32 { return select(luma(pic(uv)), luma(hint(uv)), p.follow == 1u); }

// The picture's own gradient, turned around: the seam is pulled down the slope and settles where
// the picture is dark. Bounded, because an unbounded field folds tiles through each other.
fn imageField(c: vec2f) -> vec2f {
    let e = 0.01;
    let gx = steer(cellUv(c + vec2f(e, 0.0))) - steer(cellUv(c - vec2f(e, 0.0)));
    let gy = steer(cellUv(c + vec2f(0.0, e))) - steer(cellUv(c - vec2f(0.0, e)));
    let v = -vec2f(gx, gy) / (2.0 * e);
    return v / (length(v) + 4.0);
}

// A lattice-periodic scalar's gradient, turned a quarter turn. Turned, because a gradient field
// SQUEEZES the plane and a boundary pushed by one thickens rather than bends; its perpendicular
// shears instead, which is the motion an interlocking arm is made of, and it holds the area of
// every tile it moves.
fn periodicField(c: vec2f, phase: f32) -> vec2f {
    var k = array<vec2f, 4>(vec2f(1.0, 0.0), vec2f(0.0, 1.0), vec2f(1.0, 1.0), vec2f(1.0, -1.0));
    let m = clamp(p.harmonics, 1, 4);
    var g = vec2f(0.0);
    var norm = 1e-5;
    for (var i = 0; i < 4; i++) {
        if i >= m { break; }
        let w = 1.0 / f32(i + 1);
        let ph = TAU * dot(k[i], c) + phase + hash11(p.seed + f32(i) * 7.0) * TAU;
        g = g - k[i] * (w * TAU * sin(ph));
        norm = norm + w * TAU * length(k[i]);
    }
    return vec2f(-g.y, g.x) / norm;
}

// The displacement the group carries: u(gx) = g u(x), by averaging one periodic field over the
// point group. That identity is the whole reason a deformed tile still interlocks — push a boundary
// out here and the same push arrives as a dent on every tile that meets it.
fn shiftWith(x: vec2f, bi: mat2x2f, ops: i32, amp: f32, phase: f32) -> vec2f {
    if amp <= 0.0 { return vec2f(0.0); }
    var u = vec2f(0.0);
    for (var k = 0; k < OPS; k++) {
        if k >= ops { break; }
        let o = pointOp(k);
        let c = bi * (o.m * x + o.t);
        var f = periodicField(c, phase);
        if p.fit > 0.0 { f = mix(f, imageField(c), p.fit); }
        u = u + transpose(o.m) * (transpose(bi) * f);
    }
    // The terms point every which way, so their sum grows as the root of their count rather than
    // with it. Dividing by that root is what makes `amount` mean one thing in all seventeen groups.
    return u * (amp / sqrt(f32(ops)));
}

// A parquet deformation: the field's strength and its phase are read off where in the FRAME the
// point stands, so the tiles change shape as they march and still fit together. The reading is
// frame-relative and never pattern-relative, or the drift would move whenever the scale did.
fn morphAt(seen: vec2f) -> vec2f {
    let dir = vec2f(cos(radians(p.angle)), sin(radians(p.angle)));
    let g = dot(seen, dir);
    return vec2f(clamp(1.0 + p.drift * g, 0.0, 2.0), p.swirl * g);
}

// The lattice point this one belongs to: the parallelogram first, then two sweeps of the
// neighbours, which is what turns it into the cell that is actually nearest.
fn latticeIndex(x: vec2f, b: mat2x2f, bi: mat2x2f) -> vec2f {
    var n = round(bi * x);
    for (var sweep = 0; sweep < 2; sweep++) {
        let d = x - b * n;
        var best = dot(d, d);
        var pick = vec2f(0.0);
        for (var i = -1; i <= 1; i++) {
            for (var j = -1; j <= 1; j++) {
                let v = vec2f(f32(i), f32(j));
                let e = d - b * v;
                let q = dot(e, e);
                if q < best - 1e-7 { best = q; pick = v; }
            }
        }
        n = n + pick;
    }
    return n;
}

fn voronoiEdge(d: vec2f, b: mat2x2f) -> f32 {
    var best = 1e9;
    for (var i = -1; i <= 1; i++) {
        for (var j = -1; j <= 1; j++) {
            if i == 0 && j == 0 { continue; }
            let v = b * vec2f(f32(i), f32(j));
            let l = max(length(v), 1e-5);
            best = min(best, 0.5 * l - dot(d, v) / l);
        }
    }
    return best;
}

fn wallpaperCell(x: vec2f) -> Cell {
    let b = basis();
    let bi = inv2(b);
    var out: Cell;
    var par = 0.0;
    let g = p.group;
    // pg, pmg and pgg fold along their own glides, which no rotation about a lattice point reaches.
    if g == 3u || g == 6u || g == 7u {
        let raw = bi * x;
        let cell = floor(raw);
        var c = raw - cell;
        var hi = vec2f(0.5, 1.0);
        if g == 3u {
            if c.x > 0.5 { c = vec2f(1.0 - c.x, fract(c.y + 0.5)); par = 1.0; }
        } else if g == 6u {
            if c.x >= 0.5 { c = vec2f(c.x - 0.5, 1.0 - c.y); par = 1.0 - par; }
            if c.x > 0.25 { c.x = 0.5 - c.x; par = 1.0 - par; }
            hi = vec2f(0.25, 1.0);
        } else {
            if c.x >= 0.5 { c = vec2f(c.x - 0.5, fract(0.5 - c.y)); par = 1.0 - par; }
            if c.y >= 0.5 { c = vec2f(fract(0.5 - c.x), c.y - 0.5); par = 1.0 - par; }
            hi = vec2f(0.5, 0.5);
        }
        let m = min(min(c.x, hi.x - c.x) * length(b[0]), min(c.y, hi.y - c.y) * length(b[1]));
        out.local = select(toUv(b * (c - vec2f(0.5))), c, p.source == 0u);
        out.seat = toUv(b * (cell + vec2f(0.5)));
        out.edge = m / p.scale;
        out.parity = par;
        out.id = hash11(hash21(cell) + par * 0.517);
        return out;
    }
    let n = latticeIndex(x, b, bi);
    var d = x - b * n;
    var e = voronoiEdge(d, b);
    // p4g's mirror is the one that misses the four-fold centre, so its wedge is the plain quarter
    // and the mirror's own angle belongs to the displacement field rather than to this fold.
    let centred = mirrored() && g != 11u;
    let order = spin();
    let mu = select(mirrorAngle(), 0.0, g == 11u);
    let w = TAU / f32(order);
    let r = length(d);
    let a = atan2(d.y, d.x) - mu;
    let k = floor(a / w);
    var t = a - k * w;
    if centred && t > w * 0.5 { t = w - t; par = 1.0; }
    if order > 1 || centred {
        let top = select(w, w * 0.5, centred);
        e = min(e, r * min(sin(t), sin(max(top - t, 0.0))));
    }
    d = vec2f(cos(mu + t), sin(mu + t)) * r;
    if g == 11u {
        let s = 0.5 * length(b[0]);
        e = min(e, abs(s - d.x - d.y) * 0.70710678);
        if d.x + d.y > s { d = vec2f(s - d.y, s - d.x); par = 1.0 - par; }
    }
    out.local = select(toUv(d), bi * d + vec2f(0.5), p.source == 0u);
    out.seat = toUv(b * n);
    out.edge = max(e, 0.0) / p.scale;
    out.parity = par;
    out.id = hash11(hash21(n) + f32(k) * 0.131 + par * 0.517);
    return out;
}

// A mirror as one curve: k|x|^2 - 2 n.x + d = 0, which is a circle when k is not zero and a line
// when it is. Normalised so |n|^2 - k d = 1, the domain is where the power is negative, and the
// Euclidean case falls out as a straight line with no arm of its own.
struct Arc { k: f32, n: vec2f, d: f32 }

fn arcs(i: i32) -> Arc {
    let a = TAU * 0.5 / max(p.sides, 1.001);
    let b = TAU * 0.5 / max(p.meet, 1.001);
    let c = TAU * 0.5 / max(p.hinge, 1.001);
    var m: Arc;
    m.d = 0.0;
    if i == 0 {
        m.k = 0.0;
        m.n = vec2f(0.0, 1.0);
        return m;
    }
    if i == 1 {
        m.k = 0.0;
        m.n = vec2f(sin(a), -cos(a));
        return m;
    }
    let ny = -cos(b);
    let nx = (-cos(c) - cos(a) * cos(b)) / max(sin(a), 1e-4);
    m.n = vec2f(nx, ny);
    m.k = nx + sin(b);
    m.d = nx - sin(b);
    return m;
}

fn arcDist(m: Arc, x: vec2f) -> f32 {
    let power = m.k * dot(x, x) - 2.0 * dot(m.n, x) + m.d;
    return power / max(2.0 * length(m.k * x - m.n), 1e-6);
}

fn arcReflect(m: Arc, x: vec2f) -> vec2f {
    if abs(m.k) < 1e-5 { return x - 2.0 * (dot(m.n, x) - 0.5 * m.d) * m.n; }
    let o = m.n / m.k;
    let e = x - o;
    return o + e / max(m.k * m.k * dot(e, e), 1e-9);
}

// Where the third mirror crosses the second. With the first crossing it at (1, 0) by construction,
// that is the triangle.
fn triCorner() -> vec2f {
    let a = TAU * 0.5 / max(p.sides, 1.001);
    let e = vec2f(cos(a), sin(a));
    let m = arcs(2);
    let b = dot(m.n, e);
    if abs(m.k) < 1e-5 { return e * (m.d / (2.0 * b)); }
    let root = sqrt(max(b * b - m.k * m.d, 0.0));
    let s1 = (b - root) / m.k;
    let s2 = (b + root) / m.k;
    return e * select(max(s1, s2), min(s1, s2), min(s1, s2) > 1e-4);
}

fn kaleidoCell(x: vec2f, amp: f32, phase: f32) -> Cell {
    var out: Cell;
    // A hyperbolic plane ENDS. Its limit circle is where the third mirror is orthogonal to, and
    // the three domain tests alone do not name the outside of it — they answer yes out there too.
    let third = arcs(2);
    let limit = select(third.d / third.k, -1.0, abs(third.k) < 1e-5);
    if limit > 0.0 && dot(x, x) >= limit {
        out.id = -1.0;
        return out;
    }
    var y = x;
    var par = 0.0;
    var word = 0u;
    var steps = 0u;
    var home = false;
    for (var s = 0; s < KMAX; s++) {
        var worst = -1;
        var far = 1e-6;
        for (var i = 0; i < 3; i++) {
            let v = arcDist(arcs(i), y);
            if v > far { far = v; worst = i; }
        }
        if worst < 0 { home = true; break; }
        y = arcReflect(arcs(worst), y);
        par = 1.0 - par;
        if steps < REPLAY { word = word | (u32(worst + 1) << (2u * steps)); steps = steps + 1u; }
    }
    if !home {
        out.id = -1.0;
        return out;
    }
    var e = 1e9;
    for (var i = 0; i < 3; i++) { e = min(e, -arcDist(arcs(i), y)); }
    // The tile's own place: the same mirrors, walked backwards off the triangle's centre.
    let corner = triCorner();
    var seat = (vec2f(1.0, 0.0) + corner) / 3.0;
    for (var s = i32(steps) - 1; s >= 0; s--) {
        seat = arcReflect(arcs(i32((word >> (2u * u32(s))) & 3u) - 1), seat);
    }
    if amp > 0.0 {
        var bump = 1.0;
        for (var i = 0; i < 3; i++) { bump = bump * clamp(-arcDist(arcs(i), y) * 6.0, 0.0, 1.0); }
        var f = periodicField(y * 2.0, phase);
        if p.fit > 0.0 { f = mix(f, imageField(y * 2.0), p.fit); }
        y = y + f * (amp * bump * 0.5);
    }
    let lo = min(vec2f(0.0), min(vec2f(1.0, 0.0), corner));
    let span = max(max(vec2f(1.0, 0.0), corner) - lo, vec2f(1e-3));
    out.local = select(toUv(y), (y - lo) / span, p.source == 0u);
    out.seat = toUv(seat);
    out.edge = max(e, 0.0) / p.scale;
    out.parity = par;
    out.id = hash11(f32(word) * 1e-3 + f32(steps));
    return out;
}

// De Bruijn's multigrid: n families of parallel lines, and the region a point falls in names one
// vertex of the tiling that is dual to them. Five families is Penrose. Shift slides the whole
// pattern, spread moves the families against each other, which rearranges tiles rather than
// carrying them — the phason of a real quasicrystal.
fn quasiCell(x: vec2f) -> Cell {
    let n = clamp(p.fold, 2, 12);
    var v = vec2f(0.0);
    var e = 1e9;
    var id = 0.0;
    for (var j = 0; j < 12; j++) {
        if j >= n { break; }
        let a = TAU * 0.5 * f32(j) / f32(n);
        let dir = vec2f(cos(a), sin(a));
        let off = fract(f32(j + 1) * 0.7548776662);
        let t = dot(x, dir) + p.shift + off + p.spread * hash11(f32(j) * 3.1 + 0.5);
        let kf = floor(t);
        v = v + dir * kf;
        e = min(e, min(t - kf, 1.0 - (t - kf)));
        id = id + kf * (1.0 + f32(j) * 0.618);
    }
    let seat = v * (2.0 / f32(n));
    var out: Cell;
    out.local = select(toUv(x), (x - seat) * 0.5 + vec2f(0.5), p.source == 0u);
    out.seat = toUv(seat);
    out.edge = e / p.scale;
    out.parity = f32(u32(abs(id)) & 1u);
    out.id = hash11(id);
    return out;
}

// A site wanders at most REACH cells from where it was born, which is what bounds the search for
// the nearest one to a block around the point and keeps a full walk of the set out of every texel.
// It has to be allowed to wander at all: a site pinned to its own square cannot crowd anywhere,
// and crowding is the whole of what a weighted relaxation has to say. BLOCK is then not a taste:
// a site outside it must be further off than the worst site inside, which needs 2 REACH + 1 cells.
const REACH: f32 = 1.0;
const BLOCK: i32 = 3;

fn siteAt(idx: vec2i, n: i32) -> vec2f {
    let w = ((idx % vec2i(n)) + vec2i(n)) % vec2i(n);
    let home = (vec2f(w) + vec2f(0.5)) / f32(n);
    let held = textureLoad(sites, w, 0).xy;
    let kept = all(abs(held - home) <= vec2f((REACH + 0.01) / f32(n)));
    return select(home, held, kept) + vec2f(idx - w) / f32(n);
}

struct Near { idx: vec2i, best: f32, second: f32, site: vec2f }

fn nearest(q: vec2f, n: i32) -> Near {
    let base = vec2i(floor(q * f32(n)));
    let cq = select(vec3f(0.0), pic(mosaicToUv(q)).rgb, p.affinity > 0.0);
    var r: Near;
    r.idx = base;
    r.site = q;
    r.best = 1e9;
    r.second = 1e9;
    for (var i = -BLOCK; i <= BLOCK; i++) {
        for (var j = -BLOCK; j <= BLOCK; j++) {
            let idx = base + vec2i(i, j);
            let s = siteAt(idx, n);
            let d = (q - s) * f32(n);
            var m = dot(d, d);
            if p.affinity > 0.0 {
                let dc = cq - pic(mosaicToUv(s)).rgb;
                m = m + p.affinity * p.affinity * dot(dc, dc);
            }
            if m < r.best {
                r.second = r.best;
                r.best = m;
                r.idx = ((idx % vec2i(n)) + vec2i(n)) % vec2i(n);
                r.site = s;
            } else if m < r.second {
                r.second = m;
            }
        }
    }
    return r;
}

fn density(q: vec2f) -> f32 {
    let uvq = mosaicToUv(q);
    switch p.density {
        case 1u: { return luma(pic(uvq)) + 0.02; }
        case 2u: { return 1.02 - luma(pic(uvq)); }
        case 3u: {
            // A cell can only walk towards structure its own size, so the edge is measured over
            // half of one. A one-texel tap reads noise and under-samples the quadrature entirely.
            let e = 0.5 / f32(clamp(p.cells, 2, 96));
            let gx = luma(pic(uvq + vec2f(e, 0.0))) - luma(pic(uvq - vec2f(e, 0.0)));
            let gy = luma(pic(uvq + vec2f(0.0, e))) - luma(pic(uvq - vec2f(0.0, e)));
            return sqrt(gx * gx + gy * gy) * 8.0 + 0.05;
        }
        case 4u: { return luma(hint(uvq)) + 0.02; }
        default: { return 1.0; }
    }
}

fn mosaicCell(m: vec2f) -> Cell {
    let n = clamp(p.cells, 2, 96);
    let q = fract(m);
    let r = nearest(q, n);
    var out: Cell;
    out.local = select(mosaicToUv(m), (q - r.site) * f32(n) + vec2f(0.5), p.source == 0u);
    out.seat = mosaicToUv(r.site);
    out.edge = (sqrt(r.second) - sqrt(r.best)) * 0.5 / f32(n);
    out.parity = f32((r.idx.x + r.idx.y) & 1);
    out.id = hash21(vec2f(r.idx));
    return out;
}

fn hueRot(c: vec3f, a: f32) -> vec3f {
    let k = vec3f(0.57735027);
    let ca = cos(a);
    return c * ca + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - ca);
}

fn shade(uv: vec2f) -> vec4f {
    let one = mat2x2f(vec2f(1.0, 0.0), vec2f(0.0, 1.0));
    var c: Cell;
    if p.kind == 3u {
        // The site grid is the mosaic's own lattice, so the displacement is periodic on THAT and
        // bends a cell rather than carrying it — a walk of at most a third of a cell, which the
        // search block was sized to cover.
        let m = toMosaic(uv);
        let n = f32(clamp(p.cells, 2, 96));
        let mo = morphAt(m - vec2f(0.5));
        let amp = max(p.amount * mo.x, 0.0) * 0.35;
        c = mosaicCell(m + shiftWith(m * n, one, 1, amp, p.phase + mo.y) / n);
    } else {
        let x = toPattern(uv);
        let mo = morphAt(x / max(p.scale, 1e-3));
        let amp = max(p.amount * mo.x, 0.0) * 0.35;
        let ph = p.phase + mo.y;
        if p.kind == 0u {
            let bi = inv2(basis());
            c = wallpaperCell(x + shiftWith(x, bi, pointOrder(), amp, ph));
        } else if p.kind == 1u {
            c = kaleidoCell(x, amp, ph);
        } else {
            c = quasiCell(x + shiftWith(x, one, 1, amp, ph));
        }
    }
    if c.id < 0.0 { return vec4f(0.0); }
    var col = pic(mix(c.local, c.seat, p.flatten));
    col = mix(col, vec4f(vec3f(1.0) - col.rgb, col.a), p.figure * c.parity);
    col = vec4f(hueRot(col.rgb, hash11(c.id) * TAU * p.tint), col.a);
    let px = max(fwidth(c.edge), 1e-7);
    let line = 1.0 - smoothstep(px * 0.5, px * 1.5, c.edge);
    return mix(col, vec4f(0.0, 0.0, 0.0, 1.0), line * p.contour);
}

// One Lloyd step a frame, weighted by whatever density says. The sites walk to the centroids of
// their own cells and the division settles onto the picture while it is being watched.
fn next_sites(uv: vec2f) -> vec4f {
    let n = clamp(p.cells, 2, 96);
    let at = vec2i(floor(uv * resolution));
    if p.kind != 3u || at.x >= n || at.y >= n { return vec4f(0.0); }
    let home = (vec2f(at) + vec2f(0.5)) / f32(n);
    let held = textureLoad(sites, at, 0).xy;
    var s = home;
    if frame > 0u && all(abs(held - home) <= vec2f((REACH + 0.01) / f32(n))) {
        s = held;
    } else {
        s = home + (vec2f(hash21(vec2f(at)), hash21(vec2f(at) + vec2f(0.0, 77.0))) - 0.5) * (0.4 / f32(n));
    }
    var acc = vec2f(0.0);
    var mass = 0.0;
    let step = 2.0 / (f32(n) * f32(QUAD));
    for (var i = 0; i < QUAD; i++) {
        for (var j = 0; j < QUAD; j++) {
            let q = s + (vec2f(f32(i), f32(j)) + vec2f(0.5) - f32(QUAD) * 0.5) * step;
            if all(nearest(fract(q), n).idx == at) {
                let w = max(density(q), 0.0);
                acc = acc + q * w;
                mass = mass + w;
            }
        }
    }
    if mass > 1e-5 { s = mix(s, acc / mass, p.rate); }
    return vec4f(clamp(s, home - vec2f(REACH / f32(n)), home + vec2f(REACH / f32(n))), 0.0, 1.0);
}
