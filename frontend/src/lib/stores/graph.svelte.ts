/** Central reactive graph state, backed by the control WS. The store owns the only writes, so a
 * component just reads its `$state` fields. */
import type { ParamDescriptor, VideoQuality } from '$lib/api/types';
import {
	getControl,
	type Control,
	type ControlEvent,
	type DemoExample,
	type GraphSnapshot,
	type LinkInfo,
	type NodeInstanceInfo,
	type NodeTypeInfo,
	type RecordStatus,
	type Step
} from '$lib/api/control';
import type { OpName } from '$lib/api/ops';
import { boundaryType, feeds, type SlotDtype } from '$lib/api/vocab';
import { bareName } from '$lib/editor/typeId';
import { consoleStore } from './console.svelte';
import { selection } from './selection.svelte';
import { workspace } from 'panelty';
import type { SlotView } from '$lib/viewers/inlineView';
import { history } from './history.svelte';
import { SyncClient } from '$lib/crdt/syncClient.svelte';
import type { Op } from '$lib/crdt/ops';
import {
	linkViews,
	nodesMap,
	facadeFaces,
	recordedSlots,
	variableViews,
	variableGroupLocks,
	controlGroups,
	machineGroups,
	midiGroups,
	machineViews,
	arrangementTabs,
	type Doc,
	type VariableView,
	type ControlView,
	type LockView
} from '$lib/crdt/graphDoc';
import type { Literal, Machine, Midi } from '$lib/api/generated';
import { KIND, type Cell } from '$lib/panels/controlLayout';
import { liveNode, type RuntimeOverlay, type ViewSources } from '$lib/crdt/liveNode.svelte';
import { ParamLive, type LiveSource } from '$lib/api/paramLive';
import type { SourcePatch } from '$lib/api/types';
import type { GraphFragment } from '$lib/editor/clipboard';

/** Lift a ⟳ spinner after this long when a node never reports the refresh done. Generous: an
 * LSL resolve blocks the node's ctrl thread ~4s. */
const REFRESH_SPINNER_TIMEOUT_MS = 15000;

/** Stable key for an in-flight param refresh; U+001F cannot occur in a uid/group/name. */
function refreshKey(node: string, group: string, name: string): string {
	return `${node}\u001f${group}\u001f${name}`;
}

/** Take `held` to `next`, a two-level map, leaf by leaf: a leaf `next` lacks is deleted, and one it
 * carries unchanged is left alone — which is what keeps its readers asleep. */
function follow<T>(held: Record<string, Record<string, T>>, next: Record<string, Record<string, T>>): Record<string, Record<string, T>> {
	for (const group of Object.keys(held)) if (!(group in next)) delete held[group];
	for (const [group, names] of Object.entries(next)) {
		const g = (held[group] ??= {});
		for (const name of Object.keys(g)) if (!(name in names)) delete g[name];
		for (const [name, v] of Object.entries(names)) g[name] = v;
	}
	return held;
}

/** A doc link as the wire ops spell it: two `uid/slot` endpoints. */
function linkEndpoints(link: LinkInfo): { from: string; to: string } {
	return {
		from: `${link.node_out}/${link.slot_out}`,
		to: `${link.node_in}/${link.slot_in}`
	};
}

const IDLE_RECORD: RecordStatus = {
	running: false,
	folder: null,
	elapsed: null,
	streams: [],
	error: null
};

/** The `machine` phrase's ops, every one the machine panel may speak. */
export type MachineOp = Extract<OpName, `machine ${string}`>;

export class GraphStore {
	nodeTypes = $state.raw<NodeTypeInfo[] | null>(null);

