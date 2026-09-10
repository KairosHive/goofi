//! The bundles goofi's own repo ships under `node-bundles/`, run as a user gets them: the real
//! `.py` files, installed through the same probe the CLI's scan uses, wired to real producers and
//! read back.
#![cfg(not(feature = "embed"))]

use std::path::{Path, PathBuf};

use goofi_tests::{f32s, hex, install, install_all, j, labels, require_python, shape, Goofi};

/// The `.py` files a bundle ships, read as they are checked in.
fn bundled(bundle: &str, files: &[&str]) -> Vec<(String, String)> {
    files
        .iter()
        .map(|file| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../node-bundles").join(bundle).join(file);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read the bundled node {}: {e}", path.display()));
            (file.to_string(), source)
        })
        .collect()
}

/// One of a bundle's files, installed through the same seam a user's own file takes.
fn install_bundled(g: &Goofi, bundle: &str, file: &str) -> String {
    let [(file, source)] = bundled(bundle, &[file]).try_into().expect("one file");
    install(g, &file, &source)
}

/// Several of a bundle's files under ONE refresh, as a real scan takes a folder.
fn install_bundled_all(g: &Goofi, bundle: &str, files: &[&str]) -> Vec<String> {
    let sources = bundled(bundle, files);
    let pairs: Vec<(&str, &str)> = sources.iter().map(|(f, s)| (f.as_str(), s.as_str())).collect();
    install_all(g, &pairs)
}

/// A required slot still empty is a node WAITING for the producer wired to it — the runtime faults
/// on it every tick until the first frame lands — rather than a node that failed.
fn waiting(e: &str) -> bool {
    e.contains("has no data")
}

/// Wait for a node to come up, reading its error channel WHILE waiting so a node that says why
/// fails with its own words, then for a frame `keep` accepts.
fn first_frame(
    g: &Goofi,
    ty: &str,
    node: goofi_tests::Uid,
    probe: &goofi_tests::OutputProbe,
    mut keep: impl FnMut(&goofi_core::Data) -> bool,
) -> goofi_core::Data {
    g.until(&format!("{ty} to start"), |g| {
        if let Some(e) = g.error(node).filter(|e| !waiting(e)) {
            panic!("{ty} failed to start: {e}");
        }
        (g.stage(node) == "ready").then_some(())
    });
    g.until(&format!("{ty} to answer once it is ready"), |g| {
        if let Some(e) = g.error(node).filter(|e| !waiting(e)) {
            panic!("{ty} failed instead of answering: {e}");
        }
        probe.latest().filter(&mut keep)
    })
}

#[test]
fn the_complexity_bundle_reduces_the_time_axis_and_leaves_the_channels_alone() {
    // A frame that is NOT a vector: against a single channel a flattening node reads as correct.
    let _py = require_python();
    let g = Goofi::new();
    let src = g.add("_TestGrid");
    let named = g.add("Meta");
    g.set_param(named, "meta", "labels", "Fz,Cz,Pz");
    let buf = g.add("Buffer");
    g.set_param(buf, "buffer", "unit", "samples");
    g.set_param(buf, "buffer", "size", 256);
    g.link(src, "out", named, "input");
    g.link(named, "out", buf, "input");
    // The window fills BEFORE a node is wired: a growing one answers, and answers differently.
    let window = g.probe(buf, "out");
    g.until("a full window", |_| window.latest().filter(|d| shape(d) == vec![3, 256]));

    // All registered by ONE scan, which probes them in parallel — so the ceiling below covers
    // the node boots rather than the probes.
    let files = [
        ("lempel_ziv.py", "complexity"),
        ("permutation_entropy.py", "entropy"),
        ("spectral_entropy.py", "entropy"),
        ("detrended_fluctuation.py", "exponent"),
        ("sample_entropy.py", "entropy"),
        ("hjorth.py", "complexity"),
        ("fractal_dimension.py", "dimension"),
        ("zero_crossings.py", "count"),
    ];
    let types = install_bundled_all(&g, "complexity", &files.map(|(file, _)| file));
    let nodes: Vec<_> = types
        .into_iter()
        .zip(files)
        .map(|(ty, (_, slot))| {
            let node = g.add(&ty);
            let probe = g.probe(node, slot);
            g.link(buf, "out", node, "data");
            (ty, node, probe)
        })
        .collect();

    for (ty, node, probe) in nodes {
        let d = first_frame(&g, &ty, node, &probe, |d| shape(d) == vec![3]);
        let v = f32s(&d);
        assert!(v.iter().all(|x| x.is_finite()), "{ty} answered {v:?}");
        // The three rows are one signal at three offsets, so answers that DISAGREE mean a mix.
        assert!(
            v.iter().all(|x| (x - v[0]).abs() <= v[0].abs() * 1e-3 + 1e-4),
            "{ty} read the three channels as three different signals: {v:?}",
        );
        // A value per channel is only a topomap while the channels are still named.
        assert_eq!(labels(&d, "dim0"), ["Fz", "Cz", "Pz"], "{ty} kept the channel names");
        assert!(g.error(node).is_none(), "{ty} carries no error: {:?}", g.error(node));
    }
}

#[test]
fn a_complexity_node_reads_a_real_signal_rather_than_answering_a_constant() {
    // An 8 Hz sine over a one-second window has known answers: 15 or 16 zero crossings, a Hjorth
    // complexity of exactly 1, and every entropy solidly inside its range rather than at an edge.
    let _py = require_python();
    let g = Goofi::new();
    let osc = g.add("LFO");
    let buf = g.add("Buffer");
    g.set_param(osc, "output", "sfreq", 256.0);
    g.set_param(osc, "output", "mode", "block");
    g.set_param(osc, "lfo", "frequency", 8.0);
    g.set_param(buf, "buffer", "unit", "samples");
    g.set_param(buf, "buffer", "size", 256);
    g.link(osc, "out", buf, "input");
    // Every oracle below is for a FULL window: a growing one holds fewer cycles, or one cut short.
    let window = g.probe(buf, "out");
    g.until("a full window", |_| window.latest().filter(|d| shape(d) == vec![256]));

    let files = [
        ("permutation_entropy.py", "entropy", 0.3..0.9),
        ("sample_entropy.py", "entropy", 0.05..0.8),
        ("svd_entropy.py", "entropy", 0.1..0.7),
        ("spectral_entropy.py", "entropy", 0.0..0.5),
        ("lempel_ziv.py", "complexity", 0.0..0.5),
        ("hjorth.py", "complexity", 0.9..1.1),
        ("fractal_dimension.py", "dimension", 1.0..1.1),
        ("zero_crossings.py", "count", 13.0..17.5),
    ];
    let types = install_bundled_all(&g, "complexity", &files.clone().map(|(file, _, _)| file));
    let nodes: Vec<_> = types
        .into_iter()
        .zip(files)
        .map(|(ty, (_, slot, range))| {
            let node = g.add(&ty);
            let probe = g.probe(node, slot);
            g.link(buf, "out", node, "data");
            (ty, node, probe, range)
        })
        .collect();

    // The MIDDLE of several readings, not the first. Eight Python subprocesses come up at once
    // here, and a machine that stutters under them hands the buffer a window stitched from blocks
    // that do not run on from one another. That window is a real discontinuity — it reads as extra
    // zero crossings and a Hjorth far off 1 — and it is the harness stumbling, not the node. One
    // such window cannot move a median; a node that truly answers a constant cannot hide behind it.
    for (ty, node, probe, range) in nodes {
        first_frame(&g, &ty, node, &probe, |d| shape(d) == vec![1]);
        // One reading per NEW frame, counted rather than compared: a node whose answer is a
        // constant — which a steady sine makes several of these — never changes value, and waiting
        // for five different numbers would wait forever.
        let mut seen: Vec<f32> = Vec::new();
        let mut at = probe.count();
        g.until(&format!("{ty} to answer five frames"), |_| {
            if probe.count() > at {
                at = probe.count();
                if let Some(v) = probe.latest().filter(|d| shape(d) == vec![1]).map(|d| f32s(&d)[0]) {
                    seen.push(v);
                }
            }
            (seen.len() >= 5).then_some(())
        });
        seen.sort_by(f32::total_cmp);
        let v = seen[seen.len() / 2];
        assert!(range.contains(&v), "{ty} of an 8 Hz sine is {v}, outside {range:?} (saw {seen:?})");
    }
}

