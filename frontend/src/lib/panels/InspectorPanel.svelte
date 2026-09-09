<script lang="ts">
	import type { PanelProps } from 'panelty';
	import NodeLinkedPanel from './NodeLinkedPanel.svelte';
	import Inspector from '$lib/inspector/Inspector.svelte';

	let props: PanelProps = $props();
</script>

<NodeLinkedPanel {...props} label="inspector">
	{#snippet content(node)}
		<Inspector {node} />
	{/snippet}
</NodeLinkedPanel>
