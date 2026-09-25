/**
 * A node as the editor reads it: ONE stable object per uid whose fields are read off the reactive
 * replica, the static catalog and the event-sourced runtime overlay on every access. Nothing is
 * assembled or diffed: a reader of `node.name` re-runs for that leaf of the document alone.
 */
import type { NodeInstanceInfo, NodeTypeInfo, NodeStage, NodeStats, NodeRuntime } from '$lib/api/control';
import { PARAM_MODES, type ParamDescriptor, type ParamMode } from '$lib/api/types';
import { boundaryType } from '$lib/api/vocab';
import { nodesMap, nodeView, viewersJson, baselineJson, type Doc, type FacadeFace } from './graphDoc';

/** What the runtime planes report about one node — never in the document. */
export interface RuntimeOverlay {
	error?: string | null;
	stage?: NodeStage;
	/** Which tier the node's type currently runs on; the GIL tripwire can demote a Python type. */
	runtime?: NodeRuntime;
	stats?: NodeStats | null;
	/** Refreshed StringParam options (device/stream pickers), over the catalog's. */
	options?: Record<string, Record<string, string[] | null>>;
	/** A driven param's LIVE evaluated value, which never reaches the document. */
	values?: Record<string, Record<string, unknown>>;
	/** The active source's bind, compile or arrival error per param. */
	errors?: Record<string, Record<string, string>>;
}

/** Where a live node reads from: each is a reactive read, so the fields follow their sources. */
export interface ViewSources {
	doc(): Doc;
	catalog(type: string): NodeTypeInfo | undefined;
	face(uid: string): FacadeFace | undefined;
	runtime(uid: string): RuntimeOverlay | undefined;
}

type Obj = Record<string, unknown>;
const obj = (v: unknown): Obj => (v !== null && typeof v === 'object' && !Array.isArray(v) ? (v as Obj) : {});

/** Define `getters` as enumerable accessors on `target`, so a spread or a stringify reads them too. */
function accessors<T extends object>(target: T, getters: Record<string, () => unknown>): T {
	for (const [key, get] of Object.entries(getters)) {
		Object.defineProperty(target, key, { get, enumerable: true, configurable: true });
	}
	return target;
}

/** A descriptor for a param whose node type is absent from the catalog: no bounds, no options. */
const UNKNOWN: ParamDescriptor = {
	type: 'unknown',
	value: undefined,
	default: undefined,
	doc: null,
	refreshable: false,
	mode: 'constant',
	expression: null,
	reference: null,
	triggers: false,
	error: null,
	section: 0,
	show: null
};

/** One param: the catalog's static fields, and the document's and the runtime's live ones. */
function liveParam(uid: string, group: string, name: string, catalog: ParamDescriptor | undefined, cx: ViewSources): ParamDescriptor {
	const base = catalog ?? UNKNOWN;
	const leaf = (): Obj => obj(obj(obj(obj(nodesMap(cx.doc())[uid]).params)[group])[name]);
	const mode = (): ParamMode => {
		const m = leaf().mode;
		return typeof m === 'string' && (PARAM_MODES as readonly string[]).includes(m) ? (m as ParamMode) : 'constant';
	};
	const p = { ...base } as ParamDescriptor;
	return accessors(p, {
		mode,
		expression: () => (typeof leaf().expr === 'string' ? leaf().expr : null),
		reference: () => (typeof leaf().ref === 'string' ? leaf().ref : null),
		triggers: () => leaf().triggers === true,
		error: () => cx.runtime(uid)?.errors?.[group]?.[name] ?? null,
		// What a param SHOWS: a driven one reads what its source evaluated to, a withdrawn value
		// falls back to the committed leaf, and neither leaves the declared default standing.
		value: () => {
			const committed = leaf().value;
			const held = typeof committed === 'number' || typeof committed === 'string' || typeof committed === 'boolean' ? committed : undefined;
			const driven = base.type !== 'pulse' && mode() !== 'constant';
			const live = driven ? cx.runtime(uid)?.values?.[group]?.[name] : undefined;
			const v = live !== undefined ? live : held;
			return v !== undefined ? v : base.value;
		},
		...(base.type === 'string'
			? {
					options: () => {
						const refreshed = cx.runtime(uid)?.options?.[group]?.[name];
						return refreshed !== undefined ? refreshed : (base as { options: string[] | null }).options;
					}
				}
			: {})
	});
}

/** The live node for `uid`. Its param map is rebuilt only when its SHAPE moves (the catalog, or the
 * document's own groups for a type the catalog lacks); every leaf inside it is a live read. */
export function liveNode(uid: string, cx: ViewSources): NodeInstanceInfo {
	const rec = (): Obj => obj(nodesMap(cx.doc())[uid]);
	const type = (): string => (typeof rec().type === 'string' ? (rec().type as string) : '');
	const catalog = () => cx.catalog(type());
	const params = $derived.by(() => {
		const cat = catalog();
		const groups = cat ? Object.keys(cat.params) : Object.keys(obj(rec().params));
		const out: Record<string, Record<string, ParamDescriptor>> = {};
		for (const group of groups) {
			out[group] = {};
			const names = cat ? Object.keys(cat.params[group]) : Object.keys(obj(obj(rec().params)[group]));
			for (const name of names) out[group][name] = liveParam(uid, group, name, cat?.params[group]?.[name], cx);
		}
		return out;
	});
	const viewers = $derived((viewersJson(cx.doc(), uid) ?? {}) as NodeInstanceInfo['viewers']);
	const baseline = $derived(baselineJson(cx.doc(), uid) as NodeInstanceInfo['baseline']);
	const virtual = (): boolean => !!boundaryType(type()) || cx.face(uid) !== undefined;
	return accessors({ uid } as NodeInstanceInfo, {
		name: () => nodeView(cx.doc(), uid)?.name ?? '',
		type,
		pos: () => nodeView(cx.doc(), uid)?.pos ?? [0, 0],
		scope: () => nodeView(cx.doc(), uid)?.scope ?? '',
		doc: () => catalog()?.doc ?? '',
		editor: () => catalog()?.editor,
		input_slots: () => cx.face(uid)?.input_slots ?? catalog()?.input_slots ?? {},
		input_multi: () => catalog()?.input_multi,
		output_slots: () => cx.face(uid)?.output_slots ?? catalog()?.output_slots ?? {},
		slot_labels: () => cx.face(uid)?.slot_labels,
		params: () => params,
		viewers: () => viewers,
		baseline: () => baseline,
		error: () => cx.runtime(uid)?.error ?? null,
		// A leaf is `creating` until its own thread says otherwise; a virtual node runs nothing, so
		// it is born at the stage the backend already answers for it.
		stage: () => cx.runtime(uid)?.stage ?? (virtual() ? 'ready' : 'creating'),
		runtime: () => cx.runtime(uid)?.runtime,
		stats: () => cx.runtime(uid)?.stats ?? null,
		subpatch: () => {
			const face = cx.face(uid);
			return face && { memberCount: face.memberCount };
		}
	});
}
