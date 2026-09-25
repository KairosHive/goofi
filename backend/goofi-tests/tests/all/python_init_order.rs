//! The embedded interpreter from cold, in a child process so nothing started it first: the
//! evaluator starts it, Python nodes are built after, and none leaves its module behind.

use goofi_python::inproc::{PyExprEvaluator, PyNode};
use pyo3::prelude::*;
use pyo3::types::PyModule;

const NODE: &str = concat!(
    "import goofi\n",
    "import numpy as np\n",
    "class Double(goofi.Node):\n",
    "    INPUTS = {'data': goofi.DataType.ARRAY}\n",
    "    OUTPUTS = {'out': goofi.DataType.ARRAY}\n",
    "    def process(self, data):\n",
    "        return {'out': data.data * 2.0}\n",
);

/// The env var that turns this binary into the child below. Its value is irrelevant.
const COLD: &str = "GOOFI_COLD_INTERPRETER_CHILD";

#[test]
fn a_cold_interpreter_is_started_by_the_evaluator_and_its_nodes_leave_no_module_behind() {
    let out = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args(["--exact", &format!("{}::cold_child", crate::situation(module_path!())), "--nocapture"])
        .env(COLD, "1")
        .output()
        .expect("run the child");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && said.contains("1 passed"), "{said}");
}

#[test]
fn cold_child() {
    if std::env::var(COLD).is_err() {
        return; // the ordinary run: this test is only the child's entry point
    }
    let _evaluator = PyExprEvaluator::new().expect("the evaluator starts the interpreter");
    let _nodes: Vec<PyNode> = (0..8)
        .map(|_| PyNode::from_source(NODE, vec![("data", false)], vec!["out"])
            .expect("a node built after the evaluator started the interpreter"))
        .collect();
    // `from_source` pops the module it minted; the instance keeps it alive via `__globals__`.
    let lingering = Python::attach(|py| {
        let modules = PyModule::import(py, "sys").unwrap().getattr("modules").unwrap();
        let keys = modules.call_method0("keys").unwrap();
        keys.try_iter().unwrap().filter_map(|k| k.ok()?.extract::<String>().ok())
            .filter(|k| k.starts_with("goofi_user_"))
            .count()
    });
    assert_eq!(lingering, 0, "a node's module stays in sys.modules, which then grows without bound");
}
