/* What every keyboard-walked list shares: a step wraps at both ends, and the lit row stays in view. */
import type { Action } from 'svelte/action';

/** The row a step of `by` lands on from `at`, wrapping: up from the first is the last. */
export function stepWrapped(at: number, by: number, count: number): number {
	if (count <= 0) return 0;
	return (((at + by) % count) + count) % count;
}

/** On a row: scroll it into its list's view whenever it is the lit one. */
export const reveal: Action<HTMLElement, boolean> = (el, lit) => {
	const show = (on: boolean): void => {
		if (on) el.scrollIntoView({ block: 'nearest' });
	};
	show(lit);
	return { update: show };
};
