<!-- The frame on a panel that takes a dropped node, shown while a node is dragged. -->
<script lang="ts">
	import { ui } from '$lib/stores/ui.svelte';

	let { panelId }: { panelId: string } = $props();
	const uiStore = ui();
</script>

{#if uiStore.nodeDrag !== null}
	<div class="node-drop-hint" class:active={uiStore.nodeDragOver === panelId} data-testid="node-drop-hint"></div>
{/if}