	/** One live view per node the document holds, in document order. Each is the one object its
	 * uid answers with; its fields read the document, the catalog and the runtime overlay. */
	nodes: NodeInstanceInfo[] = $derived(Object.keys(nodesMap(this.doc)).flatMap((uid) => this._views.get(uid) ?? []));
	/** The live views, by uid — made as a node enters the document, dropped as it leaves. */
	private _views = new Map<string, NodeInstanceInfo>();
	/** What the runtime planes reported per node the document holds. */
	private _rt = $state<Record<string, RuntimeOverlay>>({});
	/** The same, for a node the runtime planes named before the doc held it (their channels have
	 * no order against the doc). The node's view takes it when it is made. */
	private _stash: Record<string, RuntimeOverlay> = {};
	private _byType = $derived(new Map((this.nodeTypes ?? []).map((t) => [t.type, t])));
	private _faces = $derived(facadeFaces(this.doc));
	private _sources: ViewSources = {
		doc: () => this.doc,
		catalog: (type) => this._byType.get(type),
		face: (uid) => this._faces.get(uid),
		runtime: (uid) => this._rt[uid]
	};
	links: LinkInfo[] = $derived(linkViews(this.doc));
	savePath = $state<string | null>(null);
	unsavedChanges = $state(false);
	/** What the server said it is. Every affordance a demo withholds reads this. */
	demo = $state(false);
	/** The public set this instance belongs to, empty everywhere else. */
	examples = $state<DemoExample[]>([]);
	connected = $state(false);
	/** Latches on the first connect and never clears — see {@link disconnected}. */
	private _everConnected = $state(false);
	/** The startup hook: counts the server sessions this page saw, bumped on the first hello from
	 * a new manager. Never on page load or on a transient reconnect; 0 until the first hello. */
	sessionEpoch = $state(0);

	/** Every armed output slot, doc-authoritative: the document is the one owner of what is armed. */
	armed: { uid: string; slot: string; quality: VideoQuality }[] = $derived(recordedSlots(this.doc));

	/** The recording SESSION, as the backend last reported it. Pushed, never derived here. */
	record = $state<RecordStatus>(IDLE_RECORD);
	/** The recording's elapsed time as `mm:ss`. */
	recordClock = $derived.by(() => {
		const t = Math.max(0, Math.floor(this.record.elapsed ?? 0));
		return `${String(Math.floor(t / 60)).padStart(2, '0')}:${String(t % 60).padStart(2, '0')}`;
	});

	/** The streams whose drop count MOVED on the last report, keyed `node/slot`. */
	dropping = $state.raw<ReadonlySet<string>>(new Set());

	/** What each stream's cumulative `dropped` was on the last report. */
	private _droppedCounts: Record<string, number> = {};

	/** Patch variables (system + user), doc-authoritative, in system-first/creation order. */
	variables: VariableView[] = $derived(variableViews(this.doc));
	/** Every variable group that carries a lock, by name. */
	variableGroups: Record<string, LockView> = $derived(variableGroupLocks(this.doc));
	/** The groups a control panel may draw, in document order. */
	controlGroups: string[] = $derived(controlGroups(this.doc));
	/** Every group that reads a MIDI device, by name. */
	midiGroups: Record<string, Midi> = $derived(midiGroups(this.doc));
	machineGroups: Record<string, string> = $derived(machineGroups(this.doc));
	/** Every state machine, by name, doc-authoritative. */
	machines: Record<string, Machine> = $derived(machineViews(this.doc));

	/** Bumps on every WHOLESALE graph load, never on an incremental add/remove; editors re-fit on it. */
	loadEpoch = $state(0);
	/** The document version the last wholesale load reached. */
	private _loadVersion = $state(0);
	/** Whether the replica holds the loaded document yet: the snapshot and the doc delta cross in
	 * no fixed order, so a fit armed by the epoch waits for this. */
	get loadSettled(): boolean {
		return this._sync.version >= this._loadVersion;
	}

	/** Params with a ⟳ refresh in flight → its safety-timeout handle. Cleared when the node reports
	 * the param done (`refreshed_params`), never on the fire-and-forget RPC ack. */
	private _refreshing = $state<Record<string, ReturnType<typeof setTimeout>>>({});

	/** instance_id of the manager we last hydrated from; a change is a fresh session, not a reconnect. */
	private _lastInstanceId: string | null = null;

	private ctl: Control;
	/** The browser replica of the manager's control-plane document. */
	private _sync: SyncClient;

	/** The connection was ESTABLISHED and is now gone. Not `!connected`: at boot that would alarm
	 * for the few hundred ms before the socket first opens. */
	get disconnected(): boolean {
		return this._everConnected && !this.connected;
	}

	constructor(ctl: Control = getControl()) {
		this.ctl = ctl;
		ctl.onConnect((c) => {
			this.connected = c;
			if (c) this._everConnected = true;
		});
		ctl.on((ev) => this._handle(ev));
		this._sync = new SyncClient(ctl, (ops) => this._syncFromDoc(ops));
	}

	get doc(): Doc {
		return this._sync.doc;
	}

	/** Whether the replica has pulled from the manager yet; until then doc reads describe nothing. */
	get docSynced(): boolean {
		return this._sync.synced;
	}

