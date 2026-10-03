/* goofi
{ "doc": "brightness to two colours\nBelow the level a texel takes one colour, above it the other. Soft widens the step into a ramp around the level.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "threshold", "name": "level", "kind": "float", "default": 0.5, "min": 0.0, "max": 1.0},
    {"group": "threshold", "name": "soft", "kind": "float", "default": 0.0, "min": 0.0, "max": 0.5},
    {"group": "threshold", "section": "below", "params": [
      {"name": "r", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0},
      {"name": "g", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0},
      {"name": "b", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0} ]},
    {"group": "threshold", "section": "above", "params": [
      {"name": "r", "kind": "float", "default": 1.0, "min": 0.0, "max": 1.0},
      {"name": "g", "kind": "float", "default": 1.0, "min": 0.0, "max": 1.0},
      {"name": "b", "kind": "float", "default": 1.0, "min": 0.0, "max": 1.0} ]} ] }
*/
fn shade(uv: vec2f) -> vec4f {
    let c = textureSample(input, samp, uv);
    let l = dot(c.rgb, vec3f(0.299, 0.587, 0.114));
    // A `smoothstep` with two equal edges divides by zero; a hard step is what soft 0 asks for.
    let soft = max(p.soft, 1e-5);
    let t = smoothstep(p.level - soft, p.level + soft, l);
    return vec4f(mix(vec3f(p.below_r, p.below_g, p.below_b), vec3f(p.above_r, p.above_g, p.above_b), t), c.a);
}
