//! The transport under the per-node runtime: slots, doors and the wires a desired state declares
//! (spec §3). Every test picks its own [`Uid`] and [`instance`] scopes the names by pid.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

use goofi_core::{Data, Param, SlotType};
use goofi_graph::Uid;
use goofi_node::{DrainWaker, NodeManifest, OutputDecl, ParamDecl, ParamKey, ParamSpec, SlotDecl, Status};
use goofi_runtime::{Cx, Desired, Executor, Handle, Out, Shared, Spawn, Sub, Ticked};
use goofi_supervisor::sync::Mutex;
use goofi_tests::{f32s, frame, WAIT};
use goofi_transport::{door_service, output_service, service_base, Doorbell, Payload};

static INPUTS: &[SlotDecl] = &[SlotDecl {
    name: "input",
    kind: SlotType::Array,
    trigger_process: true,
    multi: true,
    required: false,
}];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];
static PARAMS: &[ParamDecl] = &[ParamDecl {
    group: "control",
    name: "gain",
    spec: ParamSpec::Str { default: "a", options: &["a", "b"], refresh: true },
    expression: None,
    doc: None,
    section: 0,
    show: &[],
    role: None,
}];
static MANIFEST: NodeManifest = NodeManifest {
    type_name: "_TransportTest",
    tags: &[],
    doc: "transport fixture",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

/// This run's name scope.
fn instance() -> String {
    // The process is walled off from the real home before its session is decided.
    goofi_tests::walled_home();
    format!("t{:x}", std::process::id())
}

/// The names of a node born at `uid` — what the other end's desired state names.
fn base_of(uid: Uid) -> String {
    service_base(&instance(), uid, 0)
}

/// What reached one executor, read by the test from outside its thread.
#[derive(Default)]
struct Log {
    arrived: Vec<(usize, usize, Data)>,
    wired: Vec<(usize, Vec<(String, String)>)>,
    values: Vec<Vec<Param>>,
    pulses: Vec<usize>,
}

/// An executor that logs what reaches it and emits, at its next run, the frame it is handed.
struct Probe {
    log: Arc<Mutex<Log>>,
    emit: Arc<Mutex<Option<Data>>>,
}

impl Executor for Probe {
    fn arrive(&mut self, inbox: usize, wire: usize, frame: &Data) -> bool {
        self.log.lock().arrived.push((inbox, wire, frame.clone()));
        false
    }
    fn rewire(&mut self, inbox: usize, wires: &[(String, String)]) {
        self.log.lock().wired.push((inbox, wires.to_vec()));
    }
    fn params_changed(&mut self, values: &[Param]) -> Ticked {
        self.log.lock().values.push(values.to_vec());
        Ticked::default()
    }
    fn pulse(&mut self, param: usize) -> Ticked {
        self.log.lock().pulses.push(param);
        Ticked::default()
    }
    fn refresh(&mut self, _: usize) -> Option<Vec<String>> {
        Some(vec!["a".into(), "b".into()])
    }
    fn run(&mut self, _: &Cx<'_>, publish: &mut dyn FnMut(usize, Out<'_>)) -> Ticked {
        if let Some(frame) = self.emit.lock().take() {
            publish(0, Out::Frame(&frame));
        }
        Ticked::default()
    }
}

/// One runtime with a probe on it: its handle, what the probe saw, and what it emits next.
struct Node {
    handle: Handle,
    log: Arc<Mutex<Log>>,
    emit: Arc<Mutex<Option<Data>>>,
}

/// Every runtime of one test shares a drain, as one engine's do.
struct Fleet {
    shared: Arc<Shared>,
}

impl Fleet {
    fn new() -> Fleet {
        Fleet { shared: Arc::new(Shared::new(Arc::new(DrainWaker::default()))) }
    }

