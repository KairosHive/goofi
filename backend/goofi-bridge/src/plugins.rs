//! Folder plugins: discovery, isolated Python services, and shared operation hooks.

use crate::{
    ops::{Handler, Op},
    AppState,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

const SDK: &str = include_str!("../../../sdk/python/goofi_plugin/__init__.py");
const MAIN: &str = include_str!("../../../sdk/python/goofi_plugin/__main__.py");
const FRONTEND_SDK: &str = include_str!("../../../sdk/frontend/index.ts");
const FRONTEND_PACKAGE: &str = include_str!("../../../sdk/frontend/package.json");
const TIMEOUT: Duration = Duration::from_secs(30);
/// How long one build tool may run: an `npm ci` over a slow link is minutes, never longer.
const BUILD_WAIT: Duration = Duration::from_secs(900);

thread_local! { static CHAIN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }

/// Keep nested host calls finite, including calls made by Python hooks.
pub(crate) struct CallScope;
impl CallScope {
    pub fn enter(op: &str) -> Result<Self, String> {
        CHAIN.with(|chain| {
            let mut chain = chain.borrow_mut();
            if chain.len() >= 16 || chain.iter().any(|parent| parent == op) {
                return Err(format!("recursive plugin call: {op}"));
            }
            chain.push(op.into());
            Ok(Self)
        })
    }
}
impl Drop for CallScope {
    fn drop(&mut self) {
        CHAIN.with(|chain| {
            chain.borrow_mut().pop();
        });
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    version: String,
    api: u32,
}

#[derive(Deserialize)]
struct Operation {
    name: String,
    args: String,
    kind: String,
    doc: String,
    result: String,
}

#[derive(Default, Deserialize)]
struct Contributions {
    ops: Vec<Operation>,
    pre_op: Vec<String>,
    post_op: Vec<String>,
}

struct Package {
    manifest: Manifest,
    root: PathBuf,
    frontend: Option<PathBuf>,
    contributions: Contributions,
    service: Option<Arc<Service>>,
    error: Option<String>,
}

#[derive(Default)]
pub struct Plugins {
    packages: Vec<Package>,
    pub(crate) record_start: Mutex<()>,
    stopped: AtomicBool,
}

type Reply = Result<Value, String>;
type Pending = Arc<Mutex<HashMap<u64, mpsc::Sender<Reply>>>>;

struct Service {
    input: mpsc::Sender<Value>,
    child: Mutex<goofi_core::child::Child>,
    pending: Pending,
    sequence: AtomicU64,
    output: Mutex<Option<BufReader<std::process::ChildStdout>>>,
}

fn write(input: &mpsc::Sender<Value>, message: &Value) -> Result<(), String> {
    input
        .send(message.clone())
        .map_err(|_| "plugin service input closed".into())
}

fn log(id: &str, level: &str, message: &str) {
    use goofi_core::log::{record, Level, Source};
    let level = match level {
        "error" => Level::Error,
        "warning" => Level::Warning,
        _ => Level::Info,
    };
    record(
        Source::component(&format!("plugin:{id}")),
        level,
        None,
        message,
    );
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Service {
    fn spawn(
        python: &Path,
        sdk: &Path,
        config: Value,
    ) -> Result<(Arc<Self>, Contributions), String> {
        let id = config["id"].as_str().unwrap_or_default().to_string();
        // stdout is the protocol channel; what the plugin says on stderr is its log.
        let mut child = goofi_core::child::run(
            format!("plugin {id}"),
            Command::new(python)
                .args(["-u", "-m", "goofi_plugin"])
                .arg(config.to_string())
                .env("PYTHONPATH", sdk)
                .env_remove("PYTHONHOME")
                .current_dir(
                    config["package_dir"]
                        .as_str()
                        .ok_or("missing package path")?,
                ),
        )
        .source(goofi_core::log::Source::component(&format!("plugin:{id}")))
        .stdin_piped()
        .stdout(goofi_core::child::Out::Pipe)
        .spawn()
        .map_err(|e| format!("start Python: {e}"))?;
        let input = child.stdin.take().ok_or("missing Python stdin")?;
        let output = child.stdout.take().ok_or("missing Python stdout")?;
        let (tx, rx) = mpsc::channel();
        let _ = goofi_core::worker::spawn("goofi-plugin-handshake", move || {
            let mut reader = BufReader::new(output);
            let mut line = String::new();
            let result = reader
                .read_line(&mut line)
                .map_err(|e| e.to_string())
                .and_then(|_| {
                    serde_json::from_str::<Value>(&line)
                        .map_err(|e| format!("plugin handshake: {e}"))
                })
                .and_then(|v| {
                    serde_json::from_value::<Contributions>(v["ready"].clone())
                        .map_err(|e| e.to_string())
                });
            let _ = tx.send((reader, result));
        });
        let handshake = rx.recv_timeout(TIMEOUT);
        let (reader, contributions) = match handshake {
            Ok((reader, Ok(contributions))) => (reader, contributions),
            other => {
                child.stop(Duration::ZERO);
                return Err(match other {
                    Ok((_, Err(e))) => e,
                    _ => "plugin startup timed out".into(),
                });
            }
        };
        let pending: Pending = Arc::default();
        let replies = pending.clone();
        let (sender, messages) = mpsc::channel::<Value>();
        // Pipe writes can block. Keep them off request threads so the deadline can kill a
        // service that stopped reading, including when the first payload exceeds the pipe.
        let _ = goofi_core::worker::spawn("goofi-plugin-stdin", move || {
            let mut input = input;
            for message in messages {
                let sent = serde_json::to_writer(&mut input, &message)
                    .map_err(|e| e.to_string())
                    .and_then(|()| {
                        input
                            .write_all(b"\n")
                            .and_then(|()| input.flush())
                            .map_err(|e| e.to_string())
                    });
                if let Err(error) = sent {
                    for (_, tx) in replies
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .drain()
                    {
                        let _ = tx.send(Err(format!("plugin service input: {error}")));
                    }
                    break;
                }
            }
        });
        Ok((
            Arc::new(Self {
                input: sender,
                child: Mutex::new(child),
                pending,
                sequence: AtomicU64::new(1),
                output: Mutex::new(Some(reader)),
            }),
            contributions,
        ))
    }

    fn listen(self: &Arc<Self>, state: AppState, plugin_id: String) {
        let Some(reader) = self
            .output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return;
        };
        let pending = self.pending.clone();
        let input = self.input.clone();
        let service = self.clone();
        let _ = goofi_core::worker::spawn("goofi-plugin-stdout", move || {
            for line in reader.lines() {
                let message = match line
                    .ok()
                    .and_then(|line| serde_json::from_str::<Value>(&line).ok())
                {
                    Some(message) => message,
                    None => break,
                };
                if let Some(id) = message["id"].as_u64() {
                    if let Some(tx) = pending
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&id)
                    {
                        let reply = match message["error"].as_str() {
                            Some(error) => Err(error.into()),
                            None => Ok(message["result"].clone()),
                        };
                        let _ = tx.send(reply);
                    }
                } else if let Some(id) = message["call"].as_u64() {
                    let state = state.clone();
                    let input = input.clone();
                    let plugin_id = plugin_id.clone();
                    let _ = goofi_core::worker::spawn("goofi-plugin-call", move || {
                        let chain: Vec<String> =
                            serde_json::from_value(message["chain"].clone()).unwrap_or_default();
                        CHAIN.with(|held| *held.borrow_mut() = chain);
                        let op = message["op"].as_str().unwrap_or_default();
                        let actor = message["actor"].as_str().unwrap_or(&plugin_id);
                        let result = state.call(op, message["args"].clone(), actor);
                        CHAIN.with(|held| held.borrow_mut().clear());
                        if id == 0 {
                            if let Err(error) = result {
                                log(&plugin_id, "error", &format!("log write: {error}"));
                            }
                            return;
                        }
                        let reply = match result {
                            Ok(value) => json!({"reply": id, "result": value}),
                            Err(error) => json!({"reply": id, "error": error}),
                        };
                        let _ = write(&input, &reply);
                    });
                }
            }
            service.kill();
            for (_, tx) in pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .drain()
            {
                let _ = tx.send(Err("plugin service stopped".into()));
            }
        });
    }

    fn request(&self, method: &str, data: Value, actor: &str) -> Reply {
        if self
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_wait()
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err("plugin service stopped".into());
        }
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, tx);
        let chain = CHAIN.with(|chain| chain.borrow().clone());
        if let Err(error) = write(
            &self.input,
            &json!({"id": id, "method": method, "data": data, "actor": actor, "chain": chain}),
        ) {
            self.pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id);
            return Err(error);
        }
        match rx.recv_timeout(TIMEOUT) {
            Ok(result) => result,
            Err(_) => {
                self.pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&id);
                self.kill();
                Err(
                    "plugin request timed out; service stopped; operation outcome is unknown"
                        .into(),
                )
            }
        }
    }

    /// Ask the service to leave and insist after a short grace, so a Python that is flushing
    /// its data gets to; a service that stopped answering gets no longer than that.
    fn kill(&self) {
        self.child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stop(Duration::from_secs(2));
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.kill();
    }
}

