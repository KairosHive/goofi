/** Which params a reader is actually interested in, so a plugin's hundreds collapse to the few in play. */
import type { ParamDescriptor } from '$lib/api/types';

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

/** The zero point for one param: the recorded one, or the declared default with no source. */
function zero(d: ParamDescriptor, base: Baseline, group: string, name: string): ParamBaseline {
	return base?.[`${group}/${name}`] ?? { value: d.default, mode: 'constant', expression: '', reference: '' };
}

/**
 * Whether a param has CHANGED from its zero point — its constant value, its expression, or its
 * reference. DERIVED rather than recorded: a knob turned in a plugin's own window enters the
 * document by the same param door as any other author, so its value alone already says it moved,
 * and there is no second record of "which knobs were turned" to keep in step.
 *
 * What IS recorded is the zero point, and only because a plugin's declared default is its FACTORY
 * default: loading a preset moves hundreds of params off it at once, so the filter that exists to
 * show the few in play fills with everything the preset moved, and reshuffles at every preset
 * change. Clearing records the current state as the new zero.
 *
 * Being DRIVEN is deliberately not "touched" — an expression and a reference have filters of their
 * own, and folding them in here made "what did I change" and "what is driven" one question when
 * they are two.
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

/** Whether a param is driven by an expression. */
export function isExpression(d: ParamDescriptor): boolean {
	return d.mode === 'expression';
}

/** Whether a param is driven by a reference. */
export function isReference(d: ParamDescriptor): boolean {
	return d.mode === 'reference';
}

/** Which of the three filters are on. All off means no filter at all. */
export interface Filters {
	touched: boolean;
	expression: boolean;
	reference: boolean;
}

export const NO_FILTERS: Filters = { touched: false, expression: false, reference: false };

export function anyFilter(f: Filters): boolean {
	return f.touched || f.expression || f.reference;
}

/**
 * Whether one param survives the filters. The three are OR'd: each names a set worth seeing, and a
 * reader who wants "what I changed AND what I mapped" gets the union by turning both on. AND would
 * make every second combination empty.
 */
export function admits(f: Filters, d: ParamDescriptor, base?: Baseline, group = '', name = ''): boolean {
	if (!anyFilter(f)) return true;
	if (f.touched && isModified(d, base, group, name)) return true;
	if (f.expression && isExpression(d)) return true;
	if (f.reference && isReference(d)) return true;
	return false;
}

/** One row of a filtered list: the param, and the group it had to be fetched out of. */
export interface TouchedRow {
	group: string;
	name: string;
	descriptor: ParamDescriptor;
}

/**
 * Every param the filters admit, ACROSS the groups. Spanning them is the whole point: a knob was
 * turned in the plugin's own window, and which tab goofi filed it under is the one thing the reader
 * does not know — a per-tab filter answers "nothing here" while the count says otherwise.
 */
export function filteredRows(
	groups: Record<string, Record<string, ParamDescriptor>> | undefined,
	f: Filters,
	base?: Baseline,
	order?: string[]
): TouchedRow[] {
	const names = order ?? Object.keys(groups ?? {});
	return names.flatMap((group) =>
		Object.entries(groups?.[group] ?? {})
			.filter(([name, descriptor]) => admits(f, descriptor, base, group, name))
			.map(([name, descriptor]) => ({ group, name, descriptor }))
	);
}

/** The admitted subset of rows already gathered — a search narrowed to what the filters admit. */
export function onlyAdmitted(rows: TouchedRow[], f: Filters, base?: Baseline): TouchedRow[] {
	return rows.filter((r) => admits(f, r.descriptor, base, r.group, r.name));
}

/** How many of a node's params each filter would show, across every group. */
export function counts(
	groups: Record<string, Record<string, ParamDescriptor>> | undefined,
	base?: Baseline
): { touched: number; expression: number; reference: number } {
	let touched = 0;
	let expression = 0;
	let reference = 0;
	for (const [group, named] of Object.entries(groups ?? {})) {
		for (const [name, d] of Object.entries(named ?? {})) {
			if (isModified(d, base, group, name)) touched += 1;
			if (isExpression(d)) expression += 1;
			if (isReference(d)) reference += 1;
		}
	}
	return { touched, expression, reference };
}