    fn spawn(&self, uid: Uid) -> Node {
        let (log, emit) = (Arc::new(Mutex::new(Log::default())), Arc::new(Mutex::new(None)));
        let spawn = Spawn {
            engine: "signal",
            uid,
            instance: instance(),
            base: base_of(uid),
            manifest: &MANIFEST,
            decls: MANIFEST.params.to_vec(),
            params: Arc::new([AtomicU64::new(0)]),
            time: Arc::new(goofi_supervisor::timing::Time::new()),
        };
        let (probe_log, probe_emit) = (log.clone(), emit.clone());
        let handle = goofi_runtime::spawn(&goofi_tests::hub(), spawn, self.shared.clone(), move || Probe {
            log: probe_log,
            emit: probe_emit,
        })
        .expect("a runtime");
        Node { handle, log, emit }
    }

    /// Every status the runtimes reported so far, by uid.
    fn reports(&self) -> Vec<(Uid, Status)> {
        let mut out = Vec::new();
        self.shared.drain(&mut Vec::new(), &|_, _| true, &mut |uid, status| out.push((uid, status)));
        out
    }
}

/// The desired state of a node that drinks from `producers` on its one inbox, in that order.
fn drinking(producers: &[Uid]) -> Desired {
    let subs = producers
        .iter()
        .enumerate()
        .map(|(wire, p)| Sub::Slot { inbox: 0, wire, service: output_service(&base_of(*p), "out"), source: format!("p{}.out", p.0) })
        .collect();
    Desired { consts: vec![PARAMS[0].spec.to_param()], subs, targets: vec![Vec::new()], record: Vec::new() }
}

/// The desired state of a node whose output rings the doors of `consumers`.
fn ringing(consumers: &[Uid]) -> Desired {
    let targets = consumers.iter().map(|c| (door_service(&base_of(*c)), 1)).collect();
    Desired { consts: vec![PARAMS[0].spec.to_param()], subs: Vec::new(), targets: vec![targets], record: Vec::new() }
}

/// Status and arrivals are asynchronous by design, so a test waits for them with a deadline
/// rather than reading once.
fn until<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "waited {WAIT:?} for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn arrived(node: &Node) -> Vec<(usize, usize, Vec<f32>)> {
    node.log.lock().arrived.iter().map(|(i, w, d)| (*i, *w, f32s(d))).collect()
}

#[test]
fn a_slot_is_latest_wins_and_a_link_never_replays() {
    // One cell per slot: a reader takes the newest payload once, a reader that comes late to a
    // node's slot takes nothing it did not see published, and a held slot hands its newest frame
    // to whoever comes.
    let hub = goofi_tests::hub();
    let name = output_service(&base_of(Uid(1)), "out");
    let publisher = hub.publisher(&name, false);
    let mut early = hub.subscribe(&name);
    assert!(early.receive().is_none(), "nothing published yet");
    publisher.publish(Payload::Data(frame(&[1.0])));
    publisher.publish(Payload::Data(frame(&[2.0])));
    let got = early.receive().expect("the newest frame");
    assert!(matches!(got, Payload::Data(ref d) if f32s(d) == [2.0]), "latest wins, one deep");
    assert!(early.receive().is_none(), "and once");
    let mut late = hub.subscribe(&name);
    assert!(late.receive().is_none(), "a link never replays a previous output");
    assert_eq!(hub.readers(&name), 2, "both readers count as the slot's demand");
    drop(late);
    assert_eq!(hub.readers(&name), 1);

    let held = hub.publisher(&output_service(&base_of(Uid(1)), "held"), true);
    held.publish(Payload::Data(frame(&[3.0])));
    let mut late = hub.subscribe(&output_service(&base_of(Uid(1)), "held"));
    assert!(matches!(late.receive(), Some(Payload::Data(d)) if f32s(&d) == [3.0]), "a held slot replays its newest frame");
}

