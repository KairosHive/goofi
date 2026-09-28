<!-- PresenceOverlay — the other browsers' pointers on this tab, over the whole window. It takes no
     pointer events and dirties nothing: a peer's point is where they are, not a thing in the patch. -->
<script lang="ts">
	import { presence } from '$lib/stores/presence.svelte';

	const p = presence();
</script>

<div class="presence" aria-hidden="true">
	{#each p.others as c (c.id)}
		<span
			class="peer"
			style={`left: ${(c.x * 100).toFixed(2)}%; top: ${(c.y * 100).toFixed(2)}%; --peer-hue: ${c.hue}`}
			data-testid="peer-cursor"
			data-peer={c.id}
		></span>
	{/each}
</div>

<style>
	.presence {
		position: fixed;
		inset: 0;
		pointer-events: none;
		z-index: var(--z-menu);
	}
	.peer {
		position: absolute;
		width: var(--space-6);
		height: var(--space-6);
		border-radius: 50%;
		background: hsl(var(--peer-hue) 70% 55%);
		box-shadow: 0 0 0 var(--space-1) var(--bg);
		transform: translate(-50%, -50%);
		transition:
			left var(--dur-fast) linear,
			top var(--dur-fast) linear;
	}
</style>