	/** Follow the document's membership: a view per node that entered, none for one that left.
	 * Everything else a view shows is read live, so nothing here re-derives it. */
	private _syncFromDoc(ops: Op[] | null): void {
		const doc = this._sync.doc;
		// The workspace store rebuilds its tree from this; the client holds no second copy.
		if (!ops || ops.some((o) => o.path[0] === 'arrangement')) workspace().syncFromDoc(arrangementTabs(doc));
		const held = nodesMap(doc);
		for (const uid of this._views.keys()) {
			if (!(uid in held)) {
				this._views.delete(uid);
				delete this._rt[uid];
			}
		}
		for (const uid of Object.keys(held)) {
			if (this._views.has(uid)) continue;
			// The view takes whatever the runtime planes said of this uid before the doc held it,
			// once: a later node at the same uid starts fresh.
			const early = this._stash[uid];
			delete this._stash[uid];
			this._rt[uid] = early ?? {};
			this._views.set(uid, liveNode(uid, this._sources));
		}
	}

	/** The runtime overlay to write for `uid`: the node's own while the document holds it, else the
	 * stash its view takes when it is made. */
	private _runtimeOf(uid: string): RuntimeOverlay {
		if (this._views.has(uid)) return (this._rt[uid] ??= {});
		return (this._stash[uid] ??= {});
	}

	/** Apply a wholesale snapshot, returning whether it came from a NEW backend session — which is
	 * what a same-session reconnect must not look like. */
	private _replaceSnapshot(snap: GraphSnapshot): boolean {
		// A `hello` always carries the palette; `graph_replaced` never does — the `node_types` event
		// is what re-announces it there.
		if (snap.node_types?.length) this.nodeTypes = snap.node_types;
		// Snapshot and doc delta cross in no order: the overlay lands on the node's own entry or on
		// the stash. A new session's nodes are all still to come, so its whole overlay is stashed.
		const freshSession = snap.instance_id !== this._lastInstanceId;
		this._stash = {};
		for (const [uid, rt] of Object.entries(snap.runtime ?? {})) {
			const own = !freshSession && this._views.has(uid) ? (this._rt[uid] ??= {}) : (this._stash[uid] ??= {});
			own.stage = rt.stage;
			own.error = rt.error ?? null;
			own.runtime = rt.runtime;
		}
		this._setRecord(snap.record);
		this.savePath = snap.save_path;
		this.unsavedChanges = snap.unsaved_changes;
		// `hello` alone carries it; a `graph_replaced` snapshot must not clear what the mode is.
		if (snap.demo !== undefined) this.demo = snap.demo;
		if (snap.examples !== undefined) this.examples = snap.examples;

		// The arrangement rides the doc; what the snapshot carries is the VIEWPOINT, this client's
		// alone — persisted, never converged.
		this._lastInstanceId = snap.instance_id;
		if (snap.viewpoint != null) workspace().restoreViewpoint(snap.viewpoint);
		return freshSession;
	}

	/** Drop every projection assembled from the OUTGOING document. Reconciliation runs only from the
	 * doc observer, and a fresh manager with an EMPTY graph answers our SV with no change at all. */
	private _resetProjection(): void {
		this._views.clear();
		this._rt = {};
		// `_stash` is NOT cleared: `_replaceSnapshot` ran first, so it already holds the INCOMING
		// session's overlay. The arrangement store is a separate singleton, so it is here.
		workspace().syncFromDoc([]);
	}

	private _onWholesaleLoad(docVersion: number): void {
		this.loadEpoch += 1;
		this._loadVersion = docVersion;
		// A wholesale load mints new uids and clears the manager's history, so a kept client entry
		// would pop against a command that is not there. A same-session reconnect never comes here.
		history().reset();
		selection().forgetAll();
	}

	/** Write ONE slot's inline view, merging into the node's blob. The kind stored is the user's RAW
	 * pick. A facade and a boundary port take the same op a leaf does — each is a node here. */
	setSlotView(uid: string, slot: string, view: SlotView): void {
		const node = this.nodeById(uid);
		if (!node?.output_slots[slot]) return;
		void this.ctl
			.call('node edit', { node: uid, viewer: [{ slot, ...view }] })
			.catch(() => {
				/* soft view state — the next edit re-sends it */
			});
	}