/// One build tool — `npm`, `uv` — to completion, or killed at a ceiling a stuck fetch cannot
/// hold a load past.
fn run(command: &mut Command) -> Result<(), String> {
    let tool = command.get_program().to_string_lossy().into_owned();
    let output = goofi_core::child::output(format!("plugin build: {tool}"), command, BUILD_WAIT).map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} failed ({}):\n{}{}",
            command.get_program().to_string_lossy(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

impl Plugins {
    /// Discover and prepare packages before the initial node scan.
    pub fn load(state: &mut AppState, home: &Path, python: &Path) -> Result<(), String> {
        if state.mode.demo {
            return Ok(());
        }
        let home = std::path::absolute(home).map_err(|e| e.to_string())?;
        let home = home.as_path();
        if !state.plugins.packages.is_empty() {
            return Err("plugins are already loaded; restart to change packages".into());
        }
        let sdk = std::path::absolute(goofi_build::base_dir(home))
            .map_err(|e| e.to_string())?
            .join("plugin-sdk")
            .join(goofi_build::digest([SDK.as_bytes(), MAIN.as_bytes()]));
        std::fs::create_dir_all(sdk.join("goofi_plugin")).map_err(|e| e.to_string())?;
        for (name, source) in [("__init__.py", SDK), ("__main__.py", MAIN)] {
            let destination = sdk.join("goofi_plugin").join(name);
            if !destination.is_file() {
                let temporary = destination.with_extension(format!("{}.tmp", goofi_core::session::tag()));
                std::fs::write(&temporary, source)
                    .and_then(|()| std::fs::rename(temporary, destination))
                    .map_err(|e| e.to_string())?;
            }
        }
        let root = home.join("plugins");
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let mut dirs: Vec<_> = std::fs::read_dir(&root)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        let mut packages = Vec::new();
        for dir in dirs {
            let manifest = std::fs::read_to_string(dir.join("goofi-plugin.toml"))
                .map_err(|e| e.to_string())
                .and_then(|text| toml::from_str::<Manifest>(&text).map_err(|e| e.to_string()));
            let manifest = match manifest {
                Ok(manifest)
                    if valid_id(&manifest.id)
                        && manifest.api == 1
                        && dir.file_name().and_then(|s| s.to_str()) == Some(&manifest.id) =>
                {
                    manifest
                }
                other => {
                    let id = dir
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let error = match other {
                        Err(e) => e,
                        _ => "plugin ID must match its folder; api must be 1".into(),
                    };
                    log(&id, "error", &error);
                    packages.push(Package {
                        manifest: Manifest {
                            id,
                            version: String::new(),
                            api: 1,
                        },
                        root: dir,
                        frontend: None,
                        contributions: Contributions::default(),
                        service: None,
                        error: Some(error),
                    });
                    continue;
                }
            };
            goofi_core::startup::report(format!("Preparing plugin {}", manifest.id));
            let mut package = Package {
                manifest,
                root: dir,
                frontend: None,
                contributions: Contributions::default(),
                service: None,
                error: None,
            };
            if let Err(error) = package.prepare(home, python, &sdk, state) {
                log(&package.manifest.id, "error", &error);
                package.error = Some(error);
                package.service = None;
                package.contributions = Contributions::default();
                package.frontend = None;
            }
            packages.push(package);
        }
        // Resolve hooks after all operation declarations are known. Remove failed packages
        // before the next pass so a hook cannot retain a target that was just withdrawn.
        loop {
            let names: std::collections::HashSet<String> =
                state
                    .ops()
                    .iter()
                    .map(|op| op.name.to_string())
                    .chain(packages.iter().flat_map(|package| {
                        package.contributions.ops.iter().map(|op| op.name.clone())
                    }))
                    .collect();
            let mut removed = false;
            for package in &mut packages {
                if package.error.is_some() {
                    continue;
                }
                let invalid = package
                    .contributions
                    .pre_op
                    .iter()
                    .chain(&package.contributions.post_op)
                    .find(|name| {
                        !names.contains(*name)
                            || matches!(name.as_str(), "compound" | "undo" | "redo")
                    });
                if let Some(name) = invalid {
                    let error = format!("unsupported hook operation: {name}");
                    log(&package.manifest.id, "error", &error);
                    package.error = Some(error);
                    package.service = None;
                    package.frontend = None;
                    package.contributions = Contributions::default();
                    removed = true;
                }
            }
            if !removed {
                break;
            }
        }
        state.plugins = Arc::new(Self {
            packages,
            record_start: Mutex::new(()),
            stopped: AtomicBool::new(false),
        });
        for package in &state.plugins.packages {
            if let Some(service) = &package.service {
                service.listen(state.clone(), package.manifest.id.clone());
            }
        }
        for package in &state.plugins.packages {
            if let Some(service) = &package.service {
                if let Err(error) = service.request("start", json!({}), &package.manifest.id) {
                    log(&package.manifest.id, "error", &format!("on_start: {error}"));
                    service.kill();
                }
            }
        }
        Ok(())
    }

    pub fn operations(&self) -> impl Iterator<Item = Op<'_>> {
        self.packages.iter().flat_map(|package| {
            package.contributions.ops.iter().map(|op| Op {
                name: &op.name,
                args: &op.args,
                positional: 0,
                doc: &op.doc,
                result: &op.result,
                handler: if op.kind == "read" {
                    Handler::PluginRead
                } else {
                    Handler::PluginEffect
                },
            })
        })
    }

    pub fn node_roots(&self) -> impl Iterator<Item = (PathBuf, goofi_graph::Origin)> + '_ {
        self.packages
            .iter()
            .filter(|p| p.error.is_none() && p.root.join("nodes").is_dir())
            .map(|p| {
                (
                    p.root.join("nodes"),
                    goofi_graph::Origin::Root(p.manifest.id.clone()),
                )
            })
    }

    pub fn has_hooks(&self, op: &str) -> bool {
        self.packages.iter().any(|p| {
            p.contributions
                .pre_op
                .iter()
                .chain(&p.contributions.post_op)
                .any(|name| name == op)
        })
    }

    pub fn pre_op(&self, _state: &AppState, op: &str, mut args: Value, actor: &str) -> Reply {
        let mut changed = BTreeMap::new();
        for package in &self.packages {
            if !package.contributions.pre_op.iter().any(|name| name == op) {
                continue;
            }
            let service = package
                .service
                .as_ref()
                .ok_or("plugin service unavailable")?;
            let patch = service
                .request(
                    "pre_op",
                    json!({"op": op, "args": args, "actor": actor}),
                    actor,
                )
                .map_err(|e| format!("{} pre_op: {e}", package.manifest.id))?;
            if patch.is_null() {
                continue;
            }
            let patch = patch
                .as_object()
                .ok_or("pre_op must return an argument patch or None")?;
            let args = args
                .as_object_mut()
                .ok_or("operation arguments must be an object")?;
            for (key, value) in patch {
                if let Some(owner) = changed.insert(key.clone(), &package.manifest.id) {
                    return Err(format!(
                        "pre_op conflict: {owner} and {} write `{key}`",
                        package.manifest.id
                    ));
                }
                args.insert(key.clone(), value.clone());
            }
        }
        Ok(args)
    }

    pub fn post_op(&self, _state: &AppState, op: &str, args: &Value, result: &Reply, actor: &str) {
        for package in &self.packages {
            if !package.contributions.post_op.iter().any(|name| name == op) {
                continue;
            }
            let Some(service) = &package.service else {
                continue;
            };
            let mut data = json!({"op": op, "args": args, "actor": actor, "ok": result.is_ok()});
            match result {
                Ok(value) => data["result"] = value.clone(),
                Err(error) => data["error"] = json!(error),
            }
            if let Err(error) = service.request("post_op", data, actor) {
                log(
                    &package.manifest.id,
                    "error",
                    &format!("post_op {op}: {error}"),
                );
            }
        }
    }

    pub fn call(&self, _state: &AppState, op: &str, args: &Value, actor: &str) -> Reply {
        let package = self
            .packages
            .iter()
            .find(|p| p.contributions.ops.iter().any(|o| o.name == op))
            .ok_or("unknown plugin operation")?;
        let name = op
            .strip_prefix(&format!("plugin {} ", package.manifest.id))
            .ok_or("invalid plugin operation")?;
        package
            .service
            .as_ref()
            .ok_or("plugin service unavailable")?
            .request("op", json!({"name": name, "args": args}), actor)
    }

    pub fn stop(&self, _state: &AppState) {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        for package in self.packages.iter().rev() {
            if let Some(service) = &package.service {
                if let Err(error) = service.request("stop", json!({}), &package.manifest.id) {
                    log(&package.manifest.id, "error", &error);
                }
                service.kill();
            }
        }
    }

    pub fn admits_panel(&self, id: &str) -> bool {
        self.packages.iter().any(|p| {
            p.frontend.is_some()
                && id
                    .strip_prefix(&format!("plugin:{}:", p.manifest.id))
                    .is_some_and(valid_id)
        })
    }
}

