<!-- The chip that follows a source carried beyond its home: the label of what a drop would write.
     Mounted once, in the shell. -->
<script lang="ts">
	import { drag } from './engine.svelte';

	const engine = drag();
</script>

{#if engine.source?.label && engine.point && !engine.inHome}
	<div class="ghost" style={`left: ${engine.point.x}px; top: ${engine.point.y}px`} aria-hidden="true" data-testid="drag-ghost">{engine.source.label}</div>
{/if}

<style>
	.ghost {
		position: fixed;
		z-index: var(--z-drag-ghost);
		/* Offset off the cursor so it reads as carried, not pinned. */
		transform: translate(12px, 12px);
		pointer-events: none;
		max-width: 220px;
		padding: var(--space-2) var(--space-4);
		background: var(--surface-3);
		color: var(--text);
		border: 1px solid var(--accent);
		border-radius: var(--radius-sm);
		box-shadow: var(--shadow-2);
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
</style>