	private _handle(ev: ControlEvent): void {
		switch (ev.event) {
			case 'hello': {
				// Not wholesale: a `hello` is also what a transient reconnect delivers.
				if (this._replaceSnapshot(ev.payload)) {
					// A new session mints uids from 1 again, so the stale replica falls now, before
					// this connection answers the server's binary hello SV. Projections first.
					this._resetProjection();
					this._sync.reset();
					this._onWholesaleLoad(ev.payload.doc_version);
					this.sessionEpoch += 1;
				}
				// After the reset: a reloaded tab keeps its actor, and the manager still holds its steps.
				if (ev.payload.history) history().adopt(ev.payload.history);
				break;
			}
			case 'graph_replaced':
				this._replaceSnapshot(ev.payload);
				this._onWholesaleLoad(ev.payload.doc_version);
				break;
			case 'state_update': {
				const t = this._runtimeOf(ev.payload.node);
				// Only the refreshed options are the runtime's. Not the param error: an echo predates
				// the re-evaluation, so it carries the previous source's failure.
				for (const [group, names] of Object.entries(ev.payload.params ?? {})) {
					for (const [name, desc] of Object.entries(names)) {
						const d = desc as { options?: string[] | null };
						if (d.options !== undefined) ((t.options ??= {})[group] ??= {})[name] = d.options ?? null;
					}
				}
				if (ev.payload.stage) t.stage = ev.payload.stage;
				// The state plane re-pushes the current error, so backend truth wins here even
				// when no diff-driven `error` event fired.
				if ('error' in ev.payload) t.error = ev.payload.error ?? null;
				if (ev.payload.runtime !== undefined) t.runtime = ev.payload.runtime ?? undefined;
				// Lift each spinner exactly when the fresh options land. Keyed by node, not by `t`.
				for (const [group, name] of ev.payload.refreshed_params ?? []) {
					this._endRefresh(refreshKey(ev.payload.node, group, name));
				}
				break;
			}
			case 'node_stage': {
				// The stage plane pushes each transition ONCE, so one outrun by its node's own doc
				// delta lands on the stash its view takes, never dropped.
				const t = this._runtimeOf(ev.payload.node);
				t.stage = ev.payload.stage;
				if (ev.payload.error !== undefined) t.error = ev.payload.error ?? null;
				// The tier arrives here as well as on the snapshot: a node added after connecting
				// is in no snapshot, and a GIL demotion moves it while the session is live.
				if (ev.payload.runtime !== undefined) t.runtime = ev.payload.runtime ?? undefined;
				break;
			}
			case 'node_stats':
				for (const [uid, stats] of Object.entries(ev.payload.stats)) {
					const t = this._rt[uid];
					if (t && t.stats?.updates_per_second !== stats.updates_per_second) t.stats = stats;
				}
				break;
			case 'param_values':
				for (const [uid, live] of Object.entries(ev.payload.nodes)) this.applyLiveSource(uid, live);
				break;
			case 'logs':
				consoleStore().apply(ev.payload);
				break;
			case 'error':
				// A REPORT, so only a node that RUNS raises one — a facade's health rides `node_stage`.
				this._runtimeOf(ev.payload.node).error = ev.payload.error;
				break;
			case 'unsaved_changes':
				this.unsavedChanges = ev.payload.unsaved_changes;
				break;
			case 'record_changed':
				this._setRecord(ev.payload);
				break;
			case 'save_path_changed':
				this.savePath = ev.payload.save_path;
				break;
			case 'node_types':
				this.nodeTypes = ev.payload.types;
				break;
		}
	}

	/** Adopt a recording report. Which streams are DROPPING is the counts that moved since the
	 * last one: `dropped` is cumulative, so it never falls and cannot answer that on its own. */
	private _setRecord(status: RecordStatus): void {
		const before = this._droppedCounts;
		const now: Record<string, number> = {};
		const moved = new Set<string>();
		for (const s of status.streams) {
			const key = `${s.node}/${s.slot}`;
			now[key] = s.dropped;
			if (s.dropped > (before[key] ?? s.dropped)) moved.add(key);
		}
		this._droppedCounts = now;
		this.dropping = moved;
		this.record = status;
	}

	/** Arm one output slot, as `node/slot`. It works while a recording runs. The manager records
	 * no command when the slot is already armed, so neither does the history. */
	async armSlot(node: string, slot: string, step?: Step): Promise<void> {
		await this.ctl.call('record arm', { output: `${node}/${slot}` }, step);
	}

	async setRecordQuality(node: string, slot: string, quality: VideoQuality): Promise<void> {
		await this.ctl.call('record quality', { output: `${node}/${slot}`, quality });
	}

	async disarmSlot(node: string, slot: string): Promise<void> {
		await this.ctl.call('record disarm', { output: `${node}/${slot}` });
	}

