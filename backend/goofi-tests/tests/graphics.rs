//! The graphics engine under the external clock: one session, every action through the op
//! vocabulary, every probe a subscriber on the derived name of a texture slot — the door `/data`
//! opens — and every frame a readback the GPU actually made.

use std::sync::Arc;

use goofi_tests::{ep, f32s, hex, j, render, shape, Goofi, Uid, Viewer};

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

/// The texel at `(row, col)`: four floats, row 0 the top.
fn px(d: &goofi_core::Data, row: usize, col: usize) -> [f32; 4] {
    let s = shape(d);
    let at = (row * s[1] + col) * 4;
    f32s(d)[at..at + 4].try_into().expect("four channels")
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

/// How far the frame disagrees with itself shifted `dx` columns, as luminance per texel.
fn slid(d: &goofi_core::Data, dx: usize) -> f32 {
    let (h, w) = (shape(d)[0], shape(d)[1]);
    let v = f32s(d);
    let lum = |i: usize| v[i * 4] + v[i * 4 + 1] + v[i * 4 + 2];
    let mut off = 0.0;
    for y in 0..h {
        for x in 0..w - dx {
            off += (lum(y * w + x) - lum(y * w + x + dx)).abs();
        }
    }
    off / ((w - dx) * h) as f32
}

/// A frame as the mean of each of its 256 blocks. Fine enough that two folds which merely
/// rearrange one tile's content disagree, coarse enough that two renders of one setting do not —
/// a whole-frame checksum failed the first half of that, agreeing to nine digits across frames
/// whose texels differed by thirty percent.
fn mark(d: &goofi_core::Data) -> Vec<f32> {
    let (h, w) = (shape(d)[0], shape(d)[1]);
    let v = f32s(d);
    let mut out = vec![0.0f32; 256];
    let mut per = vec![0.0f32; 256];
    for y in (0..h).step_by(3) {
        for x in (0..w).step_by(3) {
            let b = (y * 16 / h) * 16 + x * 16 / w;
            let i = (y * w + x) * 4;
            out[b] += v[i] + v[i + 1] + v[i + 2];
            per[b] += 1.0;
        }
    }
    out.iter().zip(per).map(|(s, n)| s / n.max(1.0)).collect()
}

/// The furthest any block moved between two frames. Two renders of one setting answer exactly 0.
fn apart(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max)
}

