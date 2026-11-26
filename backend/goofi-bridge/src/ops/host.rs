//! The host's own doors: plugins, the filesystem, the log and the agent harnesses. A plugin's
//! state is the one thing here the patch holds.

use serde_json::{json, Value};

use super::{op, Any, EffectOp, NoArgs, ReadOp, WriteOp};
use crate::{fsbrowse, AppState, Caller, Event, Txn};

op!(PluginList, "plugin list", 0, NoArgs,
    "List installed plugins and their status.",
    "{plugins}");

op!(PluginStateList, "plugin state list", 0, NoArgs,
    "What each plugin keeps with the patch, by plugin id. The `.gfi` carries it, and an undo takes an edit of it back.",
    "{states: {<plugin>: any}}");

op!(PluginStateEdit, "plugin state edit", 2, PluginStateEditArgs {
    pub plugin: String,
    pub value: Option<Any>,
},
    "Keep a JSON value with the patch for a plugin, at most 1 MiB; no `value`, or null, removes the plugin's record. The plugin's `on_state` hook hears what the patch holds after every change: this edit, a load, a new patch, an undo.",
    "{plugin, state}");

op!(DirStat, "dir stat", 1, DirStatArgs {
    pub path: String,
},
    "Resolve a host path and report whether it names a file, a directory, or nothing.",
    "{path, kind: file | dir | missing}");

op!(DirList, "dir list", 1, DirListArgs {
    pub path: Option<String>,
    pub hidden: Option<bool>,
    pub sort: Option<String>,
    pub reverse: Option<bool>,
},
    "List a directory on the goofi host — the save/load browser's read. Without a path it opens the folder of the last patch loaded or saved, else the working directory; `roots` lists Home, the working directory and the five folders patches were last loaded from or saved to (`recent`). Dot-names are left out; `--hidden` includes them. Directories come first; `--sort` is name (the default), modified or size, and `--reverse` turns the order.",
    "{path, parent, entries: [{name, kind, is_gfi, modified, size}], roots: [{label, path, recent}]} — `modified` is epoch milliseconds, `size` bytes and null for a directory");

op!(LogList, "log list", 0, NoArgs,
    "Read the retained log groups, ordered by their last occurrence.",
    "{cursor, oldest, reset, groups}");

op!(LogWrite, "log write", 1, LogWriteArgs {
    pub text: String,
    pub level: Option<String>,
    pub component: Option<String>,
},
    "Write an application message. Level is info, warning or error.",
    "{logged: true}");

op!(AgentList, "agent list", 0, NoArgs,
    "The agents the config offers to launch, and the ones goofi has running.",
    "{instances: [{id, harness, model, effort, state, exit_code, pid, owner, port, born}], agents: [{name, command, model, effort}], config_error: string | null} — an instance's `owner` is the goofi process whose PTY it runs on; an agent's `model` and `effort` are the flag templates its config entry gives, null where it takes no such option");

op!(AgentStart, "agent start", 1, AgentStartArgs {
    pub name: String,
    pub model: Option<String>,
    pub effort: Option<String>,
},
    "Launch a config-listed agent on a PTY with the patch workspace as its cwd, under a login shell — a command that cannot launch fails on the PTY itself. `--model` and `--effort` are appended as the flags the config entry names for them, and refused on an entry that names none. Read its terminal at /term/<instance_id>. An unknown name is refused with the config's list.",
    "{instance_id: string}");

op!(AgentStop, "agent stop", 1, AgentStopArgs {
    pub instance: String,
},
    "Stop a running agent (SIGTERM, then SIGKILL), or dismiss one that already exited. A drag it left in flight ends with it; the exit code arrives on harness_changed.",
    "{ok: true}");

impl ReadOp for PluginList {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        crate::plugins::list(tx)
    }
}

impl ReadOp for PluginStateList {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(json!({ "states": tx.g.plugin_states() }))
    }
}