	/** Start a recording. Both fields fall back to the `record.*` variables in the backend. */
	async startRecording(name: string, root: string): Promise<string> {
		const r = await this.ctl.call<{ folder: string }>('record start', { name, root });
		return r?.folder ?? '';
	}

	async stopRecording(): Promise<string> {
		const r = await this.ctl.call<{ folder: string }>('record stop', {});
		return r?.folder ?? '';
	}

	async addNode(type: string, pos: [number, number], instId?: string, step?: Step): Promise<string> {
		const born = await this.ctl.call<{ uid: string }>('node add', { type, pos, inst_id: instId }, step);
		return born?.uid ?? '';
	}

	async removeNode(uid: string, step?: Step): Promise<void> {
		await this.ctl.call('node remove', { node: uid }, step);
	}

	/** Respawn a node in place, keeping its uid, name, params, position, scope and links. A
	 * recovery action rather than an edit, so it records no history. */
	async restartNode(uid: string): Promise<void> {
		await this.ctl.call('node restart', { node: uid });
	}

	async addLink(link: LinkInfo, step?: Step): Promise<void> {
		await this.ctl.call('link add', linkEndpoints(link), step);
	}

	async removeLink(link: LinkInfo, step?: Step): Promise<void> {
		await this.ctl.call('link remove', linkEndpoints(link), step);
	}

	/** One step of a drag on a param: the value moves everywhere, the history keeps nothing yet. */
	previewParam(node: string, group: string, name: string, value: unknown): void {
		this.ctl.preview(`param ${node} ${group}/${name}`, 'node param edit', { node, param: `${group}/${name}`, value });
	}

	/** Send a param op on `group/name`; a guarded call refuses a param the node does not hold. */
	private _paramCall(op: OpName, node: string, group: string, name: string, extra: Record<string, unknown>, guard = true): Promise<unknown> {
		// `name[i]` addresses one element of a vector param; the param it belongs to must exist.
		if (guard && !this.nodeById(node)?.params?.[group]?.[name.replace(/\[\d+\]$/, '')])
			return Promise.reject(new Error(`${op}: no param ${group}.${name} on node ${node}`));
		return this.ctl.call(op, { node, param: `${group}/${name}`, ...extra });
	}

	async updateParam(node: string, group: string, name: string, value: unknown): Promise<void> {
		await this._paramCall('node param edit', node, group, name, { value });
	}

	/** The slots of a list section as they are stored: each member's whole source, by base and slot. */
	private _listSlots(node: string, group: string, section: string): { count: string; bases: Map<string, ParamDescriptor[]> } | null {
		const params = this.nodeById(node)?.params?.[group];
		if (!params) return null;
		let count: string | null = null;
		const bases = new Map<string, ParamDescriptor[]>();
		for (const [name, d] of Object.entries(params)) {
			const role = d.role;
			if (role?.as === 'count' && role.section === section) count = name;
			if (role?.as === 'member' && role.section === section && role.slot != null) {
				const held = bases.get(role.base) ?? [];
				held[role.slot] = d;
				bases.set(role.base, held);
			}
		}
		return count ? { count, bases } : null;
	}

	/** Re-store a list so slot `k` holds what slot `order[k]` held, as ONE undo step; `count` sets
	 * how many are open. A slot past the order keeps what it has. */
	private async _restoreSlots(node: string, group: string, section: string, order: number[], count?: number): Promise<void> {
		const list = this._listSlots(node, group, section);
		if (!list) throw new Error(`no list ${group}/${section} on node ${node}`);
		const ops: { op: string; payload: Record<string, unknown> }[] = [];
		for (const [base, slots] of list.bases) {
			order.forEach((from, to) => {
				const d = slots[from];
				if (from === to || !d) return;
				ops.push({
					op: 'node param edit',
					payload: {
						node,
						param: `${group}/${base}_${to}`,
						value: d.value,
						expression: d.expression ?? '',
						mode: d.mode ?? 'constant'
					}
				});
			});
		}
		if (count !== undefined) ops.push({ op: 'node param edit', payload: { node, param: `${group}/${list.count}`, value: count } });
		if (ops.length === 0) return;
		await this.ctl.call('compound', { ops });
	}

	/** Move the slot at `from` so it sits at `to`, the slots between shifting one step. */
	async moveListSlot(node: string, group: string, section: string, from: number, to: number): Promise<void> {
		if (from === to) return;
		const n = Math.max(from, to) + 1;
		const order = Array.from({ length: n }, (_, i) => i);
		order.splice(to, 0, ...order.splice(from, 1));
		await this._restoreSlots(node, group, section, order);
	}

