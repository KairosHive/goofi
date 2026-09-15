//! The signal engine's ends of the shared transport: a node's own services, and the graph's end
//! of one node's control channel. The machinery and the names are `goofi-transport`'s.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use iceoryx2::prelude::*;

use goofi_core::Data;
use goofi_node::NodeManifest;
use goofi_transport::{
    control_service, data_service, door_service, event_service, iox_node, message_service,
    output_service, publisher, record_door_service,
    record_service, record_shape, service_base,
    status_service, ByteService, ByteSubscriber, Doorbell, EventService, IoxNode, INITIAL_SLICE,
    MESSAGE_SLICE,
};

use super::wire::{ControlSink, Envelope, EventId, ServiceName, Transport, WireStatus};
use goofi_node::Uid;

type BytePublisher = goofi_transport::BytePublisher;

/// The one id the graph itself rings with — every `Control`, whatever it says.
const CONTROL_EVENT_ID: EventId = 0;
/// One output slot: its publisher, and the doorbells to ring once a frame is out.
struct OutputPort {
    service: ByteService,
    publisher: BytePublisher,
    /// The full desired set the graph last sent, already opened — nothing is reconciled per frame.
    targets: Mutex<Vec<(Doorbell, EventId)>>,
}

/// One wire feeding an input slot. The order of these within a slot IS `Inputs::get_multi`'s
/// connection order (§3.5).
struct InputWire {
    service: ServiceName,
    subscriber: ByteSubscriber,
}

/// The armed slots' ports by slot, each with the arming it serves, and the ports a newer arming
/// replaced — kept until the recorder has let go of them.
#[derive(Default)]
struct Records {
    live: HashMap<String, (u64, goofi_transport::RecordPort)>,
    retired: Vec<goofi_transport::RecordPort>,
    /// The node's one bell on the recorder's door, opened by the first arming and kept for its life.
    bell: Option<std::sync::Arc<goofi_transport::Doorbell>>,
}

/// A node's end of every service it owns.
pub struct IoxTransport {
    /// The name every service of this node is derived from.
    base: String,
    door: EventService,
    listener: goofi_transport::Listener,
    control: ByteSubscriber,
    status: BytePublisher,
    /// Fixed at birth from the manifest: an output slot cannot appear later.
    outputs: HashMap<&'static str, OutputPort>,
    /// Grown and shrunk by `InSlot`, which is why it is the one map behind a lock.
    inputs: Mutex<Vec<(String, Vec<InputWire>)>>,
    /// The recorder's one door, by name; a bell onto it is opened with the slot that rings it.
    record_door: ServiceName,
    /// The armed slots' second publishers, reconciled by `RecSlot` and released by the recorder.
    records: Mutex<Records>,
    /// What the recording last cost and why — the count and the cause a node wears as a fault.
    trouble: Mutex<Option<(u64, String)>>,
    /// Must outlive every port built from it, so it is declared LAST — Rust drops a struct's fields
    /// in declaration order, and a node dropped first cannot remove its own directory.
    node: IoxNode,
}

impl IoxTransport {
    /// Create every service this node owns and open its own end of each. The control SUBSCRIBER
    /// exists before the graph is told the node is ready, which closes the attach window (§4).
    pub fn create(
        instance: &str,
        uid: Uid,
        gen: u64,
        manifest: &NodeManifest,
    ) -> Result<IoxTransport, String> {
        let base = service_base(instance, uid, gen);
        let record_door = record_door_service(instance);
        let node = iox_node()?;

        let door = event_service(&node, &door_service(&base))?;
        let listener = door.listener_builder().create().map_err(|e| format!("listener: {e}"))?;

        let control = message_service(&node, &control_service(&base))?
            .subscriber_builder()
            .create()
            .map_err(|e| format!("control subscriber: {e}"))?;
        let status = publisher(&message_service(&node, &status_service(&base))?, "status", MESSAGE_SLICE)?;

        let mut outputs = HashMap::new();
        for out in manifest.outputs {
            let service = data_service(&node, &output_service(&base, out.name))?;
            let publisher = publisher(&service, out.name, INITIAL_SLICE)?;
            outputs.insert(
                out.name,
                OutputPort { service, publisher, targets: Mutex::new(Vec::new()) },
            );
        }

        Ok(IoxTransport {
            node,
            base,
            door,
            listener,
            control,
            status,
            outputs,
            inputs: Mutex::new(Vec::new()),
            record_door,
            records: Mutex::new(Records::default()),
            trouble: Mutex::new(None),
        })
    }

    /// The doorbell service's creation-time limits, read back from iceoryx2 itself.
    pub fn event_config(&self) -> iceoryx2::service::static_config::event::StaticConfig {
        *self.door.static_config()
    }

    /// A declared output slot's creation-time limits, read back from iceoryx2 itself. `None` for a
    /// slot this node does not declare.
    pub fn data_config(
        &self,
        slot: &str,
    ) -> Option<iceoryx2::service::static_config::publish_subscribe::StaticConfig> {
        self.outputs.get(slot).map(|o| *o.service.static_config())
    }

