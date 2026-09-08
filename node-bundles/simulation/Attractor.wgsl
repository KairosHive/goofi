/* goofi
{ "doc": "the orbit a chaotic system draws, on a turning stage\nThe trace a phosphor screen would leave: every point the model reaches is painted and everything already there fades. x across, z up, y into the screen, and `spin` turns the whole figure. Wire the model's `out`; in `value` mode each tick is one dot, and in `block` mode the block is a continuous path.",
  "tags": ["image", "simulation"],
  "inputs": [{"name": "out", "kind": "ARRAY"}],
  "state": ["trace"],
  "params": [
    {"group": "orbit", "name": "spin", "kind": "float", "default": 12.0, "min": -180.0, "max": 180.0},
    {"group": "orbit", "name": "tilt", "kind": "float", "default": 18.0, "min": -90.0, "max": 90.0},
    {"group": "orbit", "name": "zoom", "kind": "float", "default": 0.55, "min": 0.02, "max": 4.0},
    {"group": "orbit", "name": "fade", "kind": "float", "default": 0.04, "min": 0.0, "max": 1.0},
    {"group": "orbit", "name": "thickness", "kind": "float", "default": 1.5, "min": 0.5, "max": 8.0},
    {"group": "orbit", "name": "autoscale", "kind": "bool", "default": true} ] }
*/

// A block may hold a thousand steps, so the path walks at most this many of them.
const MAX_WALK: i32 = 64;

fn ink(under: vec4f, colour: vec3f, cover: f32) -> vec4f {
    return vec4f(mix(under.rgb, colour, cover), max(under.a, cover));
}

fn to_segment(here: vec2f, a: vec2f, b: vec2f) -> f32 {
    let run = b - a;
    let t = clamp(dot(here - a, run) / max(dot(run, run), 1e-6), 0.0, 1.0);
    let off = here - (a + run * t);
    return dot(off, off);
}

/// The frame's own range mapped onto [-1, 1], one range for every axis so the figure is not
/// stretched. The range is what the engine carries in beside the params.
fn scaled(v: f32) -> f32 {
    if p.autoscale == 0u {
        return v;
    }
    return (v - (p.out_lo + p.out_hi) * 0.5) / max((p.out_hi - p.out_lo) * 0.5, 1e-6);
}

/// How far from the middle the figure can possibly reach, in scaled units: the corner of the box
/// the frame's range spans.
fn reach() -> f32 {
    return select(max(abs(p.out_lo), abs(p.out_hi)), 1.0, p.autoscale != 0u) * 1.7320508;
}

fn point_at(k: i32, dims: vec2i) -> vec3f {
    let rank = select(dims.y, dims.x, dims.y == 1);
    var v = vec3f(0.0);
    for (var a = 0; a < min(rank, 3); a = a + 1) {
        v[a] = scaled(textureLoad(out, select(vec2i(k, a), vec2i(a, 0), dims.y == 1), 0).r);
    }
    return v;
}

/// Where a point lands and how far into the screen it is: yaw about the up axis, then tilt.
fn project(v: vec3f) -> vec3f {
    let yaw = radians(p.spin) * time;
    let flat = vec2f(v.x * cos(yaw) - v.y * sin(yaw), v.x * sin(yaw) + v.y * cos(yaw));
    let ct = cos(radians(p.tilt));
    let st = sin(radians(p.tilt));
    return vec3f(flat.x, -(flat.y * st + v.z * ct), flat.y * ct - v.z * st);
}

fn next_trace(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        return vec4f(0.0);
    }
    let held = textureLoad(trace, at, 0);
    let faded = vec4f(held.rgb, held.a * (1.0 - p.fade));
    let dims = vec2i(textureDimensions(out));
    if dims.x == 1 && dims.y == 1 && textureLoad(out, vec2i(0, 0), 0).a < 0.5 {
        return faded;
    }
    let mid = resolution * 0.5;
    let span = min(resolution.x, resolution.y) * 0.5 * p.zoom;
    let here = uv * resolution;
    if distance(here, mid) > reach() * span + p.thickness + 1.0 {
        return faded;
    }
    let points = select(dims.x, 1, dims.y == 1);
    let stride = max(1, (points + MAX_WALK - 1) / MAX_WALK);
    var last = project(point_at(0, dims));
    var prev = mid + last.xy * span;
    var near = distance(here, prev);
    var depth = last.z;
    for (var k = stride; k < points; k = k + stride) {
        last = project(point_at(k, dims));
        let now = mid + last.xy * span;
        let away = sqrt(to_segment(here, prev, now));
        if away < near {
            near = away;
            depth = last.z;
        }
        prev = now;
    }
    // The newest point always, so a stride can never drop where the model is right now.
    last = project(point_at(points - 1, dims));
    let end = mid + last.xy * span;
    let away = sqrt(to_segment(here, prev, end));
    if away < near {
        near = away;
        depth = last.z;
    }
    let colour = mix(vec3f(0.36, 0.55, 0.95), vec3f(1.00, 0.72, 0.36), clamp(depth * 0.5 + 0.5, 0.0, 1.0));
    return ink(faded, colour, clamp(p.thickness + 0.5 - near, 0.0, 1.0));
}

fn shade(uv: vec2f) -> vec4f {
    return textureLoad(trace, vec2i(floor(uv * resolution)), 0);
}
