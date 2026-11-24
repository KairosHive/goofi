/* goofi
{ "doc": "find edges, by eight standard methods\nSobel, Prewitt, Scharr and Roberts measure the brightness gradient; Laplacian measures its curvature; LoG and DoG do that after a Gaussian blur of sigma texels; Canny blurs, thins the Sobel gradient to one texel and keeps it above high, or above low next to a texel above high. A black-to-white step reads 1 in the gradient and Laplacian modes, and less after a blur. Transparent texels count as black, and the output is opaque.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "edge", "name": "mode", "kind": "str", "default": "sobel",
     "options": ["sobel", "prewitt", "scharr", "roberts", "laplacian", "log", "dog", "canny"],
     "doc": "The edge detection method."},
    {"group": "edge", "name": "sigma", "kind": "num", "default": 1.0, "min": 0.5, "max": 3.0,
     "show": {"param": "mode", "any_of": ["log", "dog", "canny"]},
     "doc": "Standard deviation of the Gaussian blur, in texels."},
    {"group": "edge", "name": "ratio", "kind": "num", "default": 1.6, "min": 1.1, "max": 2.5,
     "show": {"param": "mode", "any_of": ["dog"]},
     "doc": "Sigma of the wide Gaussian over sigma of the narrow one."},
    {"group": "edge", "name": "neighbours", "kind": "str", "default": "4", "options": ["4", "8"],
     "show": {"param": "mode", "any_of": ["laplacian", "log"]},
     "doc": "Laplacian kernel: the 4 side neighbours, or all 8."},
    {"group": "edge", "name": "output", "kind": "str", "default": "magnitude", "options": ["magnitude", "direction"],
     "show": {"param": "mode", "any_of": ["sobel", "prewitt", "scharr", "roberts"]},
     "doc": "Gradient strength, or the gradient angle as hue (red points right, toward brighter) with strength as value."},
    {"group": "edge", "name": "response", "kind": "str", "default": "absolute", "options": ["absolute", "signed", "crossings"],
     "show": {"param": "mode", "any_of": ["laplacian", "log", "dog"]},
     "doc": "Curvature size, curvature with 0.5 as zero, or white where it changes sign."},
    {"group": "edge", "name": "threshold", "kind": "num", "default": 0.05, "min": 0.0, "max": 1.0,
     "show": [{"param": "mode", "any_of": ["laplacian", "log", "dog"]}, {"param": "response", "any_of": ["crossings"]}],
     "doc": "Smallest jump across a sign change that counts as an edge."},
    {"group": "edge", "name": "low", "kind": "num", "default": 0.1, "min": 0.0, "max": 1.0,
     "show": {"param": "mode", "any_of": ["canny"]},
     "doc": "Weak edge level: kept only next to a strong edge."},
    {"group": "edge", "name": "high", "kind": "num", "default": 0.25, "min": 0.0, "max": 1.0,
     "show": {"param": "mode", "any_of": ["canny"]},
     "doc": "Strong edge level: always kept."},
    {"group": "edge", "name": "mono", "kind": "bool", "default": true,
     "show": [{"param": "mode", "any_of": ["sobel", "prewitt", "scharr", "roberts", "laplacian", "log", "dog"]}, {"param": "output", "any_of": ["magnitude"]}],
     "doc": "Detect on brightness, or on each colour channel by its own."} ] }
*/
const LUMA = vec3f(0.299, 0.587, 0.114);
// sqrt(2 pi e): the peak of a sigma-normalized LoG across a unit step is its inverse.
const STEP_LOG = 4.13273135;
// 2.5 sigma at the largest sigma times ratio, so no kernel is cut short.
const MAX_RADIUS = 19;

// Two 5x5 blurred neighbourhoods: the fine one from 0, DoG's wide one from 25.
var<private> grid: array<vec3f, 50>;

fn edge_texel(t: vec2i, size: vec2i) -> vec3f {
    let c = textureLoad(input, clamp(t, vec2i(0), size - vec2i(1)), 0);
    return c.rgb * clamp(c.a, 0.0, 1.0);
}

