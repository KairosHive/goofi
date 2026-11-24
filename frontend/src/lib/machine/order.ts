/** The score's columns: every state in loop order, and the widths their intervals give them. The
 * largest loop leads, each smaller loop opens at the state it shares with what is placed, and a
 * state in no loop follows the state that leads to it; so a melody reads left to right along
 * the way a playhead most often goes. */
import type { Machine } from '$lib/api/generated';

/** How many simple cycles are enumerated before the search stops: a machine wired all-to-all has
 * more than anyone would read, and the longest are found first. */
const CYCLE_CAP = 400;

/** Every simple cycle among `succ`, each as the indices it visits, the largest first. From each
 * start only later indices are walked, so a cycle is found once, from its smallest member. */
function cycles(n: number, succ: number[][]): number[][] {
	const found: number[][] = [];
	const path: number[] = [];
	const onPath = new Array<boolean>(n).fill(false);
	function walk(start: number, at: number): void {
		if (found.length >= CYCLE_CAP) return;
		path.push(at);
		onPath[at] = true;
		for (const next of succ[at]) {
			if (next === start) found.push([...path]);
			else if (next > start && !onPath[next]) walk(start, next);
		}
		path.pop();
		onPath[at] = false;
	}
	for (let s = 0; s < n && found.length < CYCLE_CAP; s++) walk(s, s);
	return found.sort((a, b) => b.length - a.length || a[0] - b[0] || a.join(',').localeCompare(b.join(',')));
}

/** `cycle` begun at `at`, the rest following in cycle order. */
function rotated(cycle: number[], at: number): number[] {
	const k = cycle.indexOf(at);
	return k < 0 ? cycle : [...cycle.slice(k), ...cycle.slice(0, k)];
}

/** The states in column order. */
export function columnOrder(m: Machine): string[] {
	const names = Object.keys(m.states);
	const n = names.length;
	if (n === 0) return [];
	const index = new Map(names.map((s, i) => [s, i]));
	const succ: number[][] = names.map(() => []);
	for (const t of Object.values(m.transitions)) {
		const a = index.get(t.from);
		const b = index.get(t.to);
		if (a !== undefined && b !== undefined && a !== b && !succ[a].includes(b)) succ[a].push(b);
	}
	const placed: number[] = [];
	const isPlaced = (i: number): boolean => placed.includes(i);
	/** `items` into `placed` right after `after`, in order; at the end with no anchor. */
	const insertAfter = (after: number | null, items: number[]): void => {
		const k = after === null ? placed.length : placed.indexOf(after) + 1;
		placed.splice(k, 0, ...items.filter((i) => !isPlaced(i)));
	};
	const start = index.get(Object.values(m.playheads)[0]?.start ?? '');
	for (const cycle of cycles(n, succ)) {
		if (cycle.every(isPlaced)) continue;
		if (placed.length === 0) {
			insertAfter(null, rotated(cycle, start !== undefined && cycle.includes(start) ? start : Math.min(...cycle)));
			continue;
		}
		// The attachment point: the shared state placed first, else the placed state that leads
		// into the cycle, else the cycle stands on its own at the end.
		const shared = placed.find((p) => cycle.includes(p));
		if (shared !== undefined) {
			insertAfter(shared, rotated(cycle, shared).slice(1));
			continue;
		}
		const door = placed.find((p) => succ[p].some((c) => cycle.includes(c)));
		if (door !== undefined) {
			const entry = succ[door].find((c) => cycle.includes(c))!;
			insertAfter(door, rotated(cycle, entry));
		} else {
			insertAfter(null, rotated(cycle, Math.min(...cycle)));
		}
	}
	// What no loop holds: each after the placed state that leads to it, the rest in their order.
	const left = (): number[] => names.map((_, i) => i).filter((i) => !isPlaced(i));
	for (let rest = left(); rest.length > 0; rest = left()) {
		const led = rest.find((u) => placed.some((p) => succ[p].includes(u)));
		if (led === undefined) {
			insertAfter(null, [rest[0]]);
			continue;
		}
		// After the last of its placed predecessors, so a chain out of a loop reads on in order.
		const pred = [...placed].reverse().find((p) => succ[p].includes(led))!;
		insertAfter(pred, [led]);
	}
	return placed.map((i) => names[i]);
}

/** Whether a transition joins `a` and `b` either way: a `*` leaves `a` for `b` as any other does. */
export function joined(m: Machine, a: string, b: string): boolean {
	return Object.values(m.transitions).some((t) => ((t.from === a || t.from === '*') && t.to === b) || ((t.from === b || t.from === '*') && t.to === a));
}

/** Each state's share of the score's width: its interval in seconds, nothing for a state that
 * decides once or never. */
export function duration(m: Machine, state: string): number {
	const s = m.states[state];
	return s && s.interval > 0 ? s.interval : 0;
}

/** The columns' widths in px over `total`: proportional to their durations, none under `min`,
 * and the rest sharing what the floored ones leave. Equal where nothing lasts. */
export function widths(durations: number[], total: number, min: number): number[] {
	const n = durations.length;
	if (n === 0) return [];
	const sum = durations.reduce((a, b) => a + b, 0);
	const share = sum > 0 ? durations : durations.map(() => 1);
	const out = new Array<number>(n).fill(0);
	const floored = new Array<boolean>(n).fill(false);
	for (;;) {
		const room = total - floored.filter(Boolean).length * min;
		const weight = share.reduce((a, w, i) => (floored[i] ? a : a + w), 0);
		let changed = false;
		for (let i = 0; i < n; i++) {
			if (floored[i]) continue;
			out[i] = weight > 0 ? (room * share[i]) / weight : min;
			if (out[i] < min) {
				floored[i] = true;
				changed = true;
			}
		}
		if (!changed) break;
	}
	return out.map((w, i) => (floored[i] ? min : w));
}
