/* goofi
{ "doc": "Color and contours for a signed harmonic field.\nWire HarmonicChladni.out, or upload GeometryView.field/HarmonicTransport.field through SignalIn. The raw field is unchanged. BioColors.rgb can feed palette directly as an [N,3] array. Alpha preserves domain masks. range is fixed, so morphs do not silently change their contrast.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}, {"name": "palette", "kind": "ARRAY"}],
  "params": [
    {"group": "ink", "name": "style", "kind": "str", "default": "nodal", "options": ["nodal", "signed", "magnitude", "contours"], "doc": "Zero lines, signed field, absolute value, or topographic contours."},
    {"group": "ink", "name": "range", "kind": "float", "default": 1.0, "min": 0.001, "max": 100.0, "doc": "Absolute field value that fills the color range."},
    {"group": "ink", "name": "width", "kind": "float", "default": 0.045, "min": 0.001, "max": 0.4, "doc": "Nodal width relative to field range."},
    {"group": "ink", "name": "contours", "kind": "float", "default": 8.0, "min": 1.0, "max": 40.0},
    {"group": "tone", "name": "warmth", "kind": "float", "default": 0.45, "min": 0.0, "max": 1.0},
    {"group": "tone", "name": "exposure", "kind": "float", "default": 1.6, "min": 0.1, "max": 6.0}
  ] }
*/

fn color_at(t: f32) -> vec3f {
    let dims = textureDimensions(palette);
    if dims.x == 3u && dims.y >= 1u {
        let at = clamp(t, 0.0, 1.0) * f32(dims.y - 1u);
        let a = i32(floor(at)); let b = min(a + 1, i32(dims.y) - 1);
        let ca = vec3f(textureLoad(palette, vec2i(0, a), 0).r, textureLoad(palette, vec2i(1, a), 0).r, textureLoad(palette, vec2i(2, a), 0).r);
        let cb = vec3f(textureLoad(palette, vec2i(0, b), 0).r, textureLoad(palette, vec2i(1, b), 0).r, textureLoad(palette, vec2i(2, b), 0).r);
        return max(mix(ca, cb, fract(at)), vec3f(0.0));
    }
    let dark = vec3f(0.006, 0.016, 0.031);
    let cool = vec3f(0.035, 0.55, 0.53);
    let warm = vec3f(1.0, 0.46, 0.14);
    let hue = mix(cool, warm, p.warmth);
    return mix(dark, hue, smoothstep(0.0, 0.65, t)) + vec3f(0.78, 0.85, 0.67) * pow(max(t, 0.0), 5.0);
}

fn shade(uv: vec2f) -> vec4f {
    let sample = textureSample(input, samp, uv);
    let v = sample.r / p.range;
    var tone = exp(-0.5 * v * v / (p.width * p.width));
    switch p.style {
        case 1u: { tone = clamp(0.5 + 0.5 * v, 0.0, 1.0); }
        case 2u: { tone = clamp(abs(v), 0.0, 1.0); }
        case 3u: { tone = pow(0.5 + 0.5 * cos(v * 6.28318530718 * p.contours), 10.0); }
        default: {}
    }
    let color = 1.0 - exp(-color_at(tone) * p.exposure);
    return vec4f(pow(max(color, vec3f(0.0)), vec3f(1.0 / 2.2)), sample.a);
}
