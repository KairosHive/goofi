/** A node as the editor reads it: ONE stable object per uid, whose fields read the replica, the
 * catalog and the runtime overlay on each access, so a reader re-runs for its own leaf alone. */
import type { NodeInstanceInfo, NodeTypeInfo, NodeStage, NodeStats, NodeRuntime } from '$lib/api/control';
import { PARAM_MODES, type ElementSource, type ParamDescriptor, type ParamMode } from '$lib/api/types';
import { boundaryType } from '$lib/api/vocab';
import { nodesMap, type Doc, type FacadeFace } from './graphDoc';
import { ROOT_ID } from '$lib/editor/subpatchScene';
import { isObj, obj, type Obj } from './ops';

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
	default: null,
	doc: null,
	refreshable: false,
	mode: 'constant',
	expression: null,
	reference: null,
	triggers: false,
	error: null,
	section: 0,
	show: null,
	role: null
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
	const dims = base.type === 'num' && Array.isArray(base.value) ? base.value.length : 1;
	// One element's source record: the leaf `name[i]` beside the param, and its live value.
	const element = (i: number): ElementSource => {
		const key = `${name}[${i}]`;
		const el = obj(obj(obj(obj(nodesMap(cx.doc())[uid]).params)[group])[key]);
		const m = el.mode;
		const mode = typeof m === 'string' && (PARAM_MODES as readonly string[]).includes(m) ? (m as ParamMode) : 'constant';
		const live = mode !== 'constant' ? cx.runtime(uid)?.values?.[group]?.[key] : undefined;
		return {
			mode,
			expression: typeof el.expression === 'string' ? el.expression : null,
			reference: typeof el.reference === 'string' ? el.reference : null,
			triggers: el.triggers === true,
			error: cx.runtime(uid)?.errors?.[group]?.[key] ?? null,
			value: typeof live === 'number' ? live : undefined
		};
	};
	return accessors(p, {
		mode,
		...(dims > 1 ? { elements: () => Array.from({ length: dims }, (_, i) => element(i)) } : {}),
		expression: () => (typeof leaf().expression === 'string' ? leaf().expression : null),
		reference: () => (typeof leaf().reference === 'string' ? leaf().reference : null),
		triggers: () => leaf().triggers === true,
		error: () => cx.runtime(uid)?.errors?.[group]?.[name] ?? null,
		// What a param SHOWS: a driven one reads what its source evaluated to, a withdrawn value
		// falls back to the committed leaf, and neither leaves the declared default standing.
		value: () => {
			const committed = leaf().value;
			const held =
				Array.isArray(committed) || typeof committed === 'number' || typeof committed === 'string' || typeof committed === 'boolean'
					? committed
					: undefined;
			const driven = base.type !== 'pulse' && mode() !== 'constant';
			const live = driven ? cx.runtime(uid)?.values?.[group]?.[name] : undefined;
			const v = live !== undefined ? live : held;
			const whole = v !== undefined ? v : base.value;
			// An element driven on its own shows what IT evaluated to, in its dimension.
			if (dims > 1 && Array.isArray(whole)) {
				const merged = [...whole];
				for (let i = 0; i < dims; i++) {
					const e = element(i).value;
					if (e !== undefined) merged[i] = e;
				}
				return merged;
			}
			return whole;
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
	const text = (key: string): string | undefined => {
		const value = rec()[key];
		return typeof value === 'string' ? value : undefined;
	};
	const type = (): string => text('type') ?? '';
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
	const viewers = $derived(obj(rec().viewers) as NodeInstanceInfo['viewers']);
	const baseline = $derived(isObj(rec().baseline) ? rec().baseline as NodeInstanceInfo['baseline'] : undefined);
	const virtual = (): boolean => !!boundaryType(type()) || cx.face(uid) !== undefined;
	return accessors({ uid } as NodeInstanceInfo, {
		name: () => text('name') ?? '',
		type,
		pos: () => {
			const p = rec().pos;
			return [0, 1].map((i) => Array.isArray(p) && typeof p[i] === 'number' ? p[i] : 0);
		},
		scope: () => text('scope') ?? ROOT_ID,
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
