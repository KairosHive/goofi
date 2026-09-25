//! Probe-based discovery over the real `goofi` extension. It finds an interpreter itself, and
//! FAILS with an actionable message rather than skipping. Override with GOOFI_PYMOD_TEST_PYTHON.

use std::path::Path;

use goofi_python::{discover_one, probe_introspect, Discovery};
use goofi_node::Isolation;

/// Whether `py` runs `code` cleanly, with the host's Python variables stripped as the probe strips them.
fn answers(py: &str, code: &str) -> bool {
    std::process::Command::new(py)
        .args(["-c", code])
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// A repo venv's interpreter, where goofi-init made it.
fn venv(name: &str) -> Option<String> {
    goofi_init::venv_python(&goofi_init::repo_root().join(name)).map(|p| p.to_string_lossy().into_owned())
}

/// A set, non-empty override.
fn named(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|p| !p.is_empty())
}

/// The first interpreter that can `import goofi`: an override, the repo's venvs, then the system one.
fn test_python() -> String {
    let cands: Vec<String> = named("GOOFI_PYMOD_TEST_PYTHON").into_iter()
        .chain(venv(goofi_init::FT_VENV))
        .chain(venv(goofi_init::GIL_VENV))
        .chain(["python3".to_string()])
        .collect();
    cands.iter().find(|c| answers(c, "import goofi")).cloned().unwrap_or_else(|| panic!(
        "no python with `goofi` importable (tried {cands:?}). Provision one with \
         goofi once (it provisions both venvs at startup), or set GOOFI_PYMOD_TEST_PYTHON. Refusing to \
         silent-skip a cross-language test."
    ))
}

/// The FREE-THREADED interpreter (the in-process tier's host), for the routing gate's SAFE half.
fn ft_python() -> String {
    let cands: Vec<String> = named("GOOFI_FT_PYTHON").into_iter().chain(venv(goofi_init::FT_VENV)).collect();
    // Free-threaded builds are exactly the ones where sys._is_gil_enabled() is False.
    let ft = "import sys; sys.exit(0 if not sys._is_gil_enabled() else 1)";
    cands.iter().find(|c| answers(c, ft)).cloned().unwrap_or_else(|| panic!(
        "no free-threaded python found (tried {cands:?}). Provision one with \
         goofi once (it provisions both venvs at startup), or set GOOFI_FT_PYTHON."
    ))
}

/// The GIL interpreter (the subprocess tier's host) — the repo's `.gfivenv`.
fn gil_python() -> String {
    venv(goofi_init::GIL_VENV).filter(|p| answers(p, "import goofi")).unwrap_or_else(|| panic!(
        "no `.gfivenv` python with goofi importable. Run goofi once (it provisions both venvs at startup)"
    ))
}

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A probe memo this process alone writes, so no earlier run's answer stands in for a probe.
fn memo() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("goofi-probe-memo-{}", std::process::id()))
}

#[test]
fn discovers_a_valid_node_with_declarations() {
    let py = test_python();
    let Discovery::Found(d) = discover_one(&fixtures().join("negate.py"), &py, Isolation::Subprocess, &memo())
    else {
        panic!("negate.py discovers")
    };
    assert_eq!(d.manifest.type_name, "Negate");
    assert_eq!(d.manifest.doc, "Negate the input.");
    assert_eq!(d.manifest.inputs[0].name, "data");
    assert_eq!(d.manifest.outputs[0].name, "out");
}

#[test]
fn a_python_input_slot_declares_required_and_trigger_independently() {
    // A bare `DataType` is the pre-`InputSlot` behaviour and must stay it — triggering, not
    // required — while `InputSlot` makes each authorable WITHOUT touching the other.
    let py = test_python();
    let Discovery::Found(d) =
        discover_one(&fixtures().join("declared.py"), &py, Isolation::Subprocess, &memo())
    else {
        panic!("declared.py discovers")
    };
    let slot = |name: &str| {
        d.manifest.inputs.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("slot {name}"))
    };
    assert!(!slot("bare").required && slot("bare").trigger_process, "a bare DataType declares nothing");
    assert!(slot("needed").required && slot("needed").trigger_process, "required does not touch trigger");
    assert!(!slot("passive").trigger_process && !slot("passive").required, "nor trigger required");
}