	/** Close the slot at `index`: the slots after it move up one, and the list is one shorter. */
	async removeListSlot(node: string, group: string, section: string, index: number): Promise<void> {
		const list = this._listSlots(node, group, section);
		const held = list && this.nodeById(node)?.params?.[group]?.[list.count];
		const count = held && typeof held.value === 'number' ? held.value : 0;
		if (!list || index >= count) return;
		const order = Array.from({ length: count }, (_, i) => (i < index ? i : i + 1));
		await this._restoreSlots(node, group, section, order, count - 1);
	}

	/** Add a NEW user variable; the server refuses a name the patch already holds. A `control` makes
	 * it a control-panel element. */
	async addVariable(name: string, value: Literal, control?: ControlView): Promise<void> {
		if (this.variables.some((g) => g.name === name)) throw new Error(`variable ${name} already exists`);
		await this.ctl.call('variable entry add', control ? { name, value, control } : { name, value });
	}

	previewVariableValue(name: string, value: Literal): void {
		this.ctl.preview(`variable ${name}`, 'variable entry edit', { name, value });
	}

	async setVariableValue(name: string, value: Literal): Promise<void> {
		if (!this.variables.some((g) => g.name === name)) throw new Error(`no variable ${name}`);
		await this.ctl.call('variable entry edit', { name, value });
	}

	async addVariableEntry(group: string): Promise<string> {
		const result = await this.ctl.call('variable entry add', { group }) as { name: string };
		return result.name;
	}

	async addVariableGroup(group?: string): Promise<string> {
		const result = await this.ctl.call('variable group add', group ? { group } : {}) as { group: string };
		return result.group;
	}

	/** Delete a group with every variable in it, as one step. */
	async removeVariableGroup(group: string): Promise<void> {
		await this.ctl.call('variable group remove', { group });
	}

	async removeVariable(name: string): Promise<void> {
		await this.ctl.call('variable entry remove', { name });
	}

	/** Rename a user variable; every expression that reads it is rewritten by the manager. */
	async renameVariable(oldName: string, newName: string): Promise<void> {
		if (!this.variables.some((g) => g.name === oldName)) throw new Error(`no variable ${oldName}`);
		await this.ctl.call('variable entry rename', { name: oldName, to: newName });
	}

	async renameVariableGroup(from: string, to: string): Promise<void> {
		await this.ctl.call('variable group rename', { from, to });
	}

	/** Start or end a MIDI learn: every port the host lists opens as a group for `seconds`, and
	 * the ones nothing reads close again when it ends. */
	learnMidi(on: boolean): Promise<{ groups: { group: string; port: string }[]; seconds: number }> {
		return this.ctl.call('midi learn', { on });
	}

	/** Bear a widget in a control panel's group through the `control` door, which lifts the
	 * group's lock for the one command: the manager mints the name and the cell when none is given. */
	async addControl(group: string, kind: ControlView['kind'], cell?: Cell): Promise<string> {
		const r = await this.ctl.call('control add', { group, kind, ...(cell ?? {}) });
		return String((r as { name?: unknown }).name ?? '');
	}

	/** Change a widget's name, kind, range, options or place, through the `control` door. */
	async editControl(group: string, element: string, patch: Partial<ControlView> & { name?: string }): Promise<void> {
		await this.ctl.call('control edit', { group, element, ...patch });
	}

	/** Append drawing ops, in their text form, to a `paint` widget: one undoable edit. */
	async paintControl(group: string, element: string, ops: string): Promise<void> {
		await this.ctl.call('control paint', { group, element, ops });
	}

	/** Compute a widget by `expression`; an empty text hands it back to the hand. */
	async computeControl(group: string, element: string, expression: string): Promise<void> {
		await this.ctl.call('control edit', { group, element, expression });
	}

	/** The bare read of node `uid` a param or a variable of `type` may take — a string reads a STRING
	 * output, everything else an ARRAY one — or null where no output fits. With `slot`, only that
	 * slot may answer. */
	readFor(uid: string, type: string, slot?: string): string | null {
		const node = this.nodeById(uid);
		if (!node) return null;
		const want: SlotDtype = type === 'string' ? 'STRING' : 'ARRAY';
		const key = Object.entries(node.output_slots).find(([k, d]) => (slot === undefined || k === slot) && feeds(d as SlotDtype, want))?.[0];
		return key ? readExpression(node, key) : null;
	}

