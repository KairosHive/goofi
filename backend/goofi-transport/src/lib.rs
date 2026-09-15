//! Cross-engine transport: iceoryx2 names, rendezvous and endpoint machinery, one shared
//! mechanism for every engine. A phone book, not a switchboard — the resolver here is pure name
//! and config derivation, and whichever side settles first waits on `open_or_create`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use iceoryx2::config::Config;
use iceoryx2::prelude::*;

use goofi_node::{BoundVar, EventId, GraphView, Uid, Var};

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
    _lease: goofi_core::registry::Lease,
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

/// `EventId(0)` is a control message; `1..=64` an input slot; `65..=128` an `nd()` channel (§3.2).
/// 255 is the ceiling those three ranges are budgeted against.
const EVENT_ID_MAX: usize = 255;
/// Every producer feeding this node needs a notifier, plus the graph. The default 16 busts on a
/// 20-wire multi-input.
const MAX_NOTIFIERS: usize = 256;
/// Fan-out plus the `/data` reducer. The default 8 busts on a 9-consumer slot.
const MAX_SUBSCRIBERS: usize = 256;
/// How many iceoryx2 NODES may open one service. One graph node is one iceoryx2 node, so this is
/// really a per-peer bound, and it binds below both ceilings above.
const MAX_NODES: usize = 256;
/// How many messages a control or status subscriber may hold unread. Unlike the one-deep data
/// services, control and status are message STREAMS: an ack never read parks a wire sequence.
const MESSAGE_BUFFER: usize = 1024;
/// How many readers a control or status service admits — exactly one, by construction. Not
/// cosmetic: iceoryx2 sizes a segment as readers × buffer × slice, so these three numbers MULTIPLY.
const MESSAGE_READERS: usize = 1;
/// The pool a message publisher starts with; `PowerOfTwo` grows the segment for a rare large one.
pub const MESSAGE_SLICE: usize = 1024;
/// The pool a data publisher starts with; `PowerOfTwo` grows it for a larger frame.
pub const INITIAL_SLICE: usize = 64 * 1024;
/// The largest frame a SIGNAL recording service takes: 1 MiB clears a 64-channel, 2500-sample
/// frame with room, and a frame over it is refused rather than grown into.
pub const RECORD_SLICE: usize = 1024 * 1024;
/// The largest frame an AUDIO recording service takes: one block of the widest output, with room.
pub const AUDIO_RECORD_SLICE: usize = 64 * 1024;
/// What one armed slot's segment costs, exactly and for any frame size — the publisher allocates
/// [`RecordShape::buffer`] slices of [`RecordShape::slice`] and never grows.
pub const RECORD_BUDGET: usize = 64 * 1024 * 1024;

/// How one armed slot's segment is cut. The budget is the same whatever the engine; what differs is
/// the frame — a signal frame is large and rare, an audio block is tiny and unceasing, so the same
/// bytes buy 64 signal frames or 1024 audio blocks. The service overflows safely, so a reader
/// slower than that depth loses the OLDEST frame, which the recorder counts by index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecordShape {
    pub slice: usize,
    pub buffer: usize,
}

/// The shape an engine's armed slots publish with and the recorder subscribes with. Both ends
/// derive it from the engine name, so the service they meet on has only one description.
pub fn record_shape(engine: &str) -> RecordShape {
    let slice = match engine {
        "audio" => AUDIO_RECORD_SLICE,
        _ => RECORD_SLICE,
    };
    RecordShape { slice, buffer: RECORD_BUDGET / slice }
}

/// The id every armed slot rings the recorder's door with; the drain ignores it, so one is enough.
pub const RECORD_EVENT_ID: EventId = 0;

/// The name every service of one node is derived from: `<instance>_<uid>_<gen>`. `gen` is bumped on
/// EVERY birth, because teardown never blocks and a rebirth would else race its predecessor.
pub fn service_base(instance: &str, uid: Uid, gen: u64) -> String {
    format!("{instance}_{}_{gen}", uid.to_hex())
}

/// The one event service a node parks on for its whole life (§3.2).
pub fn door_service(base: &str) -> ServiceName {
    format!("goofi_{base}_door")
}

/// Graph → node control messages.
pub fn control_service(base: &str) -> ServiceName {
    format!("goofi_{base}_ctl")
}

