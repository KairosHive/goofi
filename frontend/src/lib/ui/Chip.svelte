<!-- Chip — the pressable sibling of <Badge>: the same tone pill as a real <button>. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLButtonAttributes } from 'svelte/elements';
	import type { BadgeTone } from './Badge.svelte';
	import type { ButtonDensity } from 'panelty';

	let {
		tone = 'neutral',
		density = 'comfortable',
		type = 'button',
		class: klass = '',
		children,
		...rest
	}: HTMLButtonAttributes & {
		tone?: BadgeTone;
		/** Box density; `chrome` is the dense box a toolbar strip wears. */
		density?: ButtonDensity;
		children?: Snippet;
	} = $props();
</script>

<button
	{...rest}
	{type}
	class={`ui-chip pill-type t-${tone} ${density === 'chrome' ? 'd-chrome ' : ''}${klass}`.trim()}
>
	{@render children?.()}
</button>

<style>
	.ui-chip {
		display: inline-flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--hit);
		padding: var(--space-1) var(--space-4);
		border: 1px solid transparent;
		border-radius: var(--radius-sm);
		white-space: nowrap;
		cursor: pointer;
		transition:
			background var(--dur-fast) var(--ease),
			border-color var(--dur-fast) var(--ease),
			color var(--dur-fast) var(--ease);
	}
	/* Unset, the hook falls back to --hit, so `density="chrome"` alone is a no-op, not a collapse. */
	.ui-chip.d-chrome {
		min-height: var(--chip-size, var(--hit));
	}
	/* The width floor lives here, not in app.css: a blanket one there widens the frozen node-canvas
	   exceptions. */
	@media (hover: none) and (pointer: coarse) {
		.ui-chip {
			min-width: var(--hit);
			justify-content: center;
		}
		.ui-chip.d-chrome {
			min-height: var(--hit);
		}
	}
	.ui-chip:disabled {
		opacity: var(--disabled-opacity);
		cursor: not-allowed;
	}
	.ui-chip:focus-visible {
		outline-offset: 2px;
	}

	/* `neutral` is a ghost, unlike Badge's filled one: it is the RESTING state and must not outshout
	   the label it sits beside. */
	.ui-chip.t-neutral {
		background: transparent;
		border-color: transparent;
		color: var(--text-muted);
	}
	.ui-chip.t-neutral:hover:not(:disabled) {
		background: var(--hover-fill);
		color: var(--text);
	}
	.ui-chip:not(.t-neutral) {
		background: var(--tone-fill);
		border-color: color-mix(in srgb, var(--tone) 40%, transparent);
		color: var(--tone);
	}
	.ui-chip:not(.t-neutral):hover:not(:disabled) {
		background: color-mix(in srgb, var(--tone) 28%, transparent);
		border-color: var(--tone);
	}
</style>
