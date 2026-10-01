<!-- Badge — a small, static tone pill; its pressable sibling is <Chip>, on the same tone scale. -->
<script module lang="ts">
	export type BadgeTone = 'neutral' | 'accent' | 'success' | 'warning' | 'danger';
</script>

<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';

	let {
		tone = 'neutral',
		class: klass = '',
		children,
		...rest
	}: HTMLAttributes<HTMLSpanElement> & {
		tone?: BadgeTone;
		children?: Snippet;
	} = $props();
</script>

<span {...rest} class={`ui-badge pill-type t-${tone} ${klass}`.trim()}>{@render children?.()}</span>

<style>
	.ui-badge {
		display: inline-flex;
		align-items: center;
		gap: var(--space-2);
		padding: var(--space-1) var(--space-3);
		border: 1px solid transparent;
		border-radius: var(--radius-sm);
		white-space: nowrap;
	}
	.ui-badge.t-neutral {
		background: var(--surface-3);
		border-color: var(--border);
		color: var(--text-dim);
	}
	.ui-badge:not(.t-neutral) {
		background: var(--tone-fill);
		border-color: color-mix(in srgb, var(--tone) 40%, transparent);
		color: var(--tone);
	}
</style>
