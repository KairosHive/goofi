//! The simulation bundle doing simulation work: models are driven through the one op vocabulary
//! and judged on the BEHAVIOUR they are named for — synchronisation, a bifurcation, an epidemic
//! curve, an avalanche, a memory retrieved — never on a pinned number.

use goofi_tests::{f32s, hex, j, labels, shape, Goofi, Uid};

/// The newest frame of `slot` once `keep` accepts it.
fn settle(g: &Goofi, what: &str, node: Uid, slot: &str, mut keep: impl FnMut(&[f32]) -> bool) -> Vec<f32> {
    let probe = g.probe(node, slot);
    g.until(what, |g| {
        if let Some(e) = g.error(node) {
            panic!("{what}: the node failed instead — {e}");
        }
        probe.latest().map(|d| f32s(&d)).filter(|v| keep(v))
    })
}

/// The newest frame of `slot` that `keep` accepts. A param a step has just set lands on the node's
/// OWN thread, so a frame from before it landed is one to wait past rather than one to read.
fn frame(g: &Goofi, what: &str, node: Uid, slot: &str, keep: impl Fn(&goofi_core::Data) -> bool) -> goofi_core::Data {
    let probe = g.probe(node, slot);
    g.until(what, |g| {
        if let Some(e) = g.error(node) {
            panic!("{what}: the node failed instead — {e}");
        }
        probe.latest().filter(&keep)
    })
}

fn pulse(g: &Goofi, node: Uid, param: &str) {
    g.call("node param pulse", j!({ "node": hex(node), "param": param }));
}

#[test]
fn a_network_of_oscillators_synchronises_when_the_coupling_rises() {
    // Sixty-four oscillators two hertz apart: far enough that weak coupling cannot hold them and
    // strong coupling visibly can, which is the whole content of the Kuramoto model.
    let g = Goofi::new();
    let k = g.add("signal:Kuramoto");
    g.set_param(k, "kuramoto", "size", 64);
    g.set_param(k, "kuramoto", "spread", 2.0);
    g.set_param(k, "kuramoto", "coupling", 0.0);
    g.set_param(k, "sim", "seed", 7);
    g.set_param(k, "output", "sfreq", 2000.0);

    // The phases come out one per oscillator, named, and inside one turn.
    let phases = frame(&g, "one phase per oscillator", k, "phases", |d| shape(d) == vec![64]);
    assert_eq!(labels(&phases, "dim0").first().map(String::as_str), Some("osc0"), "the oscillators are named");
    assert!(f32s(&phases).iter().all(|p| (0.0..std::f64::consts::TAU as f32).contains(p)), "a phase is one turn");

    // Uncoupled, sixty-four oscillators at their own speeds cannot agree for long.
    let scattered = settle(&g, "the uncoupled network to scatter", k, "order", |v| v[0] < 0.4);
    assert!(scattered[0] < 0.4, "{scattered:?}");

    // Coupled hard, they lock — and this is the state the next steps stack on, so it is not soft.
    g.set_param(k, "kuramoto", "coupling", 80.0);
    let locked = settle(&g, "the coupled network to lock", k, "order", |v| v[0] > 0.85);
    assert!(locked[0] > 0.85, "{locked:?}");

    // A block is a signal: one row per oscillator, time along the last axis, at the model's rate.
    g.set_param(k, "output", "mode", "block");
    let block = g.probe(k, "waveforms");
    let frame = g.until("a block of waveforms", |_| block.latest().filter(|d| shape(d).len() == 2));
    assert_eq!(shape(&frame)[0], 64, "a row per oscillator: {:?}", shape(&frame));
    assert_eq!(frame.meta().sfreq(), Some(2000.0), "a block carries the model's own sample rate");
    assert!(f32s(&frame).iter().all(|v| (-1.001..=1.001).contains(v)), "a waveform is a sine");

    // A reset scatters them again, and the same seed puts them back where they started.
    g.set_param(k, "output", "mode", "value");
    g.set_param(k, "kuramoto", "coupling", 0.0);
    pulse(&g, k, "sim/reset");
    let again = settle(&g, "the reset network to scatter again", k, "order", |v| v[0] < 0.4);
    assert!(again[0] < 0.4, "{again:?}");

    // A wired matrix decides the size, and it wins over the param that would otherwise.
    let matrix = g.add("signal:Constant");
    g.set_param(matrix, "constant", "value", 1.0);
    g.set_param(matrix, "constant", "shape", "8, 8");
    settle(&g, "the matrix to take its shape", matrix, "out", |v| v.len() == 64);
    g.link(matrix, "out", k, "coupling");
    let sized = settle(&g, "the network to take the matrix's size", k, "phases", |v| v.len() == 8);
    assert_eq!(sized.len(), 8, "the coupling matrix, not `size`, says how many oscillators there are");
}

