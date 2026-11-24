/* goofi
{ "doc": "show this texture in a window on the machine goofi runs on\nThe window is the frame's own size — set common/width and height to resize it. Screen picks the monitor it opens on, 0 the primary and the rest left to right; fullscreen covers that monitor, and the frame takes the monitor's size. It stays a normal node: its output is the same texture, so a viewer or another node can read it too.",
  "tags": ["image", "output"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "window", "name": "fullscreen", "kind": "bool", "default": false},
    {"group": "window", "name": "screen", "kind": "num", "int": true, "default": 0, "min": 0, "max": 8} ],
  "window": true }
*/
fn shade(uv: vec2f) -> vec4f {
    return textureSample(input, samp, uv);
}
