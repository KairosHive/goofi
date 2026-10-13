/** The words a transition's triggers are summarised and edited by; the model is `Trigger`. */
import type { Machine, Trigger } from '$lib/api/generated';
import { decisionText } from './decision';

export type TriggerKind = Trigger['kind'];
export const TRIGGER_KINDS: readonly TriggerKind[] = ['manual', 'when', 'meet', 'alone'];
export const POLICIES = ['fifo', 'lifo', 'all'] as const;
export const CURVES = ['step', 'linear', 'in', 'out', 'in_out', 'smooth'] as const;

/** A fresh trigger of `kind`, with the defaults the manager would fill. */
export function blankTrigger(kind: TriggerKind): Trigger {
	switch (kind) {
		case 'when': return { kind, expression: '', weight: 1 };
		case 'meet': return { kind, policy: 'fifo' };
		case 'alone': return { kind };
		default: return { kind: 'manual' };
	}
}

/** One trigger in a few words, as an edge label carries it. */
export function triggerText(t: Trigger): string {
	switch (t.kind) {
		case 'manual': return 'manual';
		case 'when': return `when ${t.expression}${t.weight !== 1 ? ` ×${t.weight}` : ''}`;
		case 'meet': return `meet ${t.policy}`;
		case 'alone': return 'alone';
	}
}

/** A transition's label: its part in its state's decision, then every trigger, then its duration. */
export function summary(m: Machine, id: string): string {
	const t = m.transitions[id];
	const parts = [decisionText(m, id), ...(t.triggers ?? []).map(triggerText)].filter(Boolean);
	const travel = t.duration > 0 ? ` ${t.duration}s` : '';
	return (parts.length ? parts.join(' | ') : 'never') + travel;
}