#[test]
fn each_model_shows_the_behaviour_it_is_named_for() {
    let g = Goofi::new();

    // Lorenz: normalized into a range a param reference can use, and never still.
    let lorenz = g.add("signal:Attractor");
    g.set_param(lorenz, "output", "sfreq", 5000.0);
    let first = settle(&g, "a point on the attractor", lorenz, "out", |v| v.len() == 3);
    assert!(first.iter().all(|v| v.abs() < 4.0), "normalized, the attractor stays near the unit range: {first:?}");
    let moved = settle(&g, "the attractor to move", lorenz, "out", |v| (v[0] - first[0]).abs() > 0.05);
    assert!(moved != first, "a chaotic system does not sit still");

    // The logistic map is one number and it is a map, so `dt` cannot matter to it.
    g.set_param(lorenz, "attractor", "system", "logistic");
    let logistic = settle(&g, "the logistic map", lorenz, "out", |v| v.len() == 1);
    assert!((0.0..=1.0).contains(&logistic[0]), "the logistic map lives in the unit interval: {logistic:?}");

    // The Hopf bifurcation, which is the one knob `stuartlandau` exists for: below zero the
    // oscillator dies into its fixed point, above zero it holds a cycle of its own.
    let hopf = g.add("Oscillator");
    g.set_param(hopf, "oscillator", "model", "stuartlandau");
    g.set_param(hopf, "oscillator", "nonlinearity", -1.0);
    g.set_param(hopf, "output", "sfreq", 5000.0);
    let dead = settle(&g, "the oscillator below the bifurcation to decay", hopf, "out", |v| v.len() == 2 && v[0].hypot(v[1]) < 0.02);
    assert!(dead[0].hypot(dead[1]) < 0.02, "below the bifurcation there is only the fixed point: {dead:?}");
    g.set_param(hopf, "oscillator", "nonlinearity", 1.0);
    let alive = settle(&g, "the oscillator above the bifurcation to grow a cycle", hopf, "out", |v| v.len() == 2 && v[0].hypot(v[1]) > 0.7);
    assert!(alive[0].hypot(alive[1]) > 0.7, "above it the amplitude settles at the square root of lambda: {alive:?}");

    // An epidemic: everyone starts susceptible, the infection peaks, and it burns out.
    let sir = g.add("Population");
    g.set_param(sir, "population", "model", "sir");
    g.set_param(sir, "population", "predation", 4.0);
    g.set_param(sir, "population", "mortality", 0.5);
    g.set_param(sir, "output", "sfreq", 2000.0);
    let peak = settle(&g, "the outbreak to take hold", sir, "out", |v| v.len() == 3 && v[1] > 0.2);
    assert!(peak[0] < 0.8, "the susceptible share falls as the outbreak grows: {peak:?}");
    let over = settle(&g, "the outbreak to burn out", sir, "out", |v| v.len() == 3 && v[1] < 0.01 && v[2] > 0.5);
    assert!(over[2] > 0.5, "most of the population ends up recovered: {over:?}");

    // Kauffman's knob: one connection each freezes the network, five leaves it churning.
    let rbn = g.add("signal:Boolean");
    g.set_param(rbn, "boolean", "size", 256);
    g.set_param(rbn, "boolean", "connections", 1);
    g.set_param(rbn, "output", "sfreq", 200.0);
    let frozen = settle(&g, "the frozen network to stop changing", rbn, "activity", |v| v[0] < 0.02);
    assert!(frozen[0] < 0.02, "at one connection each the network freezes: {frozen:?}");
    g.set_param(rbn, "boolean", "connections", 5);
    let churning = settle(&g, "the chaotic network to churn", rbn, "activity", |v| v[0] > 0.1);
    assert!(churning[0] > 0.1, "at five it never settles: {churning:?}");

    // Vicsek's transition: quiet particles align, noisy ones do not.
    let flock = g.add("signal:Swarm");
    g.set_param(flock, "swarm", "model", "vicsek");
    g.set_param(flock, "swarm", "count", 200);
    g.set_param(flock, "swarm", "radius", 0.15);
    g.set_param(flock, "swarm", "noise", 0.0);
    g.set_param(flock, "output", "sfreq", 200.0);
    let ordered = settle(&g, "the quiet swarm to align", flock, "order", |v| v[0] > 0.8);
    assert!(ordered[0] > 0.8, "with no noise the swarm points one way: {ordered:?}");
    g.set_param(flock, "swarm", "noise", 5.0);
    let disordered = settle(&g, "the noisy swarm to lose its heading", flock, "order", |v| v[0] < 0.5);
    assert!(disordered[0] < 0.5, "noise breaks the alignment: {disordered:?}");

    // Avalanches: below the critical point they die out, above it they take the network.
    let avalanche = g.add("signal:Branching");
    g.set_param(avalanche, "branching", "size", 2000);
    g.set_param(avalanche, "branching", "branching", 0.5);
    g.set_param(avalanche, "branching", "drive", 0.002);
    g.set_param(avalanche, "output", "sfreq", 200.0);
    let quiet = settle(&g, "the subcritical network to stay quiet", avalanche, "activity", |v| v[0] < 0.05);
    assert!(quiet[0] < 0.05, "below one, activity dies: {quiet:?}");
    g.set_param(avalanche, "branching", "branching", 2.0);
    let storm = settle(&g, "the supercritical network to catch", avalanche, "activity", |v| v[0] > 0.1);
    assert!(storm[0] > 0.1, "above one, it spreads: {storm:?}");

    // A spiking network: below threshold it is silent, and one knob turns it on.
    let net = g.add("signal:Spiking");
    g.set_param(net, "network", "size", 200);
    g.set_param(net, "neuron", "drive", 0.0);
    g.set_param(net, "neuron", "noise", 0.0);
    g.set_param(net, "network", "weight", 0.0);
    g.set_param(net, "output", "sfreq", 4000.0);
    let silent = settle(&g, "the undriven network to fall silent", net, "rate", |v| v[0] == 0.0);
    assert_eq!(silent[0], 0.0, "with nothing driving them, no neuron reaches threshold");
    g.set_param(net, "neuron", "drive", 1.5);
    let firing = settle(&g, "the driven network to fire", net, "rate", |v| v[0] > 0.0);
    assert!(firing[0] > 0.0, "driven past threshold, the network spikes: {firing:?}");
    frame(&g, "one potential per neuron", net, "potentials", |d| shape(d) == vec![200]);

    // Quantum: a statevector is normalized, and entanglement is what the ring of gates buys.
    let q = g.add("signal:Quantum");
    g.set_param(q, "quantum", "qubits", 4);
    g.set_param(q, "quantum", "entangle", false);
    let probabilities = settle(&g, "sixteen amplitudes for four qubits", q, "probabilities", |v| v.len() == 16);
    let total: f32 = probabilities.iter().sum();
    assert!((total - 1.0).abs() < 1e-3, "the probabilities sum to one, not {total}");
    let separable = settle(&g, "the unentangled circuit", q, "entropy", |_| true);
    assert!(separable[0] < 1e-3, "with no two-qubit gate every qubit stays its own: {separable:?}");
    g.set_param(q, "quantum", "entangle", true);
    let entangled = settle(&g, "the entangled circuit", q, "entropy", |v| v[0] > 0.1);
    assert!(entangled[0] > 0.1, "the ring of controlled-nots entangles them: {entangled:?}");

    // Slime mould: the field is an image, and it grows from nothing.
    let mould = g.add("signal:Physarum");
    g.set_param(mould, "physarum", "size", 64);
    g.set_param(mould, "physarum", "agents", 2000);
    frame(&g, "the trail is a square field", mould, "trail", |d| shape(d) == vec![64, 64]);
    let grown = settle(&g, "the trail to be laid down", mould, "trail", |v| v.iter().any(|x| *x > 0.5));
    assert!(grown.iter().any(|x| *x > 0.5), "the agents deposit something");
}

