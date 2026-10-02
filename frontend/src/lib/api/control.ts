/** Control-plane WebSocket client: typed RPC and event subscription over `/control`. */
import type { LogBatch } from '$lib/stores/console.svelte';
import type { ParamDescriptor } from '$lib/api/types';
import type { OpName } from '$lib/api/ops';
import type { TAGS } from '$lib/api/vocab';
import type { Link } from './generated';
import type { LiveSource } from './paramLive';
import { wsUrl } from './wsUrl';

/** Control-plane protocol version. Bump it together with PROTOCOL_VERSION in
 * `backend/goofi-bridge/src/schemas.rs`. */
export const PROTOCOL_VERSION = 5;

/** A node's lifecycle stage. 'error' is terminal: the backend does not auto-restart it. */
export type NodeStage = 'creating' | 'setup' | 'ready' | 'error';

/** Where a node's code runs. Absent for a port and a facade, which run nowhere. */
export type NodeRuntime = 'native' | 'in-process' | 'subprocess' | 'shader' | 'hosted';

export interface NodeTypeInfo {
	/** The qualified `engine:Name` id; a structural type is bare. `engineOf` reads the engine. */
	type: string;
	tags: (typeof TAGS)[number][];
	/** Which tree the type came from: `custom` the user's own library, `patch` the open patch's
	 * workspace, `plugin` an engine's own find (a VST3 class); an `--extra-nodes` root is `builtin`. */
	source: 'builtin' | 'custom' | 'patch' | 'plugin';
	/** The node root the type was scanned from, by directory name — a shipped bundle, or an
	 * `--extra-nodes` root. Absent for `custom`, `patch` and `plugin`, which name no root. */
	bundle?: string;
	doc: string;
	/** Whether this machine resolves the type's unconditional top-level deps. */
	available: boolean;
	missing_deps: string[];
	/** Whether a node of this type has an editor window of its own — on the machine goofi runs on. */
	editor?: boolean;
	input_slots: Record<string, string>;
	/** Names of the variadic (multi) input slots. */
	input_multi?: string[];
	output_slots: Record<string, string>;
	params: Record<string, Record<string, ParamDescriptor>>;
}

/** What one `library refresh` changed, by type name. */
export interface ScanDiff {
	added: string[];
	changed: string[];
	removed: string[];
}

export interface NodeStats {
	updates_per_second: number;
}

export interface NodeInstanceInfo
	extends Pick<NodeTypeInfo, 'type' | 'doc' | 'editor' | 'input_slots' | 'input_multi' | 'output_slots' | 'params'> {
	/** The universal node identity, stable across rename/restart/reload. */
	uid: string;
	/** Mutable display name — the label only, never an identity key. */
	name: string;
	/** Optional per-slot display label, keyed by slot id; the slot id is shown when absent. */
	slot_labels?: Record<string, string>;
	pos: [number, number];
	/** Per-output-slot view state restored from the .gfi patch. */
	viewers: Record<string, { collapsed?: boolean; kind?: string; settings?: Record<string, unknown> }>;
	/** The touched filter's zero points, keyed `group/name`; absent until Clear is pressed. */
	baseline?: Record<string, { value?: unknown; mode?: string; expression?: string | null; reference?: string | null }>;
	/** The scope this node sits in; `'__root__'` at the top level. */
	scope: string;
	error: string | null;
	/** Lifecycle stage; absent until the node reports one. */
	stage?: NodeStage;
	/** Where this node's code runs; absent for a port and a facade, which run nowhere. */
	runtime?: NodeRuntime;
	/** Rolling execution telemetry, absent until the node's first `node_stats` event. */
	stats?: NodeStats | null;
	/** Set only on a sub-patch facade — the node that stands for a scope. */
	subpatch?: SubpatchMeta;
}

/** Marks a node record as the facade of a sub-patch scope. */
export interface SubpatchMeta {
	memberCount: number;
}

export type LinkInfo = Link;

export function linkKey(l: LinkInfo): string {
	return `${l.node_out}.${l.slot_out}→${l.node_in}.${l.slot_in}`;
}


export interface FsEntry {
	name: string;
	path: string;
	kind: 'dir' | 'file';
	is_gfi: boolean;
	/** Epoch milliseconds; null when the filesystem gives no time. */
	modified: number | null;
	/** Bytes; null for a directory. */
	size: number | null;
}

