/** Which params a reader is actually interested in, so a plugin's hundreds collapse to the few in play. */
import { PARAM_MODES, type ParamDescriptor, type ParamMode } from '$lib/api/types';
import type { NodeInstanceInfo } from '$lib/api/control';
import type { ParamGroups } from './paramSearch';

/** A node's cleared zero points, keyed `group/name`. Absent for a node nobody has cleared. */
export type Baseline = NodeInstanceInfo['baseline'];
/** One param's zero point: what it held at the last clear, or with none its default and no source. */
export type ParamBaseline = NonNullable<Baseline>[string];

/** How a param is addressed in a baseline, and in the non-default list beside it. */
export const paramKey = (group: string, name: string): string => `${group}/${name}`;

/** A param's zero point: its baseline entry, else its factory default on a constant. */
export function zero(d: ParamDescriptor, base: Baseline, group: string, name: string): ParamBaseline {
	return base?.[paramKey(group, name)] ?? { value: d.default, mode: 'constant', expression: '' };
}

/** Whether two zero points are the same one. `Object.is`, so a NaN default compares equal to
 *  itself and `settleNonDefault` cannot answer a fresh list forever. */
function sameZero(a: ParamBaseline, b: ParamBaseline): boolean {
	return (
		Object.is(a.value, b.value) &&
		(a.mode ?? 'constant') === (b.mode ?? 'constant') &&
		(a.expression ?? '') === (b.expression ?? '')
	);
}

/** Whether the active source moved from its zero point; the zero is recorded only because presets
 *  move a factory default. */
export function isModified(d: ParamDescriptor, base?: Baseline, group = '', name = ''): boolean {
	const z = zero(d, base, group, name);
	const mode = d.mode ?? 'constant';
	if (mode !== (z.mode ?? 'constant')) return true;
	// Only the ACTIVE source is compared. The text is RETAINED across a mode switch, so an
	// expression left behind by a param now on a constant is not a change to that param.
	if (mode === 'expression') return (d.expression ?? '') !== (z.expression ?? '');
	// A pulse holds no value, so nothing about one can have moved.
	if (d.type === 'pulse') return false;
	return d.value !== z.value;
}

/** What a reader has narrowed one group's list to: WHERE a param takes its value from, AND whether
 *  it still holds its default. */
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

/** Whether the inspector shows a param: its `show` holds for its controller's value, and the
 *  controller is shown too; a slot of a list is one its count has opened. A hidden param keeps its
 *  value and its source. */
export function shown(groups: ParamGroups | undefined, group: string, name: string): boolean {
	const d = groups?.[group]?.[name];
	const role = d?.role;
	if (role?.as === 'member' && role.slot != null) {
		const count = groups?.[group]?.[role.section];
		if (!count || typeof count.value !== 'number' || role.slot >= count.value) return false;
	}
	const show = d?.show;
	const controller = show && groups?.[show.group]?.[show.name];
	return !controller || (show.any_of.includes(String(controller.value)) && shown(groups, show.group, show.name));
}