#[test]
fn a_ring_landing_mid_drain_is_not_lost_and_each_id_wakes_once() {
    // The ring is only a HINT; the truth is in the slots. Several ids can ring between two
    // parks, and each wakes once.
    let hub = goofi_tests::hub();
    let door = hub.door(&door_service(&base_of(Uid(2))));
    let bell = Doorbell::on(hub.door(&door_service(&base_of(Uid(2)))));
    let wait = |d: &goofi_transport::Door| d.wait_until(Instant::now() + WAIT).ids().collect::<Vec<_>>();
    bell.ring(1);
    assert_eq!(wait(&door), vec![1]);
    bell.ring(2); // lands while "draining"
    assert_eq!(wait(&door), vec![2], "retained across the re-park");
    // A wait already past looks once and returns: nothing rang, nothing is taken.
    assert!(door.wait_until(Instant::now()).is_empty());
    bell.ring(2);
    assert_eq!(wait(&door), vec![2], "the door is still parked on after the short wait");
    bell.ring(0);
    bell.ring(3);
    bell.ring(3);
    assert_eq!(wait(&door), vec![0, 3], "each id wakes once, however often it rang");

    // A deadline is kept to the millisecond, not rounded to a kernel tick.
    let t = Instant::now();
    assert!(door.wait_until(t + Duration::from_millis(2)).is_empty());
    let waited = t.elapsed();
    assert!(waited >= Duration::from_millis(2) && waited < Duration::from_millis(6), "a 2 ms wait took {waited:?}");
}

#[test]
fn a_desired_state_reaches_the_executor_and_a_request_is_answered() {
    // The whole state crosses at once, a re-send that moved nothing says nothing, and a request
    // — a refresh, a pulse — is answered on the node's own thread.
    let fleet = Fleet::new();
    let node = fleet.spawn(Uid(4));
    let key = ParamKey::new("control", "gain");
    let desired = Desired {
        consts: vec![Param::Str { value: "b".into(), options: Some(vec!["a".into(), "b".into()]), refresh: true }],
        subs: Vec::new(),
        targets: vec![Vec::new()],
        record: Vec::new(),
    };
    assert!(node.handle.send_if_changed(desired.clone()));
    until("the values to reach the executor", || {
        node.log.lock().values.last().filter(|v| v[0].as_str() == Some("b")).map(|_| ())
    });
    assert!(!node.handle.send_if_changed(desired), "what was last sent is not sent again");
    until("the node to report itself ready", || {
        fleet.reports().iter().any(|(u, s)| *u == Uid(4) && *s == Status::Stage { stage: goofi_node::NodeStage::Ready }).then_some(())
    });

    node.handle.request(goofi_node::Request { kind: goofi_node::RequestKind::Refresh, key: key.clone() });
    let options = until("the refresh to answer", || {
        fleet.reports().into_iter().find_map(|(_, s)| match s {
            Status::RefreshOptions { options, .. } => Some(options),
            _ => None,
        })
    });
    assert_eq!(options, Some(vec!["a".to_string(), "b".to_string()]));
    node.handle.request(goofi_node::Request { kind: goofi_node::RequestKind::Pulse, key });
    until("the pulse to fire", || (node.log.lock().pulses == [0]).then_some(()));
}

#[test]
fn a_frame_reaches_a_wired_consumer_and_a_new_bell_rings_once() {
    // A wire is two declarations and nothing else: neither end knows the other's uid. The
    // consumer subscribes at ITS settle and the producer rings at its own, so a frame published
    // between the two lies on the wire unannounced — until the bell just hung rings once.
    let fleet = Fleet::new();
    let (producer, consumer) = (fleet.spawn(Uid(5)), fleet.spawn(Uid(6)));
    consumer.handle.send_if_changed(drinking(&[Uid(5)]));
    until("the consumer to subscribe", || consumer.log.lock().wired.last().filter(|(_, w)| w.len() == 1).map(|_| ()));
    producer.handle.send_if_changed(ringing(&[]));
    *producer.emit.lock() = Some(frame(&[1.0, 2.0, 3.0]));
    until("the producer to emit", || producer.emit.lock().is_none().then_some(()));
    producer.handle.send_if_changed(ringing(&[Uid(6)]));
    until("the frame to reach the consumer", || (!arrived(&consumer).is_empty()).then_some(()));
    assert_eq!(arrived(&consumer), vec![(0, 0, vec![1.0, 2.0, 3.0])], "inbox, wire, and the frame");

    // A frame of any size crosses as the `Arc` it is.
    let big: Vec<f32> = (0..80_000).map(|i| i as f32).collect();
    *producer.emit.lock() = Some(frame(&big));
    until("the large frame to land", || (arrived(&consumer).len() == 2).then_some(()));
    assert_eq!(arrived(&consumer)[1].2, big);

    // A wire whose producer puts bytes that are not a frame on it is that wire's error, worn by
    // the consumer as its fault rather than never heard of.
    let hub = goofi_tests::hub();
    let raw = hub.publisher(&output_service(&base_of(Uid(50)), "out"), false);
    let bell = Doorbell::on(hub.door(&door_service(&base_of(Uid(6)))));
    consumer.handle.send_if_changed(drinking(&[Uid(50)]));
    until("the consumer to re-subscribe", || consumer.log.lock().wired.last().filter(|(_, w)| w[0].0.contains(&format!("_{}_", Uid(50).to_hex()))).map(|_| ()));
    raw.publish(Payload::Wire(bytes::Bytes::from_static(b"NOPE")));
    bell.ring(1);
    let why = until("the bad wire to be worn as a fault", || {
        fleet.reports().into_iter().find_map(|(u, s)| match s {
            Status::Fault { fault: Some(goofi_node::NodeFault::Process { msg, .. }) } if u == Uid(6) => Some(msg),
            _ => None,
        })
    });
    assert!(why.contains("input 0 does not decode") && why.contains("too small"), "the fault says what was wrong: {why}");
}