/// Node → graph status transitions.
pub fn status_service(base: &str) -> ServiceName {
    format!("goofi_{base}_sts")
}

/// One output slot's data service — the name a consumer is given in its `InSlot` set.
pub fn output_service(base: &str, slot: &str) -> ServiceName {
    format!("goofi_{base}_out_{slot}")
}

/// One output slot's recording service — the recorder's own deep buffer, never the shared wire.
pub fn record_service(base: &str, slot: &str) -> ServiceName {
    format!("goofi_{base}_rec_{slot}")
}

/// The ONE door every armed slot rings once its frame is out. It is the recorder's, not a node's,
/// so a burst across every armed slot coalesces into one sweep of every armed buffer.
pub fn record_door_service(instance: &str) -> ServiceName {
    format!("goofi_{instance}_recdoor")
}

/// A notifier onto one node's door; the ringer knows nothing else about the node it rings.
pub struct Doorbell {
    notifier: iceoryx2::port::notifier::Notifier<Svc>,
    /// The service it was opened for: a reconcile keeps a survivor rather than reopening it, and
    /// a bell counts against `MAX_NOTIFIERS` for as long as it lives.
    service: String,
}

impl Doorbell {
    pub fn names(&self, service: &str) -> bool {
        self.service == service
    }

    /// Open a door by name, on the ringer's OWN iceoryx2 node — one bell per producing NODE, since
    /// each node counts against `max_nodes`. `open_or_create`: the service is the rendezvous.
    pub fn open(node: &IoxNode, service: &str) -> Result<Doorbell, String> {
        let door = event_service(node, service)?;
        let notifier = door.notifier_builder().create().map_err(|e| format!("notifier `{service}`: {e}"))?;
        Ok(Doorbell { notifier, service: service.to_string() })
    }

    /// Ring it. A failed ring costs a wake, never a message: the payload is already in a queue the
    /// node drains.
    pub fn ring(&self, id: EventId) -> Result<(), String> {
        self.notifier
            .notify_with_custom_event_id(iceoryx2::prelude::EventId::new(id as usize))
            .map(|_| ())
            .map_err(|e| format!("notify: {e}"))
    }
}

/// The session this process mints under, decided once: `GOOFI_SESSION` names a parent's to JOIN
/// — a spawned node child, a test's crash helper — and with none set the process holds one of its
/// own. Deciding it also runs the boot pass: every dead session's ephemeral directory and shared
/// memory go, and nothing a living session owns is ever enumerated.
pub fn session() -> &'static str {
    static SESSION: OnceLock<String> = OnceLock::new();
    SESSION.get_or_init(|| {
        set_log_level_from_env_or(LogLevel::Error);
        raise_fd_limit();
        let joined = std::env::var(goofi_core::session::ENV).ok().filter(|id| goofi_core::session::alive(id));
        let id = match joined {
            Some(id) => id,
            None => {
                let held = goofi_core::session::hold(&goofi_core::session::fresh_id()).expect("hold a session");
                let id = held.id().to_string();
                *HELD.lock().unwrap_or_else(|e| e.into_inner()) = Some(held);
                // A process that never calls `release_session` — a test binary, a `process::exit`
                // on a second Ctrl-C — releases at exit, where every thread is past the point of
                // caring. The binary's own release, earlier, makes this a no-op.
                // SAFETY: registers a plain `extern "C"` function with no arguments; the C
                // runtime calls it once, on this process's own exit.
                unsafe {
                    libc::atexit(release_at_exit);
                }
                id
            }
        };
        goofi_core::session::decide(&id);
        let _ = std::fs::create_dir_all(iox_root(&id));
        sweep_dead();
        id
    })
}

/// The session this process holds, if it holds one rather than joining a parent's.
static HELD: Mutex<Option<goofi_core::session::Held>> = Mutex::new(None);

/// A clean shutdown: after every port is gone, the record goes first, then the ephemeral
/// directory, then the shared memory the session's prefix names. The workspace is not touched.
pub fn release_session() {
    if let Some(held) = HELD.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let id = held.id().to_string();
        drop(held);
        remove_tree(&goofi_core::session::system_dir(&id));
        sweep_shared_memory(|owner| owner == id);
    }
}

