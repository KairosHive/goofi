/** The machine as the panel and the inspector read it: a state's decision and the transitions out
 * of it in the order the manager picks among them, the shares of a `random` draw linked so they
 * sum to one, a transition's label, and an edit of several states as one step. */
import type { Decision, Literal, Machine, State } from '$lib/api/generated';
import { graph, type MachineOp } from '$lib/stores/graph.svelte';
import { history } from '$lib/stores/history.svelte';
import { notify } from '$lib/stores/notify.svelte';

/** What the canvas hands the score: the selected states, lit as columns and edited together, and
 * the selected transitions, lit as arcs. */
export type Subject = { selected: string[]; open: string[] };

export const DECISIONS: { id: Decision; label: string; title: string }[] = [
	{ id: 'sequence', label: 'seq', title: 'Each interval, the next transition out, in order' },
	{ id: 'random', label: 'rand', title: 'Each interval, a draw by the shares below; wait is no move' },
	{ id: 'expression', label: 'expr', title: 'Each interval, the index an expression gives; a negative one waits' }
];

/** The ids of the transitions out of `state`, in authored order: `*` ones included. */
export function outgoing(m: Machine, state: string): string[] {
	return Object.entries(m.transitions)
		.filter(([, t]) => t.from === state || t.from === '*')
		.map(([id]) => id);
}

/** The draw's shares, the wait first then each transition out: the weights over their total, all
 * on the wait when nothing weighs. A transition with no weight weighs 1, as the manager draws it. */
export function shares(s: State, out: string[]): number[] {
	const raw = [s.noop, ...out.map((id) => s.weights[id] ?? 1)];
	const total = raw.reduce((a, b) => a + b, 0);
	return total > 0 ? raw.map((w) => w / total) : raw.map((_, k) => (k === 0 ? 1 : 0));
}

/** `p` with share `i` moved to `v`, the others rescaled to fill the rest in their own ratio, or
 * evenly when they are all 0. */
export function relink(p: number[], i: number, v: number): number[] {
	if (p.length < 2) return [1];
	const at = Math.min(1, Math.max(0, v));
	const rest = p.reduce((a, b, k) => (k === i ? a : a + b), 0);
	return p.map((x, k) => (k === i ? at : rest > 0 ? (x * (1 - at)) / rest : (1 - at) / (p.length - 1)));
}

/** Shares as the `machine state edit` fields that hold them. */
export function weightsOf(p: number[], out: string[]): { noop: number; weights: Record<string, number> } {
	return { noop: p[0], weights: Object.fromEntries(out.map((id, k) => [id, p[k + 1]])) };
}

/** A transition's part in its state's decision, in a few words for its label: its share of the
 * draw, its turn in the sequence, or its index; '' for a `*` transition, which has no one state. */
export function decisionText(m: Machine, id: string): string {
	const t = m.transitions[id];
	const s = m.states[t.from];
	if (!s) return '';
	const out = outgoing(m, t.from);
	const k = out.indexOf(id);
	switch (s.mode) {
		case 'sequence':
			return `#${k + 1}`;
		case 'expression':
			return `[${k}]`;
		default:
			return `${Math.round(shares(s, out)[k + 1] * 100)}%`;
	}
}

/** A transition's label: its part in its state's decision, then its duration. A condition shows
 * in the inspector, where it is read whole. */
export function summary(m: Machine, id: string): string {
	const t = m.transitions[id];
	return decisionText(m, id) + (t.duration > 0 ? ` ${t.duration}s` : '');
}

/** `patch` into each of `ids` through `op`, the id under `key`, as ONE undo step. */
export async function editEach(machine: string, op: MachineOp, key: 'name' | 'id', ids: string[], patch: Record<string, unknown>): Promise<void> {
	const g = graph();
	try {
		await history().transaction(op, async (step) => {
			for (const id of ids) await g.machine(op, { machine, [key]: id, ...patch }, step);
		});
	} catch (e) {
		notify().failure(op, e);
	}
}

/** Whether `values` hold more than one value. */
export const mixed = (values: unknown[]): boolean => new Set(values.map((v) => JSON.stringify(v))).size > 1;

/** What the states hold for `attr`: the value shown (the first set, else the default), whether
 * any sets it, and whether they differ, unset counting as a value of its own. */
export function held(m: Machine, ids: string[], attr: string): { value: Literal; set: boolean; mixed: boolean } {
	const values = ids.map((id) => m.states[id]?.values?.[attr]);
	const first = values.find((v) => v !== undefined);
	return { value: first ?? m.attributes[attr].default, set: first !== undefined, mixed: mixed(values) };
}