#[test]
fn a_memory_completes_a_fragment_and_a_readout_learns_to_predict() {
    let g = Goofi::new();

    // A Hopfield network told nothing settles into one of the memories it was given, and says
    // which: exactly one overlap goes high while the rest stay near zero.
    let memory = g.add("signal:Hopfield");
    g.set_param(memory, "hopfield", "size", 128);
    g.set_param(memory, "hopfield", "patterns", 3);
    g.set_param(memory, "hopfield", "beta", 20.0);
    g.set_param(memory, "sim", "seed", 3);
    g.set_param(memory, "output", "sfreq", 100.0);
    let overlap = settle(&g, "the memory to settle into a stored pattern", memory, "overlap", |v| {
        v.iter().any(|o| o.abs() > 0.9)
    });
    let strong = overlap.iter().filter(|o| o.abs() > 0.9).count();
    assert_eq!(strong, 1, "it falls into ONE memory, not a blur of them: {overlap:?}");
    frame(&g, "the state is as wide as a memory", memory, "state", |d| shape(d) == vec![128]);

    // A reservoir driven by a slow wave, and a readout trained to say what the wave is doing. The
    // claim is not a number: it is that training makes the error smaller than not training does.
    let wave = g.add("LFO");
    g.set_param(wave, "lfo", "frequency", 0.5);
    let pool = g.add("signal:Reservoir");
    g.set_param(pool, "reservoir", "size", 60);
    g.set_param(pool, "reservoir", "spectral_radius", 0.9);
    g.set_param(pool, "output", "sfreq", 100.0);
    let readout = g.add("Readout");
    g.set_param(readout, "readout", "learning", false);
    g.link(wave, "out", pool, "input");
    g.link(pool, "state", readout, "state");
    g.link(wave, "out", readout, "target");

    let untrained = settle(&g, "the untrained readout to answer", readout, "error", |v| v[0].abs() > 0.0);
    frame(&g, "a weight per unit of the pool", readout, "weights", |d| shape(d) == vec![1, 60]);

    g.set_param(readout, "readout", "learning", true);
    let trained = settle(&g, "the readout to learn the wave", readout, "error", |v| v[0].abs() < 0.02);
    assert!(
        trained[0].abs() < untrained[0].abs(),
        "training brought the error down from {} to {}",
        untrained[0].abs(),
        trained[0].abs()
    );

    // Frozen, it keeps predicting: the weights are the model, not the training loop.
    g.set_param(readout, "readout", "learning", false);
    let frozen = settle(&g, "the frozen readout to keep predicting", readout, "error", |v| v[0].abs() < 0.1);
    assert!(frozen[0].abs() < 0.1, "what it learned survives the training being turned off: {frozen:?}");
}

