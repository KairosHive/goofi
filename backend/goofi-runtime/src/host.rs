//! The host executor: a node behind the host SDK's `Node`, run when its inputs or its own
//! schedule ask and held to its rate cap. Its params are the runtime's values, its faults are the
//! node's own, and what it emits is a frame the runtime puts on the wire.

use std::time::{Duration, Instant};

use goofi_core::{Data, Param};
use goofi_host_sdk::{Inputs, Node, NodeCtx, NodeError, NodeResult, Outputs};
use goofi_node::{NodeManifest, NodeStage, ParamDecl, ParamGroups, ParamKey, Params};
use indexmap::IndexMap;

use crate::{Cx, Executor, Fault, Out, RunPolicy, Ticked, COMMON};

/// How long a node waits between retries of a failed initialization — a free-running producer
/// would otherwise retry tens of times a second. Only a WAKE is paced: a param edit is a user asking.
const SETUP_RETRY: Duration = Duration::from_secs(1);

/// The smoothing factor of the `ufreq` EMA: how much the newest interval moves the estimate.
const UFREQ_EMA_ALPHA: f64 = 0.2;

/// How a node's instance is BUILT, deferred so the building happens on the node's own thread. A
/// Python node's construction executes its module, and `Graph::add_node` holds the graph mutex.
pub type NodeBuild = Box<dyn FnOnce(&ParamGroups) -> Box<dyn Node> + Send>;

fn guard<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(goofi_node::panic_message)
}

fn fold_panic(panicked: String) -> NodeResult {
    Err(NodeError(panicked))
}

/// One wire's cell on a `multi` slot: its service, its `node.slot` source, and its newest frame.
type MultiCells = Vec<(String, String, Option<Data>)>;

/// The `ufreq` meter's state: when this node last emitted, and the smoothed interval between emits.
#[derive(Default)]
struct UfreqMeter {
    last_emit: Option<f64>,
    ema: Option<f64>,
}

pub struct HostExecutor {
    manifest: &'static NodeManifest,
    decls: Vec<ParamDecl>,
    /// `None` when the build failed; the fault says why, and nothing runs.
    node: Option<Box<dyn Node>>,
    /// The live value per declared param, as the runtime last gave them.
    values: Vec<Param>,
    /// The same values as the node reads them, with the options a refresh re-enumerated.
    effective: ParamGroups,
    policy: RunPolicy,
    /// Something asked this node to run and it has not run since.
    trigger_pending: bool,
    last_run: Option<Instant>,
    /// Latest-wins input cells, one per declared single input slot.
    inputs: IndexMap<&'static str, Option<Data>>,
    /// Per-WIRE latest-wins cells for each `multi` input slot, in the order the runtime last gave
    /// — which IS `Inputs::get_multi`'s connection order.
    multi_wires: IndexMap<&'static str, MultiCells>,
    ctx: NodeCtx,
    /// The node's emit counter for `meta["index"]` — engine-owned, the node never sees it.
    emits: u64,
    meter: UfreqMeter,
    initialized: bool,
    last_attempt: Option<Instant>,
    fault: Option<Fault>,
}

impl HostExecutor {
    /// Build the node, seed its params and run `setup()`, on the thread that will run it.
    pub fn new(
        manifest: &'static NodeManifest,
        decls: Vec<ParamDecl>,
        build: NodeBuild,
        params: &ParamGroups,
    ) -> HostExecutor {
        let values: Vec<Param> = decls.iter().map(|d| crate::param_of(params, d)).collect();
        let mut effective = ParamGroups::new();
        for (d, v) in decls.iter().zip(&values) {
            effective.entry(d.group.to_string()).or_default().insert(d.name.to_string(), v.clone());
        }
        let (node, fault) = match guard(|| build(&effective)) {
            Ok(node) => (Some(node), None),
            Err(msg) => (None, Some(Fault::Process(msg))),
        };
        let mut exec = HostExecutor {
            manifest,
            decls,
            node,
            values,
            policy: RunPolicy::from_params(&effective),
            effective,
            trigger_pending: false,
            last_run: None,
            inputs: manifest.inputs.iter().filter(|s| !s.multi).map(|s| (s.name, None)).collect(),
            multi_wires: manifest.inputs.iter().filter(|s| s.multi).map(|s| (s.name, Vec::new())).collect(),
            ctx: NodeCtx::new(),
            emits: 0,
            meter: UfreqMeter::default(),
            initialized: false,
            last_attempt: None,
            fault,
        };
        exec.initialize();
        exec
    }

