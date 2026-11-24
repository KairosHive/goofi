/* goofi
{ "doc": "one colour everywhere\nThe simplest source there is: every texel takes the colour below, alpha included.",
  "tags": ["image", "generator"],
  "params": [
    {"group": "constant", "name": "colour", "kind": "color"} ] }
*/
fn shade(uv: vec2f) -> vec4f {
    return p.colour;
}
