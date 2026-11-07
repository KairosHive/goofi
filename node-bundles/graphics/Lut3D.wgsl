/* goofi
{ "doc": "colours through a 3D lookup table\nEach texel's colour is looked up in `lut`, an `[N, N*N, 4]` table: green down its rows, then blue slices side by side, red across each slice. ColorLut writes one from two images or reads one from a `.cube` file. `lut2` and `morph` cross-fade to a second table, and `amount` mixes the result with the input's own colour. With no table wired the input passes through.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}, {"name": "lut", "kind": "ARRAY"}, {"name": "lut2", "kind": "ARRAY"}],
  "params": [
    {"group": "lut3d", "name": "amount", "kind": "num", "default": 1.0, "min": 0.0, "max": 1.0,
     "doc": "How much of the looked-up colour: 0 is the input as it came, 1 the table's colour."},
    {"group": "lut3d", "name": "morph", "kind": "num", "default": 0.0, "min": 0.0, "max": 1.0,
     "doc": "Between `lut` (0) and `lut2` (1). With one table wired it does nothing."} ] }
*/

/// The table's side, or 0 when what is wired is not a table: a frame `N` rows by `N * N` columns.
fn side(t: texture_2d<f32>) -> i32 {
    let d = vec2i(textureDimensions(t));
    if d.y >= 2 && d.x == d.y * d.y {
        return d.y;
    }
    return 0;
}

fn entry(t: texture_2d<f32>, n: i32, r: i32, g: i32, b: i32, scale: f32) -> vec3f {
    return textureLoad(t, vec2i(b * n + r, g), 0).rgb * scale;
}

/// The table's colour for `c`, read between its eight nearest entries.
fn looked_up(t: texture_2d<f32>, n: i32, c: vec3f, scale: f32) -> vec3f {
    let x = clamp(c, vec3f(0.0), vec3f(1.0)) * f32(n - 1);
    let lo = vec3i(floor(x));
    let hi = min(lo + vec3i(1), vec3i(n - 1));
    let f = x - vec3f(lo);
    let c00 = mix(entry(t, n, lo.x, lo.y, lo.z, scale), entry(t, n, hi.x, lo.y, lo.z, scale), f.x);
    let c10 = mix(entry(t, n, lo.x, hi.y, lo.z, scale), entry(t, n, hi.x, hi.y, lo.z, scale), f.x);
    let c01 = mix(entry(t, n, lo.x, lo.y, hi.z, scale), entry(t, n, hi.x, lo.y, hi.z, scale), f.x);
    let c11 = mix(entry(t, n, lo.x, hi.y, hi.z, scale), entry(t, n, hi.x, hi.y, hi.z, scale), f.x);
    return mix(mix(c00, c10, f.y), mix(c01, c11, f.y), f.z);
}

fn shade(uv: vec2f) -> vec4f {
    let c = textureSample(input, samp, uv);
    let n1 = side(lut);
    let n2 = side(lut2);
    var graded = c.rgb;
    if n1 > 0 {
        graded = looked_up(lut, n1, c.rgb, p.lut_scale);
    }
    if n2 > 0 {
        let second = looked_up(lut2, n2, c.rgb, p.lut2_scale);
        graded = select(second, mix(graded, second, p.morph), n1 > 0);
    }
    return vec4f(mix(c.rgb, graded, p.amount), c.a);
}
