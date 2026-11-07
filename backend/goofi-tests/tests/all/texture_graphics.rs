//! What the texture nodes hand the GPU: Composite's four non-separable colour modes and its mask, and
//! Shade's light and slopes, each read back as the GPU drew it.

use goofi_tests::{f32s, j, render, shape, Goofi, Uid};

fn drawn(g: &Goofi, uid: Uid, what: &str, want: impl Fn(&goofi_core::Data) -> bool) -> goofi_core::Data {
    let probe = g.probe(uid, "out");
    g.until(what, |g| {
        render(g, 1);
        probe.latest().filter(&want)
    })
}

fn px(d: &goofi_core::Data, row: usize, col: usize) -> [f32; 4] {
    let s = shape(d);
    let v = f32s(d);
    let at = (row * s[1] + col) * 4;
    [v[at], v[at + 1], v[at + 2], v[at + 3]]
}

fn near(a: [f32; 4], b: [f32; 4], tol: f32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < tol)
}

fn constant(g: &Goofi, colour: [f32; 4]) -> Uid {
    let c = g.add("graphics:Constant");
    g.ready(c);
    g.set_param(c, "common", "width", 32);
    g.set_param(c, "common", "height", 32);
    g.set_param(c, "constant", "colour", j!(colour));
    c
}

#[test]
fn composite_wears_colour_and_light_and_reads_a_mask() {
    let g = Goofi::new();
    let red = constant(&g, [1.0, 0.0, 0.0, 1.0]);
    let grey = constant(&g, [0.5, 0.5, 0.5, 1.0]);
    let comp = g.add("graphics:Composite");
    g.ready(comp);
    g.link(red, "out", comp, "a");
    g.link(grey, "out", comp, "b");

    // Step: color keeps B's light (0.5) and wears A's hue and saturation. Red lifted to light 0.5
    // leaves the gamut and is drawn back toward its light: (1, 0.2857, 0.2857).
    g.set_param(comp, "composite", "mode", "color");
    drawn(&g, comp, "grey wearing red", |d| near(px(d, 16, 16), [1.0, 0.2857, 0.2857, 1.0], 2e-3));

    // Step: luminosity keeps B's hue and saturation (none) and takes A's light, 0.3 for red.
    g.set_param(comp, "composite", "mode", "luminosity");
    drawn(&g, comp, "grey at red's light", |d| near(px(d, 16, 16), [0.3, 0.3, 0.3, 1.0], 2e-3));

    // Step: hue on a grey backdrop has no saturation to give, so the light of B comes back grey.
    g.set_param(comp, "composite", "mode", "hue");
    drawn(&g, comp, "hue on grey", |d| near(px(d, 16, 16), [0.5, 0.5, 0.5, 1.0], 2e-3));

    // Step: a black mask leaves B as it was, a white one lets the mode through, a mid-grey one halfway.
    g.set_param(comp, "composite", "mode", "over");
    let mask = constant(&g, [0.0, 0.0, 0.0, 1.0]);
    g.link(mask, "out", comp, "mask");
    drawn(&g, comp, "black mask keeps B", |d| near(px(d, 16, 16), [0.5, 0.5, 0.5, 1.0], 2e-3));
    g.set_param(mask, "constant", "colour", j!([1.0, 1.0, 1.0, 1.0]));
    drawn(&g, comp, "white mask lets A over", |d| near(px(d, 16, 16), [1.0, 0.0, 0.0, 1.0], 2e-3));
    g.set_param(mask, "constant", "colour", j!([0.5, 0.5, 0.5, 1.0]));
    drawn(&g, comp, "grey mask halfway", |d| near(px(d, 16, 16), [0.75, 0.25, 0.25, 1.0], 5e-3));
}

#[test]
fn shade_lights_by_slope_and_writes_the_map_displace_reads() {
    let g = Goofi::new();
    let flat = constant(&g, [0.4, 0.4, 0.4, 1.0]);
    let shade = g.add("graphics:Shade");
    g.ready(shade);
    g.link(flat, "out", shade, "height");

    // Step: a flat height has no slope. Unwired input, relief gives the ambient light as grey, and
    // slopes gives 0.5, the no-push of Displace.
    g.set_param(shade, "shade", "ambient", 0.55);
    drawn(&g, shade, "ambient on flat", |d| near(px(d, 16, 16), [0.55, 0.55, 0.55, 1.0], 2e-3));
    g.set_param(shade, "shade", "mode", "slopes");
    drawn(&g, shade, "no push on flat", |d| near(px(d, 16, 16), [0.5, 0.5, 0.5, 1.0], 2e-3));

    // Step: a ramp brightening left to right rises toward the right. Its slope map pushes right
    // (red above 0.5, green at 0.5), and a sun at angle 0 lights it above ambient, at pi below.
    let ramp = g.add("graphics:Gradient");
    g.ready(ramp);
    g.set_param(ramp, "common", "width", 64);
    g.set_param(ramp, "common", "height", 64);
    g.link(ramp, "out", shade, "height");
    let slope = drawn(&g, shade, "a rightward slope", |d| shape(d) == vec![64, 64, 4] && px(d, 32, 32)[0] > 0.52);
    assert!((px(&slope, 32, 32)[1] - 0.5).abs() < 2e-3, "no vertical slope: {:?}", px(&slope, 32, 32));
    // The ramp climbs 1/64 a texel, so the light is read at a gain that makes that slope count.
    g.set_param(shade, "shade", "mode", "light");
    g.set_param(shade, "shade", "gain", 40.0);
    g.set_param(shade, "shade", "angle", 0.0);
    drawn(&g, shade, "lit from the slope's side", |d| px(d, 32, 32)[0] > 0.6);
    g.set_param(shade, "shade", "angle", 3.14159);
    drawn(&g, shade, "dark from the other side", |d| px(d, 32, 32)[0] < 0.5);

    // Step: relief multiplies a wired input by that light, keeping the input's alpha.
    let tint = constant(&g, [1.0, 0.5, 0.25, 0.75]);
    g.link(tint, "out", shade, "input");
    g.set_param(shade, "shade", "mode", "relief");
    g.set_param(flat, "constant", "colour", j!([0.4, 0.4, 0.4, 1.0]));
    g.link(flat, "out", shade, "height");
    drawn(&g, shade, "the input at ambient", |d| near(px(d, 16, 16), [0.55, 0.275, 0.1375, 0.75], 3e-3));
}
