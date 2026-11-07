/* goofi
{ "doc": "one channel of a texture through a colormap\nEach texel's channel value, clamped to 0..1, picks its colour on the map. The texel's own alpha scales the colour's alpha.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "map", "name": "channel", "kind": "str", "default": "luminance",
     "options": ["luminance", "red", "green", "blue", "alpha"],
     "doc": "The channel that picks the colour."},
    {"group": "map", "section": "stops", "widget": "colormap", "doc": "the stops of the colormap",
     "repeat": {"min": 2, "max": 16, "default": 2}} ] }
*/
fn channel(c: vec4f) -> f32 {
    switch p.channel {
        case 1u: { return c.r; }
        case 2u: { return c.g; }
        case 3u: { return c.b; }
        case 4u: { return c.a; }
        default: { return dot(c.rgb, vec3f(0.299, 0.587, 0.114)); }
    }
}

fn shade(uv: vec2f) -> vec4f {
    let c = textureSample(input, samp, uv);
    let t = clamp(channel(c), 0.0, 1.0);
    let n = u32(max(p.stops, 1));
    var rgba = p.colour[n - 1u];
    // The stops run in declared order; the first one at or past `t` ends the segment it is in.
    for (var i = 0u; i < n; i++) {
        if t <= p.at[i] {
            if i == 0u {
                rgba = p.colour[0u];
            } else {
                let span = max(p.at[i] - p.at[i - 1u], 1e-6);
                var f = (t - p.at[i - 1u]) / span;
                switch p.blend[i - 1u] {
                    case 1u: { f = smoothstep(0.0, 1.0, f); }
                    case 2u: { f = 0.0; }
                    default: {}
                }
                rgba = mix(p.colour[i - 1u], p.colour[i], f);
            }
            break;
        }
    }
    return vec4f(rgba.rgb, rgba.a * c.a);
}
