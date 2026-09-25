//! The iceoryx2 transport, against real shared memory (spec §3).
//!
//! Every test picks its own [`Uid`] and [`instance`] scopes the target by pid: a service name is
//! variable to the MACHINE, and `open_or_create` means a colliding loser reads the winner's config.

use std::sync::Arc;
use std::time::Duration;

use goofi_core::{Param, SlotType};
use goofi_tests::{f32s, frame, WAIT};
use goofi_graph::Uid;
use goofi_signal::runtime::{
    Control, ControlSink, Envelope, IoxTransport, NodeChannel, NodeEnv, NodeFault, NodeRuntime,
    ParamValue, Status, Transport, WireStatus,
};
use goofi_transport::{door_service, iox_node, output_service, service_base, Doorbell, IoxNode};
use goofi_node::{NodeManifest, OutputDecl, ParamKey, Params, SlotDecl};
use goofi_signal_sdk::{Inputs, Node, NodeCtx, NodeResult, Outputs};

/// A node with one input and one output. Nothing runs it; the manifest is what the transport reads.
struct Passthrough;
impl Node for Passthrough {
    fn process(&mut self, _i: &Inputs<'_>, _o: &mut Outputs<'_>, _c: &mut NodeCtx, _p: &Params<'_>) -> NodeResult {
        Ok(())
    }
}
static INPUTS: &[SlotDecl] = &[SlotDecl {
    name: "input",
    kind: SlotType::Array,
    trigger_process: true,
    multi: false,
    required: false,
}];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];
static MANIFEST: NodeManifest = NodeManifest {
    type_name: "_TransportTest",
    tags: &[],
    doc: "transport fixture",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: &[],
    producer: false,
};

fn manifest() -> &'static NodeManifest {
    &MANIFEST
}

/// This run's service-name scope.
fn instance() -> String {
    // The process is walled off from the real home before its session is decided.
    goofi_tests::walled_home();
    format!("t{:x}", std::process::id())
}

