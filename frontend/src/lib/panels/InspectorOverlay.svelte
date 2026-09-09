<!-- Selection inspector for one editor panel: parameters, metadata and errors for the selected
     node, on the side pane anchored to the host editor's edge. -->
<script lang="ts">
	import Inspector from '$lib/inspector/Inspector.svelte';
	import SidePane from './SidePane.svelte';
	import type { NodeInstanceInfo } from '$lib/api/control';

	let {
		node,
		enabled,
		onClose
	}: {
		node: NodeInstanceInfo | null;
		enabled: boolean;
		/** Turn this editor's inspector off — the same switch the corner toggle flips. */
		onClose: () => void;
	} = $props();

	/** Closing is a real outro, so the last node stays rendered until the slide finishes. */
	let renderedNode = $state<NodeInstanceInfo | null>(null);
	const open = $derived(enabled && node !== null);

	$effect(() => {
		if (open) renderedNode = node;
	});
</script>

<SidePane {open} onClosed={() => (renderedNode = null)} testid="auto-side-panel">
	<Inspector node={renderedNode} {onClose} />
</SidePane>
