/* goofi
{ "doc": "a gradient through any number of colour stops\nThe angle turns the gradient: 0 runs left to right, 90 top to bottom. Each stop sits at a position along it, and the colour runs from one stop to the next; before the first and past the last stop the colour holds.",
  "tags": ["image", "generator"],
  "params": [
    {"group": "ramp", "name": "angle", "kind": "float", "default": 0.0, "min": 0.0, "max": 360.0},
    {"group": "ramp", "section": "stops", "doc": "how many colour stops the gradient runs through",
     "repeat": {"min": 2, "max": 16, "default": 2},
     "params": [
       {"name": "at", "kind": "float", "default": [0.0, 1.0], "min": 0.0, "max": 1.0},
       {"name": "r", "kind": "float", "default": [0.0, 1.0], "min": 0.0, "max": 1.0},
       {"name": "g", "kind": "float", "default": [0.0, 1.0], "min": 0.0, "max": 1.0},
       {"name": "b", "kind": "float", "default": [0.0, 1.0], "min": 0.0, "max": 1.0} ]} ] }
*/
fn colour(i: u32) -> vec3f {
    return vec3f(p.r[i], p.g[i], p.b[i]);
}

fn shade(uv: vec2f) -> vec4f {
    let a = radians(p.angle);
    let t = clamp(dot(uv - vec2f(0.5), vec2f(cos(a), sin(a))) + 0.5, 0.0, 1.0);
    let n = u32(max(p.stops, 1));
    var rgb = colour(n - 1u);
    // The stops run in declared order; the first one at or past `t` ends the segment it is in.
    for (var i = 0u; i < n; i++) {
        if t <= p.at[i] {
            if i == 0u {
                rgb = colour(0u);
            } else {
                let span = max(p.at[i] - p.at[i - 1u], 1e-6);
                rgb = mix(colour(i - 1u), colour(i), (t - p.at[i - 1u]) / span);
            }
            break;
        }
    }
    return vec4f(rgb, 1.0);
}