extern "C" fn release_at_exit() {
    release_session();
}

/// Record where this session serves, for `goofi session list` and the shell.
pub fn record_url(url: &str) {
    if let Some(held) = HELD.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        held.record_url(url);
    }
}

/// The boot pass: dead records, ephemeral directories whose record is dead or gone, empty
/// workspace parents, and every shared-memory segment whose session is not alive — each judged
/// by the lock alone, so a segment whose record was swept long ago still goes.
pub fn sweep_dead() {
    let _ = goofi_core::session::sessions(remove_tree);
    goofi_core::session::sweep_dead_system(remove_tree);
    goofi_core::session::sweep_empty_workspaces();
    let mut known = std::collections::HashMap::new();
    sweep_shared_memory(|id| !*known.entry(id.to_string()).or_insert_with(|| goofi_core::session::alive(id)));
}

/// Every alive session, dead ones swept as they are met.
pub fn sessions() -> Vec<goofi_core::session::Session> {
    goofi_core::session::sessions(remove_tree)
}

/// Where iceoryx2 keeps a session's files: node directories, service configs, monitors.
fn iox_root(id: &str) -> std::path::PathBuf {
    goofi_core::session::system_dir(id).join("iox")
}

/// The name every shared-memory segment of a session carries. Segments live in one platform
/// directory iceoryx2 cannot be pointed away from, so the prefix is what makes them a session's.
/// Short, because it is also part of a unix socket path capped at 108 bytes.
fn shm_prefix(id: &str) -> String {
    format!("g{id}_")
}

/// The directory the iceoryx2 platform layer keeps its shared memory in — a compile-time
/// constant there, restated here because a sweep by prefix has to know where to look.
fn shm_dir() -> std::path::PathBuf {
    if cfg!(windows) {
        std::path::PathBuf::from(r"C:\Temp\iceoryx2\shm")
    } else {
        std::path::PathBuf::from("/dev/shm")
    }
}

/// The session id a segment name carries, when the name is one of ours.
fn shm_owner(name: &str) -> Option<&str> {
    let id = name.strip_prefix('g')?.split_once('_')?.0;
    (id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

/// Remove every segment of ours whose owning session `dead` says so of.
fn sweep_shared_memory(mut dead: impl FnMut(&str) -> bool) {
    let Ok(entries) = std::fs::read_dir(shm_dir()) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if shm_owner(&name.to_string_lossy()).is_some_and(&mut dead) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The iceoryx2 configuration every goofi port is built against: the session's own root and
/// prefix, and iceoryx2's three automatic dead-node passes OFF — the session lock is the one
/// liveness answer, and a pass over a directory only this session writes has nothing to find.
fn iox_config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let id = session();
        let mut config = Config::global_config().clone();
        let root = iox_root(id).to_string_lossy().replace('\\', "/");
        config.global.set_root_path(&iceoryx2::prelude::Path::new(root.as_bytes()).expect("a root path"));
        config.global.prefix = iceoryx2::prelude::FileName::new(shm_prefix(id).as_bytes()).expect("a prefix");
        config.global.service.cleanup_dead_nodes_on_open = false;
        config.global.node.cleanup_dead_nodes_on_creation = false;
        config.global.node.cleanup_dead_nodes_on_destruction = false;
        config
    })
}

/// One iceoryx2 node per port OWNER, never per port: each is a directory under the session's
/// root and each is counted by every service's `max_nodes`.
pub fn iox_node() -> Result<IoxNode, String> {
    let node = NodeBuilder::new().config(iox_config()).create::<Svc>().map_err(|e| format!("iox node: {e}"))?;
    // Named by the thread that opened it, which is what a reader of the inventory can act on.
    let owner = std::thread::current().name().unwrap_or("?").to_string();
    Ok(IoxNode { node, _lease: goofi_core::registry::lease(goofi_core::registry::Kind::Port, owner) })
}

/// Remove a tree this session owns.
#[cfg(not(windows))]
fn remove_tree(path: &std::path::Path) {
    let _ = std::fs::remove_dir_all(path);
}

/// On Windows the files iceoryx2 writes carry a protected DACL their owner cannot unlink through
/// (eclipse-iceoryx/iceoryx2#1869), so each is taken back by name first.
#[cfg(windows)]
fn remove_tree(path: &std::path::Path) {
    take_path(path);
}