impl Package {
    fn prepare(
        &mut self,
        home: &Path,
        python: &Path,
        sdk: &Path,
        state: &AppState,
    ) -> Result<(), String> {
        let data = home.join("plugin-data").join(&self.manifest.id);
        let cache = std::path::absolute(goofi_build::base_dir(home))
            .map_err(|e| e.to_string())?
            .join("plugins")
            .join(&self.manifest.id);
        std::fs::create_dir_all(&data)
            .and_then(|()| std::fs::create_dir_all(&cache))
            .map_err(|e| e.to_string())?;
        if !state.mode.headless && self.root.join("frontend/package.json").is_file() {
            let sources = read_source(&self.root)?;
            let key = source_key(&sources);
            let build = cache.join("frontend").join(&key);
            if !build.join("index.js").is_file() {
                static BUILD_ID: AtomicU64 = AtomicU64::new(0);
                let work = cache.join(format!(
                    "work-{}-{}",
                    goofi_core::session::tag(),
                    BUILD_ID.fetch_add(1, Ordering::Relaxed)
                ));
                let prepared = || -> Result<(), String> {
                    for (relative, bytes) in &sources {
                        let destination = work.join(relative);
                        if let Some(parent) = destination.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                        }
                        std::fs::write(destination, bytes).map_err(|e| e.to_string())?;
                    }
                    let frontend = work.join("frontend");
                    let out = work.join("output");
                    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
                    run(Command::new(npm).args(["ci"]).current_dir(&frontend))?;
                    let frontend_sdk = frontend.join("node_modules/@goofi/plugin");
                    std::fs::create_dir_all(&frontend_sdk).map_err(|e| e.to_string())?;
                    std::fs::write(frontend_sdk.join("index.ts"), FRONTEND_SDK)
                        .map_err(|e| e.to_string())?;
                    std::fs::write(frontend_sdk.join("package.json"), FRONTEND_PACKAGE)
                        .map_err(|e| e.to_string())?;
                    run(Command::new(npm)
                        .args(["run", "build"])
                        .env("GOOFI_PLUGIN_OUT_DIR", &out)
                        .current_dir(&frontend))?;
                    if !out.join("index.js").is_file() {
                        return Err(
                            "frontend build must write index.js to GOOFI_PLUGIN_OUT_DIR".into()
                        );
                    }
                    std::fs::create_dir_all(cache.join("frontend")).map_err(|e| e.to_string())?;
                    if let Err(error) = std::fs::rename(&out, &build) {
                        if !build.join("index.js").is_file() {
                            return Err(error.to_string());
                        }
                    }
                    Ok(())
                }();
                let _ = std::fs::remove_dir_all(&work);
                prepared?;
            }
            self.frontend = Some(build);
        }
        if !self.root.join("backend/plugin.py").is_file() {
            return Ok(());
        }
        let interpreter = if self.root.join("pyproject.toml").is_file() {
            let project =
                std::fs::read(self.root.join("pyproject.toml")).map_err(|e| e.to_string())?;
            let lock = std::fs::read(self.root.join("uv.lock"))
                .map_err(|e| format!("backend dependencies need uv.lock: {e}"))?;
            let identity = format!(
                "{}:{}:{}",
                python.display(),
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            let env = cache.join("venvs").join(goofi_build::digest([
                project.as_slice(),
                lock.as_slice(),
                identity.as_bytes(),
            ]));
            run(Command::new("uv")
                .args(["sync", "--locked", "--no-install-project", "--python"])
                .arg(python)
                .env("UV_PROJECT_ENVIRONMENT", &env)
                .current_dir(&self.root))?;
            env.join(if cfg!(windows) {
                "Scripts/python.exe"
            } else {
                "bin/python"
            })
        } else {
            python.to_path_buf()
        };
        let config = json!({"id": self.manifest.id, "package_dir": self.root, "data_dir": data, "cache_dir": cache});
        let (service, mut contributions) = Service::spawn(&interpreter, sdk, config)?;
        for operation in &mut contributions.ops {
            if !operation.name.split(' ').all(valid_id)
                || !["read", "effect"].contains(&operation.kind.as_str())
            {
                return Err("invalid plugin operation declaration".into());
            }
            operation.name = format!("plugin {} {}", self.manifest.id, operation.name);
        }
        let names: Vec<_> = state
            .ops()
            .iter()
            .map(|op| op.name.to_string())
            .chain(contributions.ops.iter().map(|op| op.name.clone()))
            .collect();
        for (i, name) in names.iter().enumerate() {
            for other in names.iter().skip(i + 1) {
                if name == other
                    || name.starts_with(&format!("{other} "))
                    || other.starts_with(&format!("{name} "))
                {
                    return Err(format!("operation names conflict: `{name}` and `{other}`"));
                }
            }
        }
        self.contributions = contributions;
        self.service = Some(service);
        Ok(())
    }
}