#[test]
fn the_cortical_model_answers_in_the_band_it_is_named_for() {
    // Jansen-Rit is in the library because it makes an alpha rhythm, and a claim like that is
    // measured through the same chain a user would measure it with — a window and a spectrum.
    let g = Goofi::new();
    let mass = g.add("NeuralMass");
    g.set_param(mass, "mass", "model", "jansenrit");
    g.set_param(mass, "output", "mode", "block");
    g.set_param(mass, "output", "sfreq", 1000.0);
    g.set_param(mass, "sim", "seed", 11);
    let buffer = g.add("Buffer");
    g.set_param(buffer, "buffer", "size", 2048);
    let psd = g.add("Psd");
    g.set_param(psd, "psd", "mode", "fft");

    let spectrum = g.probe(psd, "out");
    g.link(mass, "out", buffer, "input");
    g.link(buffer, "out", psd, "input");

    // 2048 samples at a kilohertz is a two-second window, so a bin is just under half a hertz.
    let full = g.until("a spectrum of a full window", |g| {
        if let Some(e) = g.error(mass) {
            panic!("the model failed instead: {e}");
        }
        spectrum.latest().filter(|d| shape(d) == vec![1, 1025])
    });
    let coords = full.meta().channels().get(1).and_then(|a| a.coords.clone()).expect("bin coords in hertz");
    let power = f32s(&full);
    // Below one hertz is the model settling from its starting state, not a rhythm.
    let band: Vec<usize> = (0..power.len())
        .filter(|i| matches!(coords[*i], goofi_core::Coord::Num(hz) if hz >= 1.0))
        .collect();
    let peak = *band.iter().max_by(|a, b| power[**a].total_cmp(&power[**b])).expect("a peak");
    let goofi_core::Coord::Num(hz) = coords[peak] else { panic!("the frequency axis is numbers") };
    assert!(
        (7.0..14.0).contains(&hz),
        "Jansen-Rit is in the library for its alpha rhythm, and it peaked at {hz} Hz instead"
    );
}