	/** Make the widget named `name` read the first output of node `uid` that can feed it. */
	async linkControl(name: string, uid: string): Promise<string | null> {
		const gv = this.variables.find((v) => v.name === name);
		const expression = gv ? this.readFor(uid, gv.control && KIND[gv.control.kind].draws === 'text' ? 'string' : 'float') : null;
		if (gv && expression) await this.computeControl(gv.group, gv.element, expression);
		return expression;
	}

	async removeControl(group: string, element: string): Promise<void> {
		await this.ctl.call('control remove', { group, element });
	}

	/** One `machine …` op, as the panel speaks them: the op vocabulary is the interface, so the
	 * panel names the op and the manager answers. */
	machine<T = Record<string, unknown>>(op: MachineOp, payload: Record<string, unknown>): Promise<T> {
		return this.ctl.call(op, payload) as Promise<T>;
	}

	/** A card drag or a widget turn in flight: the state's place or values, previewed under the
	 * machine's key. */
	previewState(machine: string, name: string, patch: { pos?: [number, number]; values?: Record<string, Literal> }): void {
		this.ctl.preview(`machine ${machine}`, 'machine state edit', { machine, name, ...patch });
	}

	/** Ask a live node to re-evaluate a param's options. Options only, never the value, so it is
	 * not an undoable edit; the fresh list arrives on the node's next state_update. */
	async refreshParam(node: string, group: string, name: string): Promise<void> {
		const key = refreshKey(node, group, name);
		this._beginRefresh(key);
		try {
			await this._paramCall('node param request', node, group, name, { request: 'refresh' }, false);
		} catch (e) {
			// A failed dispatch means the node never re-scans, so do not wait out the safety timeout.
			this._endRefresh(key);
			throw e;
		}
	}

	/** Fire a pulse param: a request the node acts on, with no value and so no inverse to undo. */
	async pulse(node: string, group: string, name: string): Promise<void> {
		await this._paramCall('node param request', node, group, name, { request: 'pulse' });
	}

	isRefreshing(node: string, group: string, name: string): boolean {
		return refreshKey(node, group, name) in this._refreshing;
	}

	private _beginRefresh(key: string): void {
		this._endRefresh(key); // coalesce a rapid re-click onto one in-flight refresh
		const handle = setTimeout(() => this._endRefresh(key), REFRESH_SPINNER_TIMEOUT_MS);
		this._refreshing = { ...this._refreshing, [key]: handle };
	}

	private _endRefresh(key: string): void {
		const handle = this._refreshing[key];
		if (handle === undefined) return;
		clearTimeout(handle);
		const { [key]: _drop, ...rest } = this._refreshing;
		this._refreshing = rest;
	}

	/** Edit a param's source record: any subset of mode and expression. A text
	 * given implies its mode; an empty text clears it. The manager's rules are the op's. */
	async setSource(node: string, group: string, name: string, source: SourcePatch): Promise<void> {
		await this._paramCall('node param edit', node, group, name, { ...source });
	}

	/** One command puts a param back on its zero point: source and value together. */
	async resetParam(node: string, group: string, name: string, zero: NonNullable<NodeInstanceInfo['baseline']>[string]): Promise<void> {
		await this._paramCall('node param edit', node, group, name, {
			mode: zero.mode ?? 'constant',
			value: zero.value,
			expression: zero.expression ?? ''
		});
	}

	async setNodePos(uid: string, pos: [number, number]): Promise<void> {
		// Committed on drag-stop only; a live drag stays local to Svelte Flow.
		await this.ctl.call('node edit', { node: uid, pos });
	}

	/** Move several nodes as ONE command — one op, one resync, one undo step. */
	async setNodePositions(moves: [string, [number, number]][]): Promise<void> {
		if (moves.length === 0) return; // an empty compound records nothing, so no undo entry either
		if (moves.length === 1) return this.setNodePos(...moves[0]);
		const ops = moves.map(([node, pos]) => ({ op: 'node edit', payload: { node, pos } }));
		await this.ctl.call('compound', { ops });
	}

	async renameNode(uid: string, name: string): Promise<void> {
		const oldName = this.nodeById(uid)?.name ?? '';
		if (oldName === name) return;
		await this.ctl.call('node edit', { node: uid, name });
	}

	/** Store where THIS client is looking. Persisted in the `.gfi`, but never converged and never
	 * dirtying: persistence and dirtiness are separate axes. */
	async setViewpoint(viewpoint: unknown): Promise<void> {
		try {
			await this.ctl.call('layout viewpoint edit', { value: viewpoint });
		} catch {
			/* not connected / in flight — ignore */
		}
	}

