//! Cross-engine transport: iceoryx2 names, rendezvous and endpoint machinery, one shared
//! mechanism for every engine. A phone book, not a switchboard — the resolver here is pure name
//! and config derivation, and whichever side settles first waits on `open_or_create`.


use iceoryx2::config::Config;
use iceoryx2::prelude::*;


pub mod exchange;
pub mod names;
pub mod services;

pub use exchange::*;
pub use names::*;
pub use services::*;

/// An iceoryx2 service name — a wire's identity, which is why slot messages carry no source uid.
pub type ServiceName = String;

/// The service variant every goofi port uses. `ipc_threadsafe` (rather than `ipc`) is what makes
/// the ports `Send + Sync`, which an engine's transport must be.
type Svc = ipc_threadsafe::Service;
/// The iceoryx2 node every port of one owner is built from. It must outlive them, and it is what
/// `max_nodes` counts on each service — so owners share one rather than minting one per port.
/// An iceoryx2 node under the session's root and prefix, entered in the process's resource index
/// for as long as it lives. Declare it AFTER the ports it minted, so they are dropped first.
pub struct IoxNode {
    node: iceoryx2::node::Node<Svc>,
    _lease: goofi_supervisor::scope::Lease,
}

impl std::ops::Deref for IoxNode {
    type Target = iceoryx2::node::Node<Svc>;
    fn deref(&self) -> &Self::Target {
        &self.node
    }
}
pub type BytePublisher = iceoryx2::port::publisher::Publisher<Svc, [u8], ()>;
pub type ByteSubscriber = iceoryx2::port::subscriber::Subscriber<Svc, [u8], ()>;
pub type ByteService = iceoryx2::service::port_factory::publish_subscribe::PortFactory<Svc, [u8], ()>;
pub type EventService = iceoryx2::service::port_factory::event::PortFactory<Svc>;
pub type Listener = iceoryx2::port::listener::Listener<Svc>;
/// The id a listener is handed per wake, as iceoryx2 spells it.
pub type WakeId = iceoryx2::prelude::EventId;

/// Park on `listener` for at most `within`, at least 1 µs: iceoryx2 reads a zero timeval as NO
/// timeout, so a shorter wait would park until something rings, which a source never gets.
pub fn wait_within(listener: &Listener, within: std::time::Duration, f: impl FnMut(WakeId)) {
    let _ = listener.timed_wait_all(f, within.max(std::time::Duration::from_micros(1)));
}

/// Every port of one session is built against this: its iceoryx2 root and prefix, and
/// iceoryx2's three automatic dead-node passes OFF — the session lock is the one liveness
/// answer, and a pass over a directory only this session writes has nothing to find.
pub struct Iox {
    id: String,
    config: Config,
}

impl Iox {
    pub fn new(session: &goofi_supervisor::session::Session) -> Result<Iox, String> {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            set_log_level_from_env_or(LogLevel::Error);
            raise_fd_limit();
        });
        let id = session.id().to_string();
        let mut config = Config::global_config().clone();
        let root = iox_root(&id).to_string_lossy().replace('\\', "/");
        let path = iceoryx2::prelude::Path::new(root.as_bytes()).map_err(|e| format!("iceoryx2 refuses the root {root}: {e:?}"))?;
        config.global.set_root_path(&path);
        let prefix = goofi_supervisor::session::shm_prefix(&id);
        config.global.prefix = iceoryx2::prelude::FileName::new(prefix.as_bytes()).map_err(|e| format!("iceoryx2 refuses the prefix {prefix}: {e:?}"))?;
        config.global.service.cleanup_dead_nodes_on_open = false;
        config.global.node.cleanup_dead_nodes_on_creation = false;
        config.global.node.cleanup_dead_nodes_on_destruction = false;
        let _ = std::fs::create_dir_all(iox_root(&id));
        Ok(Iox { id, config })
    }

    /// A child's: the session its parent named in `GOOFI_SESSION`.
    pub fn from_env() -> Result<Iox, String> {
        Iox::new(&goofi_supervisor::session::Session::join_from_env()?)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// One iceoryx2 node per port OWNER, never per port: each is a directory under the session's
    /// root and each is counted by every service's `max_nodes`.
    pub fn node(&self) -> Result<IoxNode, String> {
        let node = NodeBuilder::new().config(&self.config).create::<Svc>().map_err(|e| format!("iox node: {e}"))?;
        // Named by the thread that opened it, which is what a reader of the inventory can act on.
        let owner = std::thread::current().name().unwrap_or("?").to_string();
        Ok(IoxNode { node, _lease: goofi_supervisor::scope::lease(goofi_supervisor::scope::Kind::Port, owner) })
    }
}