/// Wider than the noise between two renders, narrower than the closest two folds come.
const MOVED: f32 = 0.02;

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

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
    g.set_param(c, "colour", "r", 0.25);
    g.set_param(c, "colour", "g", 0.5);
    g.set_param(c, "colour", "b", 1.0);
    let frame = drawn(&g, c, "the constant's colour", |d| close(px(d, 0, 0), [0.25, 0.5, 1.0, 1.0]));
    assert_eq!(shape(&frame), vec![1024, 1024, 4], "a node with nothing behind it is 1024 square");
    assert!(close(px(&frame, 511, 511), [0.25, 0.5, 1.0, 1.0]), "the same colour to the far corner");

    // Step: the universal `common` group resizes it, and what is wired behind FOLLOWS the size.
    g.set_param(c, "common", "width", 64);
    g.set_param(c, "common", "height", 32);
    let frame = drawn(&g, c, "the resized frame", |d| shape(d) == vec![32, 64, 4]);
    assert!(close(px(&frame, 31, 63), [0.25, 0.5, 1.0, 1.0]));
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
    let up = g.add("graphics:ArrayIn");
    g.ready(up);
    g.set_param(up, "common", "width", 4);
    g.set_param(up, "common", "height", 4);
    g.link(img, "out", up, "input");
    let frame = drawn(&g, up, "the uploaded image", |d| shape(d) == vec![4, 4, 4]);
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
    let frame = drawn(&g, up, "the raw [1, 64] frame", |d| shape(d) == vec![32, 64, 4]);
    assert!(close(px(&frame, 0, 0), [0.0, 0.0, 0.0, 1.0]), "texture mode is the frame itself: {:?}", px(&frame, 0, 0));

    g.set_param(up, "plot", "mode", "line");
    g.set_param(up, "plot", "autoscale", false);
    g.set_param(up, "plot", "min", 0.0);
    g.set_param(up, "plot", "max", 1.0);
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
    g.set_param(up, "plot", "mode", "trajectory");
    g.set_param(up, "plot", "autoscale", true);
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

    g.set_param(up, "plot", "mode", "texture");
    g.set_param(ramp, "ramp", "channels", 1);
    drawn(&g, up, "the raw frame again", |d| close(px(d, 0, 0), [0.0, 0.0, 0.0, 1.0]));
    g.call("link remove", j!({ "from": ep(hex(ramp), "out"), "to": ep(hex(up), "input") }));

    // Step: a texture chain does not flip either. A gradient down the frame, copied by a Level,
    // still runs the same way — the half of the orientation rule an upload cannot see.
    let vert = g.add("graphics:Ramp");
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

    // Step: a reference moves a param at control rate, the one door every modulation uses.
    let knob = g.add("_TestScalar");
    g.ready(knob);
    g.set_param(knob, "control", "value", 0.5);
    let knob_name = g.name(&hex(knob));
    g.call("node param edit", j!({ "node": hex(level), "param": "level/gain",
                                  "reference": format!("{knob_name}.out"), "mode": "reference" }));
    drawn(&g, level, "half gain by reference", |d| close(px(d, 0, 0), [0.125, 0.25, 0.5, 1.0]));
    g.set_param(knob, "control", "value", 2.0);
    drawn(&g, level, "double gain by reference", |d| close(px(d, 0, 0), [0.5, 1.0, 2.0, 1.0]));

    // Step: the size is a param like any other, so a reference DRIVES it — a signal decides how
    // many texels a stage has, and the engine re-plans around what the reference last said.
    g.set_param(knob, "control", "value", 96.0);
    g.call("node param edit", j!({ "node": hex(level), "param": "common/width",
                                   "reference": format!("{knob_name}.out"), "mode": "reference" }));
    drawn(&g, level, "the referenced width", |d| shape(d) == vec![32, 96, 4]);
    assert!(g.error(level).is_none(), "a reference on the size is not a fault");
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

    // Step: the shipped set composes, and every one of it compiles on this machine.
    let ramp = g.add("graphics:Ramp");
    g.ready(ramp);
    let comp = g.add("graphics:Composite");
    g.ready(comp);
    g.set_param(comp, "composite", "mode", "add");
    g.link(ramp, "out", comp, "a");
    g.link(c, "out", comp, "b");
    drawn(&g, comp, "the sum of a ramp and the constant", |d| px(d, 0, 0)[2] > 1.0 - 1e-3);
    // A row says naga read the file; a FRAME says this device built the pipeline behind it, which
    // is the half a validation pass cannot answer for.
    let shipped: Vec<String> = g.call("library list", j!({}))["types"]
        .as_array()
        .expect("a palette")
        .iter()
        .filter_map(|r| r["type"].as_str())
        .filter(|t| t.starts_with("graphics:"))
        .map(String::from)
        .collect();
    // Against the bundles on disk, not a number: a fourteenth node must not fail the suite for
    // existing, and a node that stops registering must fail it. EVERY bundle, because a `.wgsl`
    // is the graphics engine's wherever it is shipped from — the simulation pack ships the
    // graphics half of each of its models beside the model.
    let bundles = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../node-bundles");
    let mut want: Vec<String> = std::fs::read_dir(&bundles)
        .expect("the shipped bundles")
        .filter_map(|e| e.ok())
        .flat_map(|bundle| std::fs::read_dir(bundle.path()).into_iter().flatten().flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x == "wgsl"))
        .map(|e| format!("graphics:{}", e.path().file_stem().unwrap().to_string_lossy()))
        .collect();
    want.sort();
    let mut got = shipped.clone();
    got.sort();
    assert_eq!(got, want, "every shipped `.wgsl` is a type, and nothing else is");
    for ty in &shipped {
        let node = g.add(ty);
        g.ready(node);
        drawn(&g, node, ty, |d| shape(d).len() == 3);
        assert!(g.error(node).is_none(), "{ty} stands with an error");
        g.call("node remove", j!({ "node": hex(node) }));
    }

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
    std::fs::write(dir.join("Loud.wgsl"), FOREIGN).unwrap();
    g.call("library refresh", j!({}));
    let listed = g.call("library list", j!({ "full": true }));
    let row = listed["types"].as_array().unwrap().iter().find(|r| r["type"] == "graphics:Loud").cloned();
    let row = row.expect("a shader that names an audio slot is still a row");
    assert_eq!(row["available"], false, "{row}");
    assert!(row["doc"].as_str().unwrap_or_default().contains("TEXTURE or ARRAY"), "{row}");

    // Step: a node holds its own state between two ticks. A state buffer starts empty, so a body
    // seeds itself on `frame == 0` and reads what the last tick wrote from then on.
    std::fs::write(dir.join("Count.wgsl"), COUNT).unwrap();
    assert_eq!(g.call("library refresh", j!({}))["added"], j!(["graphics:Count"]));
    let counter = g.add("graphics:Count");
    g.ready(counter);
    let counted = g.probe(counter, "out");
    let read = || counted.latest().map(|d| px(&d, 0, 0));
    let opened = g.until("a frame off the node's own buffer", |g| {
        render(g, 1);
        read().filter(|t| t[0] > 0.0)
    });
    let mut held = opened;
    for _ in 0..40 {
        render(&g, 1);
        let Some(now) = read() else { continue };
        // Green is what the seed wrote and nothing since has touched; red is what each tick adds
        // to what the last one left. A buffer remade under the node loses both.
        assert!((now[1] - 0.75).abs() < 1e-3, "the seed the first tick wrote is gone: {now:?}");
        assert!(now[0] + 1e-3 >= held[0], "the buffer went backwards: {held:?} then {now:?}");
        held = now;
    }
    assert!(held[0] > opened[0], "nothing accumulated: {opened:?} then {held:?}");
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
    let opened = |g: &Goofi| goofi_bridge::graphics_engine(&mut g.state.graph.lock().unwrap()).window_of(win);
    let id = g.until("the window is open", |g| {
        render(g, 1);
        opened(g)
    });
    assert_eq!(g.call("session status", j!({}))["graphics"]["windows"], j!(1), "status names the window");
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
    let generation = g.state.graph.lock().unwrap().node_generation(c);
    let stale = g.probe(c, "out");
    g.call("node restart", j!({ "node": hex(c) }));
    g.ready(c);
    assert_eq!(g.state.graph.lock().unwrap().node_generation(c), generation + 1);
    drawn(&g, c, "the reborn constant", |d| close(px(d, 0, 0), [0.25, 0.5, 1.0, 1.0]));
    let seen = stale.count();
    render(&g, 5);
    assert_eq!(stale.count(), seen, "the corpse's service name went silent");

    // Step: a remove through the one op surface tears the node down and the rest stand.
    g.call("node remove", j!({ "node": hex(lonely) }));
    assert!(!g.nodes().contains(&hex(lonely)));
    drawn(&g, c, "the constant still renders", |d| close(px(d, 0, 0), [0.25, 0.5, 1.0, 1.0]));

    // Step: a node that makes its own frames carries the patch's default size as a live
    // expression, so ONE global re-sizes every producer at once. The seeding wants an evaluator
    // present; reading a bare global does not, which is why this one needs no interpreter.
    g.state.graph.lock().unwrap().set_evaluator(Arc::new(goofi_tests::FirstVar));
    let gen = g.add("graphics:Noise");
    g.ready(gen);
    let bound = g.doc()["nodes"][hex(gen)]["params"]["common"]["width"].clone();
    assert_eq!((&bound["expr"], &bound["mode"]), (&j!("globals.system.default_width"), &j!("expression")),
               "the declared binding was seeded live, not flattened to a literal: {bound}");
    drawn(&g, gen, "the patch's default size", |d| shape(d) == vec![1024, 1024, 4]);
    g.call("global entry edit", j!({ "name": "system.default_width", "value": 96 }));
    g.call("global entry edit", j!({ "name": "system.default_height", "value": 48 }));
    drawn(&g, gen, "every producer follows the global", |d| shape(d) == vec![48, 96, 4]);

    // Step: the noise itself, walked one param at a time on the small frame the global just made.
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
    g.set_param(c, "colour", "r", 0.75);

    // Step: with no reader the clock turns and nothing is drawn — the demand rule holds here too.
    let idle = |g: &Goofi| g.call("session status", j!({}))["graphics"].clone();
    let start = g.until("the clock turns", |g| {
        idle(g)["frames"].as_u64().filter(|f| *f > 3)
    });
    assert_eq!(idle(&g)["clock"], "timer");
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
    let big = g.add("graphics:Ramp");
    g.ready(big);
    g.set_param(big, "common", "width", 1024);
    g.set_param(big, "common", "height", 512);

    // Upstream of the reducer: what the ENGINE published, before anything on a CPU shrank it.
    let engine = g.probe(big, "out");
    let made = |want: Vec<usize>| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(d) = engine.latest().filter(|d| shape(d) == want) {
                return d;
            }
            assert!(std::time::Instant::now() < deadline, "the engine never published {want:?}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    made(vec![512, 1024, 4]);

    // Step: a viewer declares the box it can draw, and the ENGINE starts rendering that — with the
    // aspect kept, so 1024x512 into 128x128 is 128x64 rather than a squashed square.
    let mut viewer = Viewer::open(&base, &hex(big), "out").await;
    viewer
        .view(j!([{ "dtype": "array", "ndim": [["ge", 2], ["le", 3]], "dims": [],
                    "reduce": [{ "dim": 0, "max": 128, "method": "area" },
                               { "dim": 1, "max": 128, "method": "area" }] }]))
        .await;
    let small = made(vec![64, 128, 4]);
    let reduced = small.meta().reduced().cloned().expect("a frame shrunk on the way out says so");
    assert!(format!("{reduced:?}").contains("1024"), "it names the width it came from: {reduced:?}");
    let drawn = viewer.until(|d| shape(d) == vec![64, 128, 4]).await;
    assert_eq!(shape(&drawn), vec![64, 128, 4], "and that is what the viewer draws");

    // Step: a snapshot asks for the frame ITSELF. The two-call protocol the op documents: the ask
    // widens the demand, and the answer is the frame the producer then made for it.
    let key = (big, "out".to_string());
    let mut full = None;
    for _ in 0..400 {
        full = g.state.reducers.latest(key.clone()).filter(|d| shape(d) == vec![512, 1024, 4]);
        if full.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let full = full.expect("a snapshot reads real pixels, not the viewer's preview");
    assert!(full.meta().reduced().is_none(), "and the frame it answers with is not a reduction");

    // Step: the viewer's box returns — one full frame was the cost of the ask, not a new mode.
    made(vec![64, 128, 4]);

    // Every reader from here on declares NOTHING, and none of them may widen the readback.
    let holds_at = |want: Vec<usize>, why: &str| {
        let tick = std::time::Duration::from_millis(20);
        // Settled first: a snapshot is answered with one full frame, and a negative measured
        // across that answer would name the wrong cause.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut run = 0;
        while run < 10 {
            assert!(std::time::Instant::now() < deadline, "the readback never settled at {want:?}: {why}");
            run = if engine.latest().is_some_and(|d| shape(&d) == want) { run + 1 } else { 0 };
            std::thread::sleep(tick);
        }
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < until {
            assert!(engine.latest().is_none_or(|d| shape(&d) == want), "{why}");
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
                    "reduce": [{ "dim": 0, "max": 300, "method": "subsample" },
                               { "dim": -1, "max": 800, "method": "envelope" }] }]))
        .await;
    holds_at(vec![1, 1, 4], "a viewer that cannot draw the frame widened the readback");

    // Step: and beside one that CAN draw it, the fold takes the largest box per dim rather than
    // falling out to the whole frame — the line panel drops out, it does not degrade the kernel.
    let mut draws = Viewer::open(&base, &hex(big), "out").await;
    draws
        .view(j!([{ "dtype": "array", "ndim": [["ge", 2], ["le", 3]], "dims": [],
                    "reduce": [{ "dim": 0, "max": 128, "method": "area" },
                               { "dim": 1, "max": 128, "method": "area" }] }]))
        .await;
    holds_at(vec![64, 128, 4], "a viewer that cannot draw the frame shrank one that can");
}