/// Status is asynchronous by design, so a test waits for it with a deadline rather than reading once.
fn status_within(channel: &NodeChannel, timeout: Duration) -> Vec<WireStatus> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let got = channel.drain_status();
        if !got.is_empty() || std::time::Instant::now() >= deadline {
            return got;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The service names of a node born at `uid` — what the graph puts in the other end's slot message.
fn base_of(uid: Uid) -> String {
    service_base(&instance(), uid, 0)
}

#[test]
fn the_services_are_created_with_limits_the_defaults_do_not_give_us() {
    // iceoryx2 fixes these at CREATION, so they are hard patch limits, and every default is wrong
    // for this design.
    let t = IoxTransport::create(&instance(), Uid(1), 0, manifest()).expect("services");
    let cfg = t.event_config();
    assert_eq!(cfg.event_id_max_value(), 255);
    assert_eq!(cfg.max_notifiers(), 256);
    assert_eq!(cfg.max_listeners(), 2);
    let d = t.data_config("out").expect("the declared output slot has a service");
    assert_eq!(d.history_size(), 0, "a link NEVER replays a previous output");
    assert_eq!(d.max_subscribers(), 256);
    assert_eq!(d.max_publishers(), 1);
    assert_eq!(d.subscriber_max_buffer_size(), 1, "latest-wins, per wire");
    assert!(d.has_safe_overflow(), "drop-oldest, matching the no-queue delivery model");
}

#[test]
fn an_undrained_control_mailbox_keeps_the_whole_burst() {
    // Control and status are message STREAMS, not the latest-wins CELL a data wire is. The count is
    // past any plausible drain interval: a node deep inside `process` is the burst this has to survive.
    const BURST: u64 = 200;
    let transport = IoxTransport::create(&instance(), Uid(30), 0, manifest()).unwrap();
    let graph_node = goofi_transport::iox_node().unwrap();
    let channel = NodeChannel::open(&graph_node, &base_of(Uid(30))).unwrap();
    for seq in 1..=BURST {
        channel.send(Envelope {
            seq,
            control: Control::InSlot { slot: "input".to_string(), wires: Vec::new() },
        });
    }

    let got = transport.drain_control();
    assert_eq!(
        got.iter().map(|e| e.seq).collect::<Vec<_>>(),
        (1..=BURST).collect::<Vec<_>>(),
        "every message, in the order it was sent",
    );
}

/// The doorbell of a node born at `uid`, opened on the ringer's own iceoryx2 node.
fn bell_for(uid: Uid, ringer: &IoxNode) -> Doorbell {
    Doorbell::open(ringer, &door_service(&base_of(uid))).expect("doorbell")
}

#[test]
fn a_notify_landing_mid_drain_is_not_lost() {
    // The notification is only a HINT; the truth is in the subscriber queues and the control mailbox.
    let t = IoxTransport::create(&instance(), Uid(2), 0, manifest()).unwrap();
    let ringer = iox_node().unwrap();
    let bell = bell_for(Uid(2), &ringer);
    bell.ring(1).unwrap();
    assert_eq!(t.wait(Some(WAIT)), vec![1]);
    bell.ring(2).unwrap(); // lands while "draining"
    assert_eq!(t.wait(Some(WAIT)), vec![2], "retained across the re-park");
}

#[test]
fn a_control_and_a_data_notification_both_survive() {
    let t = IoxTransport::create(&instance(), Uid(3), 0, manifest()).unwrap();
    let ringer = iox_node().unwrap();
    let bell = bell_for(Uid(3), &ringer);
    bell.ring(0).unwrap();
    bell.ring(3).unwrap();
    let mut got = Vec::new();
    while !(got.contains(&0) && got.contains(&3)) {
        let woke = t.wait(Some(WAIT));
        assert!(!woke.is_empty(), "both ids were rung, and only {got:?} woke the node");
        got.extend(woke);
    }
    got.sort();
    assert_eq!(got, vec![0, 3], "each id wakes once");
}

#[test]
fn a_control_message_crosses_shared_memory_and_comes_back_acked() {
    // The ack is the only thing that orders a wire change, so a message without one stalls the sequence.
    let transport = Arc::new(IoxTransport::create(&instance(), Uid(4), 0, manifest()).unwrap());
    let mut node = NodeRuntime::new(
        manifest(),
        Box::new(Passthrough),
        manifest().default_params(),
        transport.clone(),
        NodeEnv::detached(),
    );
    let graph_node = goofi_transport::iox_node().unwrap();
    let channel = NodeChannel::open(&graph_node, &base_of(Uid(4))).unwrap();

    assert_eq!(node.next_wake(), None, "parked: nothing has asked this node to run");
    channel.send(Envelope {
        seq: 41,
        control: Control::SetParam {
            key: ParamKey::new("common", "autotrigger"),
            value: ParamValue::Literal(Param::boolean(true)),
        },
    });
    assert_eq!(transport.wait(Some(WAIT)), vec![0], "the graph rang the control id");

    node.run_once();
    assert!(node.next_wake().is_some(), "the node applied what it was sent and re-paced");
    assert_eq!(status_within(&channel, WAIT), vec![WireStatus::Ack { seq: 41, ok: Ok(()) }]);

    // The ack carries the VERDICT, not a receipt: the graph abandons a refused sequence.
    channel.send(Envelope {
        seq: 42,
        control: Control::OutSlot { slot: "nope".to_string(), targets: Vec::new() },
    });
    node.run_once();
    assert_eq!(
        status_within(&channel, WAIT),
        vec![WireStatus::Ack { seq: 42, ok: Err("no output slot `nope`".to_string()) }]
    );

    let fault = NodeFault::Process { msg: "boom".to_string(), since: 12.5 };
    transport.report(WireStatus::Health(Status::Fault { fault: Some(fault.clone()) }));
    assert_eq!(status_within(&channel, WAIT), vec![WireStatus::Health(Status::Fault { fault: Some(fault) })]);
}

#[test]
fn a_frame_reaches_a_wired_consumer_and_rings_its_slot() {
    // A wire is two declarations and nothing else: neither end knows the other's uid.
    let producer = IoxTransport::create(&instance(), Uid(5), 0, manifest()).unwrap();
    let consumer = IoxTransport::create(&instance(), Uid(6), 0, manifest()).unwrap();
    consumer.wire_in("input", &[output_service(&base_of(Uid(5)), "out")]).unwrap();
    producer.wire_out("out", &[(door_service(&base_of(Uid(6))), 1)]).unwrap();

    producer.publish("out", &frame(&[1.0, 2.0, 3.0]));
    assert_eq!(consumer.wait(Some(WAIT)), vec![1], "woken by the slot's own event id");
    let got = consumer.drain_inputs();
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].0.as_str(), got[0].1), ("input", 0), "slot, and its position in the wire order");
    assert_eq!(f32s(&got[0].2), vec![1.0, 2.0, 3.0]);
    assert!(consumer.drain_inputs().is_empty(), "a drained wire is empty");
}

