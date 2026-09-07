/* goofi
{ "doc": "a signal frame as a texture\nThe door in from the rest of the patch. `input/mode` says what the frame becomes: its own texels — [N] is one row, [H, W] is gray, [H, W, C] keeps the channels it has — or the line plot or the trajectory a viewer would draw of it. Set common/width and height, or it is 512 square.",
  "tags": ["image", "generator"],
  "inputs": [{"name": "input", "kind": "ARRAY"}] }
*/
fn shade(uv: vec2f) -> vec4f {
    return textureSample(input, samp, uv);
}