#[test]
fn a_python_node_can_declare_itself_a_producer() {
    // `#[serde(default)]` on the schema is load-bearing: an older installed wheel emits no key, and
    // a hard parse failure greys out every node it discovers.
    let py = test_python();
    let Discovery::Found(d) =
        discover_one(&fixtures().join("producer.py"), &py, Isolation::InProcess, &memo())
    else {
        panic!("producer.py discovers")
    };
    assert!(d.manifest.producer, "the class attribute reached the manifest");

    let Discovery::Found(n) =
        discover_one(&fixtures().join("negate.py"), &py, Isolation::InProcess, &memo())
    else {
        panic!("negate.py discovers")
    };
    assert!(!n.manifest.producer, "and a node that does not declare it is not one");
}

#[test]
fn a_producer_that_is_not_a_bool_is_refused_rather_than_read_as_false() {
    // `Manifest` refuses a non-bool where it is WRITTEN, so the import fails and the node greys out.
    let py = test_python();
    match discover_one(&fixtures().join("bad_producer.py"), &py, Isolation::InProcess, &memo()) {
        Discovery::Unavailable { type_name, reason } => {
            assert_eq!(type_name, "BadProducer");
            assert!(!reason.is_empty(), "the palette tooltip gets something to show");
        }
        _ => panic!("a non-bool `producer` must not discover as a healthy node"),
    }
}

#[test]
fn missing_dep_greys_out_instead_of_crashing() {
    let py = test_python();
    // The probe import fails -> the REASON, never a panic, so the palette can explain itself.
    let err = probe_introspect(&fixtures().join("missing_dep.py"), &py).unwrap_err();
    assert_eq!(err, "definitely_not_installed_pkg", "a missing import names the module");
    match discover_one(&fixtures().join("missing_dep.py"), &py, Isolation::Subprocess, &memo()) {
        Discovery::Unavailable { type_name, reason } => {
            assert_eq!(type_name, "MissingDep");
            assert_eq!(reason, "definitely_not_installed_pkg");
        }
        _ => panic!("a node whose dep is missing is Unavailable, not skipped"),
    }
    // A file that imports CLEANLY and simply declares no node is Unavailable too, and for a reason
    // of its own — the palette must be able to say which of the two happened.
    match discover_one(&fixtures().join("no_node.py"), &py, Isolation::Subprocess, &memo()) {
        Discovery::Unavailable { type_name, reason } => {
            assert_eq!(type_name, "NoNode");
            assert!(!reason.is_empty(), "a file with no node says so rather than greying out blank");
        }
        _ => panic!("a file declaring no node is Unavailable, not Found and not skipped"),
    }
    // A file that is not OFFERED as a node at all is a different outcome entirely.
    assert!(matches!(
        discover_one(&fixtures().join("_hidden.py"), &py, Isolation::Subprocess, &memo()),
        Discovery::Skip
    ));
    // A slot an expression could not read as an attribute — `in` is a keyword — greys the type out
    // with the slot quoted, so a bad name cannot enter through a Python node.
    match discover_one(&fixtures().join("bad_slot.py"), &py, Isolation::Subprocess, &memo()) {
        Discovery::Unavailable { type_name, reason } => {
            assert_eq!(type_name, "BadSlot");
            assert!(reason.contains("slot `in`") && reason.contains("letters or digits"), "{reason}");
        }
        _ => panic!("a node declaring an illegal slot name is Unavailable, with the slot named"),
    }
    // A tag outside the closed vocabulary greys the type out the same way, and the reason lists
    // the vocabulary — the author is told what to write instead.
    match discover_one(&fixtures().join("bad_tag.py"), &py, Isolation::Subprocess, &memo()) {
        Discovery::Unavailable { type_name, reason } => {
            assert_eq!(type_name, "BadTag");
            assert!(reason.contains("unknown tag `sparkly`") && reason.contains("transform"), "{reason}");
        }
        _ => panic!("a node declaring a tag the vocabulary does not hold is Unavailable"),
    }
}