/// Take one path's DACL back, then unlink it — contents first, since a directory goes empty only.
#[cfg(windows)]
fn take_path(path: &std::path::Path) {
    let dir = path.is_dir();
    if dir {
        for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
            take_path(&entry.path());
        }
    }
    grant_owner(path);
    let _ = if dir { std::fs::remove_dir(path) } else { std::fs::remove_file(path) };
}

/// Grant OWNER RIGHTS full control on ONE path, explicitly. Naming the file is the whole point: a
/// protected DACL is precisely one that refuses an inherited ace, so a grant on the parent — an
/// `icacls /T` walk, which this replaces — never reaches the file it was meant for.
#[cfg(windows)]
fn grant_owner(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, GENERIC_ALL, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSidToSidW, EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SET_ACCESS,
        SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, NO_INHERITANCE, PROTECTED_DACL_SECURITY_INFORMATION,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // `S-1-3-4` is OWNER RIGHTS: it grants the object's own owner, which is this user, and nobody else.
    let sid_text: Vec<u16> = "S-1-3-4".encode_utf16().chain(std::iter::once(0)).collect();
    let mut sid = std::ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(sid_text.as_ptr(), &mut sid) } == 0 {
        return;
    }
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL,
        grfAccessMode: SET_ACCESS,
        grfInheritance: NO_INHERITANCE,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
            ptstrName: sid.cast(),
        },
    };
    let mut acl: *mut ACL = std::ptr::null_mut();
    if unsafe { SetEntriesInAclW(1, &access, std::ptr::null(), &mut acl) } == ERROR_SUCCESS {
        unsafe {
            SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                acl,
                std::ptr::null(),
            );
        }
    }
    unsafe {
        LocalFree(acl.cast());
        LocalFree(sid.cast());
    }
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

/// The request or response service of one spawned node child: the parent's end and the child's
/// are built through this one function, so the two cannot disagree on a limit. `open_or_create`:
/// whichever side settles first waits for the other.
pub fn subprocess_service(node: &IoxNode, name: &str) -> Result<ByteService, String> {
    node.service_builder(&parse_name(name)?)
        .publish_subscribe::<[u8]>()
        .enable_safe_overflow(true)
        .max_publishers(1)
        .max_subscribers(16)
        .open_or_create()
        .map_err(|e| format!("service `{name}`: {e}"))
}

/// The event service every door is: §3.2's three id ranges against one ceiling, and one listener.
pub fn event_service(node: &IoxNode, name: &str) -> Result<EventService, String> {
    node.service_builder(&parse_name(name)?)
        .event()
        .max_nodes(MAX_NODES)
        .event_id_max_value(EVENT_ID_MAX)
        .max_notifiers(MAX_NOTIFIERS)
        .max_listeners(1)
        .open_or_create()
        .map_err(|e| format!("event service `{name}`: {e}"))
}

/// The control and status services: the same publish/subscribe shape as a data wire, but a message
/// STREAM rather than a latest-wins cell — see [`MESSAGE_BUFFER`].
pub fn message_service(node: &IoxNode, name: &str) -> Result<ByteService, String> {
    node.service_builder(&parse_name(name)?)
        .publish_subscribe::<[u8]>()
        .max_nodes(MAX_NODES)
        .enable_safe_overflow(true)
        .history_size(0)
        .subscriber_max_buffer_size(MESSAGE_BUFFER)
        .max_publishers(1)
        .max_subscribers(MESSAGE_READERS)
        .open_or_create()
        .map_err(|e| format!("message service `{name}`: {e}"))
}

/// The publish/subscribe service every data wire is: one publisher, because a slot has exactly one
/// producer; no history, because a link never replays; a one-deep buffer, which is latest-wins.
pub fn data_service(node: &IoxNode, name: &str) -> Result<ByteService, String> {
    node.service_builder(&parse_name(name)?)
        .publish_subscribe::<[u8]>()
        .max_nodes(MAX_NODES)
        .enable_safe_overflow(true)
        .history_size(0)
        .subscriber_max_buffer_size(1)
        .max_publishers(1)
        .max_subscribers(MAX_SUBSCRIBERS)
        .open_or_create()
        .map_err(|e| format!("data service `{name}`: {e}"))
}