	/** Write the patch. Where it landed comes back from the MANAGER (`save_path_changed`), never
	 * latched from this reply — a latch names the patch only in the tab that saved it. */
	async save(path: string, overwrite = true): Promise<{ path: string }> {
		// A given `path` becomes the patch's home; the arrangement is the manager's already.
		return this.ctl.call<{ path: string }>('session save', { path, overwrite });
	}

	/** Reset to an empty, unnamed patch. Nothing is written here: a New emits no
	 * `save_path_changed`, so the `graph_replaced` snapshot is the sole carrier of the null path. */
	async newPatch(): Promise<void> {
		await this.ctl.call('session new', {});
	}

	async groupNodes(members: string[], pos?: [number, number]): Promise<string> {
		const r = await this.ctl.call<{ inst_id: string }>('nodes group', { nodes: members, pos });
		return r.inst_id;
	}

	async expandInstance(instId: string): Promise<void> {
		await this.ctl.call('nodes ungroup', { subpatch: instId });
	}

	/** Resolve a node by uid — the ONE accessor, and every kind of node record answers it. Read
	 * off the document, so a reader re-runs when the node enters or leaves it. */
	nodeById(id: string): NodeInstanceInfo | null {
		return nodesMap(this.doc)[id] === undefined ? null : (this._views.get(id) ?? null);
	}

	/** One node's live source state, both maps whole: a driven param neither names shows its
	 * literal again. Leaf by leaf, so an unmoved value wakes nobody. */
	applyLiveSource(node: string, live: LiveSource): void {
		const t = this._rt[node];
		if (!t) return;
		t.values = follow(t.values ?? {}, live.values);
		t.errors = follow(t.errors ?? {}, live.errors);
	}

	/** Read `uids` and everything they hold — members, ports and nested sub-patches, to any depth —
	 * as a self-contained fragment in the shape a `.gfi` carries. */
	async copyNodes(uids: string[]): Promise<GraphFragment> {
		const r = await this.ctl.call<{ doc: GraphFragment }>('nodes copy', { nodes: uids });
		return r.doc;
	}

	/** Add a fragment on fresh uids, shifted by `offset` and rooted in `instId`. ONE command, so a
	 * paste of any depth is one undo step; answers each record's uid mapped to what it became. */
	async pasteNodes(
		doc: GraphFragment,
		offset: [number, number] = [0, 0],
		instId?: string,
		step?: Step
	): Promise<Record<string, string>> {
		const r = await this.ctl.call<{ rename: Record<string, string> }>(
			'nodes paste',
			{ doc, pos: offset, inst_id: instId ?? null },
			step
		);
		return r.rename ?? {};
	}

	/** Duplicate `uids` in place as a copy and a paste. `instId` is where the selection came from:
	 * a fragment names a scope only when that scope is in it. */
	async cloneNodes(
		uids: string[],
		offset: [number, number] = [40, 40],
		instId?: string,
		step?: Step
	): Promise<Record<string, string>> {
		if (uids.length === 0) return {};
		return this.pasteNodes(await this.copyNodes(uids), offset, instId, step);
	}

	async removeNodes(uids: Iterable<string>, within?: Step): Promise<void> {
		const uidList = [...uids];
		if (uidList.length === 0) return;
		const label = `Delete ${uidList.length} node${uidList.length > 1 ? 's' : ''}`;
		// Each removeNode captures its OWN subtree, and a link rides with whichever endpoint owns
		// it, so delete order is immaterial.
		await history().transaction(label, async (step) => {
			for (const uid of uidList) await this.removeNode(uid, step);
		}, within);
	}
}

/** The expression that reads `slot` of `node`: bare on a node with one output, the spelling its
 * completion offers, else `.out.<label>` — a facade keys slots by port uid, so the label names it. */
export function readExpression(node: NodeInstanceInfo, slot: string): string {
	if (Object.keys(node.output_slots).length === 1) return `nd('${node.name}')`;
	return `nd('${node.name}').out.${node.slot_labels?.[slot] ?? slot}`;
}

let _live: ParamLive | null = null;
/** The live param plane, wired to the one store it writes into. */
export function paramLive(): ParamLive {
	if (!_live) _live = new ParamLive((node, live) => graph().applyLiveSource(node, live));
	return _live;
}

let _store: GraphStore | null = null;
export function graph(): GraphStore {
	if (!_store) _store = new GraphStore();
	return _store;
}
