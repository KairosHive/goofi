<!-- The frame on a target that takes the drag in flight, lit while the pointer is over it. It sits
     inside the target's element, which is `position: relative`. -->
<script lang="ts">
	import { drag } from './engine.svelte';

	let { id }: { id: string } = $props();
	const engine = drag();
</script>

{#if engine.takes(id) !== null}
	<div class="drop-hint" class:active={engine.isOver(id)} data-testid="drop-hint"></div>
{/if}

<style>
	.drop-hint {
		position: absolute;
		inset: 4px;
		pointer-events: none;
		border: 2px dashed color-mix(in srgb, var(--accent) 55%, transparent);
		border-radius: var(--radius-sm);
		z-index: var(--z-drag-ghost);
	}
	.drop-hint.active {
		border-style: solid;
		background: color-mix(in srgb, var(--accent) 16%, transparent);
	}
</style>