/** Transitions `pairs` as ONE step, each `[from, to]`; a pair already wired is left alone. */
export async function connect(machine: string, m: Machine, pairs: [string, string][], label = 'Connect states'): Promise<void> {
	const wired = new Set(Object.values(m.transitions).map((t) => `${t.from}>${t.to}`));
	const fresh = pairs.filter(([from, to]) => from !== to && !wired.has(`${from}>${to}`));
	if (fresh.length === 0) return;
	const g = graph();
	try {
		await history().transaction(label, async (step) => {
			for (const [from, to] of fresh) await g.machine('machine transition add', { machine, from, to }, step);
		});
	} catch (e) {
		notify().failure(label, e);
	}
}

/** Every ordered pair among `states`. */
export const allToAll = (states: string[]): [string, string][] => states.flatMap((a) => states.filter((b) => b !== a).map((b): [string, string] => [a, b]));

/** Each state to the next; with `ring`, the last back to the first. */
export const chained = (states: string[], ring = false): [string, string][] => {
	const pairs = states.slice(1).map((to, i): [string, string] => [states[i], to]);
	if (ring && states.length > 1) pairs.push([states[states.length - 1], states[0]]);
	return pairs;
};

/** `states` in reading order on the canvas: by row of their boxes, then left to right. */
export function byPosition(m: Machine, states: string[], rowHeight = 96): string[] {
	const row = (s: string): number => Math.round(m.states[s].pos[1] / rowHeight);
	return [...states].sort((a, b) => row(a) - row(b) || m.states[a].pos[0] - m.states[b].pos[0]);
}

/** `n` new states as ONE step, the manager placing each clear of the others; with `chain`, each
 * wired to the next, and the first from `after` when one is given. Answers their names. */
export async function addStates(machine: string, n: number, chain: boolean, after: string | null): Promise<string[]> {
	const g = graph();
	const names: string[] = [];
	try {
		await history().transaction(n === 1 ? 'Add state' : `Add ${n} states`, async (step) => {
			for (let i = 0; i < n; i++) {
				const r = await g.machine<{ name: string }>('machine state add', { machine }, step);
				names.push(r.name);
			}
			if (chain) {
				for (const [from, to] of chained(after ? [after, ...names] : names)) await g.machine('machine transition add', { machine, from, to }, step);
			}
		});
	} catch (e) {
		notify().failure('Add states', e);
	}
	return names;
}

/** What a copy of states carries: each state whole, and the transitions among them. */
export interface Fragment {
	kind: 'goofi-machine';
	states: Record<string, State>;
	transitions: Machine['transitions'][string][];
}

export function fragmentOf(m: Machine, states: string[]): Fragment {
	const set = new Set(states);
	return {
		kind: 'goofi-machine',
		states: Object.fromEntries(states.map((s) => [s, m.states[s]])),
		transitions: Object.values(m.transitions).filter((t) => set.has(t.from) && set.has(t.to))
	};
}

export function parseFragment(text: string): Fragment | null {
	try {
		const f = JSON.parse(text);
		return f && f.kind === 'goofi-machine' && f.states && Array.isArray(f.transitions) ? (f as Fragment) : null;
	} catch {
		return null;
	}
}

/** The fragment into `machine` as ONE step, each state `offset` from where it was copied and
 * named by the manager, the transitions among them rewired to the new names; the shares of a
 * draw are left to its new transitions. Answers the new names. */
export async function pasteFragment(machine: string, f: Fragment, offset: [number, number]): Promise<string[]> {
	const g = graph();
	const renamed = new Map<string, string>();
	try {
		await history().transaction('Paste states', async (step) => {
			for (const [old, s] of Object.entries(f.states)) {
				const r = await g.machine<{ name: string }>('machine state add', { machine, pos: [s.pos[0] + offset[0], s.pos[1] + offset[1]], values: s.values ?? {}, envelopes: s.envelopes ?? {} }, step);
				renamed.set(old, r.name);
				await g.machine('machine state edit', { machine, name: r.name, interval: s.interval, interval_expression: s.interval_expression ?? '', mode: s.mode, noop: s.noop, expression: s.expression }, step);
			}
			for (const t of f.transitions) {
				await g.machine('machine transition add', { machine, from: renamed.get(t.from), to: renamed.get(t.to), condition: t.condition ?? '', duration: t.duration }, step);
			}
		});
	} catch (e) {
		notify().failure('Paste', e);
	}
	return [...renamed.values()];
}
