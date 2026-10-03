/* goofi
{ "doc": "one shape on transparent black\nSize is the width across as a fraction of the frame, and soft is how far the edge fades. The colour's alpha scales the shape's own.",
  "tags": ["image", "generator"],
  "params": [
    {"group": "shape", "name": "kind", "kind": "str", "default": "circle", "options": ["circle", "square", "ring"]},
    {"group": "shape", "name": "size", "kind": "num", "default": 0.5, "min": 0.0, "max": 1.0},
    {"group": "shape", "name": "soft", "kind": "num", "default": 0.01, "min": 0.0, "max": 0.5},
    {"group": "shape", "name": "colour", "kind": "color"} ] }
*/
fn shade(uv: vec2f) -> vec4f {
    let q = uv - vec2f(0.5);
    let radius = p.size * 0.5;
    var d = length(q) - radius;
    switch p.kind {
        case 1u: {
            let box = abs(q) - vec2f(radius);
            d = length(max(box, vec2f(0.0))) + min(max(box.x, box.y), 0.0);
        }
        case 2u: {
            d = abs(length(q) - radius) - radius * 0.15;
        }
        default: {}
    }
    let soft = max(p.soft, 1e-5);
    let cover = 1.0 - smoothstep(-soft, soft, d);
    // Straight alpha, as every texture carries it: the colour stays, the coverage is the alpha.
    return vec4f(p.colour.rgb, p.colour.a * cover);
}
