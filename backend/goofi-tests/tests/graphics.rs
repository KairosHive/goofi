//! The graphics engine under the external clock: one session, every action through the op
//! vocabulary, every probe a subscriber on the derived name of a texture slot — the door `/data`
//! opens — and every frame a readback the GPU actually made.

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
    assert_eq!(shape(&frame), vec![512, 512, 4], "a node with nothing behind it is 512 square");
    assert!(close(px(&frame, 511, 511), [0.25, 0.5, 1.0, 1.0]), "the same colour to the far corner");

    // Step: the universal `output` group resizes it, and what is wired behind FOLLOWS the size.
    g.set_param(c, "output", "width", 64);
    g.set_param(c, "output", "height", 32);
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
    assert_eq!(shape(&frame), vec![512, 512, 4], "with nothing to follow, it is a generator's size");
    assert!(g.error(level).is_none(), "an unwired input is not a fault");

    // Step: a signal frame uploads — the fixture's gradient, sampled back texel for texel and the
    // right way up. The one place the two row orders could disagree.
    let img = g.add("_TestImage");
    g.ready(img);
    let up = g.add("graphics:ArrayIn");
    g.ready(up);
    g.set_param(up, "output", "width", 4);
    g.set_param(up, "output", "height", 4);
    g.link(img, "out", up, "input");
    let frame = drawn(&g, up, "the uploaded image", |d| shape(d) == vec![4, 4, 4]);
    assert!(close(px(&frame, 0, 0), [0.0, 1.0, 0.5, 1.0]), "row 0 is the top: {:?}", px(&frame, 0, 0));
    assert!(close(px(&frame, 3, 3), [1.0, 0.0, 0.5, 1.0]), "and row 3 the bottom: {:?}", px(&frame, 3, 3));

    g.call("link remove", j!({ "from": ep(hex(img), "out"), "to": ep(hex(up), "input") }));
    drawn(&g, up, "the unlinked upload", |d| close(px(d, 0, 0), [0.0, 0.0, 0.0, 0.0]));

    // Step: a texture chain does not flip either. A gradient down the frame, copied by a Level,
    // still runs the same way — the half of the orientation rule an upload cannot see.
    let vert = g.add("graphics:Ramp");
    g.ready(vert);
    g.set_param(vert, "ramp", "angle", 90.0);
    g.set_param(vert, "output", "width", 8);
    g.set_param(vert, "output", "height", 8);
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
    g.link(c, "out", level, "input");
    g.call("node param edit", j!({ "node": hex(level), "param": "level/gain",
                                  "reference": format!("{knob_name}.out"), "mode": "reference" }));
    drawn(&g, level, "half gain by reference", |d| close(px(d, 0, 0), [0.125, 0.25, 0.5, 1.0]));
    g.set_param(knob, "control", "value", 2.0);
    drawn(&g, level, "double gain by reference", |d| close(px(d, 0, 0), [0.5, 1.0, 2.0, 1.0]));

    // Step: the `output` size is settled state, so a reference on it is refused in words rather
    // than accepted and quietly ignored.
    g.call("node param edit", j!({ "node": hex(level), "param": "output/width",
                                   "reference": format!("{knob_name}.out"), "mode": "reference" }));
    let why = g.until("the engine says the size takes no reference", |g| {
        render(g, 1);
        g.error(level)
    });
    assert!(why.contains("settled state"), "{why}");
    g.call("node param edit", j!({ "node": hex(level), "param": "output/width", "mode": "constant" }));
    g.until("and it clears when the reference goes", |g| {
        render(g, 1);
        g.error(level).is_none().then_some(())
    });

    // Step: a loop closes through Feedback and accumulates a tenth a tick; one without it faults.
    let fb = g.add("graphics:Feedback");
    g.ready(fb);
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
    // Against the bundle on disk, not a number: a fourteenth node must not fail the suite for
    // existing, and a node that stops registering must fail it.
    let bundle = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../node-bundles/graphics");
    let mut want: Vec<String> = std::fs::read_dir(&bundle)
        .expect("the shipped bundle")
        .filter_map(|e| e.ok())
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
    g.set_param(win, "output", "width", 64);
    g.set_param(win, "output", "height", 32);
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
    g.set_param(big, "output", "width", 1024);
    g.set_param(big, "output", "height", 512);

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

const FOREIGN: &str = "/* goofi\n{ \"doc\": \"claims an audio slot\", \"inputs\": [{\"name\": \"input\", \"kind\": \"AUDIO\"}] }\n*/\nfn shade(uv: vec2f) -> vec4f { return vec4f(uv, 0.0, 1.0); }\n";
const BROKEN: &str = "/* goofi\n{ \"doc\": \"does not compile\" }\n*/\nfn shade(uv: vec2f) -> vec4f { return nothing(uv); }\n";
const HALF: &str = "/* goofi\n{ \"doc\": \"half of the input\", \"inputs\": [{\"name\": \"input\", \"kind\": \"TEXTURE\"}] }\n*/\nfn shade(uv: vec2f) -> vec4f { let c = textureSample(input, samp, uv); return vec4f(c.rgb * 0.5, c.a); }\n";
const QUARTER: &str = "/* goofi\n{ \"doc\": \"a quarter of the input\", \"inputs\": [{\"name\": \"input\", \"kind\": \"TEXTURE\"}] }\n*/\nfn shade(uv: vec2f) -> vec4f { let c = textureSample(input, samp, uv); return vec4f(c.rgb * 0.25, c.a); }\n";
