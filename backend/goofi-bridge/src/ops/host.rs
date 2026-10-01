//! The host's own doors: plugins, the filesystem, the log and the agent harnesses. None of
//! them touches the graph.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{op, EffectOp, NoArgs, ReadOp};
use crate::{fsbrowse, term, AppState, Caller, Event, Txn};

// ---- plugin list (Read)
op!(PluginList, "plugin list", 0, NoArgs, Value,
    "List installed plugins and their status.",
    "{plugins}");

// ---- dir stat (Read)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DirStatArgs {
    pub path: String,
}

op!(DirStat, "dir stat", 1, DirStatArgs, Value,
    "Resolve a host path and report whether it names a file, a directory, or nothing.",
    "{path, kind: file | dir | missing}");

// ---- dir list (Read)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DirListArgs {
    pub path: Option<String>,
    pub hidden: Option<bool>,
    pub sort: Option<String>,
    pub reverse: Option<bool>,
}

op!(DirList, "dir list", 1, DirListArgs, Value,
    "List a directory on the goofi host — the save/load browser's read. Without a path it opens the folder of the last patch loaded or saved, else the working directory; `roots` lists Home, the working directory and the five folders patches were last loaded from or saved to (`recent`). Dot-names are left out; `--hidden` includes them. Directories come first; `--sort` is name (the default), modified or size, and `--reverse` turns the order.",
    "{path, parent, entries: [{name, kind, is_gfi, modified, size}], roots: [{label, path, recent}]} — `modified` is epoch milliseconds, `size` bytes and null for a directory");

// ---- log list (Read)
op!(LogList, "log list", 0, NoArgs, Value,
    "Read the retained log groups, ordered by their last occurrence.",
    "{cursor, oldest, reset, groups}");

// ---- log write (Effect)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogWriteArgs {
    pub text: String,
    pub level: Option<String>,
    pub component: Option<String>,
}

op!(LogWrite, "log write", 1, LogWriteArgs, Value,
    "Write an application message. Level is info, warning or error.",
    "{logged: true}");

// ---- agent list (Read)
op!(AgentList, "agent list", 0, NoArgs, Value,
    "The agents the config offers to launch, and the ones goofi has running.",
    "{instances: [{id, harness, state, exit_code}], agents: [{name, command}], config_error: string | null}");

// ---- agent start (Effect)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentStartArgs {
    pub name: String,
}

op!(AgentStart, "agent start", 1, AgentStartArgs, Value,
    "Launch a config-listed agent on a PTY with the patch workspace as its cwd, under a login shell — a command that cannot launch fails on the PTY itself. Read its terminal at /term/<instance_id>. An unknown name is refused with the config's list.",
    "{instance_id: string}");

// ---- agent stop (Effect)
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentStopArgs {
    pub instance: String,
}

op!(AgentStop, "agent stop", 1, AgentStopArgs, Value,
    "Stop a running agent (SIGTERM, then SIGKILL), or dismiss one that already exited. The shell's undo stack dies with it; the exit code arrives on harness_changed.",
    "{ok: true}");

impl ReadOp for PluginList {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        crate::plugins::list(tx, &Value::Null)
    }
}

impl ReadOp for DirStat {
    fn run(_: &mut Txn, a: DirStatArgs) -> Result<Value, String> {
        let path = fsbrowse::resolve(&a.path);
        let kind = match std::fs::metadata(&path) {
            Ok(meta) if meta.is_dir() => "dir",
            Ok(_) => "file",
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing",
            Err(e) => return Err(format!("{path}: {e}")),
        };
        Ok(json!({ "path": path, "kind": kind }))
    }
}

impl ReadOp for DirList {
    /// Served WITHOUT the graph mutex: it walks the filesystem, which under the lock would stall
    /// the status-drain worker.
    fn run(_: &mut Txn, a: DirListArgs) -> Result<Value, String> {
        let sort = fsbrowse::Sort::parse(a.sort.as_deref().unwrap_or("name"))?;
        Ok(fsbrowse::list_dir(a.path.as_deref(), a.hidden.unwrap_or(false), sort, a.reverse.unwrap_or(false)))
    }
}

impl ReadOp for LogList {
    fn run(_: &mut Txn, _: NoArgs) -> Result<Value, String> {
        serde_json::to_value(goofi_supervisor::log::since(None)).map_err(|e| e.to_string())
    }
}

impl EffectOp for LogWrite {
    fn run(_: &AppState, a: LogWriteArgs, _: &Caller) -> Result<Value, String> {
        use goofi_supervisor::log::{record, Level, Source};
        let level = match a.level.as_deref().unwrap_or("info") {
            "info" => Level::Info,
            "warning" => Level::Warning,
            "error" => Level::Error,
            _ => return Err("Level must be info, warning or error".into()),
        };
        record(Source::component(a.component.as_deref().unwrap_or("console")), level, None, &a.text);
        Ok(json!({ "logged": true }))
    }
}

// The harness ops touch no graph state: they fork and signal children, and the roster converges
// through `harness_changed` rather than by making a caller wait.

impl ReadOp for AgentList {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(tx.state.harnesses.roster(&goofi_supervisor::home::agents()))
    }
}

impl EffectOp for AgentStart {
    fn run(state: &AppState, a: AgentStartArgs, _: &Caller) -> Result<Value, String> {
        // The mount lock is held ACROSS the spawn, so a concurrent load's swap-and-delete cannot
        // take the workspace out from under the child's cwd.
        let id = {
            let mount = state.mount.lock();
            let dir = mount.as_ref().map(crate::Mount::path).unwrap_or_default();
            state.harnesses.spawn(&a.name, &dir, &state.instance_id, &term::parent_env(), state.events.clone(), state.history.clone())?
        };
        state.events.send(Event::HarnessChanged(state.harnesses.roster(&goofi_supervisor::home::agents())));
        Ok(json!({ "instance_id": id }))
    }
}

impl EffectOp for AgentStop {
    fn run(state: &AppState, a: AgentStopArgs, _: &Caller) -> Result<Value, String> {
        // The stopped shell's undo stack is dropped by the REAPER, where the actor really dies.
        state.harnesses.stop(&a.instance)?;
        state.events.send(Event::HarnessChanged(state.harnesses.roster(&goofi_supervisor::home::agents())));
        Ok(json!({ "ok": true }))
    }
}