export const FS_SORTS = ['name', 'modified', 'size'] as const;
export type FsSort = (typeof FS_SORTS)[number];

export interface FsRoot {
	label: string;
	path: string;
	/** One of the folders patches were last loaded from or saved to. */
	recent: boolean;
}

export interface DirListing {
	path: string;
	parent: string | null;
	entries: FsEntry[];
	roots: FsRoot[];
}


export interface GraphSnapshot {
	/** Current recorder status, refreshed on load and reconnect. */
	record: RecordStatus;
	/** Control-plane protocol version, present on the `hello` handshake. */
	protocol_version?: number;
	/** Identifies the manager process; it changes when the backend is restarted. */
	instance_id: string;
	/** The document version this frame goes with; a replica behind it is still mid-load. */
	doc_version: number;
	/** This actor's undo and redo on top at the manager, carried on `hello` alone. */
	history?: HistoryLabels;
	/** Per-node runtime state, seeded here because its live stream pushes only transitions. */
	runtime: Record<string, { stage?: NodeStage; error?: string | null; runtime?: NodeRuntime }>;
	/** The node palette, carried on `hello`/`graph_replaced`. */
	node_types?: NodeTypeInfo[];
	save_path: string | null;
	unsaved_changes: boolean;
	/** Where THIS client was last looking — persisted with the patch, never converged to a peer. */
	viewpoint?: unknown;
	/** The spawned agent harnesses and the installed ones. */
	harnesses?: HarnessRoster;
	/** A PUBLIC goofi: no terminal, no agents, no filesystem, no save or load, no audio. Carried
	 * on `hello` alone, because it is decided once at start. */
	demo?: boolean;
	/** The other examples of a public set, this one among them. Absent unless the deployment
	 * named where its siblings answer. */
	examples?: DemoExample[];
}

/** One example of a public set, and the address it answers at — a whole instance, not a patch to
 * load: switching is a navigation, so nobody else's session is replaced. */
export interface DemoExample {
	slug: string;
	label: string;
	url: string;
	current: boolean;
}

/** One autosave a goofi that did not shut down cleanly left behind, as `session recoverable`
 * lists it: `home` is the `.gfi` the patch was saved as, `at` seconds since the epoch. */
export interface Recovery {
	workspace: string;
	home: string | null;
	at: number | null;
}

/** One spawned harness. `stopping` spans the grace period between the stop and the exit. */
export interface HarnessInstanceInfo {
	id: string;
	harness: string;
	state: 'running' | 'stopping' | 'exited';
	exit_code?: number | null;
}

/** One launchable agent — a config entry: a display name and its bash command line. */
export interface AgentEntry {
	name: string;
	command: string;
}

/** The shape the snapshot seeds and `harness_changed` broadcasts. */
export interface HarnessRoster {
	instances: HarnessInstanceInfo[];
	agents: AgentEntry[];
	/** Why a malformed config degraded to the default list, when it did. */
	config_error?: string | null;
}

/** One armed stream, as the recorder reports it. `fill` is the buffer's occupancy, 0…1. */
export interface RecordStreamStatus {
	node: string;
	slot: string;
	engine: string;
	file: string;
	frames: number;
	dropped: number;
	fill: number;
}

/** The recording SESSION — runtime, so it rides `record status` and `record_changed`, never the
 * document, which owns what is armed. */
export interface RecordStatus {
	running: boolean;
	folder: string | null;
	elapsed: number | null;
	streams: RecordStreamStatus[];
	error: string | null;
}

