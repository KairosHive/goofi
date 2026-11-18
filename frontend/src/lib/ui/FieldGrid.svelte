<!-- FieldGrid — the grid a block of `Field row`s stands on: the labels of the block share one
     column, as wide as the widest, and each control fills the other. `stacked` is one column, each
     label above its control, for a block too narrow to hold both side by side. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';

	let { stacked = false, class: klass = '', children, ...rest }: HTMLAttributes<HTMLDivElement> & { stacked?: boolean; children?: Snippet } = $props();
</script>

<div {...rest} class={`ui-field-grid ${klass}`.trim()} class:stacked>{@render children?.()}</div>

<style>
	.ui-field-grid {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		column-gap: var(--space-5);
		row-gap: var(--space-3);
		align-items: center;
		/* A number field fills its column, as a dropdown does. */
		--number-width: 100%;
	}
	.ui-field-grid.stacked {
		grid-template-columns: minmax(0, 1fr);
	}
</style>