#[test]
fn a_re_sent_wire_set_keeps_what_it_names_and_drops_what_it_omits() {
    // The wire set is DECLARATIVE, and the FULL set is re-sent on every change: a surviving wire
    // is kept rather than rebuilt, and what the set no longer names delivers nothing.
    let fleet = Fleet::new();
    let (held, added, consumer) = (fleet.spawn(Uid(9)), fleet.spawn(Uid(11)), fleet.spawn(Uid(10)));
    held.handle.send_if_changed(ringing(&[Uid(10)]));
    added.handle.send_if_changed(ringing(&[Uid(10)]));
    consumer.handle.send_if_changed(drinking(&[Uid(9)]));
    until("the consumer to subscribe", || consumer.log.lock().wired.last().filter(|(_, w)| w.len() == 1).map(|_| ()));
    *held.emit.lock() = Some(frame(&[1.0]));
    until("the first frame", || (arrived(&consumer).len() == 1).then_some(()));

    consumer.handle.send_if_changed(drinking(&[Uid(9), Uid(11)]));
    until("the set to grow", || consumer.log.lock().wired.last().filter(|(_, w)| w.len() == 2).map(|_| ()));
    *added.emit.lock() = Some(frame(&[2.0]));
    until("the added wire's frame", || (arrived(&consumer).len() == 2).then_some(()));
    assert_eq!(arrived(&consumer), vec![(0, 0, vec![1.0]), (0, 1, vec![2.0])], "the first wire kept its index");

    consumer.handle.send_if_changed(drinking(&[Uid(11)]));
    until("the set to shrink", || consumer.log.lock().wired.last().filter(|(_, w)| w.len() == 1).map(|_| ()));
    *held.emit.lock() = Some(frame(&[3.0]));
    until("the dropped producer to emit", || held.emit.lock().is_none().then_some(()));
    *added.emit.lock() = Some(frame(&[4.0]));
    until("the kept wire's frame", || (arrived(&consumer).len() == 3).then_some(()));
    assert_eq!(arrived(&consumer)[2], (0, 0, vec![4.0]), "the dropped wire delivered nothing, and the kept one moved to index 0");
}

#[test]
fn a_slot_feeds_any_number_of_consumers() {
    const CONSUMERS: u64 = 24;
    let fleet = Fleet::new();
    let producer = fleet.spawn(Uid(20));
    let consumers: Vec<Node> = (0..CONSUMERS).map(|i| fleet.spawn(Uid(100 + i))).collect();
    for c in &consumers {
        c.handle.send_if_changed(drinking(&[Uid(20)]));
    }
    until("every consumer to subscribe", || consumers.iter().all(|c| !c.log.lock().wired.is_empty()).then_some(()));
    producer.handle.send_if_changed(ringing(&(0..CONSUMERS).map(|i| Uid(100 + i)).collect::<Vec<_>>()));
    *producer.emit.lock() = Some(frame(&[7.0]));
    for (i, consumer) in consumers.iter().enumerate() {
        until(&format!("consumer {i} of {CONSUMERS} to get the frame"), || (arrived(consumer) == vec![(0, 0, vec![7.0])]).then_some(()));
    }
}

