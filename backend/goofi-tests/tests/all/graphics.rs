//! The graphics engine under the external clock: one session, every action through the op
//! vocabulary, every probe a subscriber on the derived name of a texture slot — the door `/data`
//! opens — and every frame a readback the GPU actually made.

use std::sync::Arc;

use goofi_tests::{drive, ep, f32s, hex, j, render, shape, Goofi, Uid, Viewer};

const NO_GPU: &str = "no graphics engine here. The suite needs a GPU adapter: install a Vulkan \
                      driver, or Mesa's lavapipe (`mesa-vulkan-drivers`)";

/// Tick until the probe on `uid`'s output holds a frame `want` accepts, and hand it back.
fn drawn(g: &Goofi, uid: Uid, what: &str, want: impl Fn(&goofi_core::Data) -> bool) -> goofi_core::Data {
    let probe = g.probe(uid, "out");
    g.until(what, |g| {
        render(g, 1);
        probe.latest().filter(&want)
    })
}

/// The texel at `(row, col)`: four floats, row 0 the top. Read off the bytes in place: a 1024²
/// frame is 16 MB, and decoding all of it to reach one texel is what made this suite slow.
fn px(d: &goofi_core::Data, row: usize, col: usize) -> [f32; 4] {
    let s = shape(d);
    let goofi_core::Value::Array(a) = d.value() else { panic!("not an array: {d:?}") };
    let at = (row * s[1] + col) * 16;
    let bytes = &a.as_bytes()[at..at + 16];
    std::array::from_fn(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("four bytes")))
}