fn edge_weight(k: i32, sigma: f32) -> f32 {
    if sigma <= 0.0 { return 1.0; }
    return exp(-f32(k * k) / (2.0 * sigma * sigma));
}

// Fills the grid from `base` with the 5x5 texels around `c` blurred by `sigma` (none at 0).
// Separable: each source row is blurred across, then added into the output rows it reaches.
fn edge_grid(base: i32, c: vec2i, sigma: f32) {
    let size = vec2i(textureDimensions(input));
    var r = 0;
    if sigma > 0.0 { r = min(i32(ceil(2.5 * sigma)), MAX_RADIUS); }
    var norm = 0.0;
    for (var k = -r; k <= r; k++) { norm += edge_weight(k, sigma); }
    for (var i = 0; i < 25; i++) { grid[base + i] = vec3f(0.0); }
    for (var row = -2 - r; row <= 2 + r; row++) {
        var h: array<vec3f, 5>;
        for (var x = 0; x < 5; x++) {
            var s = vec3f(0.0);
            for (var k = -r; k <= r; k++) {
                s += edge_texel(c + vec2i(x - 2 + k, row), size) * edge_weight(k, sigma);
            }
            h[x] = s / norm;
        }
        for (var y = 0; y < 5; y++) {
            let d = row - (y - 2);
            if abs(d) > r { continue; }
            let w = edge_weight(d, sigma) / norm;
            for (var x = 0; x < 5; x++) { grid[base + y * 5 + x] += h[x] * w; }
        }
    }
}

fn edge_at(g: i32, x: i32, y: i32) -> vec3f {
    return grid[g + (y + 2) * 5 + x + 2];
}

struct Gradient { x: vec3f, y: vec3f }

// The gradient at grid cell x/y, normalized so a unit step reads 1; y points down.
fn edge_gradient(g: i32, x: i32, y: i32, mode: u32) -> Gradient {
    if mode == 3u {
        let d1 = edge_at(g, x + 1, y + 1) - edge_at(g, x, y);
        let d2 = edge_at(g, x + 1, y) - edge_at(g, x, y + 1);
        return Gradient((d1 + d2) * 0.5, (d1 - d2) * 0.5);
    }
    var side = 1.0;
    var middle = 2.0;
    if mode == 1u { middle = 1.0; }
    if mode == 2u { side = 3.0; middle = 10.0; }
    let n = side * 2.0 + middle;
    let gx = (side * (edge_at(g, x + 1, y - 1) - edge_at(g, x - 1, y - 1)) + middle * (edge_at(g, x + 1, y) - edge_at(g, x - 1, y))
            + side * (edge_at(g, x + 1, y + 1) - edge_at(g, x - 1, y + 1))) / n;
    let gy = (side * (edge_at(g, x - 1, y + 1) - edge_at(g, x - 1, y - 1)) + middle * (edge_at(g, x, y + 1) - edge_at(g, x, y - 1))
            + side * (edge_at(g, x + 1, y + 1) - edge_at(g, x + 1, y - 1))) / n;
    return Gradient(gx, gy);
}

// The discrete Laplacian at grid cell x/y; the 8-neighbour kernel is three times the 4.
fn edge_laplacian(g: i32, x: i32, y: i32) -> vec3f {
    let sides = edge_at(g, x + 1, y) + edge_at(g, x - 1, y) + edge_at(g, x, y + 1) + edge_at(g, x, y - 1);
    if p.neighbours == 0u { return sides - 4.0 * edge_at(g, x, y); }
    let corners = edge_at(g, x + 1, y + 1) + edge_at(g, x - 1, y + 1) + edge_at(g, x + 1, y - 1) + edge_at(g, x - 1, y - 1);
    return (sides + corners - 8.0 * edge_at(g, x, y)) / 3.0;
}