/// A division of the plane, walked through its four geometries. The law under all of them is the
/// same: whatever a fold does to a picture, a tile carried onto its neighbour must land exactly —
/// and it must go on landing exactly after the Escher displacement has bent every boundary. The
/// ground is a gradient rather than a flat colour, because two groups can divide the plane along
/// the very same seams and differ only in what each tile then shows.
#[test]
fn a_tessellation_holds_its_symmetry() {
    const SIDE: usize = 360;
    const CELL: usize = 120;

    let g = Goofi::new();
    let ground = g.add("graphics:Ramp");
    g.ready(ground);
    g.set_param(ground, "ramp", "angle", 35.0);
    for (name, value) in [("r0", 0.15), ("g0", 0.1), ("b0", 0.6), ("r1", 1.0), ("g1", 0.85), ("b1", 0.2)] {
        g.set_param(ground, "ramp", name, value);
    }
    let t = g.add("graphics:Tessellate");
    g.ready(t);
    g.link(ground, "out", t, "input");
    for node in [ground, t] {
        g.set_param(node, "common", "width", SIDE as i64);
        g.set_param(node, "common", "height", SIDE as i64);
    }
    g.set_param(t, "gestalt", "contour", 1.0);
    g.set_param(t, "tile", "scale", 3.0);
    assert!(g.error(t).is_none(), "{:?}", g.error(t));
    let probe = g.probe(t, "out");

    // Step: all seventeen groups fold, and each is exactly periodic on its own lattice — three
    // cells across the frame, so one lattice vector is 160 columns. A fold that is not constant on
    // the group's orbits leaves a seam, and a seam is what this reading catches.
    let groups = ["p1", "p2", "pm", "pg", "cm", "pmm", "pmg", "pgg", "cmm", "p4", "p4m", "p4g", "p3", "p3m1",
                  "p31m", "p6", "p6m"];
    let mut seen: Vec<(&str, Vec<f32>)> = Vec::new();
    for group in groups {
        g.set_param(t, "wallpaper", "group", group);
        // A frame is this group's only once it has stopped being the last one's, which is what
        // keeps every reading below off a frame the param change had not reached yet.
        let was = seen.last().map(|(_, m)| m.clone());
        let f = drawn(&g, t, group, |d| {
            shape(d) == vec![SIDE, SIDE, 4] && was.as_ref().is_none_or(|m| apart(&mark(d), m) > MOVED)
        });
        let drift = slid(&f, CELL);
        assert!(drift < 0.02, "{group} is not periodic on its own lattice: {drift}");
        seen.push((group, mark(&f)));
    }
    // …and they are seventeen folds rather than one arm of a switch answering for several. pmm and
    // pgg draw the very same seams and are told apart only here, by what each tile is shown of.
    for (i, (a, ma)) in seen.iter().enumerate() {
        for (b, mb) in &seen[i + 1..] {
            assert!(apart(ma, mb) > MOVED, "{a} and {b} folded to the same picture");
        }
    }

    // Step: figure trades the two hands of a tile — so it has something to trade only where the
    // group carries a reflection. p4 is built from rotations alone and cannot answer it; p4m is
    // the same lattice with mirrors, and turns half its tiles over.
    g.set_param(t, "wallpaper", "group", "p4");
    let one_hand = drawn(&g, t, "p4 again", |d| apart(&mark(d), &seen[9].1) < MOVED);
    g.set_param(t, "gestalt", "figure", 1.0);
    assert!(
        g.stays(|g| {
            render(g, 1);
            probe.latest().is_none_or(|d| apart(&mark(&d), &mark(&one_hand)) < MOVED)
        }),
        "figure turned a tile p4 has only one of",
    );
    g.set_param(t, "wallpaper", "group", "p4m");
    drawn(&g, t, "p4m with its hands traded", |d| apart(&mark(d), &seen[10].1) > MOVED);
    g.set_param(t, "gestalt", "figure", 0.0);

    // Step: the Escher displacement. It bends every boundary — and because the field is averaged
    // over the point group, the bent tiling is still EXACTLY the same tiling, which is the whole
    // claim the page rests on.
    g.set_param(t, "wallpaper", "group", "p4");
    let plain = drawn(&g, t, "p4's straight grid", |d| apart(&mark(d), &seen[9].1) < MOVED);
    g.set_param(t, "escher", "amount", 1.0);
    let bent = drawn(&g, t, "the boundaries bent", |d| apart(&mark(d), &mark(&plain)) > MOVED);
    assert!(slid(&bent, CELL) < 0.02, "the displacement broke the tiling: {}", slid(&bent, CELL));

    // Step: the parquet drift. The tiles go on interlocking, but they no longer MATCH — the shape
    // is read off where in the frame it stands, so the lattice period goes and the fit stays.
    g.set_param(t, "morph", "drift", 1.5);
    drawn(&g, t, "the shape drifting across the frame", |d| slid(d, CELL) > 0.05);
    g.set_param(t, "morph", "drift", 0.0);
    g.set_param(t, "escher", "amount", 0.0);

    // Step: curvature. The three angles decide the geometry and nothing else does — (6, 3, 2) sums
    // to a flat plane and repeats, (7, 3, 2) does not sum to one and cannot, and what a hyperbolic
    // fold cannot reach is left transparent rather than filled with a guess. The (2, 3, 6) group's
    // own translation is 180 columns at this scale, which is the reading that says it came out flat.
    g.set_param(t, "tile", "kind", "kaleido");
    g.set_param(t, "kaleido", "sides", 6.0);
    g.set_param(t, "kaleido", "meet", 3.0);
    g.set_param(t, "kaleido", "hinge", 2.0);
    g.set_param(t, "tile", "scale", 6.0);
    let flat = drawn(&g, t, "a Euclidean kaleidoscope", |d| px(d, 2, 2)[3] > 0.5 && slid(d, 180) < 0.02);
    assert!(slid(&flat, 97) > 0.05, "a flat kaleidoscope that repeats at any shift is a blank one");
    g.set_param(t, "kaleido", "sides", 7.0);
    g.set_param(t, "tile", "scale", 9.0);
    let curved = drawn(&g, t, "a circle limit", |d| px(d, 2, 2)[3] < 0.5);
    assert!(slid(&curved, 120) > 0.05, "a hyperbolic tiling repeats: {}", slid(&curved, 120));
    assert!(px(&curved, SIDE / 2, SIDE / 2)[3] > 0.5, "the disk itself is empty");

    // Step: the multigrid. Two families cross into a lattice; five cut a pattern that never
    // repeats, which is the difference between a crystal and a Penrose tiling — and the reason one
    // knob covers both.
    g.set_param(t, "tile", "kind", "quasi");
    g.set_param(t, "tile", "scale", 3.0);
    g.set_param(t, "quasi", "fold", 2);
    drawn(&g, t, "a periodic multigrid", |d| px(d, 2, 2)[3] > 0.5 && slid(d, CELL) < 0.02);
    g.set_param(t, "quasi", "fold", 5);
    let never = drawn(&g, t, "a multigrid that never repeats", |d| slid(d, CELL) > 0.05);
    for dx in [40, 80, 120, 200] {
        assert!(slid(&never, dx) > 0.03, "the pentagrid repeats at {dx}: {}", slid(&never, dx));
    }
    assert!(g.error(t).is_none(), "{:?}", g.error(t));
}