export type ControlEvent =
	| { event: 'logs'; payload: LogBatch }
	| { event: 'hello'; payload: GraphSnapshot }
	// The node itself arrives via the doc; this carries no projection of it.
	| { event: 'node_added'; payload: { uid: string } }
	| {
			event: 'state_update';
			payload: {
				node: string;
				params: Record<string, Record<string, ParamDescriptor>>;
				stage?: NodeStage;
				// Re-pushed on the state plane, so a lost first error still surfaces and a clear lifts it.
				error?: string | null;
				// Which tier the node's type runs on now; a GIL demotion moves it mid-session.
				runtime?: NodeRuntime | null;
				// Params whose ⟳ refresh completed on this push, so the UI can clear the spinner.
				refreshed_params?: [string, string][];
			};
	  }
	| { event: 'error'; payload: { node: string; error: string | null } }
	| {
			event: 'node_stage';
			payload: { node: string; stage: NodeStage; error?: string | null; runtime?: NodeRuntime | null };
	  }
	// Every node's rate, one message per period.
	| { event: 'node_stats'; payload: { stats: Record<string, NodeStats> } }
	// Every driven node's LIVE source state, both maps whole: what its driven params evaluate to,
	// and what they fail with. Applied surgically, never a wholesale params replace.
	| { event: 'param_values'; payload: { nodes: Record<string, LiveSource> } }
	| { event: 'unsaved_changes'; payload: { unsaved_changes: boolean } }
	| { event: 'save_path_changed'; payload: { save_path: string | null } }
	// The whole recording state, so a client never has to diff transitions.
	| { event: 'record_changed'; payload: RecordStatus }
	// The palette changed under an already-connected client; `hello` carries it to an arriving one.
	| { event: 'node_types'; payload: { types: NodeTypeInfo[] } }
	// Carries the WHOLE roster, so a client never has to diff transitions.
	| { event: 'harness_changed'; payload: HarnessRoster }
	| { event: 'graph_replaced'; payload: GraphSnapshot }
	// The whole document — on connect, and again to recover a client that lagged past the ring.
	| { event: 'doc_state'; payload: { v: number; doc: Record<string, unknown> } }
	// `from` is the version the delta applies TO, `v` the version it produces.
	| { event: 'doc_patch'; payload: { from: number; v: number; ops: import('$lib/crdt/ops').Op[] } };

type EventHandler = (ev: ControlEvent) => void;

type Pending = {
	resolve: (v: unknown) => void;
	reject: (e: Error) => void;
};

/** What the actor's next undo and redo would take back, by label; null where there is none. */
export interface HistoryLabels {
	undo: string | null;
	redo: string | null;
}

/** One history step several writes land in: the token the manager merges by, and its label. */
export interface Step {
	group: string;
	label: string;
}

/** The two seams between the control plane and the history store, so neither imports the other:
 * every call asks for the actor's context, and every reply that moved the history reports it. */
export const historyFeed = {
	labels: (_: HistoryLabels): void => {},
	context: (): unknown => null
};

/** Minimal structural surface of the control client — the seam a test fake substitutes for. */
export interface Control {
	/** This client's stable ACTOR id; it scopes the manager's per-actor undo history. */
	readonly actor: string;
	call<T = unknown>(op: OpName, payload?: Record<string, unknown>, step?: Step): Promise<T>;
	/** One step of a drag: the op as a PREVIEW, no reply, and the newest per `key` each frame. */
	preview(key: string, op: OpName, payload: Record<string, unknown>): void;
	on(fn: (ev: ControlEvent) => void): () => void;
	onConnect(fn: (c: boolean) => void): () => void;
}

const isLabels = (v: unknown): v is HistoryLabels =>
	typeof v === 'object' && v !== null && 'undo' in v && 'redo' in v;

/** This tab's stable actor id, minted once per tab in `sessionStorage`. */
function readOrMintActor(): string {
	const fresh = () => `s${Date.now()}-${Math.floor(Math.random() * 1e9)}`;
	try {
		// The STORAGE key keeps its historical spelling: renaming it would hand every open tab a
		// fresh actor mid-upgrade, orphaning its undo stack.
		const KEY = 'goofi:session';
		let s = sessionStorage.getItem(KEY);
		if (!s) {
			s = crypto?.randomUUID?.() ?? fresh();
			sessionStorage.setItem(KEY, s);
		}
		return s;
	} catch {
		return fresh();
	}
}

export class ControlClient implements Control {
	private ws: WebSocket | null = null;
	private url: string;
	readonly actor = readOrMintActor();
	private nextId = 1;
	private pending = new Map<number, Pending>();
	/** The previews of the coming frame, one per key: a drag sends where it is, never where it was. */
	private previews = new Map<string, { op: OpName; payload: Record<string, unknown> }>();
	private previewFrame = 0;
	private handlers = new Set<EventHandler>();
	private connectListeners = new Set<(connected: boolean) => void>();
	private protocolListeners = new Set<(mismatch: boolean) => void>();
	private _connected = false;
	private _protocolMismatch = false;
	private retryMs = 250;

	constructor(url?: string) {
		// The actor rides the URL so the hello already carries this tab's undo and redo.
		this.url = url ?? `${wsUrl(['control'])}?actor=${encodeURIComponent(this.actor)}`;
	}

