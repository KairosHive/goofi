<!-- The chip that follows a lifted variable: the expression a drop will write. -->
<script lang="ts">
	import { ui } from '$lib/stores/ui.svelte';
	import type { VariableLift } from './variableLift.svelte';

	let { lift }: { lift: VariableLift } = $props();
	const uiStore = ui();
</script>

{#if lift.active && uiStore.variableDrag}
	<div class="ghost" style={`left: ${uiStore.variableDrag.x + 12}px; top: ${uiStore.variableDrag.y + 12}px`} aria-hidden="true">variables.{uiStore.variableDrag.name}</div>
{/if}

<style>
	.ghost {
		position: fixed;
		z-index: var(--z-drag-ghost);
		pointer-events: none;
		padding: var(--space-2);
		background: var(--surface-3);
		color: var(--text);
		border: 1px solid var(--accent);
		border-radius: var(--radius-sm);
		font-family: var(--font-mono);
		font-size: var(--fs-small);
	}
</style>