    /// When the cap next admits a run: one period after the last one. `None` means now.
    fn due(&self) -> Option<Instant> {
        Some(self.last_run? + Duration::from_secs_f64(self.policy.period()?))
    }

    /// The initialization gate: a node whose `setup()` failed is UNINITIALIZED, so nothing runs
    /// against it, and any interaction retries the initialization first. Answers whether it is up.
    fn ensure_initialized(&mut self) -> bool {
        if !self.initialized {
            self.initialize();
        }
        self.initialized
    }

    /// The same gate on a WAKE, which is not a user asking but one of however many the pacer
    /// admits — so the retry is paced, and every attempt restarts the window.
    fn ensure_initialized_paced(&mut self) -> bool {
        if self.initialized {
            return true;
        }
        if self.last_attempt.is_some_and(|t| t.elapsed() < SETUP_RETRY) {
            return false;
        }
        self.ensure_initialized()
    }

    /// The param replay and `setup()` together, which are one unit — a retry re-runs both. A
    /// panic in either is the node's boot error, never an unwind through the caller's lock.
    fn initialize(&mut self) {
        let Some(node) = self.node.as_mut() else { return };
        self.last_attempt = Some(Instant::now());
        let mut last_error = None;
        for (group, entries) in &self.effective {
            if group == COMMON {
                continue;
            }
            for (name, value) in entries {
                // A pulse is a request, so there is no value to replay and nothing it ever changed.
                if matches!(value, Param::Pulse) {
                    continue;
                }
                let key = ParamKey::new(group.as_str(), name.as_str());
                if let Err(e) = guard(|| node.on_param_changed(&key, value)).unwrap_or_else(fold_panic) {
                    last_error.get_or_insert(e.0);
                }
            }
        }
        let started = guard(|| node.setup(&mut self.ctx, &Params::new(&self.effective))).unwrap_or_else(fold_panic);
        if let Err(e) = started {
            last_error.get_or_insert(e.0);
        }
        match last_error {
            None => {
                self.initialized = true;
                self.fault = None;
            }
            Some(msg) => self.fault = Some(Fault::Setup(msg)),
        }
    }

    /// Run the param hook, recording a rejection or a panic as that param's error — a node is
    /// third-party code and its hooks run where a panic would otherwise escape the loop.
    fn on_param_changed(&mut self, key: &ParamKey, value: &Param, ticked: &mut Ticked) {
        let Some(node) = self.node.as_mut() else { return };
        let result = guard(|| node.on_param_changed(key, value)).unwrap_or_else(fold_panic);
        ticked.errors.push((key.clone(), result.err().map(|e| e.0)));
    }