#[test]
fn a_multi_input_keeps_one_cell_per_wire_in_the_order_it_was_given() {
    // A multi slot's cells are keyed by service name and ordered by the SET, never by the producers.
    const WIRES: u64 = 40;
    let fleet = Fleet::new();
    let producers: Vec<Node> = (0..WIRES).map(|i| fleet.spawn(Uid(200 + i))).collect();
    let consumer = fleet.spawn(Uid(21));
    // Reversed, so a wire index that follows the producers rather than the set is visible.
    let set: Vec<Uid> = (0..WIRES).rev().map(|i| Uid(200 + i)).collect();
    consumer.handle.send_if_changed(drinking(&set));
    until("the consumer to subscribe", || consumer.log.lock().wired.last().filter(|(_, w)| w.len() == WIRES as usize).map(|_| ()));
    for (i, producer) in producers.iter().enumerate() {
        producer.handle.send_if_changed(ringing(&[Uid(21)]));
        *producer.emit.lock() = Some(frame(&[i as f32]));
    }
    until("every wire's frame", || (arrived(&consumer).len() == WIRES as usize).then_some(()));
    let mut got = arrived(&consumer);
    got.sort_by_key(|(_, wire, _)| *wire);
    assert_eq!(got.iter().map(|(_, w, _)| *w).collect::<Vec<_>>(), (0..WIRES as usize).collect::<Vec<_>>(), "one cell per wire, none merged");
    assert_eq!(
        got.iter().map(|(_, _, v)| v[0]).collect::<Vec<f32>>(),
        (0..WIRES).rev().map(|i| i as f32).collect::<Vec<f32>>(),
        "each cell holds its own producer's frame, at the set's position"
    );
}

/// The env var that turns this binary into the child below. Its value is irrelevant.
const CRASH_HELPER: &str = "GOOFI_TRANSPORT_CRASH_HELPER";

/// The child: hold a session of its own, open an exchange in it, NAME the session, then wait to
/// be killed, or for its stdin to close.
#[test]
fn crash_helper() {
    if std::env::var(CRASH_HELPER).is_err() {
        return; // the ordinary run: this test is only the child's entry point
    }
    let session = goofi_supervisor::session::Session::hold().expect("a session");
    let hub = goofi_transport::Hub::new(session.id());
    let _exchange = goofi_transport::Exchange::open(&hub).expect("an exchange, with its shared memory");
    println!("READY {}", session.id());
    let _ = std::io::stdin().read_line(&mut String::new());
}

/// The env var that turns this binary into the echo child below. Its value is irrelevant.
const ECHO_HELPER: &str = "GOOFI_TRANSPORT_ECHO_HELPER";

/// The child: serve the exchange the environment names, answering each request with its own
/// bytes, until an empty request.
#[test]
fn echo_helper() {
    if std::env::var(ECHO_HELPER).is_err() {
        return; // the ordinary run: this test is only the child's entry point
    }
    let mut served = goofi_transport::Served::open_from_env().expect("the exchange the parent opened");
    loop {
        let Some((seq, request)) = served.wait(std::time::Duration::from_secs(30)).expect("a request") else {
            panic!("no request within the wait");
        };
        served.answer(seq, &request).expect("an answer");
        if request.is_empty() {
            return;
        }
    }
}

