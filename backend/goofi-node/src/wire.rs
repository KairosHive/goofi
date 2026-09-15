//! The settled view, its touches and a scan's outcome as plain data, so an engine in another
//! process reads exactly the state the graph settled and answers in the graph's own vocabulary.

use std::collections::HashMap;

use goofi_core::record::RecordedOutput;
use goofi_core::Param;
use serde::{Deserialize, Serialize};

use crate::seam::{BindingView, BoundVar, Edge, GraphView, NodeView, Touched};
use crate::{BindingId, Isolation, NodeManifest, ParamGroups, ParamKey, Uid};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeData {
    pub producer: (Uid, String),
    pub consumer: (Uid, String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum VarData {
    Stream { var: String, producer: Uid, slot: String, event_id: u8 },
    Value { var: String, value: Param },
    Missing { var: String, reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BindingData {
    pub key: ParamKey,
    pub rewritten: String,
    pub vars: Vec<VarData>,
    pub trigger: bool,
    pub id: Option<BindingId>,
    pub live: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeData {
    pub uid: Uid,
    pub engine: String,
    pub name: String,
    pub generation: u64,
    pub rings: bool,
    /// The qualified type id, which names the manifest on both sides.
    pub type_id: String,
    pub params: ParamGroups,
    pub bindings: Vec<BindingData>,
    pub recorded: Vec<RecordedOutput>,
}

/// The whole settled graph as data, with every manifest it names described once.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ViewData {
    pub instance: String,
    pub edges: Vec<EdgeData>,
    pub nodes: Vec<NodeData>,
    /// Qualified type id → the probe schema, for the manifests a reader does not hold itself.
    pub types: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TouchedData {
    Slot(Uid, String),
    Param(Uid, ParamKey),
    Record(Uid),
}

impl From<&Touched> for TouchedData {
    fn from(t: &Touched) -> TouchedData {
        match t {
            Touched::Slot(uid, slot) => TouchedData::Slot(*uid, slot.to_string()),
            Touched::Param(uid, key) => TouchedData::Param(*uid, key.clone()),
            Touched::Record(uid) => TouchedData::Record(*uid),
        }
    }
}

impl ViewData {
    /// The view as data: every node, and the schema of every type the nodes name.
    pub fn of(view: &GraphView<'_>) -> ViewData {
        let mut types = HashMap::new();
        let nodes = view
            .nodes
            .iter()
            .map(|(uid, n)| {
                let type_id = crate::qualify(n.engine, n.manifest.type_name);
                types.entry(type_id.clone()).or_insert_with(|| describe(n.manifest));
                NodeData {
                    uid: *uid,
                    engine: n.engine.to_string(),
                    name: n.name.to_string(),
                    generation: n.generation,
                    rings: n.rings,
                    type_id,
                    params: n.params.clone(),
                    bindings: n.bindings.iter().map(binding_data).collect(),
                    recorded: n.recorded.to_vec(),
                }
            })
            .collect();
        let edges = view
            .edges
            .iter()
            .map(|e| EdgeData {
                producer: (e.producer.0, e.producer.1.to_string()),
                consumer: (e.consumer.0, e.consumer.1.to_string()),
            })
            .collect();
        ViewData { instance: view.instance.to_string(), edges, nodes, types }
    }
}

fn describe(m: &NodeManifest) -> String {
    crate::describe(m.tags, m.doc, m.inputs, m.outputs, m.params, m.producer)
}

fn binding_data(b: &BindingView<'_>) -> BindingData {
    BindingData {
        key: b.key.clone(),
        rewritten: b.rewritten.to_string(),
        vars: b
            .vars
            .iter()
            .map(|v| match v {
                BoundVar::Stream { var, producer, slot, event_id } => {
                    VarData::Stream { var: var.clone(), producer: *producer, slot: slot.to_string(), event_id: *event_id }
                }
                BoundVar::Value { var, value } => VarData::Value { var: var.clone(), value: value.clone() },
                BoundVar::Missing { var, reason } => VarData::Missing { var: var.clone(), reason: reason.clone() },
            })
            .collect(),
        trigger: b.trigger,
        id: b.id,
        live: b.live,
    }
}

/// The manifests a reader holds for types it did not register itself, leaked once per type id
/// so a view can name them as `'static`, exactly as the graph's own do.
#[derive(Default)]
pub struct Interned {
    manifests: HashMap<String, &'static NodeManifest>,
}

/// One view rebuilt from data, borrowing the data and the manifests; [`Held::view`] reads it.
pub struct Held<'a> {
    data: &'a ViewData,
    manifests: HashMap<Uid, &'static NodeManifest>,
    edges: Vec<Edge>,
    vars: Vec<Vec<BoundVar>>,
}

impl Interned {
    /// The manifest of `type_id`: `own` first, so an engine's own types keep the manifest its
    /// library registered; else the described one, leaked the first time it is seen.
    pub fn manifest(&mut self, type_id: &str, describe: Option<&str>, own: &dyn Fn(&str) -> Option<&'static NodeManifest>) -> Option<&'static NodeManifest> {
        if let Some(m) = own(type_id) {
            return Some(m);
        }
        if let Some(m) = self.manifests.get(type_id) {
            return Some(m);
        }
        let intro = crate::parse_introspection(describe?).ok()?;
        let (_, bare) = crate::split_type_id(type_id);
        let m = crate::leak_manifest(bare.to_string(), &intro).ok()?;
        self.manifests.insert(type_id.to_string(), m);
        Some(m)
    }

    /// Resolve every node's manifest and every slot to the `'static` names those manifests hold.
    /// An edge or a node whose manifest is unknown is left out; the graph never sends one.
    pub fn hold<'a>(&mut self, data: &'a ViewData, own: &dyn Fn(&str) -> Option<&'static NodeManifest>) -> Held<'a> {
        let mut manifests = HashMap::new();
        for n in &data.nodes {
            if let Some(m) = self.manifest(&n.type_id, data.types.get(&n.type_id).map(String::as_str), own) {
                manifests.insert(n.uid, m);
            }
        }
        let output = |uid: Uid, slot: &str| manifests.get(&uid).and_then(|m| m.outputs.iter().find(|o| o.name == slot).map(|o| o.name));
        let input = |uid: Uid, slot: &str| manifests.get(&uid).and_then(|m| m.inputs.iter().find(|s| s.name == slot).map(|s| s.name));
        let edges = data
            .edges
            .iter()
            .filter_map(|e| {
                Some(Edge {
                    producer: (e.producer.0, output(e.producer.0, &e.producer.1)?),
                    consumer: (e.consumer.0, input(e.consumer.0, &e.consumer.1)?),
                })
            })
            .collect();
        let vars = data
            .nodes
            .iter()
            .flat_map(|n| n.bindings.iter())
            .map(|b| {
                b.vars
                    .iter()
                    .map(|v| match v {
                        VarData::Stream { var, producer, slot, event_id } => BoundVar::Stream {
                            var: var.clone(),
                            producer: *producer,
                            slot: output(*producer, slot).unwrap_or(""),
                            event_id: *event_id,
                        },
                        VarData::Value { var, value } => BoundVar::Value { var: var.clone(), value: value.clone() },
                        VarData::Missing { var, reason } => BoundVar::Missing { var: var.clone(), reason: reason.clone() },
                    })
                    .collect()
            })
            .collect();
        Held { data, manifests, edges, vars }
    }
}

impl<'a> Held<'a> {
    pub fn view(&'a self) -> GraphView<'a> {
        let mut vars = self.vars.iter();
        let nodes = self
            .data
            .nodes
            .iter()
            .filter_map(|n| {
                let manifest = *self.manifests.get(&n.uid)?;
                let bindings = n
                    .bindings
                    .iter()
                    .map(|b| BindingView {
                        key: &b.key,
                        rewritten: &b.rewritten,
                        vars: vars.next().map(Vec::as_slice).unwrap_or(&[]),
                        trigger: b.trigger,
                        id: b.id,
                        live: b.live,
                    })
                    .collect();
                Some((
                    n.uid,
                    NodeView {
                        engine: leak_str(&n.engine),
                        name: &n.name,
                        generation: n.generation,
                        rings: n.rings,
                        manifest,
                        params: &n.params,
                        bindings,
                        recorded: &n.recorded,
                    },
                ))
            })
            .collect();
        GraphView { instance: &self.data.instance, edges: &self.edges, nodes }
    }

    /// The touches with their slot names as the consumer's manifest spells them.
    pub fn touched(&self, touched: &[TouchedData]) -> Vec<Touched> {
        touched
            .iter()
            .filter_map(|t| match t {
                TouchedData::Slot(uid, slot) => {
                    let m = self.manifests.get(uid)?;
                    Some(Touched::Slot(*uid, m.inputs.iter().find(|s| s.name == slot.as_str())?.name))
                }
                TouchedData::Param(uid, key) => Some(Touched::Param(*uid, key.clone())),
                TouchedData::Record(uid) => Some(Touched::Record(*uid)),
            })
            .collect()
    }

    /// Every node of `engine` the view holds, with its bare type name and generation.
    pub fn own(&self, engine: &str) -> Vec<(Uid, &'static str, u64, &'a ParamGroups)> {
        self.data
            .nodes
            .iter()
            .filter(|n| n.engine == engine)
            .filter_map(|n| Some((n.uid, self.manifests.get(&n.uid)?.type_name, n.generation, &n.params)))
            .collect()
    }
}

/// Engine ids are a closed, tiny set; a name interned once is `'static` for good.
fn leak_str(s: &str) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(n) = names.iter().find(|n| **n == s) {
        return n;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    names.push(leaked);
    leaked
}

/// One scanned type as a child reports it: the outcome, and the schema of a registered one.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScannedData {
    pub type_name: String,
    pub stamp: Option<(u64, std::time::SystemTime)>,
    pub registered: Option<TypeData>,
    pub unavailable: Option<String>,
    pub replaced: bool,
}

/// One registered type: what the palette shows of it, by schema.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TypeData {
    pub type_name: String,
    pub describe: String,
    pub isolation: Isolation,
}

impl TypeData {
    pub fn of(m: &NodeManifest, isolation: Isolation) -> TypeData {
        TypeData { type_name: m.type_name.to_string(), describe: describe(m), isolation }
    }
}
