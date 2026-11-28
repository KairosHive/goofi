/** The replica delta: path operations, the browser half of `goofi_bridge::doc`. A path descends
 * the document's maps, `[root, key, …]`; a `put` sets the value there and a `del` removes it. */
import type { JsonValue, Op } from '$lib/api/generated';

export type { Op };
export type Obj = Record<string, unknown>;

export const isObj = (v: unknown): v is Obj => v !== null && typeof v === 'object' && !Array.isArray(v);
export const obj = (v: unknown): Obj => (isObj(v) ? v : {});

/** Apply `ops` into `target`, in place. A `put` makes the maps on its way and lands LEAF by leaf,
 * so a reader of a leaf it did not move never re-runs; a `del` of what is absent is nothing. */
export function applyOps(target: Obj, ops: Op[]): void {
	for (const op of ops) {
		const last = op.path[op.path.length - 1];
		let cur: Obj | null = target;
		for (const seg of op.path.slice(0, -1)) {
			if (!isObj(cur[seg])) {
				if (op.op === 'del') {
					cur = null;
					break;
				}
				cur[seg] = {};
			}
			cur = cur[seg] as Obj;
		}
		if (!cur) continue;
		if (op.op === 'put') assign(cur, last, op.value);
		else delete cur[last];
	}
}

/** Make `target[key]` equal `value`, writing only the leaves that differ. A map whose keys moved
 * lands whole: a state proxy keeps the order its keys were born in, whatever is deleted and put
 * back, so the manager's order, which a list shows, takes a new object. */
function assign(target: Obj, key: string, value: unknown): void {
	const held = target[key];
	if (isObj(held) && isObj(value) && keepsOrder(held, value)) {
		for (const k of Object.keys(held)) if (!(k in value)) delete held[k];
		for (const [k, v] of Object.entries(value)) assign(held, k, v);
		return;
	}
	// An array or a scalar lands whole, and only when it moved: an equal one left alone keeps
	// its readers asleep.
	if (held !== value && JSON.stringify(held) !== JSON.stringify(value)) target[key] = value;
}

/** Whether per-key ops on `before` leave its keys in `after`'s order. */
export function keepsOrder(before: Obj, after: Obj): boolean {
	const kept = Object.keys(before).filter((k) => k in after);
	const added = Object.keys(after).filter((k) => !(k in before));
	return [...kept, ...added].join('\0') === Object.keys(after).join('\0');
}
