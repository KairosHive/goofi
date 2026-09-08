/* goofi
{ "doc": "the phase circle a ring of oscillators is read off\nOne dot per oscillator at its own phase, counter-clockwise from the right, and the order vector out of the middle: its length is how synchronised they are and its heading is the mean phase. Wire the model's `phases`; a `block` frame is read at its newest step. Past 256 oscillators the picture samples them.",
  "tags": ["image", "simulation"],
  "inputs": [{"name": "phases", "kind": "ARRAY"}],
  "params": [
    {"group": "circle", "name": "radius", "kind": "float", "default": 0.78, "min": 0.05, "max": 1.0},
    {"group": "circle", "name": "dot", "kind": "float", "default": 4.0, "min": 0.5, "max": 32.0},
    {"group": "circle", "name": "ring", "kind": "float", "default": 1.0, "min": 0.0, "max": 8.0},
    {"group": "circle", "name": "order", "kind": "float", "default": 2.0, "min": 0.0, "max": 12.0} ] }
*/

// A ring of a thousand oscillators is a thousand texture reads at every texel, so the picture
// walks at most this many of them and the order vector is the mean of the ones it walked.
const MAX_UNITS: i32 = 256;

fn hue(t: f32) -> vec3f {
    return 0.55 + 0.45 * cos(6.2831853 * (t + vec3f(0.0, 0.33, 0.67)));
}

fn ink(under: vec4f, colour: vec3f, cover: f32) -> vec4f {
    return vec4f(mix(under.rgb, colour, cover), max(under.a, cover));
}

/// How many oscillators the frame holds: a `value` frame is one row of them, a `block` frame one
/// ROW each, and the newest step is its last column.
fn count(dims: vec2i) -> i32 {
    return select(dims.y, dims.x, dims.y == 1);
}

fn phase_of(i: i32, dims: vec2i) -> f32 {
    let at = select(vec2i(dims.x - 1, i), vec2i(i, 0), dims.y == 1);
    return textureLoad(phases, at, 0).r;
}

/// How far a point is from a segment, squared.
fn to_segment(here: vec2f, a: vec2f, b: vec2f) -> f32 {
    let run = b - a;
    let t = clamp(dot(here - a, run) / max(dot(run, run), 1e-6), 0.0, 1.0);
    let off = here - (a + run * t);
    return dot(off, off);
}

fn shade(uv: vec2f) -> vec4f {
    let dims = vec2i(textureDimensions(phases));
    if dims.x == 1 && dims.y == 1 && textureLoad(phases, vec2i(0, 0), 0).a < 0.5 {
        return vec4f(0.0);
    }
    let n = count(dims);
    let mid = resolution * 0.5;
    let span = min(resolution.x, resolution.y) * 0.5;
    let ring = p.radius * span;
    let here = uv * resolution;
    let reach = length(here - mid);

    var back = vec4f(0.0);
    if p.ring > 0.0 {
        back = ink(back, vec3f(0.30, 0.33, 0.40), clamp(p.ring * 0.5 + 0.5 - abs(reach - ring), 0.0, 1.0));
    }
    if reach > ring + max(p.dot, p.order) + 2.0 {
        return back;
    }

    var front = vec4f(0.0);
    var resultant = vec2f(0.0);
    var walked = 0.0;
    let stride = max(1, (n + MAX_UNITS - 1) / MAX_UNITS);
    for (var i = 0; i < n; i = i + stride) {
        let theta = phase_of(i, dims);
        let turn = vec2f(cos(theta), -sin(theta));
        resultant = resultant + turn;
        walked = walked + 1.0;
        let cover = clamp(p.dot + 0.5 - distance(here, mid + ring * turn), 0.0, 1.0);
        if cover > 0.0 {
            front = ink(front, hue(f32(i) / f32(n)), cover);
        }
    }
    if p.order > 0.0 && walked > 0.0 {
        let tip = mid + ring * resultant / walked;
        let path = clamp(p.order * 0.5 + 0.5 - sqrt(to_segment(here, mid, tip)), 0.0, 1.0);
        let head = clamp(p.order + 1.0 - distance(here, tip), 0.0, 1.0);
        back = ink(back, vec3f(0.94, 0.95, 0.98), max(path, head));
    }
    return ink(back, front.rgb, front.a);
}