#[test]
fn a_frame_larger_than_the_initial_slice_still_lands() {
    // `AllocationStrategy::Static`, the iceoryx2 default, refuses this: a GOOF frame is variable-size.
    let producer = IoxTransport::create(&instance(), Uid(7), 0, manifest()).unwrap();
    let consumer = IoxTransport::create(&instance(), Uid(8), 0, manifest()).unwrap();
    consumer.wire_in("input", &[output_service(&base_of(Uid(7)), "out")]).unwrap();

    let big: Vec<f32> = (0..80_000).map(|i| i as f32).collect(); // 320 KB, past the 64 KiB start
    producer.publish("out", &frame(&big));
    let got = consumer.drain_inputs();
    assert_eq!(got.len(), 1, "the oversized frame was published and received");
    assert_eq!(f32s(&got[0].2), big);
}

#[test]
fn a_re_sent_wire_set_keeps_what_it_names_and_drops_what_it_omits() {
    // The slot set is DECLARATIVE, and the FULL set is re-sent on every change: a surviving wire
    // must be kept rather than rebuilt, and what the set no longer names is dropped.
    let producer = IoxTransport::create(&instance(), Uid(9), 0, manifest()).unwrap();
    let consumer = IoxTransport::create(&instance(), Uid(10), 0, manifest()).unwrap();
    let held = output_service(&base_of(Uid(9)), "out");
    let added = output_service(&base_of(Uid(11)), "out");
    consumer.wire_in("input", std::slice::from_ref(&held)).unwrap();
    producer.publish("out", &frame(&[1.0])); // in flight, unread

    consumer.wire_in("input", &[held, added]).unwrap();
    let got = consumer.drain_inputs();
    assert_eq!(got.len(), 1, "the second wire has nothing yet");
    assert_eq!(f32s(&got[0].2), vec![1.0], "and the first still holds what it was sent");

    consumer.wire_in("input", &[]).unwrap();
    producer.publish("out", &frame(&[2.0]));
    assert!(consumer.drain_inputs().is_empty(), "the dropped wire delivers nothing");
}

#[test]
fn a_slot_feeds_more_consumers_than_the_iceoryx2_defaults_allow() {
    // `max_subscribers` is inert on its own: a service is opened from one iceoryx2 node per graph
    // node, and `max_nodes` counts exactly those.
    const CONSUMERS: u64 = 24;
    let producer = IoxTransport::create(&instance(), Uid(20), 0, manifest()).unwrap();
    let service = output_service(&base_of(Uid(20)), "out");
    let consumers: Vec<IoxTransport> = (0..CONSUMERS)
        .map(|i| {
            let c = IoxTransport::create(&instance(), Uid(100 + i), 0, manifest()).unwrap();
            c.wire_in("input", std::slice::from_ref(&service)).expect("subscribe");
            c
        })
        .collect();

    producer.publish("out", &frame(&[7.0]));
    for (i, consumer) in consumers.iter().enumerate() {
        assert_eq!(consumer.drain_inputs().len(), 1, "consumer {i} of {CONSUMERS} got the frame");
    }
}

#[test]
fn a_multi_input_keeps_one_cell_per_wire_in_the_order_it_was_given() {
    // A multi slot's cells are keyed by service name and ordered by the SET, never by the producers.
    // The wire count is past the event service's `max_nodes`, which counts one node per producer.
    const WIRES: u64 = 40;
    let producers: Vec<IoxTransport> = (0..WIRES)
        .map(|i| IoxTransport::create(&instance(), Uid(200 + i), 0, manifest()).unwrap())
        .collect();
    let consumer = IoxTransport::create(&instance(), Uid(21), 0, manifest()).unwrap();
    // Reversed, so a wire index that follows the producers rather than the set is visible.
    let services: Vec<String> =
        (0..WIRES).rev().map(|i| output_service(&base_of(Uid(200 + i)), "out")).collect();
    consumer.wire_in("input", &services).expect("subscribe");

    let door = door_service(&base_of(Uid(21)));
    for (i, producer) in producers.iter().enumerate() {
        producer.wire_out("out", &[(door.clone(), 1)]).expect("ring this consumer");
        producer.publish("out", &frame(&[i as f32]));
    }
    // Twice on one wire before the drain: latest-wins keeps the second, per wire.
    producers[0].publish("out", &frame(&[100.0]));

    let got = consumer.drain_inputs();
    assert_eq!(got.len(), WIRES as usize, "one cell per wire, none merged");
    assert_eq!(
        got.iter().map(|(_, index, _)| *index).collect::<Vec<_>>(),
        (0..WIRES as usize).collect::<Vec<_>>(),
        "the cells are indexed by position in the set"
    );
    assert_eq!(
        got.iter().map(|(_, _, frame)| f32s(frame)[0]).collect::<Vec<_>>(),
        (0..WIRES).rev().map(|i| if i == 0 { 100.0 } else { i as f32 }).collect::<Vec<f32>>(),
        "each cell holds its own producer's newest frame"
    );
}

