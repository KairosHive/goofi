/** Central reactive graph state, backed by the control WS. The store owns the only writes, so a
 * component just reads its `$state` fields. */
import type { VideoQuality } from '$lib/api/types';
import {
	getControl,
	type Control,
	type ControlEvent,
	type DemoExample,
	type Recovery,
	type DirListing,
	type GraphSnapshot,
	type LinkInfo,
	type NodeInstanceInfo,
	type NodeTypeInfo,
	type RecordStatus,
	type ScanDiff
} from '$lib/api/control';
import { boundaryType, feeds, type SlotDtype } from '$lib/api/vocab';
import { wantedDtype } from '$lib/inspector/expr/refs';
import { bareName } from '$lib/editor/typeId';
import { consoleStore } from './console.svelte';
import { selection } from './selection.svelte';
import { workspace } from 'panelty';
import type { SlotView } from '$lib/viewers/inlineView';
import { history, type Action } from './history.svelte';
import { captureNavContext } from '$lib/stores/navContext';
import { SyncClient } from '$lib/crdt/syncClient';
import {
	linkViews,
	nodeViews,
	facadeFaces,
	docParams,
	viewersJson,
	baselineJson,
	recordedSlots,
	variableViews,
	variableGroupLocks,
	arrangementTabs,
	type Doc,
	type VariableView,
	type ControlView,
	type VariableType,
	type LockView
} from '$lib/crdt/graphDoc';

/** A grid cell, in the control panel's units. */
export interface Cell {
	x: number;
	y: number;
	w: number;
	h: number;
}

/** What `control edit` takes: any subset, and `name` is the element's new name. */
export interface ControlPatch extends Partial<Cell> {
	name?: string;
	kind?: ControlView['kind'];
	min?: number;
	max?: number;
	step?: number;
	options?: string[];
}
import { applyLiveParams, assembleNode, type RuntimeOverlay } from '$lib/crdt/nodeAssembly';
import { ParamLive, type LiveSource } from '$lib/api/paramLive';
import type { StringParam, SourcePatch } from '$lib/api/types';
import type { GraphFragment } from '$lib/editor/clipboard';

/** Safety net: lift a ⟳ spinner after this long when a node never reports the refresh done.
 * Generous — an LSL resolve blocks the node's ctrl thread ~4s. */
const REFRESH_SPINNER_TIMEOUT_MS = 15000;