/// A message larger than the mailbox's segment moves the mailbox to a larger one, in either
/// direction, and the other end follows without losing the message or any that comes after.
#[test]
fn a_message_past_the_segment_moves_the_mailbox_and_the_reader_follows() {
    let hub = goofi_tests::hub();
    let mut exchange = goofi_transport::Exchange::open(&hub).expect("an exchange");
    let mut cmd = std::process::Command::new(std::env::current_exe().expect("the test binary"));
    cmd.args([&format!("{}::echo_helper", crate::situation(module_path!())), "--exact", "--nocapture"])
        .env(ECHO_HELPER, "1")
        .env(goofi_transport::ENV, exchange.env());
    let mut child = goofi_supervisor::child::run("echo helper", &mut cmd).spawn().expect("spawn the child");

    // Small, then past the first segment twice over, then small again: each generation's first
    // message, and the one after it, come through in both directions.
    let large: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let larger: Vec<u8> = (0..600_000u32).map(|i| (i % 241) as u8).collect();
    for request in [&b"hello"[..], &large, &b"again"[..], &larger, &b"last"[..]] {
        let answer = exchange.ask(&mut child, &[request]).expect("an answer");
        assert_eq!(answer.len(), request.len(), "the echo's length");
        assert!(answer == request, "the echo's bytes");
    }
    let bye = exchange.ask(&mut child, &[&[][..]]).expect("the parting answer");
    assert!(bye.is_empty());
    assert!(child.wait_within(goofi_tests::WAIT).is_some(), "the child left on the empty request");
}

/// A session owns one ephemeral directory, a workspace and its cache parts; the lock alone
/// decides what a boot sweep removes, and a content key is never mistaken for a session.
#[test]
fn a_session_owns_its_directory_workspace_and_cache_parts() {
    use goofi_supervisor::session::{alive, sessions, sweep_runtime, system_dir, workspace_dir, Record, Session};
    use std::fs;
    goofi_tests::walled_home();
    let _sole = goofi_tests::sole_session();
    let held = Session::hold().unwrap();
    let id = held.id().to_string();
    held.record_url("http://127.0.0.1:9999");
    assert!(alive(&id), "held from within the same process still reads alive");
    assert!(sessions().contains(&Record { id: id.clone(), url: "http://127.0.0.1:9999".into() }));
    assert!(system_dir(&id).is_dir());

    // A dead session: its lock file exists beside its directory and nobody holds it. An orphan
    // directory with no lock at all. A lock a crashed hold left behind with no directory yet.
    let base = goofi_supervisor::session::system_base();
    fs::create_dir_all(system_dir("gone")).unwrap();
    fs::File::create(base.join("gone.alive")).unwrap();
    fs::create_dir_all(system_dir("orphan").join("scratch")).unwrap();
    fs::File::create(base.join("stale.alive")).unwrap();
    assert!(!alive("gone"));
    assert!(sessions().iter().all(|s| s.id != "gone"), "the dead one is not listed");
    assert!(system_dir("gone").exists(), "…and a list removes nothing");
    goofi_supervisor::session::sweep_dead();
    assert!(!system_dir("gone").exists() && !base.join("gone.alive").exists(), "the sweep removes it and its lock");
    assert!(!system_dir("orphan").exists(), "the orphan goes");
    assert!(!base.join("stale.alive").exists(), "a crashed hold's lock is swept");
    assert!(system_dir(&id).is_dir() && alive(&id), "the live one is untouched");

    // The workspace parent: the session's to remove once its last mount is gone, at its release.
    fs::create_dir_all(workspace_dir(&id)).unwrap();

    // The runtime: a dead session's part and work dir go, another version's tree goes; a live
    // session's part, this version's tree, the shared dirs and a 16-hex CONTENT key stay.
    let live = Session::hold().unwrap();
    let runtime = goofi_supervisor::layout::runtime();
    let out = runtime.build().join("out").join("k");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join(format!(".node.so.s{}", live.id())), b"").unwrap();
    fs::write(out.join(".node.so.sfedcba9876543210"), b"").unwrap();
    fs::write(out.join("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef.json"), b"").unwrap();
    let work = runtime.build().join("plugins").join("x").join("work-sfedcba9876543210-0");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(runtime.shipped().join("fedcba9876543210")).unwrap();
    fs::create_dir_all(runtime.root().join("0.0.1")).unwrap();
    fs::create_dir_all(runtime.cache()).unwrap();
    fs::create_dir_all(runtime.state()).unwrap();
    sweep_runtime();
    assert!(out.join(format!(".node.so.s{}", live.id())).exists(), "a live session's part stays");
    assert!(!out.join(".node.so.sfedcba9876543210").exists() && !work.exists(), "a dead session's go");
    assert!(runtime.shipped().join("fedcba9876543210").exists(), "a content key stays");
    assert!(out.join("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef.json").exists());
    assert!(!runtime.root().join("0.0.1").exists(), "another version's tree goes");
    assert!(runtime.versioned().is_dir() && runtime.cache().is_dir() && runtime.state().is_dir(), "this version's and the shared ones stay");
    let _ = fs::remove_dir_all(runtime.shipped().join("fedcba9876543210"));
    drop(live);

    drop(held);
    assert!(!alive(&id));
    assert!(!system_dir(&id).exists(), "its directory went with its release");
    assert!(!workspace_dir(&id).exists(), "the empty workspace parent went with it");
}