/// The mosaic's solver, the one part of this node that has to CONVERGE rather than be computed.
/// One Lloyd step a frame walks the sites onto the picture, and what proves the step ran is where
/// the cells end up: told to follow nothing they settle into a lattice of their own, and told to
/// follow the edges they crowd onto the one edge a disc has.
#[test]
fn the_mosaic_walks_its_cells_onto_the_picture() {
    /// Seams per texel in a ring: the disc's rim, or a patch of its flat middle. Small cells mean
    /// more seam for the same room, so this reads as how finely the ring is divided.
    fn seams(d: &goofi_core::Data, rim: bool) -> f32 {
        let (h, w) = (shape(d)[0], shape(d)[1]);
        let v = f32s(d);
        let mut hit = 0.0f32;
        let mut room = 0.0f32;
        for y in 0..h {
            for x in 0..w {
                let q = ((x as f32 / w as f32 - 0.5).powi(2) + (y as f32 / h as f32 - 0.5).powi(2)).sqrt();
                if rim != ((q - 0.3).abs() < 0.04) || (!rim && q > 0.2) {
                    continue;
                }
                room += 1.0;
                let t = &v[(y * w + x) * 4..];
                hit += f32::from(t[0] + t[1] + t[2] < 0.6 && t[3] > 0.5);
            }
        }
        hit / room.max(1.0)
    }

    let g = Goofi::new();
    let disc = g.add("graphics:Shape");
    g.ready(disc);
    let t = g.add("graphics:Tessellate");
    g.ready(t);
    g.link(disc, "out", t, "input");
    for node in [disc, t] {
        g.set_param(node, "common", "width", 192);
        g.set_param(node, "common", "height", 192);
    }
    g.set_param(disc, "shape", "size", 0.6);
    g.set_param(disc, "shape", "soft", 0.01);
    g.set_param(t, "tile", "kind", "mosaic");
    g.set_param(t, "mosaic", "cells", 12);
    g.set_param(t, "mosaic", "rate", 0.5);
    g.set_param(t, "gestalt", "contour", 1.0);
    let probe = g.probe(t, "out");
    let ring = |d: &goofi_core::Data| seams(d, true) / seams(d, false).max(1e-4);

    // Step: with no density to follow the sites relax into an even packing — Lloyd's own answer to
    // a flat picture — so the rim and the middle are divided alike wherever the disc is white.
    g.set_param(t, "mosaic", "density", "flat");
    let even = g.until("the cells settle on nothing in particular", |g| {
        render(g, 60);
        probe.latest().filter(|d| seams(d, false) > 0.05)
    });
    render(&g, 240);
    let even = probe.latest().unwrap_or(even);
    assert!(ring(&even) < 1.12, "a flat density already crowded the rim: {}", ring(&even));

    // Step: told to follow the edges, the same solver walks the same sites onto the one edge the
    // picture has. Nothing about the fold changed — only what the quadrature weighs.
    g.set_param(t, "mosaic", "density", "edges");
    let found = g.until("the cells find the rim", |g| {
        render(g, 60);
        probe.latest().filter(|d| ring(d) > 1.2)
    });
    assert!(ring(&found) > ring(&even) * 1.15, "the rim is no finer than the middle: {}", ring(&found));
    assert!(seams(&found, false) > 0.0, "the middle lost its cells entirely");
    assert!(g.error(t).is_none(), "{:?}", g.error(t));
}

