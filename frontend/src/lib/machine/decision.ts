/** A state's decision as the card draws it: the transitions out of it in the order the manager
 * picks among them, and the shares of a `random` draw, linked so they sum to one. */
import type { Decision, Machine, State } from '$lib/api/generated';

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

/** A transition's part in its state's decision, in a few words for its label; '' for none. */
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
		default: {
			const share = shares(s, out)[k + 1];
			return share > 0 ? `${Math.round(share * 100)}%` : '';
		}
	}
}
