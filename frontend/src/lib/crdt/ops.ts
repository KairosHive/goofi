/** The replica delta: path operations, the browser half of `goofi_bridge::doc`. A path is
 * `[root]` or `[root, key]`; a `put` sets the value there and a `del` removes it. */

type Obj = Record<string, unknown>;

export type Op = { op: 'put'; path: string[]; value: unknown } | { op: 'del'; path: string[] };

const isObj = (v: unknown): v is Obj => v !== null && typeof v === 'object' && !Array.isArray(v);

/** Apply `ops` into `target`, in place. A `put` makes the maps on its way and lands LEAF by
 * leaf, so a reader of a leaf the value did not move never re-runs; a `del` of what is absent is
 * nothing. */
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

/** Make `target[key]` equal `value`, writing only the leaves that differ. */
function assign(target: Obj, key: string, value: unknown): void {
	const held = target[key];
	if (isObj(held) && isObj(value)) {
		for (const k of Object.keys(held)) if (!(k in value)) delete held[k];
		for (const [k, v] of Object.entries(value)) assign(held, k, v);
		return;
	}
	// An array or a scalar lands whole, and only when it moved: an equal one left alone keeps
	// its readers asleep.
	if (held !== value && JSON.stringify(held) !== JSON.stringify(value)) target[key] = value;
}

/** The ops that take `before` to `after`: an entry of a root map re-sent whole when it moved,
 * any other root re-sent whole — as the manager computes them. */
export function diffOps(before: Obj, after: Obj): Op[] {
	const ops: Op[] = [];
	const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
	for (const [root, av] of Object.entries(after)) {
		const bv = before[root];
		if (isObj(bv) && isObj(av)) {
			for (const [key, v] of Object.entries(av)) {
				if (!(key in bv) || !same(bv[key], v)) ops.push({ op: 'put', path: [root, key], value: v });
			}
			for (const key of Object.keys(bv)) {
				if (!(key in av)) ops.push({ op: 'del', path: [root, key] });
			}
		} else if (!(root in before) || !same(bv, av)) {
			ops.push({ op: 'put', path: [root], value: av });
		}
	}
	for (const root of Object.keys(before)) {
		if (!(root in after)) ops.push({ op: 'del', path: [root] });
	}
	return ops;
}
