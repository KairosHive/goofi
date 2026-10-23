/** The machine as the panel and the inspector read it: a state's decision and the transitions out
 * of it in the order the manager picks among them, the shares of a `random` draw linked so they
 * sum to one, a transition's label, and an edit of several states as one step. */
import type { Decision, Literal, Machine, State } from '$lib/api/generated';
import { graph, type MachineOp } from '$lib/stores/graph.svelte';
import { history } from '$lib/stores/history.svelte';
import { notify } from '$lib/stores/notify.svelte';

/** What the machine panel's inspector shows: the selected states, and the transitions among
 * theirs whose rows start open — the selected ones. */
export type Subject = { ids: string[]; open: string[] };

export const CURVES = ['step', 'linear', 'in', 'out', 'in_out', 'smooth'] as const;

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