    /// A `required` input slot holding no frame, or `None`. A slot wired to a producer that has
    /// emitted nothing reads the same as an unwired one.
    fn missing_required(&self) -> Option<&'static str> {
        self.manifest.inputs.iter().filter(|s| s.required).find_map(|slot| {
            let absent = if slot.multi {
                self.multi_wires.get(slot.name).is_none_or(|c| !c.iter().any(|(_, _, f)| f.is_some()))
            } else {
                self.inputs.get(slot.name).and_then(Option::as_ref).is_none()
            };
            absent.then_some(slot.name)
        })
    }

    /// The present frames on each `multi` slot, in wire order — absent wires dropped, so a node
    /// sees only the frames that actually arrived.
    fn materialize_multis(&self) -> goofi_host_sdk::MultiFrames {
        self.multi_wires
            .iter()
            .map(|(slot, cells)| {
                (*slot, cells.iter().filter_map(|(_, source, f)| f.clone().map(|d| (source.clone(), d))).collect())
            })
            .collect()
    }

    fn process(&mut self, cx: &Cx<'_>, publish: &mut dyn FnMut(usize, Out<'_>)) -> Option<f64> {
        if let Some(slot) = self.missing_required() {
            self.fault = Some(Fault::Process(format!("required input slot `{slot}` has no data")));
            return None;
        }
        self.ctx.take_cleared_inputs();
        self.ctx.now = cx.now;
        let multis = self.materialize_multis();
        let mut outputs = self.manifest.output_buffer();
        let node = self.node.as_mut()?;
        let result = {
            let inputs = Inputs::with_multi(&self.inputs, &multis);
            let params = Params::new(&self.effective);
            let mut out = Outputs::new(&mut outputs);
            guard(|| node.process(&inputs, &mut out, &mut self.ctx, &params)).unwrap_or_else(fold_panic)
        };
        let clears = self.ctx.take_cleared_inputs();
        let result = result.and_then(|()| {
            for slot in self.manifest.outputs {
                let Some(Some(frame)) = outputs.get(slot.name) else { continue };
                match (slot.kind, frame.value()) {
                    (goofi_core::SlotType::Texture, goofi_core::Value::Texture(texture)) => texture.validate()?,
                    (goofi_core::SlotType::Texture, _) => return Err(NodeError("TEXTURE output requires a texture submission".into())),
                    (_, goofi_core::Value::Texture(_)) => return Err(NodeError("texture submissions require a TEXTURE output".into())),
                    _ => {}
                }
            }
            if let Some(name) = clears.iter().find(|name| !self.manifest.inputs.iter().any(|slot| slot.name == *name)) {
                return Err(NodeError(format!("no input slot `{name}`")));
            }
            for name in clears {
                if let Some(cell) = self.inputs.get_mut(name.as_str()) {
                    *cell = None;
                }
                if let Some(cells) = self.multi_wires.get_mut(name.as_str()) {
                    for (_, _, cell) in cells {
                        *cell = None;
                    }
                }
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                // The engine's own meta goes on before anything leaves the node — there is no
                // second stamping site.
                let ufreq = stamp_meta(&mut outputs, cx.now, &mut self.emits, &mut self.meter);
                for (i, (_, frame)) in outputs.iter().enumerate() {
                    if let Some(frame) = frame {
                        publish(i, Out::Frame(frame));
                    }
                }
                self.fault = None;
                ufreq
            }
            Err(e) => {
                self.fault = Some(Fault::Process(e.0));
                None
            }
        }
    }
}

impl Executor for HostExecutor {
    fn arrive(&mut self, inbox: usize, wire: usize, frame: &Data) -> bool {
        let Some(decl) = self.manifest.inputs.get(inbox) else { return false };
        if decl.multi {
            // A wire index the last set does not name is dropped rather than appended: appending
            // would put the frame where `Inputs::get_multi` reads another producer.
            match self.multi_wires.get_mut(decl.name).and_then(|cells| cells.get_mut(wire)) {
                Some(cell) => cell.2 = Some(frame.clone()),
                None => return false,
            }
        } else {
            self.inputs.insert(decl.name, Some(frame.clone()));
        }
        if decl.trigger_process {
            self.trigger_pending = true;
        }
        false
    }

    /// A surviving wire keeps its frame, and a wire that left takes its frame with it; a single
    /// slot with no wire left holds no frame, or a node keeps running on a producer it left.
    fn rewire(&mut self, inbox: usize, wires: &[(String, String)]) {
        let Some(decl) = self.manifest.inputs.get(inbox) else { return };
        if let Some(cells) = self.multi_wires.get_mut(decl.name) {
            let mut previous = std::mem::take(cells);
            *cells = wires
                .iter()
                .map(|(service, source)| {
                    let held = previous.iter_mut().find(|(name, _, _)| name == service).and_then(|(_, _, frame)| frame.take());
                    (service.clone(), source.clone(), held)
                })
                .collect();
        } else if wires.is_empty() {
            self.inputs.insert(decl.name, None);
        }
    }

    /// A moved `common.*` value re-paces; any other reaches the node's hook — through the retry's
    /// replay on an uninitialized node, which hears nothing else.
    fn params_changed(&mut self, values: &[Param], trigger: bool) -> Ticked {
        let mut ticked = Ticked::default();
        let mut moved = Vec::new();
        let mut repace = false;
        for (i, (d, value)) in self.decls.iter().zip(values).enumerate() {
            if self.values.get(i) == Some(value) {
                continue;
            }
            self.effective.entry(d.group.to_string()).or_default().insert(d.name.to_string(), value.clone());
            if d.group == COMMON {
                repace = true;
            } else {
                moved.push((ParamKey::new(d.group, d.name), value.clone()));
            }
        }
        self.values = values.to_vec();
        if repace {
            self.policy = RunPolicy::from_params(&self.effective);
        }
        if trigger {
            self.trigger_pending = true;
        }
        if moved.is_empty() {
            return ticked;
        }
        // A param write is an INTERACTION, and an interaction retries the initialization first.
        // `initialize` replays the whole record, so a retry that succeeded has delivered the edit.
        let was = self.initialized;
        if self.ensure_initialized() && was {
            for (key, value) in &moved {
                self.on_param_changed(key, value, &mut ticked);
            }
        }
        ticked
    }

