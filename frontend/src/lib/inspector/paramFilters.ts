/** Which params a reader is actually interested in, so a plugin's hundreds collapse to the few in play. */
import { PARAM_MODES, type ParamDescriptor, type ParamMode } from '$lib/api/types';
import type { ParamGroups, ParamHit } from './paramSearch';

/**
 * The zero point one param is measured from: what it held when the reader last cleared, or — with
 * nothing cleared — the type's own declared default with no source on it.
 */
export interface ParamBaseline {
	value?: unknown;
	mode?: string;
	expression?: string | null;
	reference?: string | null;
}

/** A node's cleared zero points, keyed `group/name`. Absent for a node nobody has cleared. */
export type Baseline = Record<string, ParamBaseline> | undefined;

/** How a param is addressed in a baseline, and in the non-default list beside it. */
export const paramKey = (group: string, name: string): string => `${group}/${name}`;

/** The zero point for one param: the recorded one, or the declared default with no source. */
function zero(d: ParamDescriptor, base: Baseline, group: string, name: string): ParamBaseline {
	return base?.[paramKey(group, name)] ?? { value: d.default, mode: 'constant', expression: '', reference: '' };
}

/** Whether two zero points are the same one. `Object.is`, so a NaN default compares equal to
 *  itself and `settleNonDefault` cannot answer a fresh list forever. */
function sameZero(a: ParamBaseline, b: ParamBaseline): boolean {
	return (
		Object.is(a.value, b.value) &&
		(a.mode ?? 'constant') === (b.mode ?? 'constant') &&
		(a.expression ?? '') === (b.expression ?? '') &&
		(a.reference ?? '') === (b.reference ?? '')
	);
}

/**
 * Whether a param has MOVED from its zero point — its constant value, its expression, or its
 * reference. DERIVED rather than recorded: a knob turned in a plugin's own window enters the
 * document by the same param door as any other author, so its value alone already says it moved,
 * and there is no second record of "which knobs were turned" to keep in step.
 *
 * What IS recorded is the zero point, and only because a plugin's declared default is its FACTORY
 * default: loading a preset moves hundreds of params off it at once, so the filter that exists to
 * show the few in play fills with everything the preset moved, and reshuffles at every preset
 * change. Clearing records the current state as the new zero.
 */
export function isModified(d: ParamDescriptor, base?: Baseline, group = '', name = ''): boolean {
	const z = zero(d, base, group, name);
	const mode = d.mode ?? 'constant';
	if (mode !== (z.mode ?? 'constant')) return true;
	// Only the ACTIVE source is compared. The other two texts are RETAINED across a mode switch, so
	// an expression left behind by a param now on a constant is not a change to that param.
	if (mode === 'expression') return (d.expression ?? '') !== (z.expression ?? '');
	if (mode === 'reference') return (d.reference ?? '') !== (z.reference ?? '');
	// A pulse holds no value, so nothing about one can have moved.
	if (d.type === 'pulse') return false;
	return d.value !== z.value;
}

/**
 * What a reader has narrowed one group's list to. The two questions are AND'd, because they are
 * about different things: WHERE a param takes its value from, and WHETHER it still holds its
 * default. Every source with `nonDefault` off is no narrowing at all, which is where a node opens.
 */
export interface Filters {
	/** Only the params that have moved off their zero point. */
	nonDefault: boolean;
	/** The sources admitted; every mode is everything. */
	sources: ParamMode[];
}

/** Everything — the state a node's inspector opens on. */
export const SHOW_ALL: Filters = { nonDefault: false, sources: [...PARAM_MODES] };

/** Whether the filters take anything away. */
export function narrowing(f: Filters): boolean {
	return f.nonDefault || f.sources.length < PARAM_MODES.length;
}

/** The filters with source `m` flipped — the source strip's whole behaviour. */
export function toggleSource(f: Filters, m: ParamMode): Filters {
	return {
		...f,
		sources: f.sources.includes(m) ? f.sources.filter((s) => s !== m) : [...f.sources, m]
	};
}

export type NonDefault = ReadonlyMap<string, ParamBaseline>;

/** Whether two lists say the same thing, so a caller can keep the one it holds. */
function sameList(a: NonDefault, b: NonDefault): boolean {
	if (a.size !== b.size) return false;
	for (const [key, z] of b) {
		const held = a.get(key);
		if (!held || !sameZero(held, z)) return false;
	}
	return true;
}

export function settleNonDefault(
	list: NonDefault,
	groups: ParamGroups | undefined,
	base?: Baseline,
	active: string | null = null
): NonDefault {
	const next = new Map<string, ParamBaseline>();
	for (const [group, named] of Object.entries(groups ?? {})) {
		for (const [name, d] of Object.entries(named ?? {})) {
			const key = paramKey(group, name);
			const z = zero(d, base, group, name);
			const held = list.get(key);
			if (isModified(d, base, group, name) || (key === active && held && sameZero(held, z))) next.set(key, z);
		}
	}
	return sameList(list, next) ? list : next;
}

/** Whether one param survives the filters. */
export function admits(f: Filters, d: ParamDescriptor, list: NonDefault, group = '', name = ''): boolean {
	if (!f.sources.includes(d.mode ?? 'constant')) return false;
	return !f.nonDefault || list.has(paramKey(group, name));
}

/** Every param the filters admit, in the groups `order` names and their order. */
export function filteredRows(
	groups: ParamGroups | undefined,
	f: Filters,
	list: NonDefault,
	order?: string[]
): ParamHit[] {
	const names = order ?? Object.keys(groups ?? {});
	return names.flatMap((group) =>
		Object.entries(groups?.[group] ?? {})
			.filter(([name, descriptor]) => admits(f, descriptor, list, group, name))
			.map(([name, descriptor]) => ({ group, name, descriptor }))
	);
}

/** The admitted subset of rows already gathered — a search narrowed to what the filters admit. */
export function onlyAdmitted(rows: ParamHit[], f: Filters, list: NonDefault): ParamHit[] {
	return rows.filter((r) => admits(f, r.descriptor, list, r.group, r.name));
}