/// The env var that turns this binary into the child below. Its value is irrelevant.
const CRASH_HELPER: &str = "GOOFI_TRANSPORT_CRASH_HELPER";

/// The child: hold a session of its own, open a node with a port in it, NAME the session, then
/// wait to be killed, or for its stdin to close.
#[test]
fn crash_helper() {
    if std::env::var(CRASH_HELPER).is_err() {
        return; // the ordinary run: this test is only the child's entry point
    }
    let node = iox_node().expect("a node");
    let _out = goofi_transport::data_service(&node, "goofi_crash_helper_out").expect("a service");
    println!("READY {}", goofi_transport::session());
    if std::env::var(CRASH_HELPER).as_deref() == Ok("exit") {
        // A process that leaves through `exit`, with its ports still open and no release called:
        // the way a test binary or a second Ctrl-C ends.
        std::process::exit(0);
    }
    let _ = std::io::stdin().read_line(&mut String::new());
}

/// A session owns one ephemeral directory, a workspace and its cache parts; the lock alone
/// decides what a boot sweep removes, and a content key is never mistaken for a session.
#[test]
fn a_session_owns_its_directory_workspace_and_cache_parts() {
    use goofi_core::session::{alive, hold, sessions, sweep_system, system_dir, workspace_dir, Session};
    use std::fs;
    goofi_tests::walled_home();
    let _sole = goofi_tests::sole_session();
    let remove = |p: &std::path::Path| {
        let _ = fs::remove_dir_all(p);
    };
    let held = hold("abcabcabcabcabc1").unwrap();
    held.record_url("http://127.0.0.1:9999");
    assert!(alive("abcabcabcabcabc1"), "held from within the same process still reads alive");
    assert!(sessions(remove).contains(&Session { id: "abcabcabcabcabc1".into(), url: "http://127.0.0.1:9999".into() }));
    assert!(system_dir("abcabcabcabcabc1").is_dir());

    // A dead session: its lock file exists in its directory and nobody holds it. An orphan
    // directory with no lock at all. A part a crashed hold left behind.
    fs::create_dir_all(system_dir("gone")).unwrap();
    fs::File::create(system_dir("gone").join("alive.lock")).unwrap();
    fs::create_dir_all(system_dir("orphan").join("iox")).unwrap();
    fs::create_dir_all(system_dir("stale.part")).unwrap();
    fs::File::create(system_dir("stale.part").join("alive.lock")).unwrap();
    assert!(!alive("gone"));
    assert!(sessions(remove).iter().all(|s| s.id != "gone"), "the dead one is not listed");
    assert!(!system_dir("gone").exists() && !system_dir("orphan").exists(), "…and is swept as it is met");
    assert!(!system_dir("stale.part").exists(), "a crashed hold's part is swept");
    assert!(system_dir("abcabcabcabcabc1").join("alive.lock").exists(), "the live one is untouched");

    // The workspace parent: the session's to remove once its last mount is gone, at its release.
    fs::create_dir_all(workspace_dir("abcabcabcabcabc1")).unwrap();

    // The caches: a dead session's part and work dir go, another version's tree goes; a live
    // session's part, this version's tree and a 16-hex CONTENT key stay.
    let live = hold("0123456789abcdef").unwrap();
    let system = goofi_core::home::system();
    let out = system.join("build").join("out").join("k");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join(".node.so.s0123456789abcdef"), b"").unwrap();
    fs::write(out.join(".node.so.sfedcba9876543210"), b"").unwrap();
    fs::write(out.join("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef.json"), b"").unwrap();
    let work = system.join("build").join("plugins").join("x").join("work-sfedcba9876543210-0");
    fs::create_dir_all(&work).unwrap();
    // The RUNNING version, since every other situation in this process reads that shipped tree.
    let version = env!("CARGO_PKG_VERSION");
    fs::create_dir_all(system.join("shipped").join(version).join("fedcba9876543210")).unwrap();
    fs::create_dir_all(system.join("build").join("sdk").join("0.0.1")).unwrap();
    fs::create_dir_all(system.join("build").join("sdk").join(version)).unwrap();
    sweep_system(version);
    assert!(out.join(".node.so.s0123456789abcdef").exists(), "a live session's part stays");
    assert!(!out.join(".node.so.sfedcba9876543210").exists() && !work.exists(), "a dead session's go");
    assert!(system.join("shipped").join(version).join("fedcba9876543210").exists(), "a content key stays");
    assert!(out.join("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef.json").exists());
    assert!(!system.join("build").join("sdk").join("0.0.1").exists(), "another version's tree goes");
    assert!(system.join("build").join("sdk").join(version).exists());
    let _ = fs::remove_dir_all(system.join("shipped").join(version).join("fedcba9876543210"));
    drop(live);

    drop(held);
    assert!(!alive("abcabcabcabcabc1"));
    assert!(!workspace_dir("abcabcabcabcabc1").exists(), "the empty workspace parent went with it");
    let _ = fs::remove_dir_all(system_dir("abcabcabcabcabc1"));
}

