<!-- What a viewer body shows in the DOM for the text kinds, which read their frames on this
     thread. Every array kind is a drawing in the worker, on the plot surface beneath. -->
<script lang="ts">
	import { isStringFrame, isTableFrame, type DataFrame } from '$lib/codec/decode';
	import type { SettingsMap } from './settingsSchema';
	import { EmptyState } from '$lib/ui';
	import StringViewer from './StringViewer.svelte';
	import TableViewer from './TableViewer.svelte';

	let { frame, settings = {} }: { frame: DataFrame | null; settings?: SettingsMap } = $props();
</script>

{#if !frame}
	<EmptyState>
		{#snippet hint()}no data yet{/snippet}
	</EmptyState>
{:else if isStringFrame(frame)}
	<StringViewer {frame} {settings} />
{:else if isTableFrame(frame)}
	<TableViewer {frame} {settings} />
{/if}
