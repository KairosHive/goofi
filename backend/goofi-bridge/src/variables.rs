//! The patch's own producer, `variables`: one slot per variable, held, written from the store
//! under its lock and nothing else. Its door is rung by a settle that moved the bindings, and
//! answered with a ring on every consumer's door.

use std::collections::HashMap;
use std::sync::Arc;

use goofi_graph::{Graph, Uid};
use goofi_node::EventId;
use goofi_supervisor::sync::Mutex;
use goofi_transport::{
    door_service, output_service, service_base, variables_base, view_door_service, Door, Doorbell, Hub, Payload, Publisher,
    VIEW_EVENT_ID,
};

/// One variable's wire: its publisher, and the bell on its view door for the reducer parked there.
struct Port {
    publisher: Publisher,
    view: Doorbell,
}

struct Inner {
    ports: HashMap<String, Port>,
    /// Per variable, the doors its consumers park on and the id each is rung with — the graph's
    /// bindings, re-read on every wake of the door below.
    ringers: HashMap<String, Vec<(String, EventId)>>,
    /// One bell per consumer door, taken once.
    bells: HashMap<String, Doorbell>,
}

pub struct Variables {
    instance: String,
    hub: Arc<Hub>,
    /// The producer's own door: a settle and the stop wake the thread through it.
    door: Arc<Door>,
    inner: Mutex<Inner>,
}

impl Variables {
    pub fn new(hub: Arc<Hub>, instance: &str) -> Variables {
        let door = hub.door(&door_service(&variables_base(instance)));
        let inner = Inner { ports: HashMap::new(), ringers: HashMap::new(), bells: HashMap::new() };
        Variables { instance: instance.to_string(), hub, door, inner: Mutex::new(inner) }
    }

    /// Wake the thread: a settle moved the bindings, or the manager is leaving.
    pub fn poke(&self) {
        self.door.ring(0);
    }

    /// Park on the door until `stopping`: a settle that moved the bindings rings it, and each wake
    /// re-reads the ringers from the graph and rings every consumer's door, so one that subscribed
    /// since takes the held frame.
    pub fn serve(&self, graph: &Mutex<Graph>, stopping: &goofi_transport::Halt) {
        while !stopping.stopped() {
            self.door.wait();
            if stopping.stopped() {
                return;
            }
            let ringers = doors_of(&graph.lock(), &self.instance);
            let mut inner = self.inner.lock();
            inner.ringers = ringers;
            let Inner { ports, ringers, bells } = &mut *inner;
            bells.retain(|door, _| ringers.values().flatten().any(|(d, _)| d == door));
            for (name, port) in ports.iter() {
                for (door, id) in ringers.get(name).into_iter().flatten() {
                    bell(bells, &self.hub, door).ring(*id);
                }
                port.view.ring(VIEW_EVENT_ID);
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

/// The one bell on `door`, taken now if this is its first ring.
fn bell<'a>(bells: &'a mut HashMap<String, Doorbell>, hub: &Hub, door: &str) -> &'a Doorbell {
    bells.entry(door.to_string()).or_insert_with(|| Doorbell::on(hub.door(door)))
}

impl goofi_core::variables::Plane for Variables {
    fn publish(&self, name: &str, value: &goofi_core::Data) {
        let mut inner = self.inner.lock();
        let Inner { ports, ringers, bells } = &mut *inner;
        let port = ports.entry(name.to_string()).or_insert_with(|| Port {
            publisher: self.hub.publisher(&output_service(&variables_base(&self.instance), name), true),
            view: Doorbell::on(self.hub.door(&view_door_service(&self.instance, Uid::VARIABLES, name))),
        });
        port.publisher.publish(Payload::Data(value.clone()));
        // The frame first, then every bell: a consumer woken first finds nothing and parks.
        for (door, id) in ringers.get(name).cloned().unwrap_or_default() {
            bell(bells, &self.hub, &door).ring(id);
        }
        port.view.ring(VIEW_EVENT_ID);
    }

    fn retire(&self, name: &str) {
        self.inner.lock().ports.remove(name);
    }
}