    fn pulse(&mut self, param: usize) -> Ticked {
        let mut ticked = Ticked::default();
        let Some(d) = self.decls.get(param) else { return ticked };
        let key = ParamKey::new(d.group, d.name);
        if self.ensure_initialized() {
            let params = Params::new(&self.effective);
            let node = self.node.as_mut().expect("initialized");
            let fired = guard(|| node.on_pulse(&key, &params)).unwrap_or_else(fold_panic);
            ticked.errors.push((key, fired.err().map(|e| e.0)));
        }
        ticked
    }

    /// Re-enumerate a refreshable `Str` param's options: the node's own copy moves too, so its
    /// next build reads fresh. An interaction retries the initialization first, so a picker whose
    /// node failed `setup()` rescans as soon as that node comes up.
    fn refresh(&mut self, param: usize) -> Option<Vec<String>> {
        let d = *self.decls.get(param)?;
        let key = ParamKey::new(d.group, d.name);
        if !self.ensure_initialized() {
            return None;
        }
        let params = Params::new(&self.effective);
        let node = self.node.as_mut()?;
        let options = guard(|| node.on_param_refreshed(&key, &params)).unwrap_or(None)?;
        if let Some(Param::Str { options: slot, .. }) = self.effective.get_mut(d.group).and_then(|g| g.get_mut(d.name)) {
            *slot = Some(options.clone());
        }
        Some(options)
    }

    /// An autotriggering node always wants to run; any other when something triggered it; both
    /// are held to the rate cap, read fresh so a cap edited mid-park takes effect on the spot.
    /// `last_run` is in the past, so answering it is "due now".
    fn next_wake(&self, last_run: Instant) -> Option<Instant> {
        (self.policy.autotrigger || self.trigger_pending).then(|| self.due().unwrap_or(last_run))
    }

    fn run(&mut self, cx: &Cx<'_>, publish: &mut dyn FnMut(usize, Out<'_>)) -> Ticked {
        // The frame that asked for this run has been seen; holding it would fire the node twice
        // the moment it recovers. Consumed whether or not the run itself gets as far as `process`.
        self.trigger_pending = false;
        self.last_run = Some(Instant::now());
        let mut ticked = Ticked::default();
        if self.ensure_initialized_paced() {
            ticked.ufreq = self.process(cx, publish);
        }
        ticked
    }

    fn fault(&self) -> Option<Fault> {
        self.fault.clone()
    }

    fn stage(&self) -> NodeStage {
        if self.initialized { NodeStage::Ready } else { NodeStage::Setup }
    }
}

/// Stamp the engine-owned meta on every frame just emitted, and answer the node's measured rate.
/// `time` and `index` are this run's — patch seconds, and the node's count of emits, so a gap in
/// the index means a frame was LOST. `ufreq` is the node's own EMA, never inherited.
fn stamp_meta(
    outputs: &mut IndexMap<&'static str, Option<Data>>,
    now: f64,
    emits: &mut u64,
    meter: &mut UfreqMeter,
) -> Option<f64> {
    // Nothing emitted → no meta to stamp, and the meter only advances on a productive emit.
    if outputs.values().all(|o| o.is_none()) {
        return None;
    }
    // EMA of the inter-emit interval, inverted. `None` until the second emit; a non-advancing
    // clock (`dt <= 0`) keeps the prior estimate.
    let node_ufreq = match meter.last_emit {
        None => {
            meter.last_emit = Some(now);
            None
        }
        Some(prev) => {
            let dt = now - prev;
            meter.last_emit = Some(now);
            if dt > 0.0 {
                let ema = meter.ema.map_or(dt, |p| UFREQ_EMA_ALPHA * dt + (1.0 - UFREQ_EMA_ALPHA) * p);
                meter.ema = Some(ema);
                Some(1.0 / ema)
            } else {
                meter.ema.map(|e| 1.0 / e)
            }
        }
    };
    let index = *emits;
    *emits += 1;
    for slot_opt in outputs.values_mut() {
        let Some(d) = slot_opt else { continue };
        *d = d.with_stamps(now, index, node_ufreq);
    }
    node_ufreq
}