/// A recorder's own service on an output slot: one subscriber, and a buffer deep enough that a
/// journal commit does not cost frames. Depth is a service-level property, so this can never be
/// the shared data service — 256 subscribers times this depth is half a gigabyte a slot.
pub fn record_data_service(node: &IoxNode, name: &str, shape: RecordShape) -> Result<ByteService, String> {
    node.service_builder(&parse_name(name)?)
        .publish_subscribe::<[u8]>()
        .max_nodes(MAX_NODES)
        .enable_safe_overflow(true)
        .history_size(0)
        .subscriber_max_buffer_size(shape.buffer)
        .max_publishers(1)
        .max_subscribers(1)
        .open_or_create()
        .map_err(|e| format!("record service `{name}`: {e}"))
}

/// How many subscribers a data service has right now — whether anyone drinks from it.
pub fn subscribers(service: &ByteService) -> usize {
    service.dynamic_config().number_of_subscribers()
}

/// Open a subscriber on an output slot's data service by name — a `/data` consumer's end of a wire.
pub fn open_output_subscriber(node: &IoxNode, service: &str) -> Result<ByteSubscriber, String> {
    data_service(node, service)?
        .subscriber_builder()
        .create()
        .map_err(|e| format!("subscriber `{service}`: {e}"))
}

/// A producer's end of one armed slot's recording service: a STATIC segment of exactly
/// [`RECORD_BUDGET`], so an outsized frame is refused at the loan instead of resizing it.
///
/// It is opened by the ARMING and released by the READER. Dropping the publisher releases the
/// segment the recorder's queue points into, so a disarm that dropped it destroyed the frames the
/// service had already delivered — a loss no gap and no count could ever show. A disarm therefore
/// only [retires](RecordPort::retire) the port; [`RecordPort::spent`] is what says it may go.
pub struct RecordPort {
    service: ByteService,
    publisher: BytePublisher,
    bell: Doorbell,
    retired: bool,
}

impl RecordPort {
    pub fn open(node: &IoxNode, service: &str, door: &str, what: &str, shape: RecordShape) -> Result<RecordPort, String> {
        let service = record_data_service(node, service, shape)?;
        let publisher = service
            .publisher_builder()
            .initial_max_slice_len(shape.slice)
            .allocation_strategy(AllocationStrategy::Static)
            .create()
            .map_err(|e| format!("record publisher `{what}`: {e}"))?;
        let bell = Doorbell::open(node, door)?;
        Ok(RecordPort { service, publisher, bell, retired: false })
    }

    /// One frame onto the service, and the recorder's door rung. `false` is a refused loan, which
    /// the next frame's own number witnesses. A retired port sends nothing.
    pub fn send(&self, bytes: &[u8]) -> bool {
        !self.retired && publish(&self.publisher, bytes, std::iter::once((&self.bell, RECORD_EVENT_ID)))
    }

    /// Disarmed: stop publishing, but stay open while the recorder still reads.
    pub fn retire(&mut self) {
        self.retired = true;
    }

    pub fn armed(&mut self) {
        self.retired = false;
    }

    pub fn retired(&self) -> bool {
        self.retired
    }

    /// Retired, and nobody drinks from it any more — the one state in which dropping it is safe.
    pub fn spent(&self) -> bool {
        self.retired && subscribers(&self.service) == 0
    }
}

/// Open the recorder's end of an armed output slot's recording service.
pub fn open_record_subscriber(node: &IoxNode, service: &str, shape: RecordShape) -> Result<ByteSubscriber, String> {
    record_data_service(node, service, shape)?
        .subscriber_builder()
        .create()
        .map_err(|e| format!("record subscriber `{service}`: {e}"))
}

/// The stack a thread needs to OPEN an iceoryx2 service: the service's static config is parsed by
/// serde and toml, whose debug-build frames overflow a platform default. `AGENTS.md` says what it
/// cost. It is the main thread's own size, so a node thread is no more constrained than the
/// process around it.
pub const STACK: usize = 8 * 1024 * 1024;