fn edge_hue(h: f32) -> vec3f {
    return clamp(abs(fract(h + vec3f(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0, vec3f(0.0), vec3f(1.0));
}

fn edge_mono(v: vec3f) -> vec3f {
    if p.mono == 0u { return v; }
    return vec3f(dot(v, LUMA));
}

fn edge_canny(c: vec2i) -> vec4f {
    edge_grid(0, c, p.sigma);
    var mag: array<f32, 9>;
    var gx = 0.0;
    var gy = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let d = edge_gradient(0, x, y, 0u);
            let l = vec2f(dot(d.x, LUMA), dot(d.y, LUMA));
            mag[(y + 1) * 3 + x + 1] = length(l);
            if x == 0 && y == 0 { gx = l.x; gy = l.y; }
        }
    }
    let m = mag[4];
    let high = max(p.high, p.low);
    if m < p.low { return vec4f(0.0, 0.0, 0.0, 1.0); }
    // Non-maximum suppression along the gradient, quantized to one of eight neighbours.
    let sector = round(atan2(gy, gx) / 0.785398163);
    let toward = vec2i(round(vec2f(cos(sector * 0.785398163), sin(sector * 0.785398163))));
    let ahead = mag[(toward.y + 1) * 3 + toward.x + 1];
    let behind = mag[(1 - toward.y) * 3 + 1 - toward.x];
    // The strict side breaks a tie on a flat ridge, so it stays one texel wide.
    if m < ahead || m <= behind { return vec4f(0.0, 0.0, 0.0, 1.0); }
    // Hysteresis reaches the eight neighbours, the centre included.
    var keep = false;
    for (var i = 0; i < 9; i++) { keep = keep || mag[i] >= high; }
    if keep { return vec4f(1.0); }
    return vec4f(0.0, 0.0, 0.0, 1.0);
}

// The curvature response at grid cell x/y of the Laplacian modes, scaled so a unit step peaks near 1.
fn edge_curvature(x: i32, y: i32) -> vec3f {
    if p.mode == 4u { return edge_laplacian(0, x, y); }
    if p.mode == 5u { return edge_laplacian(0, x, y) * p.sigma * p.sigma * STEP_LOG; }
    return (edge_at(25, x, y) - edge_at(0, x, y)) * STEP_LOG / (p.ratio - 1.0);
}

// A response within rounding of zero has no sign, so a flat region crosses nothing.
fn edge_sign(v: vec3f) -> vec3f {
    return select(vec3f(0.0), sign(v), abs(v) > vec3f(1e-4));
}

fn shade(uv: vec2f) -> vec4f {
    let size = vec2i(textureDimensions(input));
    let c = vec2i(floor(uv * vec2f(size)));
    if p.mode == 7u { return edge_canny(c); }
    if p.mode <= 3u {
        edge_grid(0, c, 0.0);
        let d = edge_gradient(0, 0, 0, p.mode);
        if p.output == 1u {
            let l = vec2f(dot(d.x, LUMA), dot(d.y, LUMA));
            let hue = fract(atan2(-l.y, l.x) / 6.28318530718);
            return vec4f(edge_hue(hue) * min(length(l), 1.0), 1.0);
        }
        let gx = edge_mono(d.x);
        let gy = edge_mono(d.y);
        let m = sqrt(gx * gx + gy * gy);
        return vec4f(min(m, vec3f(1.0)), 1.0);
    }
    var sigma = 0.0;
    if p.mode >= 5u { sigma = p.sigma; }
    edge_grid(0, c, sigma);
    if p.mode == 6u { edge_grid(25, c, p.sigma * p.ratio); }
    let v = edge_mono(edge_curvature(0, 0));
    if p.response == 0u { return vec4f(min(abs(v), vec3f(1.0)), 1.0); }
    if p.response == 1u { return vec4f(clamp(0.5 + 0.5 * v, vec3f(0.0), vec3f(1.0)), 1.0); }
    let right = edge_mono(edge_curvature(1, 0));
    let down = edge_mono(edge_curvature(0, 1));
    let across = (edge_sign(v) * edge_sign(right) < vec3f(0.0)) & (abs(v - right) >= vec3f(p.threshold));
    let along = (edge_sign(v) * edge_sign(down) < vec3f(0.0)) & (abs(v - down) >= vec3f(p.threshold));
    return vec4f(select(vec3f(0.0), vec3f(1.0), across | along), 1.0);
}
