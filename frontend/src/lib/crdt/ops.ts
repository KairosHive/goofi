/** The replica delta: path operations, the browser half of `goofi_bridge::doc`. A path descends
 * the document's maps, `[root, key, …]`; a `put` sets the value there and a `del` removes it. */
import type { JsonValue, Op } from '$lib/api/generated';

export type { Op };
export type Obj = Record<string, unknown>;

export const isObj = (v: unknown): v is Obj => v !== null && typeof v === 'object' && !Array.isArray(v);
export const obj = (v: unknown): Obj => (isObj(v) ? v : {});

/** What `target` itself holds at `key`. A key is a name a user gave (a variable group, a plugin's
 * entry), and `__proto__` or `toString` is one: read plainly, it answers the prototype's. */
const own = (target: Obj, key: string): unknown => (Object.hasOwn(target, key) ? target[key] : undefined);

/** Set `target[key]` as its own entry: a plain write to `__proto__` sets the prototype instead. */
function set(target: Obj, key: string, value: unknown): void {
	if (key === '__proto__') Object.defineProperty(target, key, { value, writable: true, enumerable: true, configurable: true });
	else target[key] = value;
}

/** Apply `ops` into `target`, in place. A `put` makes the maps on its way and lands LEAF by leaf,
 * so a reader of a leaf it did not move never re-runs; a `del` of what is absent is nothing. */
export function applyOps(target: Obj, ops: Op[]): void {
	for (const op of ops) {
		const last = op.path[op.path.length - 1];
		let cur: Obj | null = target;
		for (const seg of op.path.slice(0, -1)) {
			if (!isObj(own(cur, seg))) {
				if (op.op === 'del') {
					cur = null;
					break;
				}
				set(cur, seg, {});
			}
			cur = own(cur, seg) as Obj;
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
	const held = own(target, key);
	if (isObj(held) && isObj(value) && keepsOrder(held, value)) {
		for (const k of Object.keys(held)) if (!Object.hasOwn(value, k)) delete held[k];
		for (const [k, v] of Object.entries(value)) assign(held, k, v);
		return;
	}
	// An array or a scalar lands whole, and only when it moved: an equal one left alone keeps
	// its readers asleep.
	if (held !== value && JSON.stringify(held) !== JSON.stringify(value)) set(target, key, value);
}

/** The ops that take `before` to `after`: an entry of a root map re-sent whole when it moved,
 * any other root re-sent whole — as the manager computes them. */
export function diffOps(before: Obj, after: Obj): Op[] {
	const ops: Op[] = [];
	const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
	for (const [root, av] of Object.entries(after)) {
		const bv = before[root];
		if (isObj(bv) && isObj(av) && !keepsOrder(bv, av)) {
			ops.push({ op: 'put', path: [root], value: av as JsonValue });
		} else if (isObj(bv) && isObj(av)) {
			for (const [key, v] of Object.entries(av)) {
				if (!(key in bv) || !same(bv[key], v)) ops.push({ op: 'put', path: [root, key], value: v as JsonValue });
			}
			for (const key of Object.keys(bv)) {
				if (!(key in av)) ops.push({ op: 'del', path: [root, key] });
			}
		} else if (!(root in before) || !same(bv, av)) {
			ops.push({ op: 'put', path: [root], value: av as JsonValue });
		}
	}
	for (const root of Object.keys(before)) {
		if (!(root in after)) ops.push({ op: 'del', path: [root] });
	}
	return ops;
}

/** Whether per-key ops on `before` leave its keys in `after`'s order. */
function keepsOrder(before: Obj, after: Obj): boolean {
	const kept = Object.keys(before).filter((k) => Object.hasOwn(after, k));
	const added = Object.keys(after).filter((k) => !Object.hasOwn(before, k));
	return [...kept, ...added].join('\0') === Object.keys(after).join('\0');
}