pub(crate) fn list(state: &AppState, _: &Value, _: &str, _: &mut Vec<String>) -> Reply {
    let packages: Vec<_> = state.plugins.packages.iter().map(|p| {
        let dead = p.service.as_ref().is_some_and(|s| s.child.lock().unwrap_or_else(std::sync::PoisonError::into_inner).try_wait().ok().flatten().is_some());
        json!({"id": p.manifest.id, "version": p.manifest.version, "error": p.error.as_deref().or(dead.then_some("plugin service stopped")),
            "frontend": p.frontend.as_ref().filter(|_| !dead).map(|_| format!("/plugins/{}/index.js", p.manifest.id))})
    }).collect();
    Ok(json!({"plugins": packages}))
}

pub(crate) async fn asset(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path((id, file)): axum::extract::Path<(String, String)>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let serve = || -> Result<_, String> {
        let root = state
            .plugins
            .packages
            .iter()
            .find(|p| p.manifest.id == id)
            .and_then(|p| p.frontend.as_ref())
            .ok_or("unknown plugin frontend")?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let path = root.join(&file).canonicalize().map_err(|e| e.to_string())?;
        if !path.starts_with(&root) || !path.is_file() {
            return Err("invalid asset path".into());
        }
        let mime = crate::content_type(&file);
        Ok((mime, std::fs::read(path).map_err(|e| e.to_string())?))
    };
    match serve() {
        Ok((mime, body)) => (
            [
                ("content-type", mime),
                ("cache-control", "no-cache"),
                ("x-content-type-options", "nosniff"),
            ],
            body,
        )
            .into_response(),
        Err(_) => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

type SourceFiles = Vec<(PathBuf, Vec<u8>)>;

fn source_key(sources: &SourceFiles) -> String {
    let mut parts = vec![format!(
        "{}:{}:{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
    .into_bytes()];
    parts.push(FRONTEND_SDK.as_bytes().to_vec());
    parts.push(FRONTEND_PACKAGE.as_bytes().to_vec());
    for (path, bytes) in sources {
        parts.push(path.to_string_lossy().as_bytes().to_vec());
        parts.push(bytes.len().to_le_bytes().to_vec());
        parts.push(bytes.clone());
    }
    goofi_build::digest(parts.iter().map(Vec::as_slice))
}

fn read_source(root: &Path) -> Result<SourceFiles, String> {
    fn visit(root: &Path, path: &Path, out: &mut SourceFiles) -> Result<(), String> {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if matches!(
                entry.file_name().to_str(),
                Some("node_modules" | ".git" | "dist" | "__pycache__" | ".venv")
            ) {
                continue;
            }
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                visit(root, &path, out)?;
            } else if kind.is_file() {
                out.push((
                    path.strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_path_buf(),
                    std::fs::read(&path).map_err(|e| e.to_string())?,
                ));
            } else {
                return Err(format!(
                    "plugin build sources must be files or directories: {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(root, root, &mut out)?;
    Ok(out)
}
