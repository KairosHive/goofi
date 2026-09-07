/* goofi
{ "doc": "a membrane that rings\nThe wave equation on a grid: ripples spread, meet, interfere and fade. It keeps two frames of history, because a wave needs to know where it was going.",
  "tags": ["image", "generator", "simulation"],
  "state": ["now", "past"],
  "params": [
    {"group": "wave", "name": "speed", "kind": "float", "default": 0.5, "min": 0.0, "max": 0.7},
    {"group": "wave", "name": "damping", "kind": "float", "default": 0.002, "min": 0.0, "max": 0.2},
    {"group": "wave", "name": "drive", "kind": "float", "default": 0.5, "min": 0.0, "max": 4.0},
    {"group": "wave", "name": "frequency", "kind": "float", "default": 2.0, "min": 0.0, "max": 60.0},
    {"group": "wave", "name": "spot", "kind": "float", "default": 3.0, "min": 0.5, "max": 40.0},
    {"group": "wave", "name": "gain", "kind": "float", "default": 4.0, "min": 0.1, "max": 40.0} ] }
*/
fn height(at: vec2i) -> f32 {
    let size = vec2i(resolution);
    return textureLoad(now, ((at % size) + size) % size, 0).r;
}

fn before(at: vec2i) -> f32 {
    let size = vec2i(resolution);
    return textureLoad(past, ((at % size) + size) % size, 0).r;
}

fn laplacian(at: vec2i) -> f32 {
    let sides = height(at + vec2i(1, 0)) + height(at + vec2i(-1, 0))
        + height(at + vec2i(0, 1)) + height(at + vec2i(0, -1));
    return sides - 4.0 * height(at);
}

// A soft blob at the middle, driven at `frequency`, which is what puts energy into the membrane.
fn source(at: vec2i) -> f32 {
    let middle = resolution * 0.5;
    let d = length(vec2f(at) - middle) / max(p.spot, 0.5);
    return p.drive * exp(-d * d) * sin(6.2831853 * p.frequency * time);
}

fn advanced(at: vec2i) -> f32 {
    let c2 = p.speed * p.speed;
    let swing = 2.0 * height(at) - before(at) + c2 * laplacian(at);
    return (swing - p.damping * (height(at) - before(at))) + source(at);
}

fn next_now(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame < 2u {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    return vec4f(vec3f(clamp(advanced(at), -8.0, 8.0)), 1.0);
}

// One tick behind `now`, which is the second frame of history the wave equation needs.
fn next_past(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame < 2u {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    return vec4f(vec3f(height(at)), 1.0);
}

// Displacement swings both ways, so zero is grey and `gain` opens the picture up.
fn shade(uv: vec2f) -> vec4f {
    let h = height(vec2i(floor(uv * resolution))) * p.gain;
    return vec4f(vec3f(clamp(h * 0.5 + 0.5, 0.0, 1.0)), 1.0);
}