#[test]
fn what_a_crash_left_behind_is_gone_by_the_next_start() {
    goofi_tests::walled_home();
    let _sole = goofi_tests::sole_session();
    // A killed process drops NOTHING: its record, its ephemeral directory and the shared memory
    // of its exchanges stay. Its lock does not — the OS releases it — and that is the one thing
    // the sweep reads. Under ANOTHER home: the sweep reads the lock on the machine-wide base, so
    // a boot under a foreign `GOOFI_HOME` — a test run, a warm build — never takes a live session
    // for dead.
    let foreign = std::env::temp_dir().join(format!("goofi-foreign-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&foreign);
    let mut child = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([&format!("{}::crash_helper", crate::situation(module_path!())), "--exact", "--nocapture"])
        .env(CRASH_HELPER, "1")
        .env("GOOFI_HOME", &foreign)
        // Its OWN session, not this process's.
        .env_remove(goofi_supervisor::session::ENV)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        // SIGKILLed mid-test, so its parting "broken pipe" would otherwise read as this test's failure.
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the child");

    let mut out = std::io::BufReader::new(child.stdout.take().expect("the child's stdout"));
    let mut line = String::new();
    while !line.contains("READY") {
        line.clear();
        if std::io::BufRead::read_line(&mut out, &mut line).expect("read the child") == 0 {
            break;
        }
    }
    let id = line.split_whitespace().nth(1).map(str::to_string).unwrap_or_default();
    assert!(!id.is_empty(), "the child named its session: {line:?}");
    let system = goofi_supervisor::session::system_dir(&id);
    assert!(goofi_supervisor::session::alive(&id), "the child holds its session while it lives");
    assert!(system.is_dir(), "its ephemeral directory stands");
    assert!(goofi_supervisor::session::sessions().iter().any(|s| s.id == id), "listed from any home");
    let workspace = goofi_supervisor::session::workspace_dir(&id).join("nonce");
    std::fs::create_dir_all(&workspace).unwrap();
    goofi_supervisor::session::sweep_dead();
    goofi_bridge::autosave::sweep_dead();
    assert!(system.is_dir() && workspace.is_dir(), "a sweep from this home left the live session alone");

    // Killed, because the point is a process that drops nothing: a graceful exit would clean up.
    let _ = child.kill();
    let _ = child.wait();
    assert!(!goofi_supervisor::session::alive(&id), "the lock went with the process");
    assert!(system.exists(), "…and everything else stayed");

    let segments = || -> usize {
        std::fs::read_dir("/dev/shm")
            .map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&format!("g{id}_"))).count())
            .unwrap_or(0)
    };
    if cfg!(target_os = "linux") {
        assert!(segments() > 0, "the child's exchange segments stayed too");
    }

    goofi_supervisor::session::sweep_dead();
    goofi_bridge::autosave::sweep_dead();
    assert!(!system.exists(), "the ephemeral directory was swept");
    assert!(!workspace.exists(), "the workspace was swept");
    assert_eq!(segments(), 0, "the shared memory its prefix names was swept");
    assert!(goofi_supervisor::session::sessions().iter().all(|s| s.id != id));
    let _ = std::fs::remove_dir_all(&foreign);
}
