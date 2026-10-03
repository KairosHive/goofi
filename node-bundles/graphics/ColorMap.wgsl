/* goofi
{ "doc": "one channel of a texture through colour stops\nThe key reads one value off each texel: its luminance, or one of its channels. That value runs through the stops as a gradient does, and the texel takes the colour it lands on; its own alpha scales the stop's.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "map", "name": "key", "kind": "str", "default": "luminance",
     "options": ["luminance", "red", "green", "blue", "alpha", "maximum", "minimum"]},
    {"group": "map", "section": "stops", "doc": "how many colour stops the map runs through",
     "repeat": {"min": 2, "max": 16, "default": 2},
     "params": [
       {"name": "at", "kind": "num", "defaults": [0.0, 1.0], "min": 0.0, "max": 1.0},
       {"name": "colour", "kind": "color", "defaults": [[0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]]} ]} ] }
*/
fn key(c: vec4f) -> f32 {
    switch p.key {
        case 1u: { return c.r; }
        case 2u: { return c.g; }
        case 3u: { return c.b; }
        case 4u: { return c.a; }
        case 5u: { return max(max(c.r, c.g), c.b); }
        case 6u: { return min(min(c.r, c.g), c.b); }
        default: { return dot(c.rgb, vec3f(0.299, 0.587, 0.114)); }
    }
}

fn shade(uv: vec2f) -> vec4f {
    let c = textureSample(input, samp, uv);
    let t = clamp(key(c), 0.0, 1.0);
    let n = u32(max(p.stops, 1));
    var rgba = p.colour[n - 1u];
    // The stops run in declared order; the first one at or past `t` ends the segment it is in.
    for (var i = 0u; i < n; i++) {
        if t <= p.at[i] {
            if i == 0u {
                rgba = p.colour[0u];
            } else {
                let span = max(p.at[i] - p.at[i - 1u], 1e-6);
                rgba = mix(p.colour[i - 1u], p.colour[i], (t - p.at[i - 1u]) / span);
            }
            break;
        }
    }
    return vec4f(rgba.rgb, rgba.a * c.a);
}
