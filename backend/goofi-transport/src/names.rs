//! Service names: pure derivation from the graph's facts, so every end of a wire spells it alike.

use goofi_node::{BoundVar, EventId, GraphView, Uid, Var};

use crate::ServiceName;

/// The id every armed slot rings the recorder's door with; the drain ignores it, so one is enough.
pub const RECORD_EVENT_ID: EventId = 0;
/// The id a producer rings a slot's view door with: a frame is out.
pub const VIEW_EVENT_ID: EventId = 0;
/// The id the bridge rings a view door with: what the reducer should do may have moved — a
/// reader, a spec, a tap, an ask, or the graph itself.
pub const VIEW_POKE_ID: EventId = 1;

/// The name every service of one node is derived from: `<instance>_<uid>_<gen>`. `gen` is bumped on
/// EVERY birth, because teardown never blocks and a rebirth would else race its predecessor.
pub fn service_base(instance: &str, uid: Uid, gen: u64) -> String {
    format!("{instance}_{}_{gen}", uid.to_hex())
}

/// The one event service a node parks on for its whole life (§3.2).
pub fn door_service(base: &str) -> ServiceName {
    format!("goofi_{base}_door")
}

/// One output slot's data service — the name a consumer is given in its `InSlot` set.
pub fn output_service(base: &str, slot: &str) -> ServiceName {
    format!("goofi_{base}_out_{slot}")
}

/// The base of the patch's own producer, [`Uid::VARIABLES`], whose slots are the variables. It
/// lives as long as the manager, so it carries no generation.
pub fn variables_base(instance: &str) -> String {
    format!("{instance}_variables")
}

/// One variable's output service, derived from its birth identity.
pub fn variable_output(instance: &str, name: &str, generation: u64) -> ServiceName {
    output_service(&format!("{}_{generation}", variables_base(instance)), name)
}

/// The door a slot's reducer parks on, rung by whichever generation of the node produces the
/// slot: named without the generation, so a restart rings the same reducer.
pub fn view_door_service(instance: &str, uid: Uid, slot: &str) -> ServiceName {
    format!("goofi_{instance}_{}_view_{slot}", uid.to_hex())
}

/// One output slot's recording service, one per ARMING: a name whose subscriber left is never
/// reopened, which iceoryx2 can answer with a second service under the same name.
pub fn record_service(base: &str, slot: &str, serial: u64) -> ServiceName {
    format!("goofi_{base}_rec_{slot}_{serial}")
}

/// The ONE door every armed slot rings once its frame is out. It is the recorder's, not a node's,
/// so a burst across every armed slot coalesces into one sweep of every armed buffer.
pub fn record_door_service(instance: &str) -> ServiceName {
    format!("goofi_{instance}_recdoor")
}

/// One node's door, from the view's birth facts.
pub fn door_of(view: &GraphView<'_>, uid: Uid) -> Option<ServiceName> {
    let node = view.nodes.get(&uid)?;
    Some(door_service(&service_base(view.instance, uid, node.generation)))
}

/// Every door one output rings: its ringers' doors, and the slot's view door while a reducer
/// watches it. The one derivation, so every engine's producers ring the same set.
pub fn targets_of<'a>(
    view: &GraphView<'_>,
    producer: Uid,
    slot: &str,
    ringers: impl IntoIterator<Item = goofi_node::Ringer<'a>>,
) -> Vec<(ServiceName, EventId)> {
    let mut targets: Vec<(ServiceName, EventId)> =
        ringers.into_iter().filter_map(|r| Some((door_of(view, r.consumer)?, r.event_id))).collect();
    if view.nodes.get(&producer).is_some_and(|n| n.watched.contains(&slot)) {
        targets.push((view_door_service(view.instance, producer, slot), VIEW_EVENT_ID));
    }
    targets
}

/// One output slot's data service name, from the view's birth facts.
pub fn output_of(view: &GraphView<'_>, uid: Uid, slot: &str) -> Option<ServiceName> {
    if uid == Uid::VARIABLES { return Some(variable_output(view.instance, slot, view.variables.generation(slot)?)); }
    Some(output_service(&service_base(view.instance, uid, view.nodes.get(&uid)?.generation), slot))
}

/// A resolved variable as a node receives it: a service rather than a uid, because a node
/// addresses a producer by service and cannot resolve anything for itself.
pub fn var_of(view: &GraphView<'_>, v: &BoundVar) -> (String, Var) {
    match v {
        BoundVar::Stream { var, producer, slot, .. } => {
            // The variables producer holds its last frame; a node slot has only what comes next.
            let held = (*producer == Uid::VARIABLES).then(|| door_service(&variables_base(view.instance)));
            let src = output_of(view, *producer, slot)
                .map_or_else(|| Var::Missing(format!("`{var}` names no running node")), |service| Var::Stream { service, held });
            (var.clone(), src)
        }
        BoundVar::Value { var, value } => (var.clone(), Var::Value(value.clone())),
        BoundVar::Missing { var, reason } => (var.clone(), Var::Missing(reason.clone())),
    }
}