#[test]
fn every_bundle_names_its_packages_and_the_interpreter_asked_of_holds_them() {
    // Provisioning installs each bundle's `requirements.txt` and startup checks the same files, so
    // a Python bundle without one, or an interpreter short of one, is what either would silently pass.
    // `requirements-gil.txt` is the exception and it is asked only of the subprocess interpreter,
    // because a package with no free-threaded wheel is exactly what that tier exists for.
    let root = goofi_init::repo_root();
    let bundles = goofi_init::bundle_dirs(&root);
    assert!(!bundles.is_empty(), "the repo ships bundles under node-bundles/");
    let pythonic = |b: &Path| {
        std::fs::read_dir(b).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x == "py"))
    };
    for b in bundles.iter().filter(|b| pythonic(b)) {
        assert!(b.join("requirements.txt").is_file() || b.join("requirements-gil.txt").is_file(),
                "{} names its packages for at least one interpreter", b.display());
    }
    let shared = goofi_init::requirements_in(&bundles);
    let gil_only: Vec<PathBuf> =
        shared.iter().cloned().chain(goofi_init::gil_requirements_in(&bundles)).collect();
    for path in &gil_only {
        std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{} must be UTF-8: {e}", path.display()));
    }
    let gap = std::env::temp_dir().join(format!("goofi-gap-{}.txt", std::process::id()));
    std::fs::write(&gap, "cowsay\n").unwrap();
    for (venv, reqs) in [(goofi_init::FT_VENV, &shared), (goofi_init::GIL_VENV, &gil_only)] {
        let py = goofi_init::venv_python(&root.join(venv))
            .unwrap_or_else(|| panic!("no {venv}: {}", goofi_init::RUN_ME));
        let missing = goofi_init::missing_packages(&py, reqs).expect("uv audits the interpreter");
        assert!(missing.is_empty(), "{venv} lacks {missing:?}: {}", goofi_init::RUN_ME);
        // The check can SEE a gap: naming what is absent takes the index, as the install would.
        let missing = goofi_init::missing_packages(&py, std::slice::from_ref(&gap)).expect("uv resolves the gap");
        assert!(missing.iter().any(|m| m.starts_with("cowsay==")), "{venv}: the gap is named: {missing:?}");
    }
}

#[test]
fn invalid_requirements_fail_before_checking_or_installing_packages() {
    let dir = tempfile::tempdir().unwrap();
    let requirements = dir.path().join("requirements-gil.txt");
    std::fs::write(&requirements, b"# invalid comment: \x97\nnumpy\n").unwrap();
    let python = dir.path().join("no-interpreter");
    for error in [
        goofi_init::missing_packages(&python, std::slice::from_ref(&requirements)).unwrap_err(),
        goofi_init::install_packages(&python, std::slice::from_ref(&requirements)).unwrap_err(),
    ] {
        assert!(error.contains("requirements-gil.txt") && error.contains("UTF-8"), "{error}");
    }
}

/// A python of the tier's own, spawned as the node tier spawns one: the embedding's
/// `PYTHONHOME`/`PYTHONPATH` in this process would point a GIL interpreter at the free-threaded tree.
fn python(py: &str, script: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(py);
    cmd.args(["-c", script]).env_remove("PYTHONPATH").env_remove("PYTHONHOME");
    cmd
}


/// Four channels of white noise under a strong 10 Hz sine, in volts, saved as a FIF by mne itself.
fn write_recording(py: &str, dir: &Path) -> PathBuf {
    let path = dir.join("rec_raw.fif");
    let script = format!(
        r#"
import mne, numpy as np
sf = 128.0; t = np.arange(int(sf * 8)) / sf; rng = np.random.default_rng(0)
x = np.stack([rng.standard_normal(t.size) + 10 * np.sin(2 * np.pi * 10 * t) for _ in range(4)]) * 1e-6
mne.io.RawArray(x, mne.create_info(["Fz", "Cz", "Pz", "Oz"], sf, "eeg"), verbose=False).save({path:?}, overwrite=True, verbose=False)
"#,
        path = path.to_string_lossy()
    );
    let out = python(py, &script).output().expect("spawn python");
    assert!(out.status.success(), "mne could not write the recording: {}", String::from_utf8_lossy(&out.stderr));
    path
}


