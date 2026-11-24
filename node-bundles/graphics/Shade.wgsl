/* goofi
{ "doc": "a picture's light read as a height map\nThe slopes of `height`'s luminance, read across `reach` (a share of the frame's height), light the `input` from a sun at `angle`, as relief, or come out alone. relief multiplies the input by ambient + slope toward the sun; with no input wired it gives that light as grey. light is the grey alone. slopes writes the slope map Displace reads, red the horizontal slope and green the vertical, 0.5 where the picture is flat, so Displace pushes a texture along the picture's forms.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "height", "kind": "TEXTURE"}, {"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "shade", "name": "mode", "kind": "str", "default": "relief", "options": ["relief", "light", "slopes"],
     "doc": "relief lights the input, light is the lighting alone, slopes is the map for Displace."},
    {"group": "shade", "name": "angle", "kind": "num", "default": 0.9, "min": 0.0, "max": 6.283,
     "doc": "Where the sun is, all the way round: 0 on the right, 1.57 below.",
     "show": {"param": "mode", "any_of": ["relief", "light"]}},
    {"group": "shade", "name": "ambient", "kind": "num", "default": 0.55, "min": 0.0, "max": 2.0,
     "doc": "Light that falls everywhere. Higher flattens the relief.",
     "show": {"param": "mode", "any_of": ["relief", "light"]}},
    {"group": "shade", "name": "gain", "kind": "num", "default": 12.0, "min": 0.0, "max": 64.0,
     "doc": "How strongly a slope counts. A soft picture needs more."},
    {"group": "shade", "name": "reach", "kind": "num", "default": 0.008, "min": 0.001, "max": 0.05,
     "doc": "How far apart the slope is read, as a share of the frame's height. Wider reads only the large forms."} ] }
*/
const LUMA = vec3f(0.299, 0.587, 0.114);

fn height_at(uv: vec2f) -> f32 {
    return dot(textureSampleLevel(height, samp, uv, 0.0).rgb, LUMA);
}

/// The slope of the height's light, as the change across one reach, smoothed across the other axis.
fn slope(uv: vec2f) -> vec2f {
    let size = vec2f(textureDimensions(height));
    let r = vec2f(p.reach * size.y / size.x, p.reach);
    var g = vec2f(0.0);
    for (var j = -1; j <= 1; j++) {
        let w = select(0.25, 0.5, j == 0);
        let across = f32(j) * r;
        g.x += w * (height_at(uv + vec2f(r.x, across.y)) - height_at(uv - vec2f(r.x, -across.y)));
        g.y += w * (height_at(uv + vec2f(across.x, r.y)) - height_at(uv - vec2f(-across.x, r.y)));
    }
    return 0.5 * g;
}

fn shade(uv: vec2f) -> vec4f {
    let g = slope(uv) * p.gain;
    if p.mode == 2u {
        return vec4f(0.5 + 0.5 * g, 0.5, 1.0);
    }
    let lit = clamp(p.ambient + 0.45 * dot(vec2f(cos(p.angle), sin(p.angle)), g), 0.0, 1.7);
    let c = textureSampleLevel(input, samp, uv, 0.0);
    let size = textureDimensions(input);
    if p.mode == 1u || (size.x <= 1u && size.y <= 1u) {
        return vec4f(vec3f(lit), 1.0);
    }
    return vec4f(c.rgb * lit, c.a);
}
