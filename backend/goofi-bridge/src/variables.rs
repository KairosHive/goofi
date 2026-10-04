//! The patch's own producer, `variables`: one data service per variable, a history of one, written
//! from the store under its lock and nothing else. Its door is rung by a subscriber that just
//! opened, and answered with the frame held and a ring on every consumer's door.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use goofi_graph::{Graph, Uid};
use goofi_supervisor::sync::Mutex;
use goofi_node::EventId;
use goofi_transport::{
    deliver_held, door_service, event_service, output_service, publisher, service_base, stream_service, variables_base,
    view_door_service, BytePublisher, ByteService, Doorbell, IoxNode, ServiceKind, INITIAL_SLICE, VIEW_EVENT_ID,
};

/// One variable's wire: its publisher, and the bell on its view door for the reducer parked there.
struct Port {
    publisher: BytePublisher,
    view: Doorbell,
    _service: ByteService,
}

struct Inner {
    ports: HashMap<String, Port>,
    /// Per variable, the doors its consumers park on and the id each is rung with — the graph's
    /// bindings, re-read on every wake of the door below.
    ringers: HashMap<String, Vec<(String, EventId)>>,
    /// One bell per consumer door, opened once: a door opened twice on one node is a second state.
    bells: HashMap<String, Doorbell>,
    /// Declared LAST: fields drop in order, and a node dropped before its ports cannot remove its
    /// own directory.
    node: Arc<IoxNode>,
}

pub struct Variables {
    instance: String,
    /// The producer's own bell on its door, so a settle and the stop wake the thread.
    own: Doorbell,
    /// The door's listener, taken by the thread that serves it.
    listener: Mutex<Option<goofi_transport::Listener>>,
    /// Declared LAST: it holds the node, and the ports above must drop first.
    inner: Mutex<Inner>,
}

impl Variables {
    pub fn new(iox: &goofi_transport::Iox, instance: &str) -> Result<Variables, String> {
        let node = Arc::new(iox.node()?);
        // The door is opened ONCE on this node: a listener and a bell from the one service.
        let door = event_service(&node, &door_service(&variables_base(instance)))?;
        let listener = door.listener_builder().create().map_err(|e| format!("listener: {e}"))?;
        let own = Doorbell::on(&door, "variables")?;
        let inner = Inner { ports: HashMap::new(), ringers: HashMap::new(), bells: HashMap::new(), node };
        Ok(Variables { instance: instance.to_string(), own, listener: Mutex::new(Some(listener)), inner: Mutex::new(inner) })
    }

    /// Wake the thread: a settle moved the bindings, or the manager is leaving.
    pub fn poke(&self) {
        let _ = self.own.ring(0);
    }

    /// Park on the door until `stopping`: each wake re-reads the ringers from the graph, hands the
    /// held frames to whoever subscribed since, and rings every door. A spurious ring costs a wake.
    pub fn serve(&self, graph: &Mutex<Graph>, stopping: &goofi_transport::Halt) {
        let Some(listener) = self.listener.lock().take() else { return };
        while !stopping.stopped() {
            goofi_transport::wait_within(&listener, Duration::from_millis(500), |_| {});
            if stopping.stopped() {
                return;
            }
            let ringers = doors_of(&graph.lock(), &self.instance);
            let mut inner = self.inner.lock();
            inner.ringers = ringers;
            let Inner { ports, ringers, bells, node } = &mut *inner;
            bells.retain(|door, _| ringers.values().flatten().any(|(d, _)| d == door));
            for (name, port) in ports.iter() {
                deliver_held(&port.publisher);
                for (door, id) in ringers.get(name).into_iter().flatten() {
                    if let Some(bell) = bell(bells, node, door) {
                        let _ = bell.ring(*id);
                    }
                }
                let _ = port.view.ring(VIEW_EVENT_ID);
            }
        }
    }
}

/// Every consumer door per variable, from the settled bindings.
fn doors_of(g: &Graph, instance: &str) -> HashMap<String, Vec<(String, EventId)>> {
    let mut out: HashMap<String, Vec<(String, EventId)>> = HashMap::new();
    for (name, consumer, generation, id) in g.variable_ringers() {
        out.entry(name).or_default().push((door_service(&service_base(instance, consumer, generation)), id));
    }
    out
}

/// The one bell on `door`, opened now if this is its first ring.
fn bell<'a>(bells: &'a mut HashMap<String, Doorbell>, node: &IoxNode, door: &str) -> Option<&'a Doorbell> {
    if !bells.contains_key(door) {
        match Doorbell::open(node, door) {
            Ok(bell) => {
                bells.insert(door.to_string(), bell);
            }
            Err(e) => {
                trouble(&format!("bell onto `{door}` did not open: {e}"));
                return None;
            }
        }
    }
    bells.get(door)
}

fn trouble(why: &str) {
    goofi_supervisor::log::record(goofi_supervisor::log::Source::component("variables"), goofi_supervisor::log::Level::Error, None, why.to_string());
}

impl goofi_core::variables::Plane for Variables {
    fn publish(&self, name: &str, value: &goofi_core::Data) {
        let bytes = match goofi_codec::encode(value) {
            Ok(bytes) => bytes,
            Err(e) => return trouble(&format!("`{name}` cannot cross: {e}")),
        };
        let mut inner = self.inner.lock();
        let Inner { ports, ringers, bells, node } = &mut *inner;
        if !ports.contains_key(name) {
            let service = stream_service(node, &output_service(&variables_base(&self.instance), name), ServiceKind::Held);
            let port = service.and_then(|service| {
                let publisher = publisher(&service, name, INITIAL_SLICE)?;
                let view = Doorbell::open(node, &view_door_service(&self.instance, Uid::VARIABLES, name))?;
                Ok(Port { publisher, view, _service: service })
            });
            match port {
                Ok(port) => {
                    ports.insert(name.to_string(), port);
                }
                Err(e) => return trouble(&format!("`{name}` has no wire: {e}")),
            }
        }
        let port = &ports[name];
        let doors: Vec<(String, EventId)> = ringers.get(name).cloned().unwrap_or_default();
        for (door, _) in &doors {
            bell(bells, node, door);
        }
        let rung = doors.iter().filter_map(|(door, id)| Some((bells.get(door)?, *id)));
        let rung = rung.chain(std::iter::once((&port.view, VIEW_EVENT_ID)));
        if let Err(e) = goofi_transport::publish(&port.publisher, &bytes, rung) {
            trouble(&format!("`{name}` did not go out: {e}"));
        }
    }

    fn retire(&self, name: &str) {
        self.inner.lock().ports.remove(name);
    }
}