#[test]
fn the_eeg_bundle_plays_a_recording_reads_its_spectrum_and_receives_a_live_stream() {
    let py = require_python();
    let g = Goofi::new();
    let dir = std::env::temp_dir().join(format!("goofi-eeg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let recording = write_recording(&py.py, &dir);

    // The playback: nothing until a file is named, then the recording's own channels and rate.
    let play_ty = install_bundled(&g, "eeg", "eeg_playback.py");
    let play = g.add(&play_ty);
    let played = g.probe(play, "out");
    g.ready(play);
    assert!(g.stays(|_| played.count() == 0), "no file, no frames");
    g.set_param(play, "playback", "file", recording.to_string_lossy().to_string());
    let d = first_frame(&g, &play_ty, play, &played, |_| true);
    assert_eq!(shape(&d)[0], 4, "one row per channel: {:?}", shape(&d));
    assert_eq!(d.meta().sfreq(), Some(128.0), "the recording's rate rides the frame");
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz", "Pz", "Oz"], "and so do its channel names");

    // Its spectrum, through the shipped Buffer and Psd, into the two spectral nodes.
    let buf = g.add("Buffer");
    g.set_param(buf, "buffer", "unit", "samples");
    g.set_param(buf, "buffer", "size", 256);
    let window = g.probe(buf, "out");
    let psd = g.add("Psd");
    let bands_ty = install_bundled(&g, "eeg", "eeg_power_bands.py");
    let bands = g.add(&bands_ty);
    let power = g.probe(bands, "power");
    let fooof_ty = install_bundled(&g, "eeg", "fooof.py");
    let fooof = g.add(&fooof_ty);
    let peaks = g.probe(fooof, "peaks");
    let aperiodic = g.probe(fooof, "aperiodic");
    g.link(play, "out", buf, "input");
    g.link(buf, "out", psd, "input");
    g.link(psd, "out", bands, "psd");
    g.link(psd, "out", fooof, "psd");

    let d = g.until("a full window", |_| window.latest().filter(|d| shape(d) == vec![4, 256]));
    let peak = f32s(&d).iter().fold(0f32, |m, x| m.max(x.abs()));
    assert!((8.0..40.0).contains(&peak), "volts scaled to microvolts: the sine peaks near 10, not 1e-5 ({peak})");

    // A full window is two seconds of playback; until then the 10 Hz line is smeared.
    let d = first_frame(&g, &bands_ty, bands, &power, |d| shape(d) == vec![4, 5] && f32s(d)[2] > 0.0);
    assert_eq!(labels(&d, "dim1"), ["delta", "theta", "alpha", "beta", "gamma"]);
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz", "Pz", "Oz"], "the channel axis survives the reduction");
    let alpha_wins = |d: &goofi_core::Data| {
        f32s(d).chunks(5).all(|row| row.iter().all(|b| *b <= row[2]))
    };
    g.until("the alpha band to carry the 10 Hz sine", |_| power.latest().filter(alpha_wins));
    g.set_param(bands, "bands", "relative", true);
    g.until("relative power to answer a share", |_| {
        power.latest().filter(|d| f32s(d).chunks(5).all(|row| row[2] > 0.5 && row[2] <= 1.0))
    });
    g.set_param(bands, "bands", "gamma", "");
    g.until("a dropped band to leave the axis", |_| {
        power.latest().filter(|d| shape(d) == vec![4, 4] && labels(d, "dim1") == ["delta", "theta", "alpha", "beta"])
    });

    let d = first_frame(&g, &fooof_ty, fooof, &peaks, |d| shape(d) == vec![4, 6, 3]);
    assert_eq!(labels(&d, "dim2"), ["cf", "pw", "bw"]);
    g.until("every channel's strongest peak to sit at 10 Hz", |_| {
        peaks.latest().filter(|d| f32s(d).chunks(18).all(|row| (8.5..11.5).contains(&row[0])))
    });
    let d = g.until("the aperiodic fit", |_| aperiodic.latest().filter(|d| shape(d) == vec![4, 2]));
    assert_eq!(labels(&d, "dim1"), ["offset", "exponent"]);
    // One 256-sample periodogram is a noisy thing to fit a slope to; the bound only has to
    // separate flat from the 2 a random walk would answer.
    assert!(f32s(&d).chunks(2).all(|row| row[1].abs() < 1.5), "white noise is flat: {:?}", f32s(&d));
    g.set_param(fooof, "fooof", "mode", "knee");
    let d = g.until("the knee mode to widen the fit", |_| aperiodic.latest().filter(|d| shape(d) == vec![4, 3]));
    assert_eq!(labels(&d, "dim1"), ["offset", "knee", "exponent"]);

    // The end of the recording, once looping is off, is silence — and looping back on resumes.
    g.set_param(play, "playback", "loop", false);
    g.until("the playback to stop at the end of the recording", |g| {
        let n = played.count();
        g.stays(|_| played.count() == n).then_some(())
    });
    g.set_param(play, "playback", "loop", true);
    let n = played.count();
    g.until("the playback to resume", |_| (played.count() > n).then_some(()));

}

/// The bundle's first two stages, which every scenario below stands on: TWO NAMED channels of an
/// 8 Hz sine, the peaks they have, and the scale those peaks fold into. The names are the point of
/// the second channel: a measure per channel is only a topomap while the names reach it. `also` is
/// installed in the SAME scan, so a scenario pays for one round of biotuner imports rather than two.
fn a_scale_from_a_sine(g: &Goofi, also: &[&str]) -> (goofi_tests::Uid, goofi_tests::Uid, Vec<String>) {
    let osc = g.add("LFO");
    let buf = g.add("Buffer");
    let cut = g.add("Reshape");
    let named = g.add("Meta");
    g.set_param(osc, "output", "sfreq", 256.0);
    g.set_param(osc, "output", "mode", "block");
    g.set_param(osc, "lfo", "frequency", 8.0);
    g.set_param(buf, "buffer", "unit", "samples");
    g.set_param(buf, "buffer", "size", 512);
    // Two windows of eight whole cycles each, so the second channel holds the SAME spectrum.
    g.set_param(cut, "reshape", "shape", "2,256");
    g.set_param(named, "meta", "sfreq", 256.0);
    g.set_param(named, "meta", "labels", "Fz,Cz");
    g.link(osc, "out", buf, "input");
    g.link(buf, "out", cut, "input");
    g.link(cut, "out", named, "input");
    let window = g.probe(named, "out");
    g.until("a full window", |_| window.latest().filter(|d| shape(d) == vec![2, 256]));

    let mut files = vec!["peaks.py", "tuning.py"];
    files.extend_from_slice(also);
    let mut types = install_bundled_all(g, "biotuner", &files).into_iter();
    let peaks_ty = types.next().expect("peaks");
    let tuning_ty = types.next().expect("tuning");

    // The extraction stands alone, and everything after it reads ITS answer rather than the signal.
    let peaks = g.add(&peaks_ty);
    let found = g.probe(peaks, "peaks");
    g.link(named, "out", peaks, "input");
    let d = first_frame(g, &peaks_ty, peaks, &found, |d| shape(d) == vec![2, 5]);
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"], "the extraction keeps the channel names");
    let hz = f32s(&d);
    // Half a `precision` step of the frequency the LFO was set to: the grid is 0.5 Hz and 8 Hz over
    // a 256-sample window at 256 Hz is a whole number of cycles, so the peak is exact. The peaks
    // beside it are the window's own edges, and a looser bound would accept one of those.
    assert!(hz.iter().any(|f| (f - 8.0).abs() <= 0.25), "the 8 Hz sine is one of the peaks: {hz:?}");
    assert!(
        hz.iter().all(|f| f.is_nan() || (2.0..=30.0).contains(f)),
        "a peak is inside the search band, or it is the NaN padding: {hz:?}",
    );

    let tuning = g.add(&tuning_ty);
    let scale = g.probe(tuning, "tuning");
    g.link(peaks, "peaks", tuning, "input");
    g.link(peaks, "amps", tuning, "amps");
    // A scale is ratios inside ONE octave: that is what folding the peaks over the lowest means.
    let d = first_frame(g, &tuning_ty, tuning, &scale, |d| !f32s(d).is_empty());
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"], "and so does the scale built from them");
    let ratios = f32s(&d);
    assert!(
        ratios.iter().all(|r| r.is_nan() || (1.0..=2.0).contains(r)),
        "every degree sits in the octave, or it is padding: {ratios:?}",
    );
    (peaks, tuning, types.collect())
}

#[test]
fn the_biotuner_bundle_reads_a_scale_out_of_a_signal_and_measures_it() {
    // An 8 Hz sine is a signal whose spectrum has ONE answer, so every stage after the extraction
    // is judged against a peak that is known rather than against whatever the noise gave.
    let _py = require_python();
    let g = Goofi::new();
    let also = ["harmonicity.py", "tuning_reduction.py", "tuning_matrix.py", "timbre_controls.py"];
    let (peaks, tuning, types) = a_scale_from_a_sine(&g, &also);
    let [harm_ty, reduce_ty, matrix_ty, timbre_ty]: [String; 4] = types.try_into().expect("one type per file");

    let harm = g.add(&harm_ty);
    let harmsim = g.probe(harm, "harmsim");
    g.link(peaks, "peaks", harm, "input");
    let reduced = g.add(&reduce_ty);
    let mode = g.probe(reduced, "reduced");
    let matrix = g.add(&matrix_ty);
    let metric = g.probe(matrix, "metric");
    let timbre = g.add(&timbre_ty);
    let brightness = g.probe(timbre, "brightness");
    for node in [reduced, matrix, timbre] {
        g.link(tuning, "tuning", node, "input");
    }

    let d = first_frame(&g, &harm_ty, harm, &harmsim, |d| !f32s(d).is_empty());
    assert!(f32s(&d).iter().all(|x| x.is_finite() && *x >= 0.0), "harmonic similarity is a score: {:?}", f32s(&d));
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"], "a score per channel says WHICH channel");
    let d = first_frame(&g, &reduce_ty, reduced, &mode, |d| shape(d) == vec![2, 5]);
    assert!(f32s(&d).iter().all(|r| r.is_nan() || (1.0..=2.0).contains(r)), "the mode is a subset of the scale");
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"]);
    let d = first_frame(&g, &matrix_ty, matrix, &metric, |d| !f32s(d).is_empty());
    assert!(f32s(&d).iter().all(|x| x.is_finite()), "the matrix answers one number for the whole scale");
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"]);
    let d = first_frame(&g, &timbre_ty, timbre, &brightness, |d| !f32s(d).is_empty());
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"]);
    assert!(
        f32s(&d).iter().all(|x| (0.0..=1.0).contains(x)),
        "brightness is a plain scalar in a plain range, which is what binding it to a plugin needs: {:?}",
        f32s(&d),
    );

    for (ty, node) in [(&harm_ty, harm), (&reduce_ty, reduced), (&matrix_ty, matrix), (&timbre_ty, timbre)] {
        assert!(g.error(node).is_none(), "{ty} carries no error: {:?}", g.error(node));
    }
}

#[test]
fn biotuner_timbres_drive_audio_voices_and_export_the_same_selected_spectrum() {
    let _py = require_python();
    let g = Goofi::new();
    let mut sources = bundled("biotuner", &["timbre_controls.py", "vital_preset.py"]);
    sources.push(("timbre_source.py".into(), include_str!("fixtures/timbre_source.py").into()));
    let pairs: Vec<_> = sources.iter().map(|(name, source)| (name.as_str(), source.as_str())).collect();
    let [timbre_ty, preset_ty, source_ty]: [String; 3] = install_all(&g, &pairs).try_into().unwrap();
    let source = g.add(&source_ty);
    let timbre = g.add(&timbre_ty);
    g.set_param(timbre, "timbre", "base_freq", 261.63);
    g.set_param(timbre, "timbre", "tilt", 1.0);
    g.link(source, "tuning", timbre, "input");
    let partials = g.probe(timbre, "partials");
    let pitch = g.probe(timbre, "pitch");
    let gain = g.probe(timbre, "gain");
    let voices = g.probe(timbre, "voices");
    let brightness = g.probe(timbre, "brightness");
    let d = first_frame(&g, &timbre_ty, timbre, &partials, |d| shape(d) == vec![2, 3]);
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"]);
    let hz = f32s(&d);
    for (a, b) in hz.iter().zip([261.63, 392.445, 523.26, 261.63, 327.0375, 457.8525]) {
        assert!((a - b).abs() < 0.001, "partials are sorted, positive, and unique: {hz:?}");
    }
    let d = first_frame(&g, &timbre_ty, timbre, &pitch, |d| shape(d) == vec![8, 1]);
    let pitches = f32s(&d);
    assert!(pitches[0].abs() < 1e-5);
    assert!((pitches[1] - 1.5f32.log2()).abs() < 1e-5);
    assert!((pitches[2] - 1.0).abs() < 1e-5);
    assert!(d.meta().sfreq().is_none());
    let d = first_frame(&g, &timbre_ty, timbre, &gain, |d| shape(d) == vec![8, 1]);
    let gains = f32s(&d);
    assert!((gains.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    assert!(gains[3..].iter().all(|v| *v == 0.0));
    let d = first_frame(&g, &timbre_ty, timbre, &voices, |d| shape(d) == vec![16, 1]);
    let packed = f32s(&d);
    assert_eq!(&packed[..8], &pitches);
    for (actual, expected) in packed[8..11].iter().zip([0.7, 0.7 / 1.5, 0.35]) {
        assert!((actual - expected).abs() < 1e-6, "VST velocities use linear amplitudes");
    }
    assert!(packed[11..].iter().all(|v| *v == 0.0));
    let d = first_frame(&g, &timbre_ty, timbre, &brightness, |d| shape(d) == vec![2]);
    let bright = f32s(&d);
    assert!(bright.iter().all(|v| (0.0..=1.0).contains(v)));
    g.set_param(source, "source", "mode", "reverse");
    let n = brightness.count();
    g.until("reordered ratios to be measured", |_| (brightness.count() > n).then_some(()));
    assert_eq!(f32s(&brightness.latest().unwrap()), bright);

    // Hear the columns through the public audio boundary, without opening a device.
    let pitch_in = g.add("audio:SignalIn");
    let gain_in = g.add("audio:SignalIn");
    g.link(timbre, "pitch", pitch_in, "input");
    g.link(timbre, "gain", gain_in, "input");
    let osc = g.add("Osc");
    let amp = g.add("Gain");
    let mix = g.add("Mixdown");
    g.link(osc, "out", amp, "input");
    g.link(amp, "out", mix, "input");
    for (node, param, from) in [(osc, "osc/pitch", pitch_in), (amp, "gain/gain", gain_in)] {
        let name = g.doc()["nodes"][hex(from)]["name"].as_str().unwrap().to_string();
        g.call("node param edit", j!({"node": hex(node), "param": param,
            "reference": format!("{name}.out"), "mode": "reference"}));
    }
    let heard = g.probe(mix, "out");
    let audible = |d: &goofi_core::Data| f32s(d).iter().any(|v| v.abs() > 0.01);
    let d = g.until("the timbre to produce audio", |g| {
        goofi_tests::drive(g, 4800);
        heard.latest().filter(audible)
    });
    assert!(f32s(&d).iter().all(|v| v.is_finite() && v.abs() <= 1.001));
    g.link(source, "gates", timbre, "gate");
    g.until("the second VST voice to release", |_| voices.latest().filter(|d| {
        let v = f32s(d);
        v[8] > 0.0 && v[9] == 0.0 && v[10] > 0.0
    }));
    g.set_param(source, "source", "gate", 0.0);
    g.until("all voice gains to close", |_| gain.latest().filter(|d| f32s(d).iter().all(|v| *v == 0.0)));
    g.until("silence through the audio engine", |g| {
        goofi_tests::drive(g, 4800);
        heard.latest().filter(|d| f32s(d).iter().all(|v| *v == 0.0))
    });

    let preset = g.add(&preset_ty);
    let path = g.probe(preset, "path");
    let files = g.probe(preset, "files");
    let folder = g.state.mount().join("exports");
    g.set_param(preset, "preset", "folder", folder.to_string_lossy().to_string());
    g.set_param(preset, "preset", "row", 1);
    g.link(timbre, "partials", preset, "partials");
    g.link(timbre, "amplitudes", preset, "amplitudes");
    first_frame(&g, &preset_ty, preset, &path, |_| true);
    let n = path.count();
    g.until("exporter to receive its input frames", |_| (path.count() > n + 2).then_some(()));
    assert!(!folder.exists(), "exporting requires a pulse");
    g.call("node param pulse", j!({"node": hex(preset), "param": "preset/write"}));
    let d = first_frame(&g, &preset_ty, preset, &files, |d| d.as_table().is_ok_and(|t| t.contains_key("companion")));
    let exports = d.as_table().unwrap();
    let settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(exports["companion"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(settings["timbre"]["matching_method"], "explicit");
    assert_eq!(settings["base_freq"], 261.63, "the source frame owns the reference frequency");
    let ratios = settings["timbre"]["matched_tuning"].as_array().unwrap();
    for (ratio, expected) in ratios.iter().zip([1.0, 1.25, 1.75]) {
        assert!((ratio.as_f64().unwrap() - expected).abs() < 1e-6);
    }
    let exported: serde_json::Value = serde_json::from_slice(
        &std::fs::read(exports["vital"].as_str().unwrap()).unwrap()).unwrap();
    assert!(exported["settings"].is_object(), "a real Vital preset was written");
    let encoded = exported["settings"]["wavetables"][0]["groups"][0]["components"][0]["keyframes"][0]["wave_data"]
        .as_str().unwrap();
    let wave = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded).unwrap();
    let wave: Vec<_> = wave.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
    let expected: Vec<f64> = (0..2048).map(|i| [1.0, 1.25, 1.75].iter().map(|r| {
        (std::f64::consts::TAU * r * i as f64 / 2048.0).sin() / r
    }).sum()).collect();
    let peak = expected.iter().fold(0.0f64, |peak, v| peak.max(v.abs()));
    assert_eq!(wave.len(), expected.len());
    for (actual, expected) in wave.iter().zip(expected) {
        assert!((*actual as f64 - 0.99 * expected / peak).abs() < 1e-5, "export preserves the selected amplitudes");
    }

    // An ensemble must use the current optional signal, including its removal.
    g.set_param(preset, "preset", "kind", "ensemble");
    g.link(source, "signal", preset, "signal");
    for (name, with_signal) in [("with_signal", true), ("without_signal", false)] {
        if !with_signal {
            g.call("link remove", j!({"from": goofi_tests::ep(hex(source), "signal"), "to": goofi_tests::ep(hex(preset), "signal")}));
        }
        g.set_param(preset, "preset", "name", name);
        let n = path.count();
        g.until("the exporter snapshot to settle", |_| (path.count() > n + 2).then_some(()));
        g.call("node param pulse", j!({"node": hex(preset), "param": "preset/write"}));
        let d = first_frame(&g, &preset_ty, preset, &files, |d| {
            d.as_table().is_ok_and(|t| t.get("__manifest__").is_some_and(|d| {
                d.as_str().is_ok_and(|s| s.ends_with(&format!("{name}.ensemble.manifest.json")))
            }))
        });
        let files = d.as_table().unwrap();
        assert!(files.contains_key("base_morph/vital"));
        assert_eq!(files.contains_key("pad/vital"), with_signal, "a disconnected signal cannot be exported again");
    }

    // Empty tuning rows must mute every audio output instead of leaving a held note.
    g.call("link remove", j!({"from": goofi_tests::ep(hex(source), "gates"), "to": goofi_tests::ep(hex(timbre), "gate")}));
    g.set_param(source, "source", "mode", "empty");
    g.until("empty tunings to release all VST notes", |_| voices.latest().filter(|d| f32s(d).iter().all(|v| *v == 0.0)));
    g.set_param(source, "source", "mode", "single");
    g.until("one partial to sound without padding notes", |_| gain.latest().filter(|d| {
        let v = f32s(d);
        v[0] == 1.0 && v[1..].iter().all(|v| *v == 0.0)
    }));
    assert!(g.error(timbre).is_none());
}

#[test]
fn harmonic_spectrum_feeds_the_biotuner_bundle_and_recovers_as_windows_change() {
    let _py = require_python();
    let g = Goofi::new();
    let mut sources = bundled("biotuner", &["harmonic_spectrum.py", "tuning.py", "harmonicity.py"]);
    sources.push(("harmonic_signal.py".into(), include_str!("fixtures/harmonic_signal.py").into()));
    let pairs: Vec<_> = sources.iter().map(|(name, source)| (name.as_str(), source.as_str())).collect();
    let [spectrum_ty, tuning_ty, harm_ty, source_ty]: [String; 4] =
        install_all(&g, &pairs).try_into().expect("one type per file");
    let source = g.add(&source_ty);
    let node = g.add(&spectrum_ty);
    let spectrum = g.probe(node, "spectrum");
    let frequencies = g.probe(node, "freqs");
    let peaks = g.probe(node, "peaks");
    let peak_values = g.probe(node, "peakValues");
    let matrix = g.probe(node, "matrix");
    let activation = g.probe(node, "activation");
    let power = g.probe(node, "power");
    let waveform = g.probe(node, "waveform");
    let analysis = g.probe(node, "analysis");
    let mean = g.probe(node, "harmonicity");
    let complexity = g.probe(node, "complexity");
    g.link(source, "out", node, "input");

    let d = first_frame(&g, &spectrum_ty, node, &spectrum, |d| shape(d) == vec![2, 57]);
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"]);
    assert!(d.meta().sfreq().is_none(), "frequency bins are not time samples");
    let h = f32s(&d);
    assert!(h.iter().all(|v| v.is_finite() && *v >= 0.0));
    assert_ne!(&h[..57], &h[57..], "the two channels are analyzed separately");
    let f = first_frame(&g, &spectrum_ty, node, &frequencies, |d| shape(d) == vec![57]);
    let hz = f32s(&f);
    assert_eq!(hz, (0..57).map(|i| 2.0 + i as f32 * 0.5).collect::<Vec<_>>());
    assert_eq!(labels(&d, "dim1"), labels(&f, "dim0"));
    let d = first_frame(&g, &spectrum_ty, node, &matrix, |d| shape(d) == vec![2, 57, 57]);
    assert_eq!(labels(&d, "dim1"), labels(&f, "dim0"));
    assert_eq!(labels(&d, "dim2"), labels(&f, "dim0"));
    let m = f32s(&d);
    // 8:12 is a 2:3 ratio: the shared biotuner measure gives 66 2/3.
    assert!((m[12 * 57 + 20] - 66.66667).abs() < 0.001);
    let d = first_frame(&g, &spectrum_ty, node, &power, |d| shape(d) == vec![2, 57]);
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"]);
    assert!(d.meta().sfreq().is_none());
    let p = f32s(&d);
    for row in p.chunks_exact(57) {
        assert!(row.iter().all(|v| v.is_finite() && *v >= 0.0));
        assert!((row.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }
    let d = first_frame(&g, &spectrum_ty, node, &activation, |d| shape(d) == vec![2, 57, 57]);
    assert_eq!(labels(&d, "dim2"), labels(&f, "dim0"));
    for (row, a) in f32s(&d).chunks_exact(57 * 57).enumerate() {
        for i in 0..57 {
            for j in 0..57 {
                let expected = m[row * 57 * 57 + i * 57 + j] * p[row * 57 + i] * p[row * 57 + j];
                assert!((a[i * 57 + j] - expected).abs() < 1e-5, "activation uses the emitted power weights");
            }
        }
    }
    let d = first_frame(&g, &spectrum_ty, node, &waveform, |d| shape(d) == vec![2, 512]);
    assert_eq!(d.meta().sfreq(), Some(256.0));
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"]);
    let d = first_frame(&g, &spectrum_ty, node, &mean, |d| shape(d) == vec![2]);
    for (row, value) in h.chunks_exact(57).zip(f32s(&d)) {
        assert!((row.iter().sum::<f32>() / 57.0 - value).abs() < 1e-5);
    }
    let d = first_frame(&g, &spectrum_ty, node, &complexity, |d| shape(d) == vec![2, 4]);
    assert_eq!(labels(&d, "dim1"), ["flatness", "entropy", "spread", "higuchi"]);
    assert!(f32s(&d).iter().all(|v| v.is_finite()));
    let d = first_frame(&g, &spectrum_ty, node, &analysis, |d| d.as_table().is_ok());
    let packet = d.as_table().unwrap();
    assert_eq!(packet.len(), 10, "one analysis window contains every individual output");
    assert_eq!(f32s(&packet["spectrum"]), h);
    assert_eq!(f32s(&packet["matrix"]), m);
    assert_eq!(f32s(&packet["power"]), p);
    assert_eq!(labels(&packet["waveform"], "dim0"), ["Fz", "Cz"]);
    assert_eq!(packet["waveform"].meta().sfreq(), Some(256.0));
    let d = first_frame(&g, &spectrum_ty, node, &peaks, |d| shape(d) == vec![2, 5]);
    let pk = f32s(&d);
    for target in [8.0, 12.0, 16.0] {
        assert!(pk[..5].contains(&target), "the harmonic chord has a peak at {target}: {pk:?}");
    }
    assert!(pk[..5].iter().any(|v| v.is_nan()), "unused peak positions are padded");
    let d = first_frame(&g, &spectrum_ty, node, &peak_values, |d| shape(d) == vec![2, 5]);
    for (row, (freqs, values)) in pk.chunks_exact(5).zip(f32s(&d).chunks_exact(5)).enumerate() {
        for (freq, value) in freqs.iter().zip(values) {
            if freq.is_nan() {
                assert!(value.is_nan());
            } else {
                let index = hz.iter().position(|v| v == freq).unwrap();
                assert!((h[row * 57 + index] - value).abs() < 1e-5);
            }
        }
    }

    let tuning = g.add(&tuning_ty);
    let scale = g.probe(tuning, "tuning");
    let harm = g.add(&harm_ty);
    let score = g.probe(harm, "harmsim");
    for target in [tuning, harm] {
        g.link(node, "peaks", target, "input");
    }
    let d = first_frame(&g, &tuning_ty, tuning, &scale, |d| f32s(d).iter().any(|v| v.is_finite()));
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"]);
    assert!(f32s(&d).iter().all(|v| v.is_nan() || (1.0..=2.0).contains(v)));
    first_frame(&g, &harm_ty, harm, &score, |d| f32s(d).iter().all(|v| v.is_finite()));

    for mode in ["flat", "nan"] {
        g.set_param(source, "signal", "first", mode);
        let d = g.until("a missing first row and a valid second row", |_| {
            spectrum.latest().filter(|d| {
                let values = f32s(d);
                values[..57].iter().all(|v| v.is_nan()) && values[57..].iter().all(|v| v.is_finite())
            })
        });
        assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"]);
        assert!(g.error(node).is_none());
        g.set_param(source, "signal", "first", "chord");
        g.until("valid rows after missing data", |_| spectrum.latest().filter(|d| f32s(d).iter().all(|v| v.is_finite())));
    }
    g.set_param(node, "spectrum", "n_peaks", 7);
    g.until("the new peak width", |_| peaks.latest().filter(|d| shape(d) == vec![2, 7]));
    g.set_param(node, "spectrum", "kernel", "subharm_tension");
    g.until("the subharmonic kernel", |_| matrix.latest().filter(|d| f32s(d).iter().all(|v| (0.0..=1.0).contains(v))));
    g.set_param(node, "spectrum", "precision", 1.0);
    g.until("the coarser frequency grid", |_| spectrum.latest().filter(|d| shape(d) == vec![2, 29]));
    g.set_param(source, "signal", "vector", true);
    let d = g.until("a single time series", |_| spectrum.latest().filter(|d| shape(d) == vec![29]));
    assert_eq!(labels(&d, "dim0").len(), 29, "time labels are replaced by frequency labels");

    g.set_param(source, "signal", "sfreq", false);
    let error = g.until("missing sampling rate to be reported", |g| g.error(node));
    assert!(error.contains("sfreq"), "{error}");
    g.set_param(source, "signal", "sfreq", true);
    g.until("recovery after sampling rate is restored", |g| g.error(node).is_none().then_some(()));
    g.set_param(node, "spectrum", "f_max", 200.0);
    let error = g.until("a band above Nyquist to be reported", |g| g.error(node));
    assert!(error.contains("sfreq / 2"), "{error}");
    g.set_param(node, "spectrum", "f_max", 30.0);
    g.until("recovery after the band is restored", |g| g.error(node).is_none().then_some(()));
    for precision in [0.01, 10.0] {
        g.set_param(node, "spectrum", "precision", precision);
        let error = g.until("an unsupported frequency grid to be reported", |g| g.error(node));
        assert!(error.contains("10..512"), "{error}");
        g.set_param(node, "spectrum", "precision", 1.0);
        g.until("recovery after the grid is restored", |g| g.error(node).is_none().then_some(()));
    }
    g.set_param(source, "signal", "samples", 16);
    g.until("a short window to stop producing", |g| {
        let count = spectrum.count();
        g.stays(|g| spectrum.count() == count && g.error(node).is_none()).then_some(())
    });
    let count = spectrum.count();
    g.set_param(source, "signal", "samples", 512);
    g.until("a full window to resume analysis", |_| (spectrum.count() > count).then_some(()));
    assert!(g.error(node).is_none());
}

#[test]
fn harmonic_observatory_example_uses_the_biotuner_bundle_without_local_copies() {
    let _py = require_python();
    let mut g = Goofi::new();
    let shipped = tempfile::tempdir().unwrap();
    let biotuner = shipped.path().join("biotuner");
    std::fs::create_dir(&biotuner).unwrap();
    for (file, source) in bundled("biotuner", &["harmonic_spectrum.py", "harmonic_observatory.py"]) {
        std::fs::write(biotuner.join(file), source).unwrap();
    }
    g.state.roots.push(biotuner);
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/harmonic-observatory.gfi");
    g.call("session load", j!({"path": example.to_string_lossy()}));
    let library = g.call("library list", j!({"full": true}));
    for ty in ["signal:HarmonicSpectrum", "signal:HarmonicObservatory"] {
        let entry = library["types"].as_array().unwrap().iter().find(|entry| entry["type"] == ty).unwrap();
        assert_eq!(entry["source"], "builtin", "{ty} must come from the bundle");
        assert_eq!(entry["bundle"], "biotuner");
    }
    let local = g.state.mount().join("nodes_signal");
    assert!(!local.join("harmonic_spectrum.py").exists());
    assert!(!local.join("harmonic_observatory.py").exists());
    assert!(local.join("harmonic_scene.py").exists(), "the example keeps its signal source");
    let doc = g.doc();
    let node = |name: &str| {
        let (uid, _) = doc["nodes"].as_object().unwrap().iter().find(|(_, entry)| entry["name"] == name).unwrap();
        goofi_tests::Uid::from_hex(uid).unwrap()
    };
    let dashboard = node("observatory");
    let spectrum = node("harmonicspectrum0");
    assert_eq!(g.nodes().len(), 3, "the image viewer needs no graphics bridge or window");
    let viewers: serde_json::Value = serde_json::from_str(doc["nodes"][hex(dashboard)]["viewers"].as_str().unwrap()).unwrap();
    assert_eq!(viewers["dashboard"]["collapsed"], false);
    let image = g.probe(dashboard, "dashboard");
    let d = first_frame(&g, "HarmonicObservatory", dashboard, &image, |d| shape(d) == vec![629, 825, 3]);
    let pixels = f32s(&d);
    assert!(pixels.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    assert!(pixels.iter().any(|v| *v > 0.9), "the dashboard draws its labels and plots");

    // The single cable carries the new grid and all its arrays together.
    let frequencies = g.probe(spectrum, "freqs");
    g.set_param(spectrum, "spectrum", "precision", 1.0);
    g.until("the example's frequency grid to change", |_| frequencies.latest().filter(|d| shape(d) == vec![29]));
    let count = image.count();
    g.until("the dashboard to render the changed grid", |g| {
        (image.count() > count + 2 && g.error(dashboard).is_none()).then_some(())
    });

    // Saving must not copy bundle files back into the patch workspace.
    let saved = shipped.path().join("saved.gfi");
    g.call("session save", j!({"path": saved.to_string_lossy()}));
    let archive = zip::ZipArchive::new(std::fs::File::open(saved).unwrap()).unwrap();
    assert!(!archive.file_names().any(|name| name.ends_with("harmonic_observatory.py") || name.ends_with("harmonic_spectrum.py")));

    // Replace the demonstration with labeled channels, using the same single input.
    let source_ty = install(&g, "harmonic_signal.py", include_str!("fixtures/harmonic_signal.py"));
    let source = g.add(&source_ty);
    let scene = node("harmonicScene");
    g.call("link remove", j!({"from": goofi_tests::ep(hex(scene), "out"), "to": goofi_tests::ep(hex(spectrum), "input")}));
    g.link(source, "out", spectrum, "input");
    let named = |d: &goofi_core::Data, name: &str| matches!(d.meta().get("channel"), Some(goofi_core::MetaValue::Str(v)) if v == name);
    g.until("the first named channel", |_| image.latest().filter(|d| named(d, "Fz")));
    let mut events = g.events();
    g.call("node param refresh", j!({"node": hex(dashboard), "param": "display/channel"}));
    let reply = g.until("channel names in the parameter menu", |_| {
        let reply = events.next("state_update");
        (reply["node"] == hex(dashboard) && reply["refreshed_params"] == j!([["display", "channel"]])).then_some(reply)
    });
    assert_eq!(reply["params"]["display"]["channel"]["options"], j!(["First channel", "Fz", "Cz"]));
    g.set_param(dashboard, "display", "channel", "Cz");
    g.until("the selected channel", |_| image.latest().filter(|d| named(d, "Cz")));
    g.set_param(source, "signal", "vector", true);
    let error = g.until("a removed channel to be reported", |g| g.error(dashboard));
    assert!(error.contains("Cz") && error.contains("not available"), "{error}");
    g.set_param(dashboard, "display", "channel", "First channel");
    g.until("the unlabeled signal after channel selection is reset", |g| {
        image.latest().filter(|d| named(d, "Row 1") && g.error(dashboard).is_none())
    });
}

#[test]
fn a_scale_is_also_a_palette_and_a_rhythm_a_synth_can_play() {
    // The same numbers at three other rates, which is the whole reason the extraction stands alone.
    let _py = require_python();
    let g = Goofi::new();
    let also = ["bio_colors.py", "bio_elements.py", "euclid_rhythm.py", "polyrhythm.py", "rhythm_player.py"];
    let (peaks, tuning, types) = a_scale_from_a_sine(&g, &also);
    let [colors_ty, elements_ty, euclid_ty, poly_ty, player_ty]: [String; 5] =
        types.try_into().expect("one type per file");

    let colors = g.add(&colors_ty);
    let rgb = g.probe(colors, "rgb");
    let elements = g.add(&elements_ty);
    let scores = g.probe(elements, "scores");
    for node in [colors, elements] {
        g.link(peaks, "peaks", node, "input");
        g.link(peaks, "amps", node, "amps");
    }
    let euclid = g.add(&euclid_ty);
    let patterns = g.probe(euclid, "patterns");
    let poly = g.add(&poly_ty);
    let cycle = g.probe(poly, "cycle");
    for node in [euclid, poly] {
        g.link(tuning, "tuning", node, "input");
    }
    let player = g.add(&player_ty);
    let gate = g.probe(player, "gate");
    g.link(poly, "voices", player, "input");

    let d = first_frame(&g, &colors_ty, colors, &rgb, |d| !f32s(d).is_empty());
    assert!(
        f32s(&d).iter().all(|c| c.is_nan() || (0.0..=1.0).contains(c)),
        "a colour channel is a fraction, or it is padding: {:?}",
        f32s(&d),
    );
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"], "a palette per channel says which channel");
    let d = first_frame(&g, &elements_ty, elements, &scores, |d| !f32s(d).is_empty());
    assert!(f32s(&d).iter().all(|x| x.is_nan() || x.is_finite()), "an element's score is a number");
    // `pooled` folded both channels into one row, so a channel name would name the wrong thing.
    assert!(labels(&d, "dim0").is_empty(), "a pooled answer carries no channel names");
    // A euclidean pattern is onsets: a step either carries one or it does not.
    let d = first_frame(&g, &euclid_ty, euclid, &patterns, |d| !f32s(d).is_empty());
    assert!(
        f32s(&d).iter().all(|x| x.is_nan() || *x == 0.0 || *x == 1.0),
        "a step is an onset or it is not: {:?}",
        f32s(&d),
    );
    assert_eq!(labels(&d, "dim0"), ["Fz", "Cz"]);
    // The grid is bounded BY DESIGN — a measured tuning wanted 18018 positions — and `cycle` says
    // what the node settled on, so a cap that stopped working shows up here rather than in a crash.
    let d = first_frame(&g, &poly_ty, poly, &cycle, |d| !f32s(d).is_empty());
    assert!(f32s(&d).iter().all(|c| (1.0..=64.0).contains(c)), "the grid fits its cap: {:?}", f32s(&d));
    let d = first_frame(&g, &player_ty, player, &gate, |d| !f32s(d).is_empty());
    assert!(
        f32s(&d).iter().all(|x| *x == 0.0 || *x == 1.0),
        "a gate is open or shut, which is what a plugin's envelope reads: {:?}",
        f32s(&d),
    );

    for (ty, node) in [
        (&colors_ty, colors), (&elements_ty, elements), (&euclid_ty, euclid),
        (&poly_ty, poly), (&player_ty, player),
    ] {
        assert!(g.error(node).is_none(), "{ty} carries no error: {:?}", g.error(node));
    }
}

#[test]
fn the_ml_agent_acts_before_a_reward_is_wired_and_keeps_its_shape_as_it_is_retuned() {
    // An agent is USED as a session that outlives its own settings: it starts with nothing on its
    // reward slot, is given one, is rescaled, is widened, and is reset — and every one of those has
    // to leave a running loop still producing rather than a node that faulted or wedged.
    let _py = require_python();
    let g = Goofi::new();

    let ty = install_bundled(&g, "ml", "reinforcement_learning.py");
    let agent = g.add(&ty);
    g.set_param(agent, "agent", "width", 16);

    let osc = g.add("LFO");
    g.set_param(osc, "lfo", "frequency", 0.7);
    let buf = g.add("Buffer");
    g.set_param(buf, "buffer", "unit", "samples");
    g.set_param(buf, "buffer", "size", 8);
    g.link(osc, "out", buf, "input");
    g.link(buf, "out", agent, "observations");

    let actions = g.probe(agent, "actions");
    let readings = g.probe(agent, "diagnostics");

    // An unwired reward is a reward of zero, never a fault: the loop is built one end at a time.
    let d = first_frame(&g, &ty, agent, &actions, |d| shape(d) == vec![2]);
    let v = f32s(&d);
    assert!(v.iter().all(|x| x.is_finite() && (-1.0..=1.0).contains(x)), "acted within its range: {v:?}");
    assert_eq!(labels(&d, "dim0"), ["action0", "action1"], "each action is named");

    // Every reading is named, because stagnation is meant to be READ rather than guessed at.
    let d = first_frame(&g, &ty, agent, &readings, |d| shape(d) == vec![7]);
    assert_eq!(
        labels(&d, "dim0"),
        ["avg_reward", "td_error", "entropy", "effective_rank", "dormant", "weight_norm", "step_size"],
    );

    // A reward lands, and the agent takes it up rather than ignoring the slot it was given late.
    let reward = g.add("LFO");
    g.set_param(reward, "lfo", "frequency", 0.31);
    g.set_param(reward, "lfo", "offset", 2.0);
    g.link(reward, "out", agent, "reward");
    g.until("the reward to reach the agent", |_| readings.latest().filter(|d| f32s(d)[0] > 0.5));
    // The rank is measured over a window and is honestly 0 until one has gone by, so it is WAITED
    // for rather than read on arrival: a first observation normalises to zero and spans nothing.
    let d = g.until("the representation to span more than one direction", |_| {
        readings.latest().filter(|d| f32s(d)[3] > 1.0)
    });
    let seen = f32s(&d);
    assert!(seen[2].is_finite(), "entropy stays a number: {}", seen[2]);
    assert!(seen[5] > 0.0, "the weights have a size: {}", seen[5]);

    // Rescaling moves where the action comes out, and nothing about it has to be relearned.
    g.set_param(agent, "action", "low", 10.0);
    g.set_param(agent, "action", "high", 20.0);
    let d = g.until("actions in the new range", |_| {
        actions.latest().filter(|d| f32s(d).iter().all(|x| (10.0..=20.0).contains(x)))
    });
    assert_eq!(shape(&d), vec![2], "rescaling is not a rebuild");

    // Widening the action space IS a rebuild, and the running loop survives it.
    g.set_param(agent, "agent", "actions", 5);
    let d = g.until("five actions", |_| actions.latest().filter(|d| shape(d) == vec![5]));
    assert_eq!(labels(&d, "dim0").len(), 5, "and every one of them is named");
    assert!(f32s(&d).iter().all(|x| x.is_finite()), "{:?}", f32s(&d));

    // So is growing the networks under it.
    g.set_param(agent, "agent", "width", 32);
    g.until("frames after the networks grow", |_| {
        readings.latest().filter(|d| f32s(d)[5] > 0.0)
    });

    // And a reset leaves a loop that still runs, which is the only thing a reset must not break.
    g.call("node param pulse", j!({ "node": hex(agent), "param": "agent/reset" }));
    g.until("frames after the reset", |_| actions.latest().filter(|d| shape(d) == vec![5]));
    assert!(g.error(agent).is_none(), "no standing error: {:?}", g.error(agent));
}

/// A `.npy` of one f32 array, so a test hands `Weights` a real file rather than a stand-in.
fn npy(shape: &[usize], data: &[f32]) -> Vec<u8> {
    let dims: String = shape.iter().map(|d| format!("{d},")).collect();
    let mut head = format!("{{'descr': '<f4', 'fortran_order': False, 'shape': ({dims}), }}");
    while (10 + head.len() + 1) % 64 != 0 {
        head.push(' ');
    }
    head.push('\n');
    let mut out = b"\x93NUMPY\x01\x00".to_vec();
    out.extend_from_slice(&(head.len() as u16).to_le_bytes());
    out.extend_from_slice(head.as_bytes());
    data.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    out
}

#[test]
fn a_trained_model_reaches_the_patch_as_a_file_and_keeps_saying_so() {
    // A model is trained outside goofi and arrives as numbers. What the door has to survive is
    // being wired up LATE: pub/sub has no history, so a source that announces its file once and
    // falls silent hands the node behind it nothing at all.
    let _py = require_python();
    let g = Goofi::new();
    let ty = install_bundled(&g, "ml", "weights.py");
    let dir = g.state.mount();

    let rule: Vec<f32> = (0..12).map(|i| i as f32 * 0.5 - 2.0).collect();
    std::fs::write(dir.join("rule.npy"), npy(&[1, 3, 4], &rule)).unwrap();
    std::fs::write(dir.join("other.npy"), npy(&[2, 2], &[9.0, 8.0, 7.0, 6.0])).unwrap();
    let named = |file: &str| dir.join(file).to_string_lossy().to_string();

    let node = g.add(&ty);
    g.set_param(node, "weights", "file", named("rule.npy"));
    let probe = g.probe(node, "out");
    let d = first_frame(&g, &ty, node, &probe, |d| shape(d) == vec![1, 3, 4]);
    assert_eq!(f32s(&d), rule, "the file's own numbers, in the shape it was saved with");

    // A reader that opened after the file was read hears it too.
    let late = g.probe(node, "out");
    let d = g.until("a later reader to hear the same file", |_| late.latest());
    assert_eq!(f32s(&d), rule, "a weight file is repeated, never announced once");

    // A new path is a new model, and it needs no restart.
    g.set_param(node, "weights", "file", named("other.npy"));
    let d = g.until("the second file", |_| late.latest().filter(|d| shape(d) == vec![2, 2]));
    assert_eq!(f32s(&d), vec![9.0, 8.0, 7.0, 6.0], "the path is what says which model is loaded");
    assert!(g.error(node).is_none(), "Weights stands with no error: {:?}", g.error(node));

    // A path that names nothing says so, rather than repeating the model it used to hold.
    g.set_param(node, "weights", "file", named("absent.npy"));
    let why = g.until("the missing file to be reported", |g| g.error(node));
    assert!(why.contains("absent.npy"), "the error names the path: {why}");

    // Step: and a REAL model draws what it was trained to draw. `stripes_nca.npy` is a texture
    // automaton `training/style_ca.py` trained to a style loss of 0.0013; the numbers below are
    // what that trainer's own torch model settles at over 200 ticks, stable to 0.005 across seeds.
    // The shader is a second implementation of that same rule, and this is the one place the two
    // are held against each other — the failure it exists for is a texture that trains well and
    // runs wrong.
    std::fs::write(dir.join("stripes.npy"), include_bytes!("fixtures/stripes_nca.npy")).unwrap();
    g.set_param(node, "weights", "file", named("stripes.npy"));
    // The engine's clock is the test's own, so hundreds of ticks pass inside one beat of a source
    // paced for a static file. The rule has to be on the wire before those ticks mean anything.
    g.set_param(node, "common", "max_frequency", 60.0);
    g.until("the rule to be on the wire", |_| late.latest().filter(|d| shape(d) == vec![1, 1539, 4]));

    let ca = g.add("graphics:NeuralCA");
    g.set_param(ca, "common", "width", 128);
    g.set_param(ca, "common", "height", 128);
    g.set_param(ca, "neuralca", "start", "zero");
    g.ready(ca);
    g.link(node, "out", ca, "weights");

    // The probe FIRST: a stage nobody reads does not render, so ticks before it opens are no ticks.
    let drawn = g.probe(ca, "out");
    let stats = |d: &goofi_core::Data, c: usize| {
        let ch: Vec<f32> = f32s(d).into_iter().skip(c).step_by(4).collect();
        let mean = ch.iter().sum::<f32>() / ch.len() as f32;
        (mean, (ch.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / ch.len() as f32).sqrt())
    };
    let want = [(0.485, 0.205), (0.505, 0.289), (0.478, 0.090)];
    let settled = |d: &goofi_core::Data| {
        shape(d) == vec![128, 128, 4]
            && want.iter().enumerate().all(|(c, (m, s))| {
                let (mean, std) = stats(d, c);
                (mean - m).abs() < 0.05 && (std - s).abs() < 0.05
            })
    };
    let texture = g.until("the trained texture to settle", |g| {
        goofi_tests::render(g, 4);
        drawn.latest().filter(settled)
    });
    for (c, (m, s)) in want.iter().enumerate() {
        let (mean, std) = stats(&texture, c);
        assert!((mean - m).abs() < 0.05, "channel {c} sits at {mean}, and the trainer's own sits at {m}");
        assert!((std - s).abs() < 0.05, "channel {c} varies by {std}, and the trainer's own varies by {s}");
    }
    assert!(g.error(ca).is_none(), "NeuralCA stands with no error on a trained rule: {:?}", g.error(ca));
}

#[test]
fn a_trained_generator_draws_what_the_patch_steers_it_to() {
    // A generator wants a latent of a few hundred numbers and a patch has a handful of features.
    // What the node owns is the fit between them, and the frame it hands back: rows down, values a
    // viewer can draw, and a picture that MOVES when the patch does and holds still when it does not.
    let _py = require_python();
    let g = Goofi::new();
    let ty = install_bundled(&g, "ml", "decoder.py");
    let model = g.state.mount().join("tiny.onnx");
    std::fs::write(&model, include_bytes!("fixtures/tiny_generator.onnx")).unwrap();

    let node = g.add(&ty);
    g.set_param(node, "decoder", "file", model.to_string_lossy().to_string());
    g.set_param(node, "decoder", "spread", 0.0);
    g.set_param(node, "decoder", "smooth", 0.0);
    let probe = g.probe(node, "out");

    // At no spread the latent is the mean itself, so the picture is the model's bias alone — whose
    // ramp runs DOWN the rows, which a frame passed over upside down would get backwards.
    let d = first_frame(&g, &ty, node, &probe, |d| shape(d) == vec![4, 4, 3]);
    let held = f32s(&d);
    let row = |v: &[f32], r: usize| v[r * 4 * 3];
    assert!(row(&held, 0) < 0.05, "row 0 is the top: {}", row(&held, 0));
    assert!(row(&held, 3) > 0.95, "and row 3 the bottom: {}", row(&held, 3));
    assert!(held.iter().all(|x| (0.0..=1.0).contains(x)), "a `[-1, 1]` model is handed over as a frame a viewer draws");

    // Nothing is wired, so the same latent comes back and the picture holds still.
    let again = g.until("a second frame", |_| probe.latest().map(|d| f32s(&d)));
    assert!(again.iter().zip(&held).all(|(a, b)| (a - b).abs() < 1e-5), "an unwired generator is not noise");

    // A wired number takes a direction of its own, and the picture moves along it.
    g.set_param(node, "decoder", "spread", 1.0);
    let drive = g.add("signal:Constant");
    g.set_param(drive, "constant", "value", 2.0);
    g.set_param(drive, "constant", "shape", "3");
    g.link(drive, "out", node, "drive");
    let moved = g.until("the drive to move the picture", |_| {
        probe.latest().map(|d| f32s(&d)).filter(|v| v.iter().zip(&held).any(|(a, b)| (a - b).abs() > 0.05))
    });
    assert!(moved.iter().all(|x| (0.0..=1.0).contains(x)), "and it is still a frame: {moved:?}");
    assert!(g.error(node).is_none(), "Decoder stands with no error: {:?}", g.error(node));

    // Step: `direct` is the other way to steer, for a model whose own axes already mean something —
    // the components of a PCA, or a generator exported through them. Wired number i IS axis i, so a
    // drive of nothing must land on the mean latent EXACTLY, where `drawn` would have pushed it out
    // to the shell instead. That difference is the whole of the mode.
    g.set_param(node, "decoder", "axes", "direct");
    g.set_param(drive, "constant", "value", 0.0);
    let centred = g.until("the mean latent through the direct axes", |_| {
        probe.latest().map(|d| f32s(&d)).filter(|v| v.iter().zip(&held).all(|(a, b)| (a - b).abs() < 1e-4))
    });
    assert_eq!(centred.len(), held.len(), "and it is the same frame the zero latent drew");

    // …and the latent SCALES with the drive, which is the half of the mode a zero cannot show: a
    // bigger number lands further from the mean, where `drawn` puts every drive on the one shell and
    // hands back a picture the same distance out however hard it was pushed.
    // Smoothing off first, and every reading waits for the frame to CHANGE — a param set here lands
    // on the node's own thread, so the frame already in hand is one from before it.
    g.set_param(node, "decoder", "smooth", 0.0);
    g.set_param(drive, "constant", "value", 0.35);
    let near = g.until("the picture at a small drive", |_| {
        probe.latest().map(|d| f32s(&d)).filter(|v| v.iter().zip(&centred).any(|(a, b)| (a - b).abs() > 1e-4))
    });
    g.set_param(drive, "constant", "value", 3.0);
    let away = g.until("the picture at a large drive", |_| {
        probe.latest().map(|d| f32s(&d)).filter(|v| v.iter().zip(&near).any(|(a, b)| (a - b).abs() > 1e-4))
    });
    let reach = |v: &[f32]| v.iter().zip(&centred).map(|(a, b)| (a - b).abs()).sum::<f32>();
    assert!(
        reach(&away) > reach(&near) * 1.8,
        "a drive of 3.0 must land further out than one of 0.35 ({} vs {}); on the shell they would tie",
        reach(&away),
        reach(&near)
    );
    assert!(away.iter().all(|x| (0.0..=1.0).contains(x)), "direct mode still answers a frame");
    assert!(g.error(node).is_none(), "Decoder stands with no error in direct mode: {:?}", g.error(node));

    // A file that is no model says so, rather than drawing the one it used to hold.
    g.set_param(node, "decoder", "file", model.with_extension("absent").to_string_lossy().to_string());
    g.until("the missing model to be reported", |g| g.error(node));
}

#[test]
fn an_onnx_node_runs_a_model_and_routes_each_sender_to_the_input_it_names() {
    // The generic runner, against a model that answers `alpha - 2*beta`: an answer names which array
    // landed on which input, so binding by NAME and binding by arrival order cannot both be right.
    let _py = require_python();
    let g = Goofi::new();
    let ty = install_bundled(&g, "ml", "onnx.py");
    let model = g.state.mount().join("pair.onnx");
    std::fs::write(&model, include_bytes!("fixtures/two_inputs.onnx")).unwrap();

    let node = g.add(&ty);
    g.set_param(node, "onnx", "file", model.to_string_lossy().to_string());
    let probe = g.probe(node, "out");

    let one = g.add("signal:Constant");
    g.set_param(one, "constant", "value", 1.0);
    g.set_param(one, "constant", "shape", "3");
    let two = g.add("signal:Constant");
    g.set_param(two, "constant", "value", 2.0);
    g.set_param(two, "constant", "shape", "3");
    g.call("node edit", j!({ "node": hex(one), "name": "alpha" }));
    g.call("node edit", j!({ "node": hex(two), "name": "beta" }));

    // Wired the WRONG way round on purpose: beta reaches the slot first. By arrival order it would
    // feed `alpha` and the answer would be 2 - 2*1 = 0; by name it feeds `beta`, for 1 - 2*2 = -3.
    g.link(two, "out", node, "input");
    g.link(one, "out", node, "input");
    let d = first_frame(&g, &ty, node, &probe, |d| shape(d) == vec![1, 3]);
    let got = f32s(&d);
    assert!(
        got.iter().all(|v| (v + 3.0).abs() < 1e-4),
        "each sender feeds the input its NAME matches, whatever order it arrived in: {got:?}"
    );

    // Rename the senders past each other and the routing follows the names, not the wires. Two
    // nodes cannot hold one name at once, so the swap goes through a name neither input answers to.
    g.call("node edit", j!({ "node": hex(two), "name": "held" }));
    g.call("node edit", j!({ "node": hex(one), "name": "beta" }));
    g.call("node edit", j!({ "node": hex(two), "name": "alpha" }));
    let swapped = g.until("the rename to re-route the inputs", |_| {
        probe.latest().map(|d| f32s(&d)).filter(|v| v.iter().all(|x| x.abs() < 1e-4))
    });
    assert_eq!(swapped.len(), 3, "2 - 2*1 = 0 once the names are the other way round");
    assert!(g.error(node).is_none(), "Onnx stands with no error: {:?}", g.error(node));
}

#[test]
fn an_onnx_node_hands_back_the_models_own_shape_until_it_is_asked_for_a_picture() {
    // `raw` is the contract for a model that answers anything at all; `image` is the one convenience,
    // and it owes a viewer rows down, channels last, and values it can draw.
    let _py = require_python();
    let g = Goofi::new();
    let ty = install_bundled(&g, "ml", "onnx.py");
    let model = g.state.mount().join("tiny.onnx");
    std::fs::write(&model, include_bytes!("fixtures/tiny_generator.onnx")).unwrap();

    let node = g.add(&ty);
    g.set_param(node, "onnx", "file", model.to_string_lossy().to_string());
    let probe = g.probe(node, "out");

    // Nothing is wired that fills the latent: a half-built patch still gets an answer, padded.
    let drive = g.add("signal:Constant");
    g.set_param(drive, "constant", "value", 0.0);
    g.set_param(drive, "constant", "shape", "4");
    g.link(drive, "out", node, "input");

    let d = first_frame(&g, &ty, node, &probe, |d| shape(d).len() == 4);
    assert_eq!(shape(&d), vec![1, 3, 4, 4], "raw hands back what the model answered, batch and all");

    g.set_param(node, "onnx", "layout", "image");
    let drawn = g.until("the picture layout", |_| probe.latest().filter(|d| shape(d) == vec![4, 4, 3]));
    let v = f32s(&drawn);
    assert!(v.iter().all(|x| (0.0..=1.0).contains(x)), "a signed model is handed over as a frame: {v:?}");
    // Its bias ramps DOWN the rows, which a frame passed over upside down would get backwards.
    assert!(v[0] < 0.05 && v[3 * 4 * 3] > 0.95, "rows run down: {} then {}", v[0], v[3 * 4 * 3]);

    // A name the model does not answer to is said plainly, rather than quietly drawing output zero.
    g.set_param(node, "onnx", "output", "nope");
    g.until("the unknown output to be reported", |g| g.error(node));
}
