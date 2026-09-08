/** The three-way merge of a render `NodeInstanceInfo` from doc leaves, the static per-type catalog and
 * the event-sourced runtime overlay. */
import type { NodeInstanceInfo, NodeTypeInfo, NodeStage, NodeStats, NodeRuntime } from '$lib/api/control';
import type { ParamDescriptor } from '$lib/api/types';
import type { NodeView, DocParamLeaves, FacadeFace } from './graphDoc';

export type { DocParamLeaves };

export type ViewersBlob = NodeInstanceInfo['viewers'];
export type BaselineBlob = NodeInstanceInfo['baseline'];

/** Per-param runtime overlay (event-sourced, never in the doc). */
export interface ParamRuntime {
	error?: string | null;
	/** Refreshed StringParam options (device/stream pickers) — override the catalog default. */
	options?: string[] | null;
	/** For a driven param, its LIVE evaluated value — it overrides the committed doc leaf. */
	liveValue?: unknown;
}

/** Node-level runtime overlay (event-sourced, never in the doc). */
export interface RuntimeOverlay {
	error?: string | null;
	stage?: NodeStage;
	/** Which tier the node's type currently runs on; the GIL tripwire can demote a Python type. */
	runtime?: NodeRuntime;
	stats?: NodeStats | null;
	params?: Record<string, Record<string, ParamRuntime>>;
}

/** A descriptor for a param whose node type is absent from the catalog: no bounds, no options. */
function unknownParam(): ParamDescriptor {
	return {
		type: 'unknown',
		value: undefined,
		default: undefined,
		doc: null,
		refreshable: false,
		mode: 'constant',
		expression: null,
		reference: null,
		triggers: false,
		error: null
	};
}

function mergeParam(
	catalog: ParamDescriptor | undefined,
	leaf: DocParamLeaves[string][string] | undefined,
	runtime: ParamRuntime | undefined
): ParamDescriptor {
	// Shallow-copy so the shared catalog descriptor is never mutated.
	const p: ParamDescriptor = catalog ? { ...catalog } : unknownParam();
	p.mode = leaf?.source?.mode ?? 'constant';
	p.expression = leaf?.source?.expr ?? null;
	p.reference = leaf?.source?.ref ?? null;
	p.triggers = leaf?.source?.triggers === true;
	p.error = runtime?.error ?? null;
	if (p.type === 'string' && runtime?.options !== undefined) p.options = runtime.options;
	showValue(p, leaf?.value, runtime?.liveValue);
	return p;
}

/** What a param SHOWS: a driven one reads what its source evaluated to, a withdrawn value falls
 *  back to the committed leaf, and neither leaves the declared default standing. ONE rule, so the
 *  full assemble and the live event that updates one node in place cannot disagree. */
function showValue(p: ParamDescriptor, committed: unknown, live: unknown): void {
	const driven = p.type !== 'pulse' && p.mode !== 'constant';
	const v = driven && live !== undefined ? live : committed;
	if (v !== undefined) (p as { value: unknown }).value = v;
}

/** Write one node's live source state onto its assembled params, in place: the `param_values`
 *  event's two whole maps, which is why an absent key reads as withdrawn rather than unchanged. */
export function applyLiveParams(
	node: NodeInstanceInfo,
	committed: DocParamLeaves,
	values: Record<string, Record<string, unknown>>,
	errors: Record<string, Record<string, string>>
): void {
	for (const [group, names] of Object.entries(node.params)) {
		for (const [name, p] of Object.entries(names)) {
			p.error = errors[group]?.[name] ?? null;
			showValue(p, committed[group]?.[name]?.value, values[group]?.[name]);
		}
	}
}

/** Assemble a full render `NodeInstanceInfo` from the doc view, catalog and runtime overlay. A
 * facade has no catalog entry — it runs nothing — so its `face` carries the slots instead. */
export function assembleNode(
	view: NodeView,
	docParams: DocParamLeaves,
	viewers: ViewersBlob,
	baseline: BaselineBlob,
	catalog: NodeTypeInfo | undefined,
	runtime: RuntimeOverlay,
	face?: FacadeFace
): NodeInstanceInfo {
	const params: Record<string, Record<string, ParamDescriptor>> = {};
	const groupNames = catalog ? Object.keys(catalog.params) : Object.keys(docParams);
	for (const group of groupNames) {
		params[group] = {};
		const names = catalog ? Object.keys(catalog.params[group]) : Object.keys(docParams[group] ?? {});
		for (const name of names) {
			params[group][name] = mergeParam(
				catalog?.params[group]?.[name],
				docParams[group]?.[name],
				runtime.params?.[group]?.[name]
			);
		}
	}

	return {
		uid: view.uid,
		name: view.name,
		type: view.type,
		doc: catalog?.doc ?? '',
		editor: catalog?.editor,
		input_slots: face?.input_slots ?? catalog?.input_slots ?? {},
		input_multi: catalog?.input_multi,
		output_slots: face?.output_slots ?? catalog?.output_slots ?? {},
		slot_labels: face?.slot_labels,
		params,
		pos: view.pos,
		viewers,
		baseline,
		scope: view.scope,
		error: runtime.error ?? null,
		stage: runtime.stage,
		runtime: runtime.runtime,
		stats: runtime.stats ?? null,
		subpatch: face && { memberCount: face.memberCount }
	};
}
