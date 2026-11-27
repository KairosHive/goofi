// Matches goofi.geometry: one indexed ARRAY, tiled at 256 RGBA texels per row.
fn goofi_geo_texel(data: texture_2d<f32>, index: u32) -> vec4f {
    return textureLoad(data, vec2i(i32(index % 256u), i32(index / 256u)), 0);
}

fn goofi_geo_uint(pair: vec2f) -> u32 { return u32(pair.x)*1024u+u32(pair.y); }

fn goofi_geo_header(data: texture_2d<f32>, index: u32) -> u32 {
    let value = goofi_geo_texel(data, index/2u);
    return goofi_geo_uint(select(value.xy, value.zw, index % 2u == 1u));
}

fn goofi_geo_valid(data: texture_2d<f32>) -> bool {
    let dims = textureDimensions(data);
    if dims.x != 256u || dims.y == 0u { return false; }
    for (var i = 0u; i < 8u; i++) {
        let v = goofi_geo_texel(data, i);
        if any(v < vec4f(0.0)) || any(v >= vec4f(1024.0)) || any(v != floor(v)) { return false; }
    }
    let count = goofi_geo_header(data, 3u);
    let parts = goofi_geo_header(data, 6u);
    let edges = goofi_geo_header(data, 7u);
    let faces = goofi_geo_header(data, 8u);
    let weights = goofi_geo_header(data, 9u);
    let grid = goofi_geo_header(data, 11u);
    let total = 8u+count+(parts+1u)/2u+edges+faces*2u+(weights+3u)/4u+(grid+3u)/4u;
    return goofi_geo_header(data, 0u) == 7319u && goofi_geo_header(data, 1u) <= 12u
        && total == goofi_geo_header(data, 12u) && total <= dims.x*dims.y;
}

fn goofi_geo_field(data: texture_2d<f32>, uv: vec2f) -> vec4f {
    if !goofi_geo_valid(data) { return vec4f(0.0); }
    let kind = goofi_geo_header(data, 1u);
    let h = goofi_geo_header(data, 4u); let w = goofi_geo_header(data, 5u);
    if kind < 11u || h == 0u || w == 0u || h*w != goofi_geo_header(data, 3u) { return vec4f(0.0); }
    let at = clamp(uv*vec2f(f32(w), f32(h))-0.5, vec2f(0.0), vec2f(f32(w-1u), f32(h-1u)));
    let a = vec2u(floor(at)); let b = min(a+1u, vec2u(w-1u, h-1u));
    let t = fract(at);
    return mix(mix(goofi_geo_texel(data, 8u+a.y*w+a.x), goofi_geo_texel(data, 8u+a.y*w+b.x), t.x),
               mix(goofi_geo_texel(data, 8u+b.y*w+a.x), goofi_geo_texel(data, 8u+b.y*w+b.x), t.x), t.y);
}
