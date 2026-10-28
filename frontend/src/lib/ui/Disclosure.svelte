<!-- Disclosure — a caret plus a `summary` that mounts `children` on toggle, with an `aside` control
     beside the summary that keeps its own gestures. `--disclosure-surface` and `--disclosure-hover`
     give the summary row a ground of its own; `--disclosure-body-pad` sets the body's padding. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';
	import { Icon } from 'panelty';

	let {
		summary,
		aside,
		children,
		open = $bindable(false),
		class: klass = '',
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		summary: Snippet;
		/** A control on the summary's line, outside the toggle. */
		aside?: Snippet;
		children?: Snippet;
		/** Whether the body shows; a parent may open a row, and the caret toggles it either way. */
		open?: boolean;
	} = $props();
	const bodyId = $props.id();
</script>

<div {...rest} class={`ui-disclosure ${klass}`.trim()}>
	<div class="ui-disclosure-head">
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
		{#if aside}<span class="ui-disclosure-aside">{@render aside()}</span>{/if}
	</div>
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
	.ui-disclosure-head {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		min-width: 0;
	}
	.ui-disclosure-aside {
		display: inline-flex;
		align-items: center;
		flex: 0 0 auto;
		width: 40%;
		min-width: 0;
	}
	.ui-disclosure-summary {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		flex: 1 1 auto;
		min-width: 0;
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
		padding: var(--disclosure-body-pad, var(--space-3) var(--space-3) var(--space-3) var(--space-7));
	}
</style>