    /// Why a recording loan was refused: an outsized frame, or shared memory that ran out.
    fn loan_refused(&self, bytes: &[u8]) -> String {
        let ceiling = record_shape("signal").slice;
        match bytes.len() > ceiling {
            true => format!("a {} byte frame is over the {ceiling} byte ceiling", bytes.len()),
            false => "no shared memory left".to_string(),
        }
    }

    /// Open this end of one wire, by the name that IS the wire's identity. `open_or_create` because
    /// a consumer may be wired before its producer has published — the service is the rendezvous.
    fn open_wire(&self, service: &ServiceName) -> Result<InputWire, String> {
        let subscriber = data_service(&self.node, service)?
            .subscriber_builder()
            .create()
            .map_err(|e| format!("subscriber `{service}`: {e}"))?;
        Ok(InputWire { service: service.clone(), subscriber })
    }
}

impl Transport for IoxTransport {
    fn wait(&self, timeout: Option<Duration>) -> Vec<EventId> {
        let mut ids = Vec::new();
        let push = |id: iceoryx2::prelude::EventId| ids.push(id.as_value() as EventId);
        // `wait_all` rather than `wait_one`: several producers can ring between two parks. A ZERO
        // timeout is its own call — a timed wait's zero means NO timeout, so it would park forever.
        let _ = match timeout {
            None => self.listener.blocking_wait_all(push),
            Some(t) if t.is_zero() => self.listener.try_wait_all(push),
            Some(t) => self.listener.timed_wait_all(push, t),
        };
        ids
    }

    fn drain_control(&self) -> Vec<Envelope> {
        let mut out = Vec::new();
        while let Ok(Some(sample)) = self.control.receive() {
            match Envelope::decode(sample.payload()) {
                Ok(envelope) => out.push(envelope),
                // Undecodable means the two ends disagree about the format, and there is no seq to
                // ack against.
                Err(_) => continue,
            }
        }
        out
    }

    fn wire_in(&self, slot: &str, services: &[ServiceName]) -> Result<(), String> {
        let mut inputs = self.inputs.lock().unwrap();
        let mut held: Vec<InputWire> = match inputs.iter().position(|(name, _)| name == slot) {
            Some(at) => inputs.remove(at).1,
            None => Vec::new(),
        };
        let mut wires = Vec::with_capacity(services.len());
        let mut failed = Vec::new();
        for service in services {
            let kept = goofi_transport::take_where(&mut held, |w| &w.service == service);
            match kept.map(Ok).unwrap_or_else(|| self.open_wire(service)) {
                Ok(wire) => wires.push(wire),
                Err(e) => failed.push(e),
            }
        }
        // A slot holds its whole set or nothing: a missing wire would shift every later one's index.
        if failed.is_empty() {
            inputs.push((slot.to_string(), wires));
            Ok(())
        } else {
            Err(failed.join("; "))
        }
    }

    fn wire_out(&self, slot: &str, targets: &[(ServiceName, EventId)]) -> Result<(), String> {
        let port = self.outputs.get(slot).ok_or_else(|| format!("no output slot `{slot}`"))?;
        // Opened here, where a failure can still be reported. The previous set stands until the
        // whole new one opens, so a refused message leaves the node in the state its ack describes
        // — and a survivor is MOVED across rather than reopened, or the peak would be both sets.
        let mut held = std::mem::take(&mut *port.targets.lock().unwrap());
        let mut opened = Vec::with_capacity(targets.len());
        for (service, id) in targets {
            match goofi_transport::take_where(&mut held, |(bell, _)| bell.names(service)) {
                Some((bell, _)) => opened.push((bell, *id)),
                None => opened.push((Doorbell::open(&self.node, service)?, *id)),
            }
        }
        *port.targets.lock().unwrap() = opened;
        Ok(())
    }

    /// Draining to empty rather than one frame per wake is §3.3's wake discipline: the notification
    /// is a hint, and a wire keeps only its newest frame.
    fn drain_inputs(&self) -> Vec<(String, usize, Data)> {
        let mut out = Vec::new();
        for (slot, wires) in self.inputs.lock().unwrap().iter() {
            for (index, wire) in wires.iter().enumerate() {
                let mut newest = None;
                while let Ok(Some(sample)) = wire.subscriber.receive() {
                    newest = Some(goofi_codec::decode(sample.payload()));
                }
                // A frame that cannot be decoded is a wire whose two ends disagree about the
                // format; dropping it keeps the node running on its other inputs.
                if let Some(Ok(frame)) = newest {
                    out.push((slot.clone(), index, frame));
                }
            }
        }
        out
    }