/// A named thread with the stack [`STACK`] states. Every goofi thread that can reach this crate is
/// built here, so the platform default never decides.
pub fn thread(name: impl Into<String>) -> goofi_core::worker::Builder {
    goofi_core::worker::thread(name).stack_size(STACK)
}

/// A publisher that can grow past its initial pool: a GOOF frame is variable-size, and `Static`
/// would refuse the first one larger than `initial` instead of reallocating.
pub fn publisher(service: &ByteService, what: &str, initial: usize) -> Result<BytePublisher, String> {
    service
        .publisher_builder()
        .initial_max_slice_len(initial)
        .allocation_strategy(AllocationStrategy::PowerOfTwo)
        .create()
        .map_err(|e| format!("publisher `{what}`: {e}"))
}

fn parse_name(name: &str) -> Result<iceoryx2::service::service_name::ServiceName, String> {
    name.try_into().map_err(|e| format!("bad service name `{name}`: {e:?}"))
}

/// One node's door, from the view's birth facts.
pub fn door_of(view: &GraphView<'_>, uid: Uid) -> Option<ServiceName> {
    let node = view.nodes.get(&uid)?;
    Some(door_service(&service_base(view.instance, uid, node.generation)))
}

/// One output slot's data service name, from the view's birth facts.
pub fn output_of(view: &GraphView<'_>, uid: Uid, slot: &str) -> Option<ServiceName> {
    let node = view.nodes.get(&uid)?;
    Some(output_service(&service_base(view.instance, uid, node.generation), slot))
}

/// One output slot's recording service name, from the view's birth facts.
pub fn record_of(view: &GraphView<'_>, uid: Uid, slot: &str) -> Option<ServiceName> {
    let node = view.nodes.get(&uid)?;
    Some(record_service(&service_base(view.instance, uid, node.generation), slot))
}

/// A resolved variable as a node receives it: a service rather than a uid, because a node
/// addresses a producer by service and cannot resolve anything for itself.
pub fn var_of(view: &GraphView<'_>, v: &BoundVar) -> (String, Var) {
    match v {
        BoundVar::Stream { var, producer, slot, .. } => {
            let src = output_of(view, *producer, slot)
                .map_or_else(|| Var::Missing(format!("`{var}` names no running node")), Var::Stream);
            (var.clone(), src)
        }
        BoundVar::Value { var, value } => (var.clone(), Var::Value(value.clone())),
        BoundVar::Missing { var, reason } => (var.clone(), Var::Missing(reason.clone())),
    }
}

/// Send one frame, then ring every bell. In that order, always: a consumer woken first drains
/// nothing and parks. `false` says the loan failed — no shared memory, or a frame over a static
/// publisher's slice — which is a caller's to count.
pub fn publish<'a>(publisher: &BytePublisher, bytes: &[u8], bells: impl IntoIterator<Item = (&'a Doorbell, EventId)>) -> bool {
    let Ok(sample) = publisher.loan_slice_uninit(bytes.len()) else { return false };
    let _ = sample.write_from_slice(bytes).send();
    for (bell, id) in bells {
        let _ = bell.ring(id);
    }
    true
}

/// The survivor `keep` names, taken out of what a reconcile held; what is left is what the new
/// set does not name, and dropping it IS the unsubscribe.
pub fn take_where<T>(held: &mut Vec<T>, keep: impl Fn(&T) -> bool) -> Option<T> {
    held.iter().position(keep).map(|i| held.remove(i))
}

/// A CEILING on a teardown, not a join: a wedged node must not be able to wedge the exit. It is
/// the transport's own number because what the wait is FOR is the release of shared memory.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// Wait for every halt to release, up to [`SHUTDOWN_WAIT`]. Whether they all did.
pub fn wait_released<'a>(halts: impl Iterator<Item = &'a Halt> + Clone, ceiling: Duration) -> bool {
    let deadline = Instant::now() + ceiling;
    while halts.clone().any(|h| !h.released()) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    true
}

/// The two flags a node's thread is born holding: told to stop, and — once every port it owned
/// is dropped, which is what releases the shared memory — released. The only thing a teardown
/// can usefully wait for.
#[derive(Default)]
pub struct Halt {
    stop: AtomicBool,
    released: AtomicBool,
}

impl Halt {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    pub fn release(&self) {
        self.released.store(true, Ordering::Release);
    }
    pub fn released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }
}
