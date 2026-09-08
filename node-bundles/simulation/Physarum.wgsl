/* goofi
{ "doc": "the trail a slime mould leaves, lit the way it is usually shown\nThe field through a ramp with a gamma under it, which is what makes the faint filaments visible at all; the agents themselves ride over it as dots. Wire `trail`, and `positions` for the overlay. The overlay walks at most 512 agents, and a run of more than 8192 does not reach the engine at all.",
  "tags": ["image", "simulation"],
  "inputs": [
    {"name": "trail", "kind": "ARRAY"},
    {"name": "positions", "kind": "ARRAY"} ],
  "params": [
    {"group": "mould", "name": "palette", "kind": "str", "default": "amber",
     "options": ["amber", "ice", "mono"]},
    {"group": "mould", "name": "gain", "kind": "float", "default": 1.0, "min": 0.05, "max": 20.0},
    {"group": "mould", "name": "gamma", "kind": "float", "default": 0.6, "min": 0.1, "max": 4.0},
    {"group": "mould", "name": "agents", "kind": "float", "default": 0.0, "min": 0.0, "max": 8.0} ] }
*/

const MAX_AGENTS: i32 = 512;

fn ramp(x: f32) -> vec3f {
    let v = clamp(x, 0.0, 1.0);
    let warm = clamp(vec3f(1.5 * v, 1.5 * v * v, 2.0 * v * v * v - 0.15), vec3f(0.0), vec3f(1.0));
    switch p.palette {
        case 1u: { return warm.bgr; }
        case 2u: { return vec3f(v); }
        default: { return warm; }
    }
}

fn wired(dims: vec2i, alpha: f32) -> bool {
    return dims.x > 1 || dims.y > 1 || alpha >= 0.5;
}

fn shade(uv: vec2f) -> vec4f {
    let scent = textureSample(trail, samp, uv).r / max(p.trail_hi, 1e-6);
    var colour = ramp(pow(clamp(scent * p.gain, 0.0, 1.0), p.gamma));

    let dims = vec2i(textureDimensions(positions));
    if p.agents > 0.0 && wired(dims, textureLoad(positions, vec2i(0, 0), 0).a) {
        let here = uv * resolution;
        let stride = max(1, (dims.y + MAX_AGENTS - 1) / MAX_AGENTS);
        for (var i = 0; i < dims.y; i = i + stride) {
            let at = vec2f(textureLoad(positions, vec2i(0, i), 0).r, textureLoad(positions, vec2i(1, i), 0).r);
            let cover = clamp(p.agents + 0.5 - distance(here, at * resolution), 0.0, 1.0);
            colour = mix(colour, vec3f(0.95, 0.97, 1.0), cover);
        }
    }
    return vec4f(colour, 1.0);
}