/// A path for a file needed for a moment — a `.gfi` packed or uploaded — under the session's
/// ephemeral directory, so a crash's leftover is swept with the session.
pub fn scratch(session: &str, name: &str) -> Result<std::path::PathBuf, String> {
    let dir = goofi_supervisor::session::system_dir(session).join("scratch");
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir.join(name))
}

/// Where iceoryx2 keeps a session's files: node directories, service configs, monitors.
fn iox_root(id: &str) -> std::path::PathBuf {
    goofi_supervisor::session::system_dir(id).join("iox")
}

/// Raise the soft descriptor limit toward the hard one. A node costs about 45 descriptors, so the
/// usual 1024 soft limit is a ceiling of twenty nodes, and it lands on whatever the user does
/// next — which was a SAVE. Best effort, and capped rather than taken to the hard limit, because
/// macOS refuses the infinite one it often reports there.
#[cfg(unix)]
fn raise_fd_limit() {
    const WANTED: libc::rlim_t = 65536;
    let mut lim = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) } != 0 {
        return;
    }
    let want = WANTED.min(lim.rlim_max);
    if want > lim.rlim_cur {
        lim.rlim_cur = want;
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lim) };
    }
}

/// Windows bounds the CRT's handles rather than a per-process descriptor rlimit, and its ceiling
/// is already far above what a patch opens.
#[cfg(not(unix))]
fn raise_fd_limit() {}

/// Ports and the node they were built from, dropped in that order: a node dropped before its
/// ports cannot remove its own directory. The ports are reached through `Deref`.
pub struct PortBundle<P> {
    ports: P,
    node: IoxNode,
}

impl<P> PortBundle<P> {
    /// Build the ports on a fresh node of this session.
    pub fn open(iox: &Iox, build: impl FnOnce(&IoxNode) -> Result<P, String>) -> Result<PortBundle<P>, String> {
        PortBundle::on(iox.node()?, build)
    }

    /// Build the ports on `node`, which the bundle then owns.
    pub fn on(node: IoxNode, build: impl FnOnce(&IoxNode) -> Result<P, String>) -> Result<PortBundle<P>, String> {
        let ports = build(&node)?;
        Ok(PortBundle { ports, node })
    }

    pub fn node(&self) -> &IoxNode {
        &self.node
    }
}

impl<P> std::ops::Deref for PortBundle<P> {
    type Target = P;
    fn deref(&self) -> &P {
        &self.ports
    }
}

impl<P> std::ops::DerefMut for PortBundle<P> {
    fn deref_mut(&mut self) -> &mut P {
        &mut self.ports
    }
}

/// The stack a thread needs to OPEN an iceoryx2 service: the service's static config is parsed by
/// serde and toml, whose debug-build frames overflow a platform default. `AGENTS.md` says what it
/// cost. It is the main thread's own size, so a node thread is no more constrained than the
/// process around it.
pub const STACK: usize = 8 * 1024 * 1024;

/// A named thread with the stack [`STACK`] states. Every goofi thread that can reach this crate is
/// built here, so the platform default never decides.
pub fn thread(name: impl Into<String>) -> goofi_supervisor::worker::Builder {
    goofi_supervisor::worker::thread(name).stack_size(STACK)
}
