<!-- Field — a `<label>` linked to the first live control (see field.ts), with a sibling `adornment`.
     The row wraps on its own width; `onExpand` makes the label a disclosure summary. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';
	import { provideFieldControlId } from './field';

	let {
		label,
		doc,
		adornment,
		expanded = false,
		stretchSummary = false,
		row = false,
		onExpand,
		class: klass = '',
		children,
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		label: string;
		/** Long-form description, surfaced as the hover tooltip. */
		doc?: string;
		/** Trailing control affordance. */
		adornment?: Snippet;
		/** Whether what this row reveals is open; announced, and the hover is the one affordance. */
		expanded?: boolean;
		/** Extend the disclosure target to its positioned parent with isolated stacking. */
		stretchSummary?: boolean;
		/** One line, the control trailing the label, at any width. */
		row?: boolean;
		/** Makes the label a disclosure summary rather than a `<label>`. */
		onExpand?: () => void;
		children?: Snippet;
	} = $props();

	let controlId = $state<string | undefined>(undefined);
	provideFieldControlId((id) => (controlId = id));
</script>

<div {...rest} class={`ui-field ${klass}`.trim()} class:row title={doc ?? rest.title}>
	{#if onExpand}
		<!-- Out of the tab order and never the focus: a press opens the row and hands the focus to the
		     control it names, as a label does, so the row's shortcuts act on that control. -->
		<button
			type="button"
			class="ui-field-label ui-field-summary"
			class:stretched={stretchSummary}
			aria-expanded={expanded}
			tabindex="-1"
			onclick={() => {
				onExpand?.();
				if (controlId) document.getElementById(controlId)?.focus();
			}}
		>
			{label}
		</button>
	{:else}
		<label class="ui-field-label" for={controlId}>{label}</label>
	{/if}
	<div class="ui-field-value">
		<div class="ui-field-control">{@render children?.()}</div>
		{#if adornment}
			<span class="ui-field-adornment">{@render adornment()}</span>
		{/if}
	</div>
</div>

<style>
	.ui-field {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		column-gap: var(--space-4);
		row-gap: var(--field-gap, var(--space-3));
		min-width: 0;
		font-size: var(--fs-small);
	}
	/* A row is two cells of its parent's grid, so the rows of one menu share their columns. */
	.ui-field.row {
		display: grid;
		grid-template-columns: subgrid;
		grid-column: 1 / -1;
	}
	.ui-field.row .ui-field-control {
		justify-content: flex-end;
	}
	.ui-field-label {
		flex: 0 1 auto;
		min-width: 4rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--text);
		font-weight: 600;
		letter-spacing: 0.01em;
		cursor: pointer;
	}
	.ui-field-summary {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		padding: 0;
		border: none;
		background: none;
		font: inherit;
		text-align: left;
	}
	.ui-field-summary:not(.stretched):focus-visible {
		outline-offset: 2px;
		border-radius: var(--radius-sm);
	}
	.ui-field-summary.stretched {
		overflow: visible;
	}
	.ui-field-summary.stretched::before,
	.ui-field-summary.stretched::after {
		content: '';
		position: absolute;
		inset: 0;
		border-radius: var(--radius-sm);
	}
	.ui-field-summary.stretched::after {
		z-index: -1;
		pointer-events: none;
	}
	.ui-field-summary.stretched:hover::after {
		background: var(--surface-3);
	}
	.ui-field-summary.stretched:focus-visible::before {
		outline: var(--focus-width) solid var(--focus-ink);
		outline-offset: 2px;
	}
	/* ONE flex item, so a narrow field drops the whole value under the label. Its basis is the wrap
	   threshold, above the stack threshold below, so the two stages cannot both squeeze a control. */
	.ui-field-value {
		flex: 1 1 18rem;
		min-width: 0;
		display: flex;
		align-items: center;
		gap: var(--space-4);
	}
	/* Paired controls must be DIRECT children: the @container flip below restacks direct children. */
	.ui-field-control {
		flex: 1 1 auto;
		display: flex;
		align-items: center;
		gap: var(--space-4);
		min-width: 0;
	}
	.ui-field-adornment {
		flex-shrink: 0;
		display: inline-flex;
		align-items: center;
		gap: var(--space-2);
	}
	/* Where a Slider + NumberInput pair stops fitting beside an adornment: a structural threshold,
	   not a token — and in `rem`, the unit the controls it measures are drawn in. */
	@container (max-width: 20rem) {
		.ui-field-control {
			flex-direction: column;
			align-items: stretch;
		}
	}
</style>