	connect(): void {
		if (this.ws) return;
		const ws = new WebSocket(this.url);
		this.ws = ws;

		ws.addEventListener('open', () => {
			this.retryMs = 250;
			this._setConnected(true);
		});
		ws.addEventListener('message', (e) => this._onMessage(e));
		ws.addEventListener('close', () => this._onClose());
	}

	private _onMessage(e: MessageEvent): void {
		let msg: unknown;
		try {
			msg = JSON.parse(e.data);
		} catch {
			return;
		}
		if (typeof msg !== 'object' || msg === null) return;
		const obj = msg as Record<string, unknown>;
		if ('id' in obj && typeof obj.id === 'number') {
			const id = obj.id;
			const pending = this.pending.get(id);
			if (!pending) return;
			this.pending.delete(id);
			if ('error' in obj) pending.reject(new Error(String(obj.error)));
			else pending.resolve(obj.result);
			if (isLabels(obj.history)) historyFeed.labels(obj.history);
			return;
		}
		if ('event' in obj && typeof obj.event === 'string') {
			if (obj.event === 'hello') this._checkProtocol(obj.payload);
			for (const h of this.handlers) {
				try {
					h(msg as ControlEvent);
				} catch (err) {
					console.error('control handler crashed', err);
				}
			}
		}
	}

	/** Latch a protocol mismatch from `hello`. One-way: only a reload clears it. */
	private _checkProtocol(payload: unknown): void {
		const remote =
			payload && typeof payload === 'object'
				? (payload as { protocol_version?: unknown }).protocol_version
				: undefined;
		if (this._protocolMismatch || remote === PROTOCOL_VERSION) return;
		this._protocolMismatch = true;
		for (const h of this.protocolListeners) h(true);
	}

	private _onClose(): void {
		this.ws = null;
		this._setConnected(false);
		for (const [, p] of this.pending) p.reject(new Error('control socket closed'));
		this.pending.clear();
		const delay = this.retryMs;
		this.retryMs = Math.min(this.retryMs * 2, 5000);
		setTimeout(() => this.connect(), delay);
	}

	private _setConnected(v: boolean): void {
		if (this._connected === v) return;
		this._connected = v;
		for (const h of this.connectListeners) h(v);
	}

	on(handler: EventHandler): () => void {
		this.handlers.add(handler);
		return () => this.handlers.delete(handler);
	}

	onConnect(handler: (connected: boolean) => void): () => void {
		this.connectListeners.add(handler);
		handler(this._connected);
		return () => this.connectListeners.delete(handler);
	}

	/** Subscribe to a protocol-version mismatch (fires once, immediately if already mismatched). */
	onProtocolMismatch(handler: (mismatch: boolean) => void): () => void {
		this.protocolListeners.add(handler);
		if (this._protocolMismatch) handler(true);
		return () => this.protocolListeners.delete(handler);
	}

	call<T = unknown>(op: OpName, payload: Record<string, unknown> = {}, step?: Step): Promise<T> {
		if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
			return Promise.reject(new Error('control socket not connected'));
		}
		// A drag's last preview goes out before the op that ends it: order on the wire is order here.
		this.flushPreviews();
		const id = this.nextId++;
		return new Promise<T>((resolve, reject) => {
			this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
			// `actor` rides at the top level: the manager scopes its undo/redo history by it —
			// whose undo, where GOOFI_SESSION names which server.
			this.ws!.send(JSON.stringify({ id, op, payload, actor: this.actor, context: historyFeed.context(), ...step }));
		});
	}
	preview(key: string, op: OpName, payload: Record<string, unknown>): void {
		this.previews.set(key, { op, payload });
		if (this.previewFrame) return;
		this.previewFrame = requestAnimationFrame(() => this.flushPreviews());
	}

	private flushPreviews(): void {
		cancelAnimationFrame(this.previewFrame);
		this.previewFrame = 0;
		const open = this.ws?.readyState === WebSocket.OPEN;
		for (const [key, { op, payload }] of this.previews) {
			if (open) this.ws!.send(JSON.stringify({ op, payload, actor: this.actor, preview: key }));
		}
		this.previews.clear();
	}
}

let _client: ControlClient | null = null;
export function getControl(): ControlClient {
	if (!_client) {
		_client = new ControlClient();
		_client.connect();
	}
	return _client;
}