const FOREIGN: &str = "/* goofi\n{ \"doc\": \"claims an audio slot\", \"inputs\": [{\"name\": \"input\", \"kind\": \"AUDIO\"}] }\n*/\nfn shade(uv: vec2f) -> vec4f { return vec4f(uv, 0.0, 1.0); }\n";
const BROKEN: &str = "/* goofi\n{ \"doc\": \"does not compile\" }\n*/\nfn shade(uv: vec2f) -> vec4f { return nothing(uv); }\n";
const COUNT: &str = "/* goofi\n{ \"doc\": \"counts a tenth a tick in a buffer of its own\", \"state\": [\"acc\"] }\n*/\nfn at(uv: vec2f) -> vec2i { return vec2i(floor(uv * resolution)); }\nfn next_acc(uv: vec2f) -> vec4f {\n    if frame == 0u { return vec4f(0.1, 0.75, 0.0, 1.0); }\n    let held = textureLoad(acc, at(uv), 0);\n    return vec4f(held.r + 0.1, held.g, 0.0, 1.0);\n}\nfn shade(uv: vec2f) -> vec4f { return vec4f(textureLoad(acc, at(uv), 0).rgb, 1.0); }\n";
const HALF: &str = "/* goofi\n{ \"doc\": \"half of the input\", \"inputs\": [{\"name\": \"input\", \"kind\": \"TEXTURE\"}] }\n*/\nfn shade(uv: vec2f) -> vec4f { let c = textureSample(input, samp, uv); return vec4f(c.rgb * 0.5, c.a); }\n";
const QUARTER: &str = "/* goofi\n{ \"doc\": \"a quarter of the input\", \"inputs\": [{\"name\": \"input\", \"kind\": \"TEXTURE\"}] }\n*/\nfn shade(uv: vec2f) -> vec4f { let c = textureSample(input, samp, uv); return vec4f(c.rgb * 0.25, c.a); }\n";