impl WriteOp for PluginStateEdit {
    fn run(tx: &mut Txn, a: PluginStateEditArgs) -> Result<Value, String> {
        if !crate::plugins::valid_id(&a.plugin) {
            return Err(format!("`{}` is not a plugin id", a.plugin));
        }
        let state = a.value.map_or(Value::Null, |v| v.0);
        if state.to_string().len() > 1 << 20 {
            return Err("a plugin's state is at most 1 MiB".into());
        }
        tx.g.set_plugin_state(&a.plugin, state.clone());
        Ok(json!({ "plugin": a.plugin, "state": state }))
    }

    fn label(a: &PluginStateEditArgs, _: &Value) -> String {
        format!("Edit {} state", a.plugin)
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

/// The roster the snapshot seeds and `harness_changed` broadcasts: the session's instances and
/// the config's launchable list. `offered` is read off the graph lock by the caller.
pub fn roster(agents: &goofi_agent::Agents, offered: Offered) -> Value {
    roster_of(&agents.roster(), offered)
}

type Offered = (Vec<goofi_supervisor::layout::Agent>, Option<String>);

fn roster_of(instances: &[goofi_agent::Record], (listed, config_error): Offered) -> Value {
    json!({ "instances": instances, "agents": listed, "config_error": config_error })
}

/// The roster as the status beat holds it: read from disk when the config or a record moved or
/// an owner died, and on every beat where no watcher took the paths.
pub struct Held {
    moved: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    offered: Offered,
    instances: Vec<goofi_agent::Record>,
}

impl Held {
    pub fn new(agents: &goofi_agent::Agents) -> Held {
        let moved = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let records = agents.records_dir();
        let _ = std::fs::create_dir_all(&records);
        let watched = [records, goofi_supervisor::layout::config_file()].iter().all(|path| {
            let moved = moved.clone();
            crate::autosave::watch_path(path, std::sync::Arc::new(move || moved.store(true, std::sync::atomic::Ordering::Release)))
        });
        Held { moved: watched.then_some(moved), offered: goofi_agent::Agents::offered(), instances: agents.roster() }
    }

    /// The roster now.
    pub fn roster(&mut self, agents: &goofi_agent::Agents) -> Value {
        let moved = self.moved.as_ref().is_none_or(|moved| moved.swap(false, std::sync::atomic::Ordering::AcqRel));
        if moved || self.instances.iter().any(|r| !goofi_supervisor::child::alive(r.owner)) {
            self.offered = goofi_agent::Agents::offered();
            self.instances = agents.roster();
        }
        roster_of(&self.instances, self.offered.clone())
    }
}

impl ReadOp for AgentList {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(roster(&tx.state.agents, goofi_agent::Agents::offered()))
    }
}

impl EffectOp for AgentStart {
    fn run(state: &AppState, a: AgentStartArgs, _: &Caller) -> Result<Value, String> {
        // The mount lock is held ACROSS the spawn, so a concurrent load's swap-and-delete cannot
        // take the workspace out from under the child's cwd.
        let id = {
            let mount = state.mount.lock();
            let dir = mount.as_ref().map(crate::Mount::path).unwrap_or_default();
            let launch = goofi_agent::Launch { model: a.model, effort: a.effort };
            state.agents.spawn(&a.name, launch, &dir, &goofi_agent::parent_env())?
        };
        state.events.send(Event::HarnessChanged(roster(&state.agents, goofi_agent::Agents::offered())));
        Ok(json!({ "instance_id": id }))
    }
}

impl EffectOp for AgentStop {
    fn run(state: &AppState, a: AgentStopArgs, _: &Caller) -> Result<Value, String> {
        // A drag the stopped shell left in flight is ended where its exit is SEEN, by the roster watch.
        state.agents.stop(&a.instance)?;
        state.events.send(Event::HarnessChanged(roster(&state.agents, goofi_agent::Agents::offered())));
        Ok(json!({ "ok": true }))
    }
}