/// One rendered frame from `uid`'s output that `want` accepts, driving the external clock.
fn drawn(g: &Goofi, uid: Uid, what: &str, want: impl Fn(&goofi_core::Data) -> bool) -> goofi_core::Data {
    let probe = g.probe(uid, "out");
    g.until(what, |g| {
        if let Some(e) = g.error(uid) {
            panic!("{what}: the node failed instead — {e}");
        }
        goofi_tests::render(g, 1);
        probe.latest().filter(&want)
    })
}

#[test]
fn the_automata_keep_a_grid_of_their_own_and_it_goes_somewhere() {
    // Each of these holds state between ticks, so the claim is not "it drew something" but "what
    // it drew CHANGED, and it is not a flat field" — which is what a dead automaton would give.
    let g = Goofi::new();
    for ty in ["Lenia", "Reaction", "Ising", "Sandpile", "Elementary", "Wave", "NeuralCA"] {
        let node = g.add(&format!("graphics:{ty}"));
        g.set_param(node, "common", "width", 64);
        g.set_param(node, "common", "height", 64);
        g.ready(node);
        let seeded = drawn(&g, node, &format!("{ty} to seed its grid"), |d| {
            let v = f32s(d);
            v.iter().any(|x| x.abs() > 0.01) && v.iter().any(|x| (x - v[0]).abs() > 0.01)
        });
        let first = f32s(&seeded);
        let moved = drawn(&g, node, &format!("{ty} to advance its own grid"), |d| {
            f32s(d).iter().zip(&first).any(|(a, b)| (a - b).abs() > 0.01)
        });
        assert_eq!(shape(&moved), vec![64, 64, 4], "{ty} draws a frame the size it was told to");
        assert!(g.error(node).is_none(), "{ty} stands with an error");

        // A rule that settles the whole grid on ONE value has drawn something and gone nowhere.
        // `NeuralCA` did exactly that — its bias reached every cell, alive or not — and the two
        // steps above both passed over it, so the pattern has to be judged after it has had time.
        goofi_tests::render(&g, 300);
        let settled = drawn(&g, node, &format!("{ty} to still hold a pattern"), |_| true);
        let v = f32s(&settled);
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        let spread = (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt();
        assert!(spread > 0.01, "{ty} settled into one flat colour after 300 ticks (spread {spread})");
        g.call("node remove", j!({ "node": hex(node) }));
    }
}

#[test]
fn a_learned_automaton_grows_from_its_seed_instead_of_flooding_the_grid() {
    // The defect this pins: the rule's BIAS reaches every cell, so with nothing holding the dead
    // ones down the whole grid walks to the same value and the node shows one flat colour. The
    // mechanism is what is testable — growth may only spread from what is already alive, so a
    // single dot in the middle cannot have reached the corners after a handful of ticks.
    let g = Goofi::new();
    let node = g.add("graphics:NeuralCA");
    g.set_param(node, "common", "width", 64);
    g.set_param(node, "common", "height", 64);
    g.set_param(node, "neuralca", "start", "dot");
    g.ready(node);

    // `shade` maps the state through `rgb * 0.5 + 0.5`, so a DEAD cell renders mid-grey and the
    // alpha it writes is always 1. What says "alive" in a rendered frame is the distance from 0.5.
    let lit = |d: &goofi_core::Data, at: &[(usize, usize)]| {
        let v = f32s(d);
        at.iter()
            .flat_map(|(r, c)| (0..3).map(move |k| (r, c, k)))
            .map(|(r, c, k)| (v[r * 64 * 4 + c * 4 + k] - 0.5).abs())
            .fold(0.0f32, f32::max)
    };
    let corners = [(0usize, 0usize), (0, 63), (63, 0), (63, 63)];
    let middle = [(32usize, 32usize)];

    let first = drawn(&g, node, "the dot to be seeded", |d| {
        f32s(d).chunks_exact(4).any(|px| (px[0] - 0.5).abs() > 0.2)
    });
    assert!(lit(&first, &corners) < 0.05, "the seed is one dot in the middle, so the corners start dead");
    assert!(lit(&first, &middle) > 0.2, "the dot itself is lit");

    goofi_tests::render(&g, 20);
    let after = drawn(&g, node, "twenty ticks of growth", |_| true);
    assert!(
        lit(&after, &corners) < 0.05,
        "growth reached the corners in 20 ticks from a dot 45 cells away — the rule is flooding the \
         grid rather than growing into it (corner deviation {})",
        lit(&after, &corners)
    );
    // …and it is not simply frozen: the seed is still alive after all that.
    assert!(lit(&after, &middle) > 0.05, "the seed itself is still alive after twenty ticks");
    assert!(g.error(node).is_none(), "NeuralCA stands with an error");
}

/// Every model in this pack that ships a graphics half beside it, and the slots that half draws —
/// read off the two manifests rather than listed here, so a new pair joins the scenario by
/// existing and a renamed slot fails it.
fn pairs(g: &Goofi) -> Vec<(String, Vec<String>)> {
    let listed = g.call("library list", j!({ "full": true }));
    let types = listed["types"].as_array().expect("a palette").clone();
    let outputs = |ty: &str| {
        types
            .iter()
            .find(|r| r["type"] == j!(ty))
            .and_then(|r| r["output_slots"].as_object().cloned())
            .map(|o| o.keys().cloned().collect::<Vec<String>>())
    };
    let mut found: Vec<(String, Vec<String>)> = types
        .iter()
        .filter(|r| r["bundle"] == j!("simulation"))
        .filter_map(|r| Some(r["type"].as_str()?.strip_prefix("graphics:")?.to_string()))
        .map(|bare| {
            let half = format!("graphics:{bare}");
            let slots: Vec<String> = types
                .iter()
                .find(|r| r["type"] == j!(half))
                .and_then(|r| r["input_slots"].as_object().cloned())
                .expect("the half's own inputs")
                .keys()
                .cloned()
                .collect();
            // The whole of what makes it a PAIR: every slot the picture takes is an output of the
            // model it is named after, so wiring one needs nothing looked up.
            let model = outputs(&format!("signal:{bare}")).unwrap_or_else(|| panic!("{bare} has no model"));
            for slot in &slots {
                assert!(model.contains(slot), "graphics:{bare} draws `{slot}`, which signal:{bare} does not emit");
            }
            (bare, slots)
        })
        .collect();
    found.sort();
    assert!(found.len() >= 10, "the simulation pack ships a graphics half per model: {found:?}");
    found
}

/// How much a rendered frame differs from the same node's blank one, as a share of its texels.
fn moved(frame: &goofi_core::Data, blank: &[f32]) -> f32 {
    let v = f32s(frame);
    let apart = v.iter().zip(blank).filter(|(a, b)| (*a - *b).abs() > 0.02).count();
    apart as f32 / v.len() as f32
}

#[test]
fn every_model_draws_its_own_picture_and_the_picture_is_the_model() {
    let g = Goofi::new();

    for (model, slots) in pairs(&g) {
        let view = g.add(&format!("graphics:{model}"));
        g.set_param(view, "common", "width", 64);
        g.set_param(view, "common", "height", 64);
        g.ready(view);

        // With nothing behind it a picture is EMPTY rather than decorative — which is what makes
        // the difference below the model's own doing and not the shader's.
        let empty = f32s(&drawn(&g, view, &format!("{model} to draw its ground"), |_| true));
        let flat = empty.chunks_exact(4).all(|px| px.iter().zip(&empty[..4]).all(|(a, b)| (a - b).abs() < 1e-3));
        assert!(flat, "graphics:{model} draws something of its own with nothing wired");

        let sim = g.add(&format!("signal:{model}"));
        // Three models draw nothing worth judging at their defaults: the pool is asleep until it
        // is driven, the network is too far below criticality to cascade, and the mould's field is
        // a bigger frame than a 64-pixel picture of it has any use for.
        let mut drive = None;
        match model.as_str() {
            "Reservoir" => {
                let wave = g.add("LFO");
                g.set_param(wave, "lfo", "frequency", 2.0);
                g.link(wave, "out", sim, "input");
                drive = Some(wave);
            }
            "Branching" => {
                g.set_param(sim, "branching", "size", 200);
                g.set_param(sim, "branching", "branching", 1.0);
                g.set_param(sim, "branching", "drive", 0.02);
            }
            "Physarum" => {
                g.set_param(sim, "physarum", "size", 64);
                g.set_param(sim, "physarum", "agents", 2000);
            }
            _ => {}
        }
        for slot in &slots {
            g.link(sim, slot, view, slot);
        }

        let picture = drawn(&g, view, &format!("{model} to reach its picture"), |d| moved(d, &empty) > 0.005);
        assert!(moved(&picture, &empty) > 0.005, "graphics:{model} drew nothing the model put there");
        assert_eq!(shape(&picture), vec![64, 64, 4], "graphics:{model} draws the size it was told to");
        assert!(g.error(view).is_none(), "graphics:{model} stands with an error");
        assert!(g.error(sim).is_none(), "signal:{model} stands with an error");

        g.call("node remove", j!({ "node": hex(view) }));
        g.call("node remove", j!({ "node": hex(sim) }));
        if let Some(wave) = drive {
            g.call("node remove", j!({ "node": hex(wave) }));
        }
    }
}

/// How far out from the middle anything is drawn, as a share of the half-frame, staying inside the
/// ring the dots sit on: on a phase circle that leaves the order vector and nothing else.
fn reach(frame: &goofi_core::Data, side: usize) -> f32 {
    let v = f32s(frame);
    let mid = side as f32 * 0.5;
    let mut far = 0.0f32;
    for row in 0..side {
        for col in 0..side {
            let out = (row as f32 + 0.5 - mid).hypot(col as f32 + 0.5 - mid) / mid;
            if out < 0.80 && v[(row * side + col) * 4 + 3] > 0.5 {
                far = far.max(out);
            }
        }
    }
    far
}

#[test]
fn the_phase_circle_lengthens_its_arrow_as_the_oscillators_fall_into_step() {
    // The claim a picture has to answer for: it is not decoration, it MEASURES. The order vector
    // is the one thing on a phase circle that moves with the model rather than with the frame, so
    // the same coupling sweep the model is judged by is run again through the picture alone.
    let g = Goofi::new();
    let k = g.add("signal:Kuramoto");
    g.set_param(k, "kuramoto", "size", 64);
    g.set_param(k, "kuramoto", "spread", 2.0);
    g.set_param(k, "kuramoto", "coupling", 0.0);
    g.set_param(k, "sim", "seed", 7);
    g.set_param(k, "output", "sfreq", 2000.0);

    let circle = g.add("graphics:Kuramoto");
    g.set_param(circle, "common", "width", 192);
    g.set_param(circle, "common", "height", 192);
    // The guide ring off and the dots thin, so the only thing left inside the ring is the arrow.
    g.set_param(circle, "circle", "ring", 0.0);
    g.set_param(circle, "circle", "radius", 0.9);
    g.set_param(circle, "circle", "dot", 1.5);
    g.ready(circle);
    g.link(k, "phases", circle, "phases");

    let scattered = drawn(&g, circle, "the uncoupled circle", |d| reach(d, 192) > 0.0 && reach(d, 192) < 0.45);
    assert!(reach(&scattered, 192) < 0.45, "uncoupled, the order vector barely leaves the middle");

    g.set_param(k, "kuramoto", "coupling", 80.0);
    let locked = drawn(&g, circle, "the coupled circle", |d| reach(d, 192) > 0.70);
    assert!(reach(&locked, 192) > 0.70, "locked, it reaches most of the way out to the ring");
}
