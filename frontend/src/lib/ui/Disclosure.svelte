<!-- Disclosure — a caret plus a `summary` that mounts `children` on toggle. `--disclosure-surface`
     and `--disclosure-hover` give the summary row a ground of its own. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';
	import { Icon } from 'panelty';

	let {
		summary,
		children,
		class: klass = '',
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		summary: Snippet;
		children?: Snippet;
	} = $props();

	let open = $state(false);
	const bodyId = $props.id();
</script>

<div {...rest} class={`ui-disclosure ${klass}`.trim()}>
	<button
		type="button"
		class="ui-disclosure-summary"
		aria-expanded={open}
		aria-controls={bodyId}
		onclick={() => (open = !open)}
	>
		<span class="disclosure-caret" class:open><Icon name="chevron-right" /></span>
		<span class="ui-disclosure-label">{@render summary()}</span>
	</button>
	{#if open}
		<div id={bodyId} class="ui-disclosure-body">{@render children?.()}</div>
	{/if}
</div>

<style>
	.ui-disclosure {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.ui-disclosure-summary {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		width: 100%;
		min-height: var(--hit);
		padding: var(--space-2) var(--space-3);
		background: var(--disclosure-surface, transparent);
		border: none;
		border-radius: var(--radius-sm);
		color: var(--text);
		font-family: var(--font-sans);
		font-size: var(--fs-small);
		font-weight: 600;
		text-align: left;
		cursor: pointer;
		transition: background var(--dur-fast) var(--ease);
	}
	.ui-disclosure-summary:hover {
		background: var(--disclosure-hover, var(--surface-2));
	}
	.ui-disclosure-summary:focus-visible {
		outline-offset: -2px;
	}
	.ui-disclosure-label {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.ui-disclosure-body {
		min-width: 0;
		padding: var(--space-3) var(--space-3) var(--space-3) var(--space-7);
	}
</style>