/// The row of the brightest texel in each column, and none where a column was left alone.
fn ridge(d: &goofi_core::Data) -> Vec<Option<usize>> {
    let s = shape(d);
    let v = f32s(d);
    let alpha = |row: usize, col: usize| v[(row * s[1] + col) * 4 + 3];
    (0..s[1])
        .map(|col| {
            let lit = (0..s[0]).max_by(|a, b| alpha(*a, col).total_cmp(&alpha(*b, col)));
            lit.filter(|row| alpha(*row, col) > 0.5)
        })
        .collect()
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

const TEXTY: &str = "/* goofi\n{ \"doc\": \"claims a string slot\", \"inputs\": [{\"name\": \"input\", \"kind\": \"STRING\"}] }\n*/\nfn shade(uv: vec2f) -> vec4f { return vec4f(uv, 0.0, 1.0); }\n";

const TINTED: &str = "/* goofi\n{ \"doc\": \"a tint that shows its hue\", \"params\": [{\"group\": \"look\", \"name\": \"tint\", \"kind\": \"bool\", \"default\": false}, {\"group\": \"look\", \"section\": \"colour\", \"params\": [{\"name\": \"hue\", \"kind\": \"num\", \"default\": 0.5, \"min\": 0.0, \"max\": 1.0, \"show\": {\"param\": \"tint\", \"any_of\": [\"true\"]}}]}] }\n*/\nfn shade(uv: vec2f) -> vec4f { return vec4f(uv, 0.0, 1.0); }\n";

/// A colour param, read whole by the shader.
const INKED: &str = "/* goofi\n{ \"doc\": \"one ink\", \"params\": [{\"group\": \"look\", \"name\": \"ink\", \"kind\": \"color\", \"default\": [1.0, 0.0, 0.0, 1.0]}] }\n*/\nfn shade(uv: vec2f) -> vec4f { return p.ink; }\n";

/// A list section whose `max` is past what a shader keeps.
const OVERLONG: &str = "/* goofi\n{ \"doc\": \"too many\", \"params\": [{\"group\": \"look\", \"section\": \"many\", \"repeat\": {\"min\": 1, \"max\": 100, \"default\": 1}, \"params\": [{\"name\": \"v\", \"kind\": \"num\", \"default\": 0.0, \"min\": 0.0, \"max\": 1.0}]}] }\n*/\nfn shade(uv: vec2f) -> vec4f { return vec4f(uv, 0.0, 1.0); }\n";

const BROKEN: &str = "/* goofi\n{ \"doc\": \"does not compile\" }\n*/\nfn shade(uv: vec2f) -> vec4f { return nothing(uv); }\n";

const COUNT: &str = "/* goofi\n{ \"doc\": \"counts a tenth a tick in a buffer of its own\", \"state\": [\"acc\"] }\n*/\nfn at(uv: vec2f) -> vec2i { return vec2i(floor(uv * resolution)); }\nfn next_acc(uv: vec2f) -> vec4f {\n    if frame == 0u { return vec4f(0.1, 0.75, 0.0, 1.0); }\n    let held = textureLoad(acc, at(uv), 0);\n    return vec4f(held.r + 0.1, held.g, 0.0, 1.0);\n}\nfn shade(uv: vec2f) -> vec4f { return vec4f(textureLoad(acc, at(uv), 0).rgb, 1.0); }\n";

const HALF: &str = "/* goofi\n{ \"doc\": \"half of the input\", \"inputs\": [{\"name\": \"input\", \"kind\": \"TEXTURE\"}] }\n*/\nfn shade(uv: vec2f) -> vec4f { let c = textureSample(input, samp, uv); return vec4f(c.rgb * 0.5, c.a); }\n";

const QUARTER: &str = "/* goofi\n{ \"doc\": \"a quarter of the input\", \"inputs\": [{\"name\": \"input\", \"kind\": \"TEXTURE\"}] }\n*/\nfn shade(uv: vec2f) -> vec4f { let c = textureSample(input, samp, uv); return vec4f(c.rgb * 0.25, c.a); }\n";

#[test]
fn shaders_render_on_the_gpu() {
    let g = Goofi::new();

    // Step: the engine registered, and its library is in the ONE palette beside the others.
    let types = g.call("library list", j!({ "full": true }));
    let names: Vec<&str> = types["types"].as_array().unwrap().iter().filter_map(|r| r["type"].as_str()).collect();
    assert!(names.contains(&"graphics:Constant"), "{NO_GPU}\n{names:?}");
    let row = types["types"].as_array().unwrap().iter().find(|r| r["type"] == "graphics:Constant").unwrap();
    assert_eq!(row["output_slots"]["out"], "TEXTURE", "{row}");
    let got = g.call("library get", j!({ "type": "graphics:Constant" }));
    assert_eq!((&got["tier"], &got["language"]), (&j!("shader"), &j!("wgsl")), "{got}");
    let status = g.call("session status", j!({}))["graphics"].clone();
    assert_eq!(status["clock"], "external", "{status}");
    assert!(status["adapter"].as_str().is_some_and(|a| !a.is_empty()), "{status}");

    // Step: a Constant reads back the colour it was given, at the generator's own size.
    let c = g.add("graphics:Constant");
    g.ready(c);
    g.set_param(c, "constant", "colour", j!([0.25, 0.5, 1.0, 1.0]));
    let frame = drawn(&g, c, "the constant's colour", |d| close(px(d, 0, 0), [0.25, 0.5, 1.0, 1.0]));
    assert_eq!(shape(&frame), vec![1024, 1024, 4], "a node with nothing behind it is 1024 square");
    assert!(close(px(&frame, 511, 511), [0.25, 0.5, 1.0, 1.0]), "the same colour to the far corner");

    // Step: the universal `common` group resizes it, and what is wired behind FOLLOWS the size.
    g.set_param(c, "common", "width", 64);
    g.set_param(c, "common", "height", 32);
    let frame = drawn(&g, c, "the resized frame", |d| shape(d) == vec![32, 64, 4]);
    assert!(close(px(&frame, 31, 63), [0.25, 0.5, 1.0, 1.0]));
    // Step: a pad's sheet reaches the GPU through a Variable node: the `[H, W, 4]` variable is the
    // texture, top row first, so the red painted over the top half is red there and clear below.
    g.call("control add", j!({ "group": "paint", "kind": "paint", "element": "pad", "resolution": 4 }));
    g.call("control paint", j!({ "group": "paint", "element": "pad", "ops": "fill #ff0000 : M 0 0 L 1000 0 L 1000 500 L 0 500 Z" }));
    let var = g.add("graphics:Variable");
    g.ready(var);
    g.set_param(var, "variable", "group", "paint");
    g.set_param(var, "variable", "element", "pad");
    drawn(&g, var, "the pad as a texture", |d| shape(d) == vec![4, 4, 4] && close(px(d, 0, 0), [1.0, 0.0, 0.0, 1.0]) && px(d, 3, 3)[3] < 0.01);
    g.call("node remove", j!({ "node": hex(var) }));
    g.call("variable entry remove", j!({ "name": "paint.pad" }));

    // Step: a Shape's colour is RGBA with STRAIGHT alpha: its tint is untouched inside, and the
    // coverage scales the colour's own alpha, which is nothing outside.
    let disc = g.add("graphics:Shape");
    g.ready(disc);
    g.set_param(disc, "common", "width", 64);
    g.set_param(disc, "common", "height", 64);
    g.set_param(disc, "shape", "colour", j!([1.0, 0.5, 0.0, 0.5]));
    drawn(&g, disc, "a half-clear orange disc", |d| close(px(d, 32, 32), [1.0, 0.5, 0.0, 0.5]) && px(d, 0, 0)[3] < 0.01);
    g.call("node remove", j!({ "node": hex(disc) }));

    // Step: Threshold takes two RGBA colours, and EACH carries its alpha onto the texel: the
    // constant's luminance is 0.48, so the level picks the side.
    let th = g.add("graphics:Threshold");
    g.ready(th);
    g.link(c, "out", th, "input");
    g.set_param(th, "threshold", "below", j!([1.0, 0.0, 0.0, 0.5]));
    g.set_param(th, "threshold", "above", j!([0.0, 0.0, 1.0, 0.25]));
    g.set_param(th, "threshold", "level", 0.3);
    drawn(&g, th, "above: quarter-clear blue", |d| close(px(d, 0, 0), [0.0, 0.0, 1.0, 0.25]));
    g.set_param(th, "threshold", "level", 0.6);
    drawn(&g, th, "below: half-clear red", |d| close(px(d, 0, 0), [1.0, 0.0, 0.0, 0.5]));
    g.call("node remove", j!({ "node": hex(th) }));

    // Step: ColorMap runs one key of the texel through colour stops; the alpha key reads 1 here,
    // the red key 0.25, and the stop's alpha rides on the texel's own.
    let map = g.add("graphics:ColorMap");
    g.ready(map);
    g.link(c, "out", map, "input");
    g.set_param(map, "map", "colour_0", j!([0.0, 1.0, 0.0, 1.0]));
    g.set_param(map, "map", "colour_1", j!([1.0, 0.0, 1.0, 0.5]));
    g.set_param(map, "map", "key", "alpha");
    drawn(&g, map, "alpha 1 lands on the last stop", |d| close(px(d, 0, 0), [1.0, 0.0, 1.0, 0.5]));
    g.set_param(map, "map", "key", "red");
    drawn(&g, map, "red 0.25 is a quarter of the way", |d| close(px(d, 0, 0), [0.25, 0.75, 0.25, 0.875]));
    g.call("node remove", j!({ "node": hex(map) }));

    let level = g.add("graphics:Level");
    g.ready(level);
    g.link(c, "out", level, "input");
    let frame = drawn(&g, level, "the level follows its input's size", |d| shape(d) == vec![32, 64, 4]);
    assert!(close(px(&frame, 0, 0), [0.25, 0.5, 1.0, 1.0]), "gain 1 is a copy");

    // Step: a chain — the format is HDR, so doubling a value past 1 keeps what it made.
    g.set_param(level, "level", "gain", 2.0);
    drawn(&g, level, "the doubled frame", |d| close(px(d, 5, 5), [0.5, 1.0, 2.0, 1.0]));
    g.set_param(level, "level", "gain", 1.0);

    // Step: an unwired texture input is transparent black — present, never an error.
    g.call("link remove", j!({ "from": ep(hex(c), "out"), "to": ep(hex(level), "input") }));
    let frame = drawn(&g, level, "the unwired level", |d| close(px(d, 0, 0), [0.0, 0.0, 0.0, 0.0]));
    assert_eq!(shape(&frame), vec![1024, 1024, 4], "with nothing to follow, it is a generator's size");
    assert!(g.error(level).is_none(), "an unwired input is not a fault");
    // …and wired straight back: unwired it is a 1024-square generator, which every later step
    // would then redraw and read back on every poll of its own.
    g.link(c, "out", level, "input");

    // Step: a signal frame uploads — the fixture's gradient, sampled back texel for texel and the
    // right way up. The one place the two row orders could disagree.
    let img = g.add("_TestImage");
    g.ready(img);
    let up = g.add("graphics:SignalIn");
    g.ready(up);
    // A door for frames follows the frame: its size rests at a constant 0, not the patch's default.
    let width = g.doc()["nodes"][hex(up)]["params"]["common"]["width"].clone();
    assert_eq!(width, j!({ "value": 0 }), "the size is a constant 0, not a seeded expression");
    g.set_param(up, "common", "width", 4);
    g.set_param(up, "common", "height", 4);
    g.link(img, "out", up, "input");
    // Waited to the upload itself: the resize can draw before the signal frame lands, and that
    // frame is the right size and empty.
    let frame = drawn(&g, up, "the uploaded image", |d| shape(d) == vec![4, 4, 4] && px(d, 0, 0) != [0.0; 4]);
    assert!(close(px(&frame, 0, 0), [0.0, 1.0, 0.5, 1.0]), "row 0 is the top: {:?}", px(&frame, 0, 0));
    assert!(close(px(&frame, 3, 3), [1.0, 0.0, 0.5, 1.0]), "and row 3 the bottom: {:?}", px(&frame, 3, 3));

    g.call("link remove", j!({ "from": ep(hex(img), "out"), "to": ep(hex(up), "input") }));
    drawn(&g, up, "the unlinked upload", |d| close(px(d, 0, 0), [0.0, 0.0, 0.0, 0.0]));

    // Step: the transfer has MODES. The frame's own texels is one of them and the default; the
    // others draw the frame the way goofi's viewers draw it, at the node's own size.
    let ramp = g.add("_TestRamp");
    g.ready(ramp);
    g.set_param(ramp, "ramp", "length", 64);
    g.link(ramp, "out", up, "input");
    g.set_param(up, "common", "width", 64);
    g.set_param(up, "common", "height", 32);
    let frame = drawn(&g, up, "the raw [1, 64] frame", |d| shape(d) == vec![32, 64, 4] && px(d, 0, 0)[3] == 1.0);
    assert!(close(px(&frame, 0, 0), [0.267, 0.004, 0.329, 1.0]), "a one-channel frame reads through the colormap, its floor at viridis's: {:?}", px(&frame, 0, 0));
    assert!(close(px(&frame, 0, 63), [0.992, 0.906, 0.145, 1.0]), "and its ceiling at the top: {:?}", px(&frame, 0, 63));
    g.set_param(up, "signal", "colormap", "gray");
    let frame = drawn(&g, up, "the gray frame", |d| close(px(d, 0, 0), [0.0, 0.0, 0.0, 1.0]));
    assert!(close(px(&frame, 0, 63), [1.0, 1.0, 1.0, 1.0]), "gray is the value itself: {:?}", px(&frame, 0, 63));
    // Off autoscale, the range is the one asked for: the ramp's last step, 63/64, on a window
    // to 2 is half that gray.
    g.set_param(up, "signal", "autoscale", false);
    g.set_param(up, "signal", "min", 0.0);
    g.set_param(up, "signal", "max", 2.0);
    let half = 63.0 / 128.0;
    let frame = drawn(&g, up, "the windowed frame", |d| close(px(d, 0, 63), [half, half, half, 1.0]));
    assert!(close(px(&frame, 0, 0), [0.0, 0.0, 0.0, 1.0]), "from the floor: {:?}", px(&frame, 0, 0));
    g.set_param(up, "signal", "autoscale", true);
    // A size left at 0 follows the frame: one row of 64, the shape of what came in.
    g.set_param(up, "common", "width", 0);
    g.set_param(up, "common", "height", 0);
    drawn(&g, up, "the frame at its own size", |d| shape(d) == vec![1, 64, 4]);
    g.set_param(up, "common", "width", 64);
    g.set_param(up, "common", "height", 32);

    g.set_param(up, "signal", "mode", "line");
    g.set_param(up, "signal", "autoscale", false);
    g.set_param(up, "signal", "min", 0.0);
    g.set_param(up, "signal", "max", 1.0);
    let plot = drawn(&g, up, "the line plot", |d| {
        px(d, 0, 0)[3] == 0.0 && ridge(d).iter().all(Option::is_some)
    });
    assert_eq!(shape(&plot), vec![32, 64, 4], "a drawing is made at the node's own size");
    let path = ridge(&plot);
    let (first, last) = (path[0].expect("ink"), path[63].expect("ink"));
    assert!(first > 28 && last < 3, "the ramp rises from the floor to the ceiling: {first} to {last}");
    assert!(path.windows(2).all(|w| w[1] <= w[0]), "and never turns back down: {path:?}");
    assert!(close(px(&plot, 0, 0), [0.0, 0.0, 0.0, 0.0]), "on transparent ground: {:?}", px(&plot, 0, 0));
    let ink = px(&plot, last, 63);
    assert!(ink[2] > ink[1] && ink[1] > ink[0], "drawn in the viewers' first series colour: {ink:?}");

    // …and the same frame with two channels is a trajectory: one against the other, the range
    // shared so the shape is not distorted.
    g.set_param(up, "signal", "mode", "trajectory");
    g.set_param(up, "signal", "autoscale", true);
    g.set_param(ramp, "ramp", "channels", 2);
    let traj = drawn(&g, up, "the trajectory", |d| {
        let lit = ridge(d).iter().filter(|c| c.is_some()).count();
        px(d, 0, 0)[3] == 0.0 && (8..60).contains(&lit)
    });
    let drawn_cols: Vec<usize> = ridge(&traj).iter().enumerate().filter(|(_, c)| c.is_some()).map(|(i, _)| i).collect();
    let (left, right) = (drawn_cols[0], drawn_cols[drawn_cols.len() - 1]);
    let (at_left, at_right) = (ridge(&traj)[left].expect("ink"), ridge(&traj)[right].expect("ink"));
    assert!(at_right < at_left, "channel 1 against channel 0 climbs: {at_left} to {at_right}");
    assert!(right < 63, "and both axes share one range, so it does not fill the width: {right}");

    // …and channels named for 10-20 electrodes are a topomap: the field between them on the head
    // disc, transparent off it, under the outline — and nothing for channels without names.
    g.set_param(up, "signal", "mode", "topomap");
    g.set_param(up, "signal", "colormap", "gray");
    g.set_param(up, "signal", "electrodes", false);
    g.set_param(up, "common", "width", 64);
    g.set_param(up, "common", "height", 64);
    g.set_param(ramp, "ramp", "channels", 3);
    g.set_param(ramp, "ramp", "named", true);
    let head = drawn(&g, up, "the topomap", |d| shape(d) == vec![64, 64, 4] && px(d, 32, 32)[3] == 1.0 && px(d, 32, 3)[3] > 0.9);
    assert!(close(px(&head, 0, 0), [0.0, 0.0, 0.0, 0.0]), "off the head is transparent: {:?}", px(&head, 0, 0));
    // Cz, C3 and C4 rise in that order, so the field reads brighter from the centre to the left
    // and brightest on the right.
    let (centre, left, right) = (px(&head, 32, 32)[0], px(&head, 32, 17)[0], px(&head, 32, 47)[0]);
    assert!(centre < left && left < right, "Cz under C3 under C4: {centre} {left} {right}");
    let rim = px(&head, 32, 3);
    assert!(rim[0] > 0.7 && rim[2] > rim[0], "the outline in the viewers' head grey: {rim:?}");
    g.set_param(up, "signal", "electrodes", true);
    drawn(&g, up, "the electrodes", |d| px(d, 32, 17)[0] < 0.1 && px(d, 32, 17)[3] == 1.0);
    g.set_param(ramp, "ramp", "named", false);
    drawn(&g, up, "the unnamed topomap", |d| px(d, 32, 32)[3] == 0.0 && px(d, 32, 3)[3] == 0.0);
    g.set_param(up, "common", "height", 32);

    g.set_param(up, "signal", "mode", "texture");
    g.set_param(ramp, "ramp", "channels", 1);
    drawn(&g, up, "the gray frame again", |d| close(px(d, 0, 0), [0.0, 0.0, 0.0, 1.0]));
    g.call("link remove", j!({ "from": ep(hex(ramp), "out"), "to": ep(hex(up), "input") }));

    // Step: the audio plane and this one, both ways, over the ONE cross-engine transport. Each
    // crossing declares the plane it crosses FROM, so a wire from the wrong one is refused and
    // these chains are the only shape that links at all.
    let flat = g.add("_TestConst");
    g.ready(flat);
    g.set_param(flat, "constant", "length", 64);
    g.set_param(flat, "constant", "value", 0.5);
    let stream = g.add("audio:SignalIn");
    g.link(flat, "out", stream, "input");
    // `graphics:AudioIn` holds its own picture between renders — a HISTORY, which is what a
    // stream has and a frame has not: every render walks the picture one column left and stands
    // the newest column at the right edge.
    let scope = g.add("graphics:AudioIn");
    g.ready(scope);
    g.set_param(scope, "common", "width", 64);
    g.set_param(scope, "common", "height", 32);
    g.set_param(scope, "audio", "autoscale", false);
    g.set_param(scope, "audio", "range", 1.0);
    // The door takes the plane it is named for and no other: a signal frame is refused here, and
    // an ARRAY input on a crossing would have taken it — which is the whole reason to declare one.
    assert!(g.refuse("link add", j!({ "from": ep(hex(flat), "out"), "to": ep(hex(scope), "input") }))
        .contains("different data types"), "a signal frame must not enter the audio door");
    g.link(stream, "out", scope, "input");
    let watch = g.probe(scope, "out");
    let running = |g: &Goofi, what: &str, want: fn(&goofi_core::Data) -> bool| {
        g.until(what, |g| {
            drive(g, 480);
            render(g, 1);
            watch.latest().filter(&want)
        })
    };
    // A column holds the BAND its samples spanned, and audio is bipolar, so half of full scale
    // stands a quarter down the frame and nowhere else.
    let band = running(&g, "the stream's band, a quarter down", |d| {
        shape(d) == vec![32, 64, 4] && px(d, 8, 63)[3] > 0.5
    });
    assert!(px(&band, 0, 63)[3] < 0.5 && px(&band, 24, 63)[3] < 0.5, "and only there: {band:?}", band = band.meta());

    // The history: change the value and render on, and the picture holds BOTH at once — the new
    // column at the right, the old ones still left of it. Nothing but a read of the last render's
    // own buffer can show that, so this is what says a state buffer is really carried.
    g.set_param(flat, "constant", "value", -0.5);
    let both = running(&g, "both values standing in one picture", |d| {
        px(d, 24, 63)[3] > 0.5 && (0..64).any(|col| px(d, 8, col)[3] > 0.5)
    });
    assert!(px(&both, 24, 0)[3] < 0.5, "the newest column is the RIGHT one: {:?}", px(&both, 24, 0));

    // The waterfall lays the FRAME down the column instead of its band — which is what stacks a
    // spectrum into a spectrogram, and for a flat stream is the whole column at one level.
    g.set_param(scope, "audio", "mode", "waterfall");
    running(&g, "the frame down the column", |d| {
        (0..32).all(|row| (px(d, row, 63)[3] - 0.5).abs() < 0.1)
    });
    for uid in [scope, stream, flat] {
        g.call("node remove", j!({ "node": hex(uid) }));
    }

    // …and the other way: `audio:GraphicsIn` reads a texture's texels in scan order, so the
    // PICTURE's size is the crossing's length. A gradient across the frame arrives as a sweep,
    // and its channels arrive as channels — alpha among them, since a texture is RGBA.
    let field = g.add("graphics:Gradient");
    g.ready(field);
    g.set_param(field, "common", "width", 64);
    g.set_param(field, "common", "height", 64);
    let into_audio = g.add("audio:GraphicsIn");
    g.set_param(into_audio, "graphics", "mode", "direct");
    g.set_param(into_audio, "graphics", "channels", "all");
    g.link(field, "out", into_audio, "input");
    let sink = g.add("audio:AudioOut");
    g.link(into_audio, "out", sink, "input");
    // A whole drive of it: one that straddles the texture's arrival still opens on zeros.
    let (rgba, chans) = g.until("the texture's channels as channels, opaque throughout", |g| {
        render(g, 1);
        let (x, c) = drive(g, 480);
        (c == 4 && x.chunks_exact(4).all(|t| (t[3] - 1.0).abs() < 0.01)).then_some((x, c))
    });
    assert_eq!(chans, 4, "a texture is RGBA, so it crosses as four");
    let lanes = |c: usize| rgba.chunks_exact(4).map(|t| t[c]).collect::<Vec<f32>>();
    let red = lanes(0);
    assert!(red.iter().any(|v| *v < 0.1) && red.iter().any(|v| *v > 0.9), "the gradient sweeps: {} .. {}",
            red.iter().copied().fold(f32::INFINITY, f32::min), red.iter().copied().fold(0.0f32, f32::max));
    // Brightness is the other way to take it, on one channel, the weights summing to one.
    g.set_param(into_audio, "graphics", "channels", "luma");
    g.until("the picture's brightness on one channel", |g| {
        render(g, 1);
        let (x, c) = drive(g, 480);
        (c == 1 && x.iter().any(|v| *v > 0.9) && x.iter().any(|v| *v < 0.1)).then_some(())
    });
    for uid in [sink, into_audio, field] {
        g.call("node remove", j!({ "node": hex(uid) }));
    }

    // Step: a texture chain does not flip either. A gradient down the frame, copied by a Level,
    // still runs the same way — the half of the orientation rule an upload cannot see.
    let vert = g.add("graphics:Gradient");
    g.ready(vert);
    g.set_param(vert, "ramp", "angle", 90.0);
    g.set_param(vert, "common", "width", 8);
    g.set_param(vert, "common", "height", 8);
    let copy = g.add("graphics:Level");
    g.ready(copy);
    g.link(vert, "out", copy, "input");
    let direct = drawn(&g, vert, "the vertical ramp", |d| shape(d) == vec![8, 8, 4]);
    let copied = drawn(&g, copy, "the copy of it", |d| shape(d) == vec![8, 8, 4]);
    assert!(px(&direct, 0, 0)[0] < px(&direct, 7, 0)[0], "the ramp runs down: {:?}", px(&direct, 0, 0));
    assert!(close(px(&copied, 0, 0), px(&direct, 0, 0)), "and the copy is not mirrored");
    assert!(close(px(&copied, 7, 0), px(&direct, 7, 0)));

    // Step: a bare read moves a param at control rate, the one door every modulation uses.
    let knob = g.add("_TestScalar");
    g.ready(knob);
    g.set_param(knob, "control", "value", 0.5);
    let knob_name = g.name(&hex(knob));
    g.call("node param edit", j!({ "node": hex(level), "param": "level/gain",
                                  "expression": format!("nd('{knob_name}')") }));
    drawn(&g, level, "half gain by reference", |d| close(px(d, 0, 0), [0.125, 0.25, 0.5, 1.0]));
    g.set_param(knob, "control", "value", 2.0);
    drawn(&g, level, "double gain by reference", |d| close(px(d, 0, 0), [0.5, 1.0, 2.0, 1.0]));

    // Step: the size is a param like any other, so a bare read DRIVES it — a signal decides how
    // many texels a stage has, and the engine re-plans around what the read last said.
    g.set_param(knob, "control", "value", 96.0);
    g.call("node param edit", j!({ "node": hex(level), "param": "common/width",
                                   "expression": format!("nd('{knob_name}')") }));
    drawn(&g, level, "the referenced width", |d| shape(d) == vec![32, 96, 4]);
    assert!(g.error(level).is_none(), "a bare read on the size is not a fault");
    g.call("node param edit", j!({ "node": hex(level), "param": "common/width", "mode": "constant" }));
    g.call("node param edit", j!({ "node": hex(level), "param": "level/gain", "mode": "constant" }));
    drawn(&g, level, "and it follows its input again", |d| shape(d) == vec![32, 64, 4]);

    // Step: a loop closes through Feedback and accumulates a tenth a tick; one without it faults.
    let fb = g.add("graphics:Feedback");
    g.ready(fb);
    // Sized down: what a loop does is the same at any size, and the default is read back whole on
    // every poll of it.
    g.set_param(fb, "common", "width", 64);
    g.set_param(fb, "common", "height", 64);
    let acc = g.add("graphics:Level");
    g.ready(acc);
    g.set_param(acc, "level", "offset", 0.1);
    g.link(fb, "out", acc, "input");
    g.link(acc, "out", fb, "input");
    let first = drawn(&g, acc, "the first tick", |d| px(d, 0, 0)[0] > 0.05);
    let after = drawn(&g, acc, "five ticks on", |d| px(d, 0, 0)[0] > px(&first, 0, 0)[0] + 0.4);
    assert!(px(&after, 0, 0)[0] < 10.0, "a tenth a tick, not a runaway: {:?}", px(&after, 0, 0));
    assert!(g.error(fb).is_none() && g.error(acc).is_none(), "a loop through Feedback is not a fault");
    // A loop with no feedback node in it faults and does not render; the rest of the patch does.
    let (one, two) = (g.add("graphics:Level"), g.add("graphics:Level"));
    g.ready(one);
    g.ready(two);
    g.link(one, "out", two, "input");
    g.link(two, "out", one, "input");
    g.until("a loop with no feedback node faults", |g| {
        render(g, 1);
        g.error(one).filter(|e| e.contains("feedback"))
    });
    g.call("node remove", j!({ "node": hex(one) }));
    g.call("node remove", j!({ "node": hex(two) }));

    // A node wired to ITSELF is out too, feedback node or not: a pass cannot read what it writes,
    // and one that tried took the whole tick's command buffer down with it.
    for ty in ["graphics:Level", "graphics:Feedback"] {
        let solo = g.add(ty);
        g.ready(solo);
        g.link(solo, "out", solo, "input");
        g.until("a self-wired node faults", |g| {
            render(g, 1);
            g.error(solo).filter(|e| e.contains("its own output"))
        });
        drawn(&g, acc, "and the rest of the patch draws on", |d| px(d, 0, 0)[0] > 0.05);
        g.call("node remove", j!({ "node": hex(solo) }));
    }

    // Step: two shaders compose.
    let ramp = g.add("graphics:Gradient");
    g.ready(ramp);
    let comp = g.add("graphics:Composite");
    g.ready(comp);
    g.set_param(comp, "composite", "mode", "add");
    g.link(ramp, "out", comp, "a");
    g.link(c, "out", comp, "b");
    drawn(&g, comp, "the sum of a ramp and the constant", |d| px(d, 0, 0)[2] > 1.0 - 1e-3);

    // Step: a `.wgsl` that does not compile is a greyed type carrying naga's own line number.
    let dir = g.state.mount().join("nodes_graphics");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Broken.wgsl"), BROKEN).unwrap();
    g.call("library refresh", j!({}));
    let listed = g.call("library list", j!({ "full": true }));
    let greyed = listed["types"].as_array().unwrap().iter().find(|r| r["type"] == "graphics:Broken").cloned();
    let greyed = greyed.expect("a file that does not compile is still a row");
    assert_eq!(greyed["available"], false, "{greyed}");
    let why = greyed["doc"].as_str().unwrap_or_default();
    assert!(why.contains(":4:"), "the file's OWN line 4, not the prelude's: {why}");
    assert!(g.refuse("node add", j!({ "type": "graphics:Broken" })).contains("unavailable"));

    // Step: a workspace node is authored, loaded, and reloaded through the one refresh door.
    std::fs::write(dir.join("Half.wgsl"), HALF).unwrap();
    assert_eq!(g.call("library refresh", j!({}))["added"], j!(["graphics:Half"]));
    let half = g.add("graphics:Half");
    g.ready(half);
    g.link(c, "out", half, "input");
    drawn(&g, half, "half of the constant", |d| close(px(d, 0, 0), [0.125, 0.25, 0.5, 1.0]));
    std::fs::write(dir.join("Half.wgsl"), QUARTER).unwrap();
    assert_eq!(g.call("library refresh", j!({}))["changed"], j!(["graphics:Half"]));
    drawn(&g, half, "a quarter, after the reload", |d| close(px(d, 0, 0), [0.0625, 0.125, 0.25, 1.0]));

    // Step: the file breaks UNDER a running node. The type greys and a restart is refused, but the
    // instance keeps the pipeline it was born with and draws on.
    std::fs::write(dir.join("Half.wgsl"), BROKEN).unwrap();
    g.call("library refresh", j!({}));
    assert!(g.refuse("node restart", j!({ "node": hex(half) })).contains("unavailable"));
    drawn(&g, half, "the last good file runs on", |d| close(px(d, 0, 0), [0.0625, 0.125, 0.25, 1.0]));
    std::fs::write(dir.join("Half.wgsl"), QUARTER).unwrap();
    g.call("library refresh", j!({}));

    // Step: a slot of a kind a shader cannot carry is greyed, and the reason names what it may be.
    // A TEXTURE is a stage of this engine's plan and everything else is a frame it uploads, so
    // what is refused is a kind that is no frame at all — never another engine's, which is a
    // crossing.
    std::fs::write(dir.join("Texty.wgsl"), TEXTY).unwrap();
    g.call("library refresh", j!({}));
    let listed = g.call("library list", j!({ "full": true }));
    let row = listed["types"].as_array().unwrap().iter().find(|r| r["type"] == "graphics:Texty").cloned();
    let row = row.expect("a shader that names a string slot is still a row");
    assert_eq!(row["available"], false, "{row}");
    assert!(row["doc"].as_str().unwrap_or_default().contains("TEXTURE, ARRAY or AUDIO"), "{row}");

    // Step: a header declares a SECTION in place of a param: its members sit in a section of their
    // own, named, and show by another param; one that shows by a param the header does not
    // declare is greyed, with the reason, and so is a list that repeats past what a shader keeps.
    std::fs::write(dir.join("Tinted.wgsl"), TINTED).unwrap();
    std::fs::write(dir.join("Untinted.wgsl"), TINTED.replace("\"tint\", \"any_of\"", "\"tone\", \"any_of\"")).unwrap();
    std::fs::write(dir.join("Overlong.wgsl"), OVERLONG).unwrap();
    g.call("library refresh", j!({}));
    let hue = &g.call("library get", j!({ "type": "graphics:Tinted" }))["params"]["look"]["colour_hue"];
    assert_eq!((&hue["section"], &hue["show"]), (&j!(1), &j!([{ "group": "look", "name": "tint", "any_of": ["true"] }])));
    assert_eq!(hue["role"], j!({ "as": "member", "section": "colour", "base": "hue", "slot": null }));
    let listed = g.call("library list", j!({ "full": true }));
    let row = listed["types"].as_array().unwrap().iter().find(|r| r["type"] == "graphics:Untinted").cloned();
    let row = row.expect("a shader whose param shows by nothing is still a row");
    assert_eq!(row["available"], false, "{row}");
    assert!(row["doc"].as_str().unwrap_or_default().contains("`tone`, which this node does not declare"), "{row}");
    let row = listed["types"].as_array().unwrap().iter().find(|r| r["type"] == "graphics:Overlong").cloned();
    let row = row.expect("a shader whose list is too long is still a row");
    assert_eq!(row["available"], false, "{row}");
    assert!(row["doc"].as_str().unwrap_or_default().contains("section `many` repeats 1 to 100 times"), "{row}");

    // Step: a LIST section is one param per slot, counted by an int named after the section, and
    // the shader reads it as an array. A default given as a list is each slot's own, the last
    // carrying on; a third stop opened in the middle of a ramp shows at its centre.
    let ramp = &g.call("library get", j!({ "type": "graphics:Gradient" }))["params"]["ramp"];
    assert_eq!(ramp["stops"]["role"], j!({ "as": "count", "section": "stops" }));
    assert_eq!(ramp["at_2"]["role"], j!({ "as": "member", "section": "stops", "base": "at", "slot": 2 }));
    assert_eq!((&ramp["at_0"]["default"], &ramp["at_1"]["default"], &ramp["at_15"]["default"]), (&j!(0), &j!(1), &j!(1)));
    assert!(ramp.get("at_16").is_none(), "no slot past the list's max");
    let stops = g.add("graphics:Gradient");
    g.ready(stops);
    g.set_param(stops, "common", "width", 64);
    g.set_param(stops, "common", "height", 4);
    g.set_param(stops, "ramp", "stops", 3);
    g.set_param(stops, "ramp", "at_1", 0.5);
    g.set_param(stops, "ramp", "colour_1", j!([1.0, 0.0, 0.0, 1.0]));
    g.set_param(stops, "ramp", "at_2", 1.0);
    g.set_param(stops, "ramp", "colour_2", j!([1.0, 1.0, 1.0, 0.5]));
    drawn(&g, stops, "red at the middle stop, half-clear white past it", |d| {
        let (mid, end) = (px(d, 0, 32), px(d, 0, 63));
        mid[0] > 0.95 && mid[1] < 0.1 && mid[3] > 0.95 && end[1] > 0.9 && (end[3] - 0.5).abs() < 0.05
    });
    g.set_param(stops, "ramp", "stops", 2);
    drawn(&g, stops, "two stops read only the first two slots", |d| px(d, 0, 63)[1] < 0.05);
    g.call("node remove", j!({ "node": hex(stops) }));

    // Step: a colour is a four-dimensional number: the shader reads a `vec4f`, the palette says
    // `num` with the `color` face, a list sets every dimension and one number fills them all.
    std::fs::write(dir.join("Inked.wgsl"), INKED).unwrap();
    assert_eq!(g.call("library refresh", j!({}))["added"], j!(["graphics:Inked"]));
    let ink = &g.call("library get", j!({ "type": "graphics:Inked" }))["params"]["look"]["ink"];
    assert_eq!((&ink["type"], &ink["face"], &ink["value"]), (&j!("num"), &j!("color"), &j!([1, 0, 0, 1])));
    let inked = g.add("graphics:Inked");
    g.ready(inked);
    g.set_param(inked, "common", "width", 8);
    g.set_param(inked, "common", "height", 8);
    drawn(&g, inked, "the default ink", |d| close(px(d, 0, 0), [1.0, 0.0, 0.0, 1.0]));
    g.set_param(inked, "look", "ink", j!([0.0, 0.0, 1.0, 0.5]));
    drawn(&g, inked, "an ink set as a list", |d| close(px(d, 0, 0), [0.0, 0.0, 1.0, 0.5]));
    g.set_param(inked, "look", "ink", 0.25);
    drawn(&g, inked, "one number fills every dimension", |d| close(px(d, 0, 0), [0.25, 0.25, 0.25, 0.25]));

    // Step: ONE element of a vector takes a source of its own, addressed `ink[3]`: it drives that
    // dimension alone, a literal on an element lands in its dimension of the list, and a source on
    // the whole param gives the dimensions no element drives. The state and a copy both carry it.
    let dial = g.add("_TestScalar");
    g.ready(dial);
    g.set_param(dial, "control", "value", 0.5);
    let dial_name = g.name(&hex(dial));
    g.set_param(inked, "look", "ink", j!([0.0, 0.0, 1.0, 1.0]));
    g.call("node param edit", j!({ "node": hex(inked), "param": "look/ink[3]", "expression": format!("nd('{dial_name}')") }));
    drawn(&g, inked, "an element reference", |d| close(px(d, 0, 0), [0.0, 0.0, 1.0, 0.5]));
    g.set_param(inked, "look", "ink[1]", 0.75);
    drawn(&g, inked, "an element literal", |d| close(px(d, 0, 0), [0.0, 0.75, 1.0, 0.5]));
    let full = g.add("_TestScalar");
    g.ready(full);
    g.set_param(full, "control", "value", 1.0);
    g.call("node param edit", j!({ "node": hex(inked), "param": "look/ink", "expression": format!("nd('{}')", g.name(&hex(full))) }));
    drawn(&g, inked, "the element over the whole", |d| close(px(d, 0, 0), [1.0, 1.0, 1.0, 0.5]));
    // The evaluated values reach the state on the status drain's pace, after the frame.
    let (whole, element) = (format!("look.ink = expr: nd('{}') → [1, 1, 1, 0.5]", g.name(&hex(full))), format!("look.ink[3] = expr: nd('{dial_name}') → 0.5"));
    g.until("the state to show both sources' values", |g| {
        let text = g.call("node state", j!({ "node": hex(inked) }))["text"].as_str().expect("text").to_string();
        (text.contains(&whole) && text.contains(&element)).then_some(())
    });
    let copied = g.call("nodes copy", j!({ "nodes": [hex(inked)] }));
    assert_eq!(copied["doc"]["nodes"][hex(inked)]["params"]["look"]["ink[3]"]["expression"], j!(format!("nd('{dial_name}')")));
    assert_eq!(copied["doc"]["nodes"][hex(inked)]["params"]["look"]["ink"]["value"], j!([0, 0.75, 1, 1]));
    assert!(g.refuse("node param edit", j!({ "node": hex(inked), "param": "look/ink[4]", "value": 1.0 })).contains("no param"));
    g.call("node param edit", j!({ "node": hex(inked), "param": "look/ink[3]", "expression": "" }));
    drawn(&g, inked, "the element let go", |d| close(px(d, 0, 0), [1.0, 1.0, 1.0, 1.0]));
    // A vector's literal may come as bare numbers, and a LIST of expressions is one per element.
    g.call("node param edit", j!({ "node": hex(inked), "param": "look/ink", "mode": "constant" }));
    g.set_param(inked, "look", "ink", "0.5 0.25 1 1");
    drawn(&g, inked, "bare numbers, one per element", |d| close(px(d, 0, 0), [0.5, 0.25, 1.0, 1.0]));
    let before = g.doc()["nodes"][hex(inked)].clone();
    let why = g.refuse("node add", j!({ "type": "graphics:Inked", "param": [
        { "name": "look/ink", "expression": "[0, 0, 0, 1]", "unexpected": true }
    ] }));
    assert!(why.contains("unknown field `unexpected`"), "a list source uses the same validation: {why}");
    let why = g.refuse("node param edit", j!({ "node": hex(inked), "param": "look/ink", "expression": "[0, 0, 0, 1]", "mode": "invalid" }));
    assert!(why.contains("mode"), "a list source validates its mode: {why}");
    g.call("node param edit", j!({ "node": hex(inked), "param": "look/ink", "expression": "[0, 0, 0, 1]",
                                   "value": [0.25, 0.5, 0.75, 1.0], "mode": "constant" }));
    let params = g.doc()["nodes"][hex(inked)]["params"]["look"].clone();
    assert_eq!(params["ink"]["value"], j!([0.25, 0.5, 0.75, 1]), "the whole literal lands beside the list");
    for k in 0..4 {
        let element = &params[format!("ink[{k}]")];
        assert_eq!((&element["mode"], &element["expression"]), (&j!("constant"), &j!(if k == 3 { "1" } else { "0" })), "the list keeps each supplied source field: {element}");
    }
    drawn(&g, inked, "inactive list elements keep the supplied literal", |d| close(px(d, 0, 0), [0.25, 0.5, 0.75, 1.0]));
    g.call("undo", j!({}));
    assert_eq!(g.doc()["nodes"][hex(inked)], before, "one undo restores every element and the literal");
    g.call("node param edit", j!({ "node": hex(inked), "param": "look/ink", "expression": r#"['a\',b' and 0.5, 0.25, 1, 1]"# }));
    let params = g.doc()["nodes"][hex(inked)]["params"]["look"].clone();
    assert_eq!(params["ink"]["mode"], j!("constant"), "a string's escaped quote does not split the list: {params}");
    for (k, expression) in [r#"'a\',b' and 0.5"#, "0.25", "1", "1"].iter().enumerate() {
        assert_eq!(params[format!("ink[{k}]")]["expression"], j!(expression), "element {k}: {params}");
    }
    drawn(&g, inked, "an escaped quote and comma in one list element", |d| close(px(d, 0, 0), [0.5, 0.25, 1.0, 1.0]));
    g.call("node param edit", j!({ "node": hex(inked), "param": "look/ink", "expression": format!("[{dial_name}.out, 0.25, (1, 2)[0], 'a,b']") }));
    let text = g.call("node state", j!({ "node": hex(inked) }))["text"].as_str().expect("text").to_string();
    for line in ["look.ink[1] = expr: 0.25", "look.ink[2] = expr: (1, 2)[0]", "look.ink[3] = expr: 'a,b'"] {
        assert!(text.contains(line), "{line} in {text}");
    }
    assert!(text.contains("look.ink = [0.5, 0.25, 1, 1]"), "the whole param is back on its literal: {text}");
    for node in [dial, full] {
        g.call("node remove", j!({ "node": hex(node) }));
    }
    g.call("node remove", j!({ "node": hex(inked) }));

    // Step: a node holds its own state between two ticks. A state buffer starts empty, so a body
    // seeds itself on `frame == 0` and reads what the last tick wrote from then on.
    std::fs::write(dir.join("Count.wgsl"), COUNT).unwrap();
    assert_eq!(g.call("library refresh", j!({}))["added"], j!(["graphics:Count"]));
    let counter = g.add("graphics:Count");
    g.ready(counter);
    g.set_param(counter, "common", "width", 64);
    g.set_param(counter, "common", "height", 64);
    let counted = g.probe(counter, "out");
    let read = || counted.latest().map(|d| px(&d, 0, 0));
    let opened = g.until("a frame off the node's own buffer", |g| {
        render(g, 1);
        read().filter(|t| t[0] > 0.0)
    });
    let mut held = opened;
    g.until("the buffer to accumulate", |g| {
        render(g, 1);
        let now = read()?;
        // Green is what the seed wrote and nothing since has touched; red is what each tick adds
        // to what the last one left. A buffer remade under the node loses both.
        assert!((now[1] - 0.75).abs() < 1e-3, "the seed the first tick wrote is gone: {now:?}");
        assert!(now[0] + 1e-3 >= held[0], "the buffer went backwards: {held:?} then {now:?}");
        held = now;
        (now[0] > opened[0]).then_some(())
    });
    assert!(g.error(counter).is_none(), "a node with state is not a fault");
    g.call("node remove", j!({ "node": hex(counter) }));

    // Step: the shipped automaton is that buffer in use — it seeds itself on the first tick and
    // reads every generation off the last. Conway thins a random field to a few percent and stops
    // there, which no frozen grid and no runaway rule can do.
    let life = g.add("graphics:Life");
    g.ready(life);
    g.set_param(life, "common", "width", 64);
    g.set_param(life, "common", "height", 64);
    let grid = g.probe(life, "out");
    let alive = || grid.latest().filter(|d| shape(d) == vec![64, 64, 4])
        .map(|d| f32s(&d).chunks_exact(4).map(|t| t[0]).sum::<f32>() / 4096.0);
    let sown = g.until("a generation off the grid", |g| {
        render(g, 1);
        alive().filter(|v| *v > 0.0)
    });
    assert!(sown < 0.5, "a 35% seed, not a grid that came up full: {sown}");
    let settled = g.until("the population thins and holds", |g| {
        render(g, 20);
        alive().filter(|v| *v < 0.15)
    });
    assert!(settled > 0.0, "the rule emptied the grid: {settled}");
    g.call("node remove", j!({ "node": hex(life) }));

    // Step: a node nobody reads renders nothing — which is what makes an idle patch free.
    let stages = |g: &Goofi| g.call("session status", j!({}))["graphics"]["stages"].as_u64().unwrap();
    let lonely = g.add("graphics:Constant");
    g.ready(lonely);
    render(&g, 5);
    let before = stages(&g);
    render(&g, 10);
    assert_eq!(stages(&g), before, "ten ticks, no reader, no work");
    let probe = g.probe(lonely, "out");
    g.until("a reader wakes it", |g| {
        render(g, 1);
        probe.latest()
    });
    // EXACTLY one stage a tick, with several nodes live: "it moved" would pass a demand walk that
    // wakes the whole patch whenever anybody reads anything.
    let watched = stages(&g);
    render(&g, 3);
    assert_eq!(stages(&g), watched + 3, "one reader, one stage a tick, whatever else is live");

    // Step: a texture output is read by the ONE snapshot op, and the source door answers the file.
    let shot = g.until("a snapshot of a texture slot", |g| {
        render(g, 1);
        g.call("node snapshot", j!({ "output": ep(hex(half), "out") }))["shape"].as_array().cloned()
    });
    assert_eq!(shot.len(), 3, "a texture reads back as [H, W, 4]: {shot:?}");
    let source = g.call("library get", j!({ "type": "graphics:Half", "source": true }));
    assert!(source["text"].as_str().is_some_and(|t| t.contains("textureSample")), "{source}");

    // Step: the crossing OUT. A texture output feeds a TEXTURE input only, and on the signal plane
    // `signal:GraphicsIn` is that door: it makes the picture a size the rest of the patch can
    // carry, and the readings a picture is usually wanted for.
    let field = g.add("graphics:Gradient");
    g.ready(field);
    g.set_param(field, "common", "width", 128);
    g.set_param(field, "common", "height", 64);
    let down = g.add("signal:GraphicsIn");
    g.link(field, "out", down, "input");
    let read = g.probe(down, "out");
    let waited = |g: &Goofi, what: &str, want: fn(&goofi_core::Data) -> bool| {
        let read = g.probe(down, "out");
        g.until(what, |g| {
            render(g, 1);
            read.latest().filter(&want)
        })
    };
    // The whole frame first: `size` bounds each picture axis, and the frame says what it was
    // rendered from, so a reader can still map a value back to the texel it came from.
    g.set_param(down, "graphics", "size", 16);
    let small = waited(&g, "the frame cut to size", |d| shape(d) == vec![16, 16, 4]);
    let noted = small.meta().reduced().cloned().expect("a frame cut on the way over says so");
    assert!(format!("{noted:?}").contains("128"), "it names the width it came from: {noted:?}");
    g.set_param(down, "graphics", "size", 0);
    waited(&g, "and zero is every texel of it", |d| shape(d) == vec![64, 128, 4]);

    // A horizontal gradient rises ACROSS the frame and is flat down it, so the two profiles are
    // the two answers — and the channel axis is spent, since brightness is what they read.
    g.set_param(down, "graphics", "mode", "columns");
    let across = waited(&g, "the profile across", |d| shape(d) == vec![128]);
    let x = f32s(&across);
    assert!(x[0] < 0.05 && x[127] > 0.95, "it runs dark to light: {} to {}", x[0], x[127]);
    assert!(x.windows(2).all(|w| w[1] >= w[0] - 1e-4), "monotonically: {x:?}");
    g.set_param(down, "graphics", "mode", "rows");
    let downwards = waited(&g, "the profile down", |d| shape(d) == vec![64]);
    let y = f32s(&downwards);
    assert!(y.iter().all(|v| (v - y[0]).abs() < 0.01), "and is flat the other way: {} .. {}", y[0], y[63]);
    g.set_param(down, "graphics", "mode", "gray");
    waited(&g, "brightness keeps the picture's shape", |d| shape(d) == vec![64, 128]);
    // A viewer's 8-bit box on the producer never reaches the wire: the consumer still takes the
    // whole frame, in the engine's own 0..1.
    let tiny = goofi_view::ViewWant { size: (8, 8), depth: goofi_view::Depth::U8 };
    g.graph().set_view_demand(field, "out", Some(tiny));
    render(&g, 3);
    let whole = waited(&g, "the whole frame beside a viewer's box", |d| shape(d) == vec![64, 128]);
    assert!(f32s(&whole).iter().all(|v| (0.0..=1.0).contains(v)), "in the engine's range, not texel bytes");
    for uid in [down, field] {
        g.call("node remove", j!({ "node": hex(uid) }));
    }
    drop(read);

    // Step: the demand stops when the LAST reader goes. A reducer that keeps its subscription
    // alive is a reader as far as the engine can tell, and this node would render for ever.
    drop(probe);
    g.until("the watched node goes quiet again", |g| {
        render(g, 30);
        let a = stages(g);
        render(g, 5);
        (stages(g) == a).then_some(())
    });

    // Step: a Window node opens a window on the machine's screen and feeds it. The suite's screen
    // is a headless one, so what stands here is the SEAM — a window asked for, sized, fed and
    // closed — and never a desktop.
    let win = g.add("graphics:Window");
    g.ready(win);
    g.set_param(win, "common", "width", 64);
    g.set_param(win, "common", "height", 32);
    g.link(c, "out", win, "input");
    let ui = g.ui();
    let opened = |g: &Goofi| goofi_bridge::graphics_engine(&mut g.graph()).window_of(win);
    let id = g.until("the window is open", |g| {
        render(g, 1);
        opened(g)
    });
    let status = g.call("session status", j!({}))["graphics"].clone();
    assert_eq!(status["windows"], j!(1), "status names the window");
    // The suite's screen answers no handles, so the frame goes through the window thread.
    assert_eq!(status["surfaces"], j!(0), "and no GPU surface on a headless screen");
    assert_eq!(status["fps"], j!(goofi_core::variables::DEFAULT_FPS), "and the clock's pace is the variable's default");
    g.call("variable entry edit", j!({ "name": "graphics.fps", "value": 24 }));
    assert_eq!(g.call("session status", j!({}))["graphics"]["fps"], j!(24), "`graphics.fps` sets the pace");
    g.call("variable entry edit", j!({ "name": "graphics.fps", "value": 100000 }));
    assert_eq!(g.call("session status", j!({}))["graphics"]["fps"], j!(240), "held to the range");
    let frames = g.until("and the screen is given frames", |g| {
        render(g, 1);
        let seen = ui.run(move |host| host.presents(id));
        (seen > 0).then_some(seen)
    });
    // A window is a reader, so the stage renders with no viewer anywhere on it.
    assert!(g.error(win).is_none(), "an open window is not a fault");
    render(&g, 4);
    assert!(ui.run(move |host| host.presents(id)) > frames, "the window keeps being fed");
    // And what it shows is what it was wired: the node is a pass-through like any other.
    drawn(&g, win, "the window's own output", |d| close(px(d, 0, 0), [0.25, 0.5, 1.0, 1.0]));
    g.call("node remove", j!({ "node": hex(win) }));
    render(&g, 2);
    assert_eq!(ui.run(move |host| host.presents(id)), 0, "a removed node takes its window with it");
    assert_eq!(g.call("session status", j!({}))["graphics"]["windows"], j!(0), "and status says so");

    // Step: a restart is a rebirth through the same trait doors — new generation, new services.
    let generation = g.graph().node_generation(c);
    let stale = g.probe(c, "out");
    g.call("node restart", j!({ "node": hex(c) }));
    g.ready(c);
    assert_eq!(g.graph().node_generation(c), generation + 1);
    drawn(&g, c, "the reborn constant", |d| close(px(d, 0, 0), [0.25, 0.5, 1.0, 1.0]));
    let seen = stale.count();
    render(&g, 5);
    assert_eq!(stale.count(), seen, "the corpse's service name went silent");

    // Step: a remove through the one op surface tears the node down and the rest stand.
    g.call("node remove", j!({ "node": hex(lonely) }));
    assert!(!g.nodes().contains(&hex(lonely)));
    drawn(&g, c, "the constant still renders", |d| close(px(d, 0, 0), [0.25, 0.5, 1.0, 1.0]));

    // Step: a node that makes its own frames carries the patch's default size as a live
    // expression, so ONE variable re-sizes every producer at once. The seeding wants an evaluator
    // present; reading a bare variable does not, which is why this one needs no interpreter.
    g.graph().set_evaluator(Arc::new(goofi_tests::FirstVar::default()));
    let gen = g.add("graphics:Noise");
    g.ready(gen);
    let bound = g.doc()["nodes"][hex(gen)]["params"]["common"]["width"].clone();
    assert_eq!((&bound["expression"], &bound["mode"]), (&j!("variables.graphics.default_width"), &j!("expression")),
               "the declared binding was seeded live, not flattened to a literal: {bound}");
    drawn(&g, gen, "the patch's default size", |d| shape(d) == vec![1024, 1024, 4]);
    g.call("variable entry edit", j!({ "name": "graphics.default_width", "value": 96 }));
    g.call("variable entry edit", j!({ "name": "graphics.default_height", "value": 48 }));
    drawn(&g, gen, "every producer follows the variable", |d| shape(d) == vec![48, 96, 4]);
    // A loop that follows itself at 0,0 can follow nothing: it takes the patch's default too, not
    // the number the variables were born with.
    let (fb, comp) = (g.add("graphics:Feedback"), g.add("graphics:Composite"));
    for node in [fb, comp] {
        g.ready(node);
        g.set_param(node, "common", "width", 0);
        g.set_param(node, "common", "height", 0);
    }
    g.link(fb, "out", comp, "a");
    g.link(comp, "out", fb, "input");
    drawn(&g, comp, "a loop at 0,0 follows the variable", |d| shape(d) == vec![48, 96, 4]);
    g.call("node remove", j!({ "node": hex(fb) }));
    g.call("node remove", j!({ "node": hex(comp) }));

    // Step: the noise itself, walked one param at a time on the small frame the variable just made.
    // Speed 0 stops the drift, which is the only thing that lets one frame be compared with the
    // next at all — every reading after this one rests on it.
    let probe = g.probe(gen, "out");
    let reds = |d: &goofi_core::Data| f32s(d).chunks(4).map(|t| t[0]).collect::<Vec<f32>>();
    let span = |d: &goofi_core::Data| {
        let v = reds(d);
        v.iter().fold(f32::MIN, |a, b| a.max(*b)) - v.iter().fold(f32::MAX, |a, b| a.min(*b))
    };
    let settle = |what: &str, want: &dyn Fn(&goofi_core::Data) -> bool| {
        g.until(what, |g| {
            render(g, 1);
            probe.latest().filter(|d| want(d))
        })
    };
    g.set_param(gen, "move", "speed", 0.0);
    let still = g.until("a still field", |g| {
        render(g, 2);
        let was = probe.latest()?;
        render(g, 2);
        probe.latest().filter(|d| shape(d) == vec![48, 96, 4] && reds(d) == reds(&was))
    });
    assert!(span(&still) > 0.1, "the default field varies across the frame: {}", span(&still));
    assert!(f32s(&still).chunks(4).all(|t| (t[0] - t[1]).abs() < 1e-3),
            "monochrome noise writes one value to every channel");

    // Step: the exponent works on the SIGNED field, so a high one pulls BOTH ends towards the
    // midpoint. That is what no gamma on a 0..1 image can do, and the reason this param is here
    // while brightness and contrast are left to `Level`.
    g.set_param(gen, "noise", "exponent", 8.0);
    settle("the field pulled in towards its midpoint", &|d| span(d) < span(&still) / 2.0);
    g.set_param(gen, "noise", "exponent", 1.0);

    // Step: harmonics, then period. Dropping the finer layers changes the field; a period wider
    // than the frame leaves almost nothing for it to vary across.
    g.set_param(gen, "noise", "harmonics", 0);
    let plain = settle("the base frequency alone", &|d| reds(d) != reds(&still));
    g.set_param(gen, "noise", "period", 4.0);
    settle("a period wider than the frame", &|d| span(d) < span(&still) / 2.0);

    // Step: the kind menu picks a different function rather than a different look at one.
    g.set_param(gen, "noise", "period", 0.25);
    g.set_param(gen, "noise", "kind", "perlin");
    let lattice = settle("perlin's own field", &|d| span(d) > 0.1 && reds(d) != reds(&plain));
    g.set_param(gen, "noise", "kind", "worley");
    settle("worley's cells", &|d| span(d) > 0.1 && reds(d) != reds(&lattice));

    // Step: mono off is three decorrelated fields — what `Displace` needs, since it reads red and
    // green as two directions and one field would push every texel the same way.
    g.set_param(gen, "noise", "mono", false);
    settle("three fields, not one", &|d| f32s(d).chunks(4).any(|t| (t[0] - t[1]).abs() > 0.05));
}

/// The clock the binary actually runs on: nobody calls `render()`, and the engine draws anyway.
#[test]
fn the_engine_draws_on_its_own_clock() {
    let g = Goofi::timed();
    let c = g.add("graphics:Constant");
    g.ready(c);
    g.set_param(c, "constant", "colour", j!([0.75, 1.0, 1.0, 1.0]));

    // Step: with no reader the clock turns and nothing is drawn — the demand rule holds here too.
    let idle = |g: &Goofi| g.call("session status", j!({}))["graphics"].clone();
    let start = g.until("the clock turns", |g| {
        idle(g)["frames"].as_u64().filter(|f| *f > 3)
    });
    assert_eq!(idle(&g)["clock"], "internal");
    assert_eq!(idle(&g)["stages"], j!(0), "no reader, no stage, whatever the clock does");

    // Step: a viewer arrives and the frames it gets were drawn by nobody's hand.
    let probe = g.probe(c, "out");
    let frame = g.until("a frame off the timer", |_| probe.latest());
    assert!(close(px(&frame, 0, 0), [0.75, 1.0, 1.0, 1.0]), "{:?}", px(&frame, 0, 0));
    let ran = idle(&g);
    assert!(ran["frames"].as_u64().is_some_and(|f| f > start), "{ran}");
    assert!(ran["stages"].as_u64().is_some_and(|s| s > 0), "{ran}");
}

/// A viewer asks for the box it can draw, and the ENGINE renders that — a 4K frame is never read
/// off the GPU only to be averaged down on a CPU. The oracle is a probe on the node's OWN service,
/// upstream of the reducer: only the engine can make what it sees small. What must still hold
/// while it does: a snapshot asks for the frame itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_viewer_sizes_the_readback_and_the_full_frame_is_still_reachable() {
    let g = Goofi::timed();
    let base = g.serve().await;
    let big = g.add("graphics:Gradient");
    g.ready(big);
    g.set_param(big, "common", "width", 1024);
    g.set_param(big, "common", "height", 512);

    // Upstream of the reducer: what the ENGINE published, before anything on a CPU shrank it.
    let engine = g.probe(big, "out");
    let made = |want: Vec<usize>| {
        g.until(&format!("the engine to publish {want:?}"), |_| engine.latest().filter(|d| shape(d) == want))
    };
    made(vec![512, 1024, 4]);

    // Step: a viewer declares the box it can draw, and the ENGINE starts rendering that — with the
    // aspect kept, so 1024x512 into 128x128 is 128x64 rather than a squashed square.
    let mut viewer = Viewer::open(&base, &hex(big), "out").await;
    viewer
        .view(j!([{ "dtype": "array", "ndim": [["ge", 2], ["le", 3]], "dims": [],
                    "reduce": [{ "dim": 0, "max": 128 }, { "dim": 1, "max": 128 }], "aspect": true }]))
        .await;
    let small = made(vec![64, 128, 4]);
    let reduced = small.meta().reduced().cloned().expect("a frame shrunk on the way out says so");
    assert!(format!("{reduced:?}").contains("1024"), "it names the width it came from: {reduced:?}");
    let drawn = viewer.until(|d| shape(d) == vec![64, 128, 4]).await;
    assert_eq!(shape(&drawn), vec![64, 128, 4], "and that is what the viewer draws");

    // Step: a snapshot asks for the frame ITSELF. The two-call protocol the op documents: the ask
    // widens the demand, and the answer is the frame the producer then made for it.
    let key = (big, "out".to_string());
    let deadline = std::time::Instant::now() + goofi_tests::WAIT;
    let mut full = None;
    while full.is_none() && std::time::Instant::now() < deadline {
        full = g.state.reducers.latest(key.clone()).filter(|d| shape(d) == vec![512, 1024, 4]);
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let full = full.expect("a snapshot reads real pixels, not the viewer's preview");
    assert!(full.meta().reduced().is_none(), "and the frame it answers with is not a reduction");

    // Step: the viewer's box returns — the ask widened the readback for a while, not for good.
    made(vec![64, 128, 4]);

    // Every reader from here on declares NOTHING, and none of them may widen the readback.
    // `want` for an unbroken stretch, any other shape restarting the stretch. A STRAGGLER is why
    // it restarts rather than fails: `reducers.latest` is both the snapshot's ask and its read, so
    // the read that ends the step above arms one more full frame, which lands whenever the machine
    // gets round to it. A real dilution settles at the wrong size and never yields the stretch, so
    // the deadline is what still catches one — and it names the shape it kept seeing.
    let holds_at = |want: Vec<usize>, why: &str| {
        let tick = std::time::Duration::from_millis(20);
        let hold = std::time::Duration::from_secs(2);
        let deadline = std::time::Instant::now() + goofi_tests::WAIT;
        let mut since = std::time::Instant::now();
        let mut seen = None;
        while since.elapsed() < hold {
            assert!(std::time::Instant::now() < deadline, "{why}: {seen:?} where {want:?} should stand");
            seen = engine.latest().map(|d| shape(&d));
            if seen.as_ref().is_some_and(|s| *s != want) {
                since = std::time::Instant::now();
            }
            std::thread::sleep(tick);
        }
    };

    // Step: a second reader wants the frame but no pixels of its own — the metadata panel, and
    // every viewer between its socket opening and its first `view` op landing. It declares
    // nothing, so it dilutes nothing: the box the viewer beside it asked for still stands.
    let mut bare = Viewer::open(&base, &hex(big), "out").await;
    bare.view(j!([])).await;
    holds_at(vec![64, 128, 4], "a reader that declared nothing moved another viewer's box");

    // Step: the declaring viewer leaves, as a pan carrying its node off screen does. Nothing asks
    // for pixels now, so the readback falls to one texel — never up to the whole frame.
    drop(viewer);
    holds_at(vec![1, 1, 4], "a viewer leaving widened the readback");
    drop(bare);
    holds_at(vec![1, 1, 4], "the last reader leaving widened the readback");

    // Step: a viewer that cannot draw this frame parks on the slot — a line panel on a texture,
    // which prints a summary and renders nothing. Nothing admits the frame, so nothing has asked
    // for pixels, and a declaration that draws nothing must not cost the whole frame.
    let mut cannot = Viewer::open(&base, &hex(big), "out").await;
    cannot
        .view(j!([{ "dtype": "array", "ndim": [["le", 2]], "dims": [],
                    "reduce": [{ "dim": 0, "max": 300 }, { "dim": -1, "max": 800 }] }]))
        .await;
    holds_at(vec![1, 1, 4], "a viewer that cannot draw the frame widened the readback");

    // Step: and beside one that CAN draw it, the fold takes the largest box per dim rather than
    // falling out to the whole frame — the line panel drops out, it does not uncouple the box.
    let mut draws = Viewer::open(&base, &hex(big), "out").await;
    draws
        .view(j!([{ "dtype": "array", "ndim": [["ge", 2], ["le", 3]], "dims": [],
                    "reduce": [{ "dim": 0, "max": 128 }, { "dim": 1, "max": 128 }], "aspect": true }]))
        .await;
    holds_at(vec![64, 128, 4], "a viewer that cannot draw the frame shrank one that can");

    // Step: a frame cut TWICE — by `GraphicsIn` on the way over and by the reducer for a viewer —
    // still names what `GraphicsIn` read as its origin — the engine's box, never its own 16 —
    // on the dim the reducer cut and on the one it left alone.
    let down = g.add("signal:GraphicsIn");
    g.link(big, "out", down, "input");
    g.set_param(down, "graphics", "size", 16);
    let mut twice = Viewer::open(&base, &hex(down), "out").await;
    twice
        .view(j!([{ "dtype": "array", "ndim": [["ge", 2], ["le", 3]], "dims": [],
                    "reduce": [{ "dim": 1, "max": 8 }] }]))
        .await;
    // `GraphicsIn` bounds each axis at 16, so a width of 8 is the reducer's own cut.
    let cut = twice.until(|d| shape(d).len() == 3 && shape(d)[1] == 8).await;
    let origin = |dim: &str| match cut.meta().reduced() {
        Some(goofi_core::MetaValue::Map(dims)) => match dims.get(dim) {
            Some(goofi_core::MetaValue::Map(axis)) => match axis.get("orig_len") {
                Some(goofi_core::MetaValue::Uint(n)) => Some(*n),
                Some(goofi_core::MetaValue::Int(n)) => Some(*n as u64),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    };
    assert!(origin("1").is_some_and(|n| n > 16), "{:?}", cut.meta().reduced());
    assert!(origin("0").is_some_and(|n| n > 16), "{:?}", cut.meta().reduced());
}

#[test]
fn an_oversized_readback_reports_an_error_and_recovers_after_resize() {
    let g = Goofi::new();
    let node = g.add("graphics:Gradient");
    g.ready(node);
    g.set_param(node, "common", "width", 8192);
    g.set_param(node, "common", "height", 8192);
    let probe = g.probe(node, "out");
    g.until("oversized readback is refused", |g| {
        render(g, 1);
        g.error(node).filter(|e| e.contains("readback:"))
    });
    g.set_param(node, "common", "width", 32);
    g.set_param(node, "common", "height", 32);
    g.until("readback resumes after resize", |g| {
        render(g, 1);
        probe.latest().filter(|d| shape(d) == vec![32, 32, 4])
    });
    g.until("readback error clears", |g| g.error(node).is_none().then_some(()));
}

#[test]
fn host_pixels_use_shared_textures_and_explicit_readback() {
    let g = Goofi::new();
    let dir = g.state.mount().join("nodes_graphics");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("TextureHost.rs"), include_str!("../fixtures/texture_host.rs")).unwrap();
    g.call("library refresh", j!({}));
    let source = g.add("graphics:TextureHost");
    let math = g.add("graphics:Math");
    let readback = g.add("signal:GraphicsIn");
    for uid in [source, math, readback] { g.ready(uid); }
    g.link(source, "out", math, "input");
    g.link(math, "out", readback, "input");
    let probe = g.probe(readback, "out");
    let frame = g.until("host texture reaches a CPU consumer without a viewer", |g| {
        render(g, 1);
        probe.latest().filter(|d| shape(d) == vec![2, 2, 4] && close(px(d, 0, 0), [1.0, 0.0, 0.0, 1.0]))
    });
    assert!(close(px(&frame, 0, 1), [0.0, 1.0, 0.0, 1.0]));
    assert!(close(px(&frame, 1, 0), [0.0, 0.0, 1.0, 1.0]));
    assert!(close(px(&frame, 1, 1), [1.0; 4]));
}

#[test]
fn python_texture_sources_resize_and_restart() {
    let _python = goofi_tests::require_python();
    let g = Goofi::new();
    let dir = g.state.mount().join("nodes_graphics");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("texture_python.py"), include_str!("../fixtures/texture_python.py")).unwrap();
    g.call("library refresh", j!({}));
    let row = g.call("library get", j!({"type": "graphics:TexturePython"}));
    assert_eq!(row["tier"], if goofi_signal::Python::new(_python.py.clone()).free_threaded.is_some() { "in-process" } else { "subprocess" }, "{row}");
    let source = g.add("graphics:TexturePython");
    let math = g.add("graphics:Math");
    g.ready(source);
    g.ready(math);
    g.link(source, "out", math, "input");
    drawn(&g, math, "Python float pixels", |d| shape(d) == vec![2, 3, 4] && close(px(d, 0, 0), [0.25, 0.5, 0.75, 0.5]));
    g.set_param(source, "image", "width", 7);
    g.set_param(source, "image", "red", 0.75);
    drawn(&g, math, "new natural size propagates", |d| shape(d) == vec![2, 7, 4] && close(px(d, 0, 0), [0.75, 0.5, 0.75, 0.5]));
    g.set_param(source, "image", "invalid", true);
    g.until("invalid texture reports a process error", |g| g.error(source));
    drawn(&g, math, "a failed source keeps its last texture", |d| shape(d) == vec![2, 7, 4] && close(px(d, 0, 0), [0.75, 0.5, 0.75, 0.5]));
    g.set_param(source, "image", "invalid", false);
    g.until("valid texture clears the process error", |g| g.error(source).is_none().then_some(()));
    g.call("node param request", j!({ "node": hex(source), "param": "image/flip", "request": "pulse" }));
    drawn(&g, math, "pulse reaches the host worker", |d| shape(d) == vec![2, 7, 4] && close(px(d, 0, 0), [0.75, 1.0, 0.75, 0.5]));
    g.call("node restart", j!({"node": hex(source)}));
    g.ready(source);
    drawn(&g, math, "replacement producer retains params", |d| shape(d) == vec![2, 7, 4] && close(px(d, 0, 0), [0.75, 0.5, 0.75, 0.5]));
    g.set_param(source, "common", "width", 5);
    drawn(&g, math, "explicit dimensions resize the upload", |d| shape(d) == vec![2, 5, 4] && close(px(d, 0, 0), [0.75, 0.5, 0.75, 0.5]));
}

#[test]
fn native_host_program_writes_the_shared_output() {
    let g = Goofi::new();
    let dir = g.state.mount().join("nodes_graphics");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("TextureHost.rs"), include_str!("../fixtures/texture_host.rs")).unwrap();
    g.call("library refresh", j!({}));
    // Authored after boot, so it runs HOSTED, as a signal node does: its library in a child of
    // goofi's own binary, listed while the instance lives, and never in this process.
    let r = g.call("library get", j!({ "type": "graphics:TextureHost" }));
    assert_eq!((&r["provenance"], &r["language"], &r["tier"]), (&j!("patch"), &j!("rust"), &j!("hosted")), "{r}");
    let source = g.add("graphics:TextureHost");
    let math = g.add("graphics:Math");
    g.ready(source);
    g.ready(math);
    g.link(source, "out", math, "input");
    g.set_param(source, "image", "mode", "render");
    let hosts = |g: &Goofi| -> Vec<String> {
        g.call("session status", j!({}))["resources"].as_array().into_iter().flatten()
            .filter(|e| e["kind"] == "child")
            .filter_map(|e| e["name"].as_str()).filter(|n| n.starts_with("native node TextureHost")).map(str::to_string).collect()
    };
    g.until("one host child for the one instance", |g| (hosts(g).len() == 1).then_some(()));
    let frame = drawn(&g, math, "host GPU program sampled downstream", |d| shape(d) == vec![2, 2, 4] && close(px(d, 0, 0), [0.25, 0.25, 0.5, 1.0]));
    assert!(close(px(&frame, 1, 1), [0.75, 0.75, 0.5, 1.0]));
    g.set_param(source, "image", "mode", "broken");
    g.until("invalid host GPU program reports a fault", |g| g.error(source));
    g.set_param(source, "image", "mode", "pixels");
    drawn(&g, math, "CPU submission replaces the failed GPU program", |d| shape(d) == vec![2, 2, 4] && close(px(d, 0, 0), [1.0, 0.0, 0.0, 1.0]));
    g.until("CPU submission clears the GPU program fault", |g| g.error(source).is_none().then_some(()));
}