/** Stable key for an in-flight param refresh; U+001F cannot occur in a uid/group/name. */
function refreshKey(node: string, group: string, name: string): string {
	return `${node}\u001f${group}\u001f${name}`;
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

export class GraphStore {
	nodes = $state<NodeInstanceInfo[]>([]);
	links = $state<LinkInfo[]>([]);
	savePath = $state<string | null>(null);
	unsavedChanges = $state(false);
	/** What a goofi that did not shut down cleanly left behind, as the manager last listed it. */
	recoveries = $state<Recovery[]>([]);
	/** What the server said it is. Every affordance a demo withholds reads THIS, never a list of
	 * its own. */
	demo = $state(false);
	/** The public set this instance belongs to, empty everywhere else. */
	examples = $state<DemoExample[]>([]);
	connected = $state(false);
	/** Latches on the first connect and never clears — see {@link disconnected}. */
	private _everConnected = $state(false);
	hadHello = $state(false);
	/** The startup hook: counts the SERVER sessions this page has connected to, bumped on the
	 * first hello from a manager it has not seen. Startup UI keys on this — an offer made "at the
	 * start" is made at the start of a session, which a page that outlived the last server sees
	 * without a reload. Never on page load, and never on a transient reconnect. */
	sessionEpoch = $state(0);

	/** Every armed output slot, doc-authoritative: the document is the one owner of what is armed. */
	armed = $state<{ uid: string; slot: string; quality: VideoQuality }[]>([]);

	/** The recording SESSION, as the backend last reported it. Pushed, never derived here. */
	record = $state<RecordStatus>(IDLE_RECORD);

	/** The streams whose drop count MOVED on the last report, keyed `node/slot`. */
	dropping = $state.raw<ReadonlySet<string>>(new Set());

	/** What each stream's cumulative `dropped` was on the last report. */
	private _droppedCounts: Record<string, number> = {};

	/** Patch variables (system + user), doc-authoritative, in system-first/creation order. */
	variables = $state<VariableView[]>([]);
	/** Every variable group that carries a lock, by name. */
	variableGroups = $state<Record<string, LockView>>({});

	/** Bumps on every WHOLESALE graph load, never on an incremental add/remove; editors re-fit on it. */
	loadEpoch = $state(0);

	nodeTypes = $state<NodeTypeInfo[] | null>(null);

	/** Params with a ⟳ refresh in flight → its safety-timeout handle. Cleared when the node reports
	 * the param done (`refreshed_params`), never on the fire-and-forget RPC ack. */
	private _refreshing = $state<Record<string, ReturnType<typeof setTimeout>>>({});

	/** instance_id of the manager we last hydrated from; a change is a fresh session, not a reconnect. */
	private _lastInstanceId: string | null = null;

	/** Per-node runtime for nodes the runtime planes named but the doc has not yet materialized.
	 * One-shot: `_seedRuntime` takes its entry, so a later node at the same uid seeds fresh. */
	private _snapshotRuntime: GraphSnapshot['runtime'] = {};

	/** Hold a runtime plane's report for a node still to materialize — the stage and error planes
	 * ride their own channel in no defined order against the doc, so a report outrunning the doc
	 * is routine, and dropping it would wedge the seed's `creating` guess for good. */
	private _stashRuntime(uid: string, rt: GraphSnapshot['runtime'][string]): void {
		this._snapshotRuntime[uid] = { ...this._snapshotRuntime[uid], ...rt };
	}

	/** The control client (injectable for tests; defaults to the live WS one). */
	private ctl: Control;

	/** The document driver — the browser replica of the manager's control-plane document. */
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
		this._sync = new SyncClient(ctl);
		this._sync.onDocChange(() => this._syncFromDoc());
		this._sync.start();
	}

	/** The control-plane document — the one projection a client reads. */
	get doc(): Doc {
		return this._sync.doc;
	}

	/** Whether the replica has pulled from the manager yet; until then doc reads describe nothing. */
	get docSynced(): boolean {
		return this._sync.synced;
	}

	/** Re-derive every document-owned subtree, after each applied change. Runtime state
	 * (error/stage/ufreq) and catalog metadata (slots/tags) stay event-sourced. */
	private _syncFromDoc(): void {
		const doc = this._sync.doc;
		this.links = linkViews(doc);
		this.variables = variableViews(doc);
		this.variableGroups = variableGroupLocks(doc);
		this.armed = recordedSlots(doc);
		// The workspace store rebuilds its tree from this; the client holds no second copy.
		workspace().syncFromDoc(arrangementTabs(doc));
		// No-ops until the catalog lands, then rebuilds from the doc.
		this._reconcileNodesFromDoc();
	}

	/** Apply a wholesale snapshot, returning whether it came from a NEW backend session — which is
	 * what a same-session reconnect must not look like. */
	private _replaceSnapshot(snap: GraphSnapshot): boolean {
		// A `hello` always carries the palette; `graph_replaced` never does — the `node_types` event
		// is what re-announces it there.
		if (snap.node_types?.length) this.nodeTypes = snap.node_types;
		// The snapshot and the doc delta ride separate channels in no defined order, so the runtime
		// overlay is both stashed for nodes still to materialize and applied to those already here.
		this._snapshotRuntime = snap.runtime ?? {};
		for (const [uid, rt] of Object.entries(this._snapshotRuntime)) {
			const node = this.nodeById(uid);
			if (!node) continue;
			node.stage = rt.stage;
			node.error = rt.error ?? null;
			node.runtime = rt.runtime;
		}
		this._setRecord(snap.record);
		this.savePath = snap.save_path;
		this.unsavedChanges = snap.unsaved_changes;
		// `hello` alone carries it; a `graph_replaced` snapshot must not clear what the mode is.
		if (snap.demo !== undefined) this.demo = snap.demo;
		if (snap.examples !== undefined) this.examples = snap.examples;

		// The arrangement rides the doc; what the snapshot carries is the VIEWPOINT, this client's
		// alone — persisted, never converged.
		const freshSession = snap.instance_id !== this._lastInstanceId;
		this._lastInstanceId = snap.instance_id;
		if (snap.viewpoint != null) workspace().restoreViewpoint(snap.viewpoint);
		return freshSession;
	}

	/** Drop every projection assembled from the OUTGOING document. Reconciliation runs only from the
	 * doc observer, and a fresh manager with an EMPTY graph answers our SV with no change at all. */
	private _resetProjection(): void {
		this.nodes = [];
		this.links = [];
		this.variables = [];
		this.variableGroups = {};
		this.armed = [];
		// `_snapshotRuntime` is NOT cleared: `_replaceSnapshot` ran first, so it already holds the
		// INCOMING session's overlay. The arrangement store is a separate singleton, so it is here.
		workspace().syncFromDoc([]);
	}

	private _onWholesaleLoad(): void {
		this.loadEpoch += 1;
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
			.then(() => this._recordGraphCmd(`Set ${slot} view`))
			.catch(() => {
				/* soft view state — the next edit re-sends it */
			});
	}

	private _handle(ev: ControlEvent): void {
		switch (ev.event) {
			case 'hello': {
				// Not wholesale: a `hello` is also what a transient reconnect delivers.
				const fresh = this._replaceSnapshot(ev.payload);
				this.hadHello = true;
				if (fresh) {
					// A NEW session mints uids from 1 again, so the stale replica must fall NOW,
					// synchronously, before this connection answers the server's binary hello SV.
					// Projections first: `_resetProjection` reads `this.nodes`.
					this._resetProjection();
					this._sync.reset();
					this._onWholesaleLoad();
					this.sessionEpoch += 1;
				}
				break;
			}
			case 'graph_replaced':
				this._replaceSnapshot(ev.payload);
				this._onWholesaleLoad();
				break;
			case 'state_update': {
				const t = this.nodeById(ev.payload.node);
				if (t) {
					// Params are doc-owned: merge ONLY the runtime bits, never wholesale-replace, which
					// would clobber the reconcile's value+descriptor assembly.
					this._mergeParamRuntime(t, ev.payload.params);
					if (ev.payload.stage) t.stage = ev.payload.stage;
					// The state plane re-pushes the current error, so backend truth wins here even
					// when no diff-driven `error` event fired.
					if ('error' in ev.payload) t.error = ev.payload.error ?? null;
					if (ev.payload.runtime !== undefined) t.runtime = ev.payload.runtime ?? undefined;
				}
				// Lift each spinner exactly when the fresh options land. Keyed by node, not by `t`.
				for (const [group, name] of ev.payload.refreshed_params ?? []) {
					this._endRefresh(refreshKey(ev.payload.node, group, name));
				}
				break;
			}
			case 'node_stage': {
				const t = this.nodeById(ev.payload.node);
				if (t) {
					t.stage = ev.payload.stage;
					if (ev.payload.error !== undefined) t.error = ev.payload.error ?? null;
					// The tier arrives here as well as on the snapshot: a node added after connecting
					// is in no snapshot, and a GIL demotion moves it while the session is live.
					if (ev.payload.runtime !== undefined) t.runtime = ev.payload.runtime ?? undefined;
				} else {
					// The stage plane pushes each transition ONCE, so one outrun by its node's own
					// doc delta is stashed for the seed, never dropped.
					this._stashRuntime(ev.payload.node, {
						stage: ev.payload.stage,
						error: ev.payload.error,
						runtime: ev.payload.runtime ?? undefined
					});
				}
				break;
			}
			case 'node_stats': {
				const t = this.nodeById(ev.payload.node);
				if (t) t.stats = ev.payload.stats;
				break;
			}
			case 'param_values':
				this.applyLiveSource(ev.payload.node, ev.payload);
				break;
			case 'logs':
				consoleStore().apply(ev.payload);
				break;
			case 'error': {
				// A REPORT, so only a node that RUNS raises one — a facade's health rides `node_stage`.
				const t = this.nodeById(ev.payload.node);
				if (t) t.error = ev.payload.error;
				else this._stashRuntime(ev.payload.node, { error: ev.payload.error });
				break;
			}
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
				this._applyNodeTypes(ev.payload.types);
				break;
		}
	}

	/** Adopt a palette catalog. It supplies the descriptors, so the nodes rebuild here. */
	private _applyNodeTypes(types: NodeTypeInfo[]): void {
		this.nodeTypes = types;
		this._reconcileNodesFromDoc();
	}

	/** Re-derive the node registry from disk and report what changed; explicit, since there is no
	 * watcher. The fresh catalog arrives as a `node_types` event in every open tab. */
	async rescanNodes(): Promise<ScanDiff> {
		return this.ctl.call<ScanDiff>('library refresh', {});
	}

	/** Move one of the patch's own node files into the private library, where every later patch
	 * finds it. The fresh catalog arrives as a `node_types` event in every open tab. */
	async saveNodeToLibrary(type: string, overwrite: boolean, name?: string): Promise<{ type: string; path: string }> {
		return this.ctl.call<{ type: string; path: string }>('library save', { type, overwrite, ...(name ? { name } : {}) });
	}

	/** The private library's own file for this type, hidden behind the patch's — what a save to the
	 * library would replace, and null where it would land on nothing. */
	async libraryFileBehind(type: string): Promise<string | null> {
		const r = await this.ctl.call<{ provenance?: string; path?: string; shadowed?: { provenance: string; path: string }[] }>(
			'library get',
			{ type }
		);
		if (r.provenance === 'custom') return r.path ?? null;
		return r.shadowed?.find((s) => s.provenance === 'custom')?.path ?? null;
	}

	/** Where this patch's workspace files live — a per-run temp directory under a random name. It
	 * rides `session status` beside the save path, because both answer "where does this patch live". */
	async openWorkspace(): Promise<string> {
		const r = await this.ctl.call<{ workspace: string }>('session status', {});
		return r.workspace;
	}

	/** Push an action onto the history (unless a replay is in progress). */
	private _record(action: Action): void {
		if (!history().isSuspended) history().record(action);
	}

	/** Record ONE graph command. The manager owns the exact inverse, so this only marks the step. */
	private _recordGraphCmd(label: string): void {
		this._record({ kind: 'graph_cmd', domain: 'graph', label, context: captureNavContext() });
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
	async armSlot(node: string, slot: string): Promise<void> {
		const r = await this.ctl.call<{ changed?: boolean }>('record arm', {
			output: `${node}/${slot}`
		});
		if (r?.changed) this._recordGraphCmd(`Arm ${slot}`);
	}

	async setRecordQuality(node: string, slot: string, quality: VideoQuality): Promise<void> {
		const r = await this.ctl.call<{ changed?: boolean }>('record quality', {
			output: `${node}/${slot}`, quality
		});
		if (r?.changed) this._recordGraphCmd(`Set ${slot} recording quality`);
	}

	async disarmSlot(node: string, slot: string): Promise<void> {
		const r = await this.ctl.call<{ changed?: boolean }>('record disarm', {
			output: `${node}/${slot}`
		});
		if (r?.changed) this._recordGraphCmd(`Disarm ${slot}`);
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

	async addNode(type: string, pos: [number, number], instId?: string): Promise<string> {
		const born = await this.ctl.call<{ uid: string }>('node add', {
			type,
			pos,
			inst_id: instId
		});
		const uid = born?.uid ?? '';
		if (uid) this._recordGraphCmd(`Add ${bareName(type)}`);
		return uid;
	}

	async removeNode(uid: string): Promise<void> {
		// The label reads the node's name before it vanishes.
		const label = `Delete ${this.nodeById(uid)?.name ?? uid}`;
		await this.ctl.call('node remove', { node: uid });
		this._recordGraphCmd(label);
	}

	/** Respawn a node in place, keeping its uid, name, params, position, scope and links. A
	 * recovery action rather than an edit, so it records no history. */
	async restartNode(uid: string): Promise<void> {
		await this.ctl.call('node restart', { node: uid });
	}

	/** Open a node's own editor window. It appears on the machine the SERVER runs on, so this is a
	 * request the page sends, never something it draws. */
	async showNodeEditor(uid: string): Promise<void> {
		await this.ctl.call('node editor', { node: uid });
	}

	async addLink(link: LinkInfo): Promise<void> {
		await this.ctl.call('link add', linkEndpoints(link));
		this._recordGraphCmd('Connect');
	}

	async removeLink(link: LinkInfo): Promise<void> {
		await this.ctl.call('link remove', linkEndpoints(link));
		this._recordGraphCmd('Disconnect');
	}

	async updateParam(node: string, group: string, name: string, value: unknown): Promise<void> {
		// Guard on EXISTENCE, not truthiness — a real param may hold 0, false or ''.
		const param = this.nodeById(node)?.params?.[group]?.[name];
		if (!param) throw new Error(`node param edit: no param ${group}.${name} on node ${node}`);
		await this.ctl.call('node param edit', { node, param: `${group}/${name}`, value });
		this._recordGraphCmd(`Set ${name}`);
	}

	/** Add a NEW user variable; the server refuses a name the patch already holds. A `control` makes
	 * it a control-panel element. */
	async addVariable(
		name: string,
		value: number | string | boolean,
		type: VariableType,
		control?: ControlView
	): Promise<void> {
		if (this.variables.some((g) => g.name === name)) throw new Error(`variable ${name} already exists`);
		await this.ctl.call('variable entry add', control ? { name, value, type, control } : { name, value, type });
		this._recordGraphCmd(`Add variable ${name}`);
	}

	/** Edit an existing variable's value, keeping its type. */
	async setVariableValue(name: string, value: number | string | boolean): Promise<void> {
		if (!this.variables.some((g) => g.name === name)) throw new Error(`no variable ${name}`);
		await this.ctl.call('variable entry edit', { name, value });
		this._recordGraphCmd(`Set variable ${name}`);
	}

	async setVariableType(name: string, type: VariableType): Promise<void> {
		await this.ctl.call('variable entry edit', { name, type });
		this._recordGraphCmd(`Change variable ${name} type`);
	}

	async addVariableEntry(group: string): Promise<string> {
		const result = await this.ctl.call('variable entry add', { group }) as { name: string };
		this._recordGraphCmd(`Add variable ${result.name}`);
		return result.name;
	}

	async addVariableGroup(): Promise<string> {
		const result = await this.ctl.call('variable group add', {}) as { group: string };
		this._recordGraphCmd(`Add variable group ${result.group}`);
		return result.group;
	}

	/** Remove a user variable (a system variable is refused by the server). */
	async removeVariable(name: string): Promise<void> {
		await this.ctl.call('variable entry remove', { name });
		this._recordGraphCmd(`Remove variable ${name}`);
	}

	/** Rename a user variable; every expression that reads it is rewritten by the manager. */
	async renameVariable(oldName: string, newName: string): Promise<void> {
		if (!this.variables.some((g) => g.name === oldName)) throw new Error(`no variable ${oldName}`);
		await this.ctl.call('variable entry rename', { name: oldName, to: newName });
		this._recordGraphCmd(`Rename variable ${oldName} → ${newName}`);
	}

	/** Set a control element's widget, its range or its place. */
	async setVariableControl(name: string, control: ControlView): Promise<void> {
		await this.ctl.call('variable entry edit', { name, control });
		this._recordGraphCmd(`Edit control ${name}`);
	}

	/** Rename a group, moving every member with it. */
	async renameVariableGroup(from: string, to: string): Promise<void> {
		await this.ctl.call('variable group rename', { from, to });
		this._recordGraphCmd(`Rename variable group ${from} → ${to}`);
	}

	/** Make a variable follow `node.slot` (and `index` into a wide frame); an empty reference clears. */
	async setVariableSource(name: string, reference: string, index?: number): Promise<void> {
		await this.ctl.call('variable entry source', index === undefined ? { name, reference } : { name, reference, index });
		this._recordGraphCmd(`Source variable ${name}`);
	}

	/** Bear a widget in a control panel's group through the `control` door, which lifts the
	 * group's lock for the one command: the manager mints the name and the cell when none is given. */
	async addControl(group: string, kind: ControlView['kind'], cell?: Cell): Promise<string> {
		const r = await this.ctl.call('control add', { group, kind, ...(cell ?? {}) });
		this._recordGraphCmd(`Add ${kind} to ${group}`);
		return String((r as { name?: unknown }).name ?? '');
	}

	/** Change a widget's name, kind, range, options or place, through the `control` door. */
	async editControl(group: string, element: string, patch: ControlPatch): Promise<void> {
		await this.ctl.call('control edit', { group, element, ...patch });
		this._recordGraphCmd(`Edit ${group}.${element}`);
	}

	/** Make a widget follow `node.slot` (and `index` into a wide frame); an empty reference clears. */
	async sourceControl(group: string, element: string, reference: string, index?: number): Promise<void> {
		await this.ctl.call('control source', index === undefined ? { group, element, reference } : { group, element, reference, index });
		this._recordGraphCmd(`Source ${group}.${element}`);
	}

	/** The first output of node `uid` that can feed the variable named `name`, as `node.slot`. */
	feedFor(name: string, uid: string): string | null {
		const gv = this.variables.find((v) => v.name === name);
		return gv ? this.referenceFor(uid, gv.type) : null;
	}

	/** The `node.slot` a param or a variable of `type` may follow on node `uid`, or null for none. A
	 * facade keys its slots by port uid and a reference names the port, so the LABEL is the half. */
	referenceFor(uid: string, type: string): string | null {
		const node = this.nodeById(uid);
		if (!node) return null;
		const want = wantedDtype(type);
		const key = Object.entries(node.output_slots).find(([, d]) => feeds(d as SlotDtype, want as SlotDtype))?.[0];
		return key ? `${node.name}.${node.slot_labels?.[key] ?? key}` : null;
	}

	/** Make the widget named `name` follow the first output of node `uid` that can feed it. */
	async linkControl(name: string, uid: string): Promise<string | null> {
		const gv = this.variables.find((v) => v.name === name);
		const reference = this.feedFor(name, uid);
		if (gv && reference) await this.sourceControl(gv.group, gv.element, reference);
		return reference;
	}

	async removeControl(group: string, element: string): Promise<void> {
		await this.ctl.call('control remove', { group, element });
		this._recordGraphCmd(`Remove ${group}.${element}`);
	}

	/** Lock or unlock one variable on its own account; an axis not named keeps what it has. */
	async lockVariable(name: string, lock: Partial<LockView>): Promise<void> {
		await this.ctl.call('variable entry lock', { name, ...lock });
		this._recordGraphCmd(`Lock variable ${name}`);
	}

	/** Lock or unlock a whole group; an axis not named keeps what it has. */
	async lockVariableGroup(group: string, lock: Partial<LockView>): Promise<void> {
		await this.ctl.call('variable group lock', { group, ...lock });
		this._recordGraphCmd(`Lock variable group ${group}`);
	}

	/** Ask a live node to re-evaluate a param's options. Options only, never the value, so it is
	 * not an undoable edit; the fresh list arrives on the node's next state_update. */
	async refreshParam(node: string, group: string, name: string): Promise<void> {
		const key = refreshKey(node, group, name);
		this._beginRefresh(key);
		try {
			await this.ctl.call('node param request', { node, param: `${group}/${name}`, request: 'refresh' });
		} catch (e) {
			// A failed dispatch means the node never re-scans, so do not wait out the safety timeout.
			this._endRefresh(key);
			throw e;
		}
	}

	/** Take every param's default from what the node holds now — the zero point the non-default
	 * filter reads. It edits no param, so an expression or a reference keeps driving; it IS
	 * undoable, since the zero point is document state a later reader depends on. */
	async clearNonDefault(node: string): Promise<void> {
		await this.ctl.call('node baseline', { node });
		this._recordGraphCmd(`Clear non-default on ${this.nodeById(node)?.name ?? node}`);
	}

	/** Fire a pulse param: a request the node acts on, with no value and so no inverse to undo. */
	async pulse(node: string, group: string, name: string): Promise<void> {
		const param = this.nodeById(node)?.params?.[group]?.[name];
		if (!param) throw new Error(`node param request: no param ${group}.${name} on node ${node}`);
		await this.ctl.call('node param request', { node, param: `${group}/${name}`, request: 'pulse' });
	}

	/** Whether a ⟳ refresh is in flight for this param. */
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

	/** Edit a param's source record: any subset of mode, expression, reference and triggers. A text
	 * given implies its mode; an empty text clears it. The manager's rules are the op's. */
	async setSource(node: string, group: string, name: string, source: SourcePatch): Promise<void> {
		const d = this.nodeById(node)?.params?.[group]?.[name];
		if (!d) throw new Error(`node param edit: no param ${group}.${name} on node ${node}`);
		await this.ctl.call('node param edit', { node, param: `${group}/${name}`, ...source });
		this._recordGraphCmd(`Set ${name} source`);
	}

	async setNodePos(uid: string, pos: [number, number]): Promise<void> {
		// Committed on drag-stop only; a live drag stays local to Svelte Flow.
		await this.ctl.call('node edit', { node: uid, pos });
		this._recordGraphCmd(`Move ${this.nodeById(uid)?.name ?? uid}`);
	}

	/** Set a node's mutable display name (uid identity is unchanged). */
	async renameNode(uid: string, name: string): Promise<void> {
		const oldName = this.nodeById(uid)?.name ?? '';
		if (oldName === name) return;
		await this.ctl.call('node edit', { node: uid, name });
		this._recordGraphCmd(`Rename ${oldName} → ${name}`);
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

	/** Ask the manager what earlier sessions left unsaved; the list is disk truth, read on ask. */
	async refreshRecoveries(): Promise<void> {
		const r = await this.ctl.call<{ recoveries: Recovery[] }>('session recoverable', {});
		this.recoveries = r.recoveries;
	}

	/** Open a crash's autosave in place of the open patch: unsaved work, with its old home. */
	async recover(workspace: string): Promise<void> {
		await this.ctl.call('session recover', { workspace });
		this.recoveries = this.recoveries.filter((r) => r.workspace !== workspace);
	}

	/** Remove a crash's autosave without opening it. */
	async discardRecovery(workspace: string): Promise<void> {
		await this.ctl.call('session discard', { workspace });
		this.recoveries = this.recoveries.filter((r) => r.workspace !== workspace);
	}

	/** Reset to an empty, unnamed patch. Nothing is written here: a New emits no
	 * `save_path_changed`, so the `graph_replaced` snapshot is the sole carrier of the null path. */
	async newPatch(): Promise<void> {
		await this.ctl.call('session new', {});
	}

	/** Group the named nodes into a sub-patch. Returns its instance id. */
	async groupNodes(members: string[], pos?: [number, number]): Promise<string> {
		const r = await this.ctl.call<{ inst_id: string }>('nodes group', { nodes: members, pos });
		if (r?.inst_id) this._recordGraphCmd('Group nodes');
		return r.inst_id;
	}

	/** Dissolve a sub-patch instance back into its member nodes. */
	async expandInstance(instId: string): Promise<void> {
		await this.ctl.call('nodes ungroup', { subpatch: instId });
		this._recordGraphCmd('Ungroup');
	}

	async statPath(path: string): Promise<{ path: string; kind: 'file' | 'dir' | 'missing' }> {
		return this.ctl.call('dir stat', { path });
	}

	/** List one directory level on the BACKEND filesystem (full FS, no jail). */
	async listDir(path?: string): Promise<DirListing> {
		return this.ctl.call<DirListing>('dir list', { path });
	}

	/** Load a patch from a BACKEND filesystem path; destructive, and it resets the session, so
	 * there is no history entry. A `.gfi` is a zip, so a path is the only door the client has. */
	async load(path: string): Promise<void> {
		await this.ctl.call('session load', { path });
	}

	/** Resolve a node by uid — the ONE accessor, and every kind of node record answers it. */
	nodeById(id: string): NodeInstanceInfo | null {
		return this.nodes.find((n) => n.uid === id) ?? null;
	}

	/** Every node a panel can bind or a picker can list. ROOT is the canvas, and it is not one. */
	get bindable(): { uid: string; name: string }[] {
		return this.nodes.map((n) => ({ uid: n.uid, name: n.name }));
	}

	/** Reconcile the flat node list IN PLACE by uid, so a survivor keeps its object reference and
	 * with it its inline-viewer subscription. */
	private _reconcileNodes(next: NodeInstanceInfo[]): void {
		const byUid = new Map(this.nodes.map((n) => [n.uid, n]));
		this.nodes = next.map((n) => {
			const cur = byUid.get(n.uid);
			if (!cur) return n;
			Object.assign(cur, n);
			return cur;
		});
	}

	/** The runtime overlay for a node materializing from the doc for the FIRST time. A leaf is
	 * `creating` until its own thread says otherwise; a virtual node runs nothing, so it is born at
	 * the stage the backend already answers for it rather than waiting a stats period to hear so. */
	private _seedRuntime(uid: string, virtual: boolean): RuntimeOverlay {
		const seed = this._snapshotRuntime[uid];
		delete this._snapshotRuntime[uid];
		return {
			stage: seed?.stage ?? (virtual ? 'ready' : 'creating'),
			error: seed?.error ?? null,
			runtime: seed?.runtime
		};
	}

	/** One node's live source state, both maps WHOLE: a driven param neither names has no live value
	 * and shows its literal again, and no standing error. Written in place — a READOUT must not
	 * re-assemble a node — and the one door the control plane's own event and the faster `/params`
	 * socket both come through. */
	applyLiveSource(node: string, live: LiveSource): void {
		const t = this.nodeById(node);
		if (t) applyLiveParams(t, docParams(this._sync.doc, t.uid), live.values, live.errors);
	}

	/** Pull the RUNTIME (event-sourced, never-in-the-doc) fields off a node so a re-assemble keeps them. */
	private _extractRuntime(node: NodeInstanceInfo): RuntimeOverlay {
		const params: NonNullable<RuntimeOverlay['params']> = {};
		for (const group of Object.keys(node.params)) {
			params[group] = {};
			for (const name of Object.keys(node.params[group])) {
				const p = node.params[group][name];
				const pr: NonNullable<RuntimeOverlay['params']>[string][string] = { error: p.error };
				if (p.type === 'string') pr.options = (p as StringParam).options;
				// A driven param DISPLAYS its live evaluated value, which never reaches the doc.
				if (p.mode !== 'constant') pr.liveValue = p.value;
				params[group][name] = pr;
			}
		}
		return {
			error: node.error,
			stage: node.stage,
			runtime: node.runtime,
			stats: node.stats,
			params
		};
	}

	/** Merge ONLY the runtime param bits from a state_update's descriptor map onto an existing node.
	 * NOT the error: an echo is taken before the node has re-evaluated the source the op just moved,
	 * so it would carry the PREVIOUS source's failure. The live plane owns that field. */
	private _mergeParamRuntime(
		t: NodeInstanceInfo,
		params: Record<string, Record<string, unknown>>
	): void {
		for (const [group, names] of Object.entries(params)) {
			for (const [name, desc] of Object.entries(names)) {
				const p = t.params[group]?.[name];
				if (!p) continue;
				const d = desc as { options?: string[] | null };
				if (p.type === 'string') (p as StringParam).options = d.options ?? null;
			}
		}
	}

	/** Build `this.nodes` from the doc: each record is the doc's own fields, plus the catalog
	 * descriptor for its type, plus the runtime overlay the doc never holds. A facade has no
	 * catalog entry — it runs nothing — so its ports supply its slots instead. */
	private _reconcileNodesFromDoc(): void {
		if (!this.nodeTypes?.length) return; // no catalog yet → keep the current nodes; rebuild when it lands
		const doc = this._sync.doc;
		// Both indexes are built ONCE per reconcile rather than per node.
		const byType = new Map(this.nodeTypes.map((t) => [t.type, t]));
		const faces = facadeFaces(doc);
		const next: NodeInstanceInfo[] = nodeViews(doc).map((nv) => {
			const existing = this.nodeById(nv.uid);
			const catalog = byType.get(nv.type);
			const runtime: RuntimeOverlay = existing
				? this._extractRuntime(existing)
				: this._seedRuntime(nv.uid, !!boundaryType(nv.type) || faces.has(nv.uid));
			const viewers = (viewersJson(doc, nv.uid) ?? {}) as NodeInstanceInfo['viewers'];
			const baseline = baselineJson(doc, nv.uid) as NodeInstanceInfo['baseline'];
			return assembleNode(nv, docParams(doc, nv.uid), viewers, baseline, catalog, runtime, faces.get(nv.uid));
		});
		this._reconcileNodes(next);
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
		instId?: string
	): Promise<Record<string, string>> {
		const r = await this.ctl.call<{ rename: Record<string, string> }>('nodes paste', {
			doc,
			pos: offset,
			inst_id: instId ?? null
		});
		this._recordGraphCmd('Paste nodes');
		return r.rename ?? {};
	}

	/** Duplicate `uids` in place — a copy and a paste, which is what a duplicate IS, so a sub-patch
	 * and a leaf go through the one door. `instId` is where the selection came FROM: a fragment
	 * names a scope only when that scope is in it, so a member copied alone names none. */
	async cloneNodes(
		uids: string[],
		offset: [number, number] = [40, 40],
		instId?: string
	): Promise<Record<string, string>> {
		if (uids.length === 0) return {};
		return this.pasteNodes(await this.copyNodes(uids), offset, instId);
	}



	/** Delete several nodes as ONE undoable step. */
	async removeNodes(uids: Iterable<string>): Promise<void> {
		const uidList = [...uids];
		if (uidList.length === 0) return;
		const label = `Delete ${uidList.length} node${uidList.length > 1 ? 's' : ''}`;
		// Each removeNode captures its OWN subtree, and a link rides with whichever endpoint owns
		// it, so delete order is immaterial.
		await history().transaction(label, async () => {
			for (const uid of uidList) await this.removeNode(uid);
		});
	}

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
