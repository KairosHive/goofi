/** The words a transition is summarised and edited by; the model is `Transition`. */
import type { Machine } from '$lib/api/generated';
import { decisionText } from './decision';

export const CURVES = ['step', 'linear', 'in', 'out', 'in_out', 'smooth'] as const;

/** A transition's label: its part in its state's decision, then its duration. A condition shows
 * in the inspector, where it is read whole. */
export function summary(m: Machine, id: string): string {
	const t = m.transitions[id];
	const travel = t.duration > 0 ? ` ${t.duration}s` : '';
	return decisionText(m, id) + travel;
}
