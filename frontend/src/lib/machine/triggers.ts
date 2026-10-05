/** The words a transition's triggers are summarised and edited by; the model is `Trigger`. */
import type { Seconds, Trigger, Transition } from '$lib/api/generated';

export type TriggerKind = Trigger['kind'];
export const TRIGGER_KINDS: readonly TriggerKind[] = ['manual', 'after', 'when', 'event', 'always', 'meet', 'alone'];
export const POLICIES = ['fifo', 'lifo', 'all'] as const;
export const CURVES = ['step', 'linear', 'in', 'out', 'in_out', 'smooth'] as const;

/** A fresh trigger of `kind`, with the defaults the manager would fill. */
export function blankTrigger(kind: TriggerKind): Trigger {
	switch (kind) {
		case 'after': return { kind, seconds: 1 };
		case 'when': return { kind, expression: '', edge: 'rising' };
		case 'meet': return { kind, policy: 'fifo' };
		case 'event': return { kind, name: 'next' };
		case 'always':
		case 'alone': return { kind };
		default: return { kind: 'manual' };
	}
}

/** One trigger in a few words, as an edge label carries it. */
export function triggerText(t: Trigger): string {
	switch (t.kind) {
		case 'manual': return 'manual';
		case 'after': return `after ${durationText(t.seconds)}`;
		case 'when': return `${t.edge === 'rising' ? 'when' : t.edge} ${t.expression}`;
		case 'meet': return `meet ${t.policy}`;
		case 'event': return t.name;
		case 'always': return 'always';
		case 'alone': return 'alone';
	}
}

/** A transition's label: every trigger, or what fires none. */
export function summary(t: Transition): string {
	const parts = (t.triggers ?? []).map(triggerText);
	const dwell = t.duration !== 0 ? ` / ${durationText(t.duration)}` : '';
	return (parts.length ? parts.join(' | ') : 'never') + dwell;
}

export function durationText(value: Seconds): string {
	return typeof value === 'number' ? `${value}s` : typeof value === 'string' ? value : `${value.min}–${value.max}s`;
}