    fn record_out(&self, slots: &[(String, u64)]) -> Result<(), String> {
        let mut records = self.records.lock().unwrap();
        let Records { live, retired, bell } = &mut *records;
        // A new arming is a new service: the old port keeps the name the recorder may already
        // have let go of, and is dropped once nobody reads it.
        for (slot, (held, port)) in live.iter_mut() {
            match slots.iter().find(|(s, _)| s == slot) {
                Some((_, serial)) if serial == held => port.armed(),
                _ => port.retire(),
            }
        }
        let superseded: Vec<String> = live
            .iter()
            .filter(|(slot, (held, _))| slots.iter().any(|(s, serial)| s == *slot && serial != held))
            .map(|(slot, _)| slot.clone())
            .collect();
        for slot in superseded {
            if let Some((_, port)) = live.remove(&slot) {
                retired.push(port);
            }
        }
        live.retain(|_, (_, port)| !port.spent());
        retired.retain(|port| !port.spent());
        let mut failed = Vec::new();
        let wanted: Vec<&(String, u64)> = slots.iter().filter(|(s, _)| !live.contains_key(s)).collect();
        if !wanted.is_empty() && bell.is_none() {
            match goofi_transport::Doorbell::open(&self.node, &self.record_door) {
                Ok(opened) => *bell = Some(std::sync::Arc::new(opened)),
                Err(e) => failed.push(e),
            }
        }
        for (slot, serial) in wanted {
            if !self.outputs.contains_key(slot.as_str()) {
                failed.push(format!("no output slot `{slot}`"));
                continue;
            }
            let Some(bell) = bell.as_ref() else { continue };
            let shape = record_shape("signal");
            let opened = goofi_transport::RecordPort::open(
                &self.node,
                &record_service(&self.base, slot, *serial),
                bell,
                slot,
                shape,
            );
            match opened {
                Ok(port) => {
                    live.insert(slot.clone(), (*serial, port));
                }
                Err(e) => failed.push(e),
            }
        }
        drop(records);
        if failed.is_empty() {
            *self.trouble.lock().unwrap() = None;
            return Ok(());
        }
        let why = failed.join("; ");
        *self.trouble.lock().unwrap() = Some((0, why.clone()));
        Err(why)
    }

    fn record_trouble(&self) -> Option<String> {
        let trouble = self.trouble.lock().unwrap();
        let (dropped, why) = trouble.as_ref()?;
        Some(match dropped {
            0 => format!("recording: {why}"),
            n => format!("recording dropped {n} frames: {why}"),
        })
    }

    fn publish(&self, slot: &str, frame: &Data) {
        let Some(port) = self.outputs.get(slot) else { return };
        let bytes = goofi_codec::encode(frame);
        let targets = port.targets.lock().unwrap();
        goofi_transport::publish(&port.publisher, &bytes, targets.iter().map(|(b, id)| (b, *id)));
        let mut records = self.records.lock().unwrap();
        // A retired port is dropped HERE rather than at the disarm, so the release follows the
        // recorder's own reading and never outruns it.
        let Records { live, retired, .. } = &mut *records;
        live.retain(|_, (_, port)| !port.spent());
        retired.retain(|port| !port.spent());
        if let Some((_, rec)) = live.get(slot) {
            let ok = rec.send(&bytes);
            if !ok && !rec.retired() {
                let mut trouble = self.trouble.lock().unwrap();
                let (dropped, _) = trouble.get_or_insert_with(|| (0, self.loan_refused(&bytes)));
                *dropped += 1;
            }
        }
    }

    fn report(&self, status: WireStatus) {
        let bytes = status.encode();
        if let Ok(sample) = self.status.loan_slice_uninit(bytes.len()) {
            let _ = sample.write_from_slice(&bytes).send();
        }
    }
}

/// The graph's end of one node's control channel: control publisher, status subscriber and that
/// node's doorbell, all opened by name from the node's own service base.
pub struct NodeChannel {
    control: BytePublisher,
    status: ByteSubscriber,
    door: Doorbell,
}

impl NodeChannel {
    /// `node` is the GRAPH's own, shared by every channel: the graph is one owner, and one
    /// iceoryx2 node per host cost a monitor triple and a `node.details` per graph node.
    pub fn open(node: &IoxNode, base: &str) -> Result<NodeChannel, String> {
        let control = publisher(&message_service(node, &control_service(base))?, "control", MESSAGE_SLICE)?;
        let status = message_service(node, &status_service(base))?
            .subscriber_builder()
            .create()
            .map_err(|e| format!("status subscriber: {e}"))?;
        let door = Doorbell::open(node, &door_service(base))?;
        Ok(NodeChannel { control, status, door })
    }

    /// Ring the node's door with no message behind it — how a parked node is made to look at the
    /// [`super::Halt`] flag it was born holding.
    pub fn wake(&self) {
        let _ = self.door.ring(CONTROL_EVENT_ID);
    }

    /// Every transition the node has reported since the last drain, in order.
    pub fn drain_status(&self) -> Vec<WireStatus> {
        let mut out = Vec::new();
        while let Ok(Some(sample)) = self.status.receive() {
            if let Ok(status) = WireStatus::decode(sample.payload()) {
                out.push(status);
            }
        }
        out
    }
}

impl ControlSink for NodeChannel {
    /// Publish, then ring `EventId(0)`. In that order: a node woken first would drain an empty
    /// mailbox and park again.
    fn send(&self, envelope: Envelope) {
        let bytes = envelope.encode();
        if let Ok(sample) = self.control.loan_slice_uninit(bytes.len()) {
            let _ = sample.write_from_slice(&bytes).send();
        }
        let _ = self.door.ring(CONTROL_EVENT_ID);
    }
}