#[test]
fn a_process_that_exits_without_releasing_leaves_no_record() {
    goofi_tests::walled_home();
    let _sole = goofi_tests::sole_session();
    let out = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([&format!("{}::crash_helper", crate::situation(module_path!())), "--exact", "--nocapture"])
        .env(CRASH_HELPER, "exit")
        .env_remove(goofi_core::session::ENV)
        .stderr(std::process::Stdio::null())
        .output()
        .expect("run the child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let id = stdout.lines().find_map(|l| l.strip_prefix("READY ")).map(str::trim).unwrap_or_default().to_string();
    assert!(!id.is_empty(), "the child named its session: {stdout:?}");
    assert!(!goofi_core::session::system_dir(&id).exists(), "the directory went at exit, with no release called");
}

#[test]
fn what_a_crash_left_behind_is_gone_by_the_next_start() {
    goofi_tests::walled_home();
    let _sole = goofi_tests::sole_session();
    // A killed process drops NOTHING: its record, its ephemeral directory and its shared memory
    // stay. Its lock does not — the OS releases it — and that is the one thing the sweep reads.
    // Under ANOTHER home: the sweep reads the lock on the machine-wide base, so a boot under a
    // foreign `GOOFI_HOME` — a test run, a warm build — never takes a live session for dead.
    let foreign = std::env::temp_dir().join(format!("goofi-foreign-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&foreign);
    let mut child = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([&format!("{}::crash_helper", crate::situation(module_path!())), "--exact", "--nocapture"])
        .env(CRASH_HELPER, "1")
        .env("GOOFI_HOME", &foreign)
        // Its OWN session, not this process's.
        .env_remove(goofi_core::session::ENV)
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
    let system = goofi_core::session::system_dir(&id);
    assert!(goofi_core::session::alive(&id), "the child holds its session while it lives");
    assert!(system.join("iox").is_dir(), "its ephemeral directory is where iceoryx2 wrote");
    assert!(goofi_transport::sessions().iter().any(|s| s.id == id), "listed from any home");
    let workspace = goofi_core::session::workspace_dir(&id).join("nonce");
    std::fs::create_dir_all(&workspace).unwrap();
    goofi_transport::sweep_dead();
    goofi_bridge::autosave::sweep_dead();
    assert!(system.join("iox").is_dir() && workspace.is_dir(), "a sweep from this home left the live session alone");

    // Killed, because the point is a process that drops nothing: a graceful exit would clean up.
    let _ = child.kill();
    let _ = child.wait();
    assert!(!goofi_core::session::alive(&id), "the lock went with the process");
    assert!(system.exists(), "…and everything else stayed");

    let segments = || -> usize {
        std::fs::read_dir("/dev/shm")
            .map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&format!("g{id}_"))).count())
            .unwrap_or(0)
    };
    if cfg!(target_os = "linux") {
        assert!(segments() > 0, "the child's shared memory stayed too");
    }

    goofi_transport::sweep_dead();
    goofi_bridge::autosave::sweep_dead();
    assert!(!system.exists(), "the ephemeral directory was swept");
    assert!(!workspace.exists(), "the workspace was swept");
    assert_eq!(segments(), 0, "the shared memory its prefix names was swept");
    assert!(goofi_transport::sessions().iter().all(|s| s.id != id));
    let _ = std::fs::remove_dir_all(&foreign);
}