#[test]
fn a_node_whose_import_prints_still_discovers() {
    // A dependency that greets stdout on import would prepend itself to the payload, so the probe
    // child routes fd 1 to stderr the way the serve loop already does.
    let py = test_python();
    let Discovery::Found(d) =
        discover_one(&fixtures().join("chatty.py"), &py, Isolation::Subprocess, &memo())
    else {
        panic!("a node whose import prints to stdout still discovers")
    };
    assert_eq!(d.manifest.type_name, "Chatty");
    assert_eq!(d.manifest.doc, "A node whose dependency prints on import.");
}

#[test]
fn probe_ignores_a_host_pythonpath() {
    // The probe interpreter must import ITS OWN installed goofi, never one leaked in through a host
    // `PYTHONPATH`, which `.cargo/config.toml` sets for the embedded FT interpreter.
    let py = test_python();
    let poison = std::env::temp_dir().join(format!("goofi_poison_{}", std::process::id()));
    std::fs::create_dir_all(poison.join("goofi")).unwrap();
    std::fs::write(
        poison.join("goofi").join("__init__.py"),
        "raise RuntimeError('a host-PYTHONPATH goofi must be ignored by the probe')\n",
    )
    .unwrap();

    // Every probe Command strips PYTHONPATH, so setting it here cannot leak into sibling tests.
    std::env::set_var("PYTHONPATH", &poison);
    let d = discover_one(&fixtures().join("negate.py"), &py, Isolation::Subprocess, &memo());
    std::env::remove_var("PYTHONPATH");
    let _ = std::fs::remove_dir_all(&poison);

    assert!(
        matches!(d, Discovery::Found(_)),
        "discovery must ignore a poison goofi on the host PYTHONPATH"
    );
}

#[test]
fn every_param_kind_carries_its_doc_across_the_probe() {
    // `doc=` crosses Python, the wheel's introspect and ParamDecl — one arm per param kind on each side.
    let py = test_python();
    let Discovery::Found(d) =
        discover_one(&fixtures().join("documented.py"), &py, Isolation::Subprocess, &memo())
    else {
        panic!("documented.py discovers")
    };
    let doc = |name: &str| {
        d.manifest
            .params
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("param `{name}` discovered"))
            .doc
    };
    assert_eq!(doc("count"), Some("how many"));
    let count = d.manifest.params.iter().find(|p| p.name == "count").unwrap();
    assert!(matches!(count.spec, goofi_node::ParamSpec::Int { options: &[1, 2, 4, 8], .. }));
    assert_eq!(doc("gain"), Some("how loud"));
    assert_eq!(doc("enabled"), Some("whether to run"));
    assert_eq!(doc("mode"), Some("which mode"));
}

#[test]
fn the_probe_is_the_gil_routing_gate_and_samples_after_the_whole_import() {
    // `Discovered.gil_safe` is the ONE oracle deciding which tier a Python node runs on; a wrong
    // answer either quarantines a fast node or re-enables the GIL for every in-process node.
    let node = fixtures().join("negate.py");

    let Discovery::Found(ft) = discover_one(&node, &ft_python(), Isolation::InProcess, &memo()) else {
        panic!("negate.py discovers on the free-threaded interpreter")
    };
    assert!(ft.gil_safe, "a free-threaded probe whose imports left the GIL disabled → in-process tier");

    let Discovery::Found(gil) = discover_one(&node, &gil_python(), Isolation::Subprocess, &memo()) else {
        panic!("negate.py discovers on the GIL interpreter")
    };
    assert!(!gil.gil_safe, "a GIL interpreter can never host in-process → subprocess tier");

    // The GIL state is read after the module has been IMPORTED: a C extension built without
    // free-threading support re-enables it, and sampled earlier such a node routes to the wrong tier.
    let Discovery::Found(flip) =
        discover_one(&fixtures().join("gil_flip.py"), &ft_python(), Isolation::InProcess, &memo())
    else {
        panic!("gil_flip.py discovers on the free-threaded interpreter")
    };
    assert!(!flip.gil_safe, "an import that re-enabled the GIL must route to the subprocess tier");
}
