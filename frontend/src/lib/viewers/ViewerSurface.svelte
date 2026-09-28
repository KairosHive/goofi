<!-- What a viewer body shows in the DOM: the text kinds, and the summary of an array shape the
     kind cannot draw. Every drawable array is the feed's, on the plot surface beneath. -->
<script lang="ts">
	import { isArrayFrame, isStringFrame, isTableFrame, type DataFrame } from '$lib/codec/decode';
	import { isRenderable, type ViewerKind } from './kind';
	import { summaryOf } from './viewMeta';
	import type { SettingsMap } from './settingsSchema';
	import { EmptyState } from '$lib/ui';
	import StringViewer from './StringViewer.svelte';
	import TableViewer from './TableViewer.svelte';
	import HighDimFallback from './HighDimFallback.svelte';

	let {
		frame,
		kind,
		settings = {}
	}: { frame: DataFrame | null; kind: ViewerKind; settings?: SettingsMap } = $props();
</script>

{#if !frame}
	<EmptyState>
		{#snippet hint()}no data yet{/snippet}
	</EmptyState>
{:else if isArrayFrame(frame)}
	{#if !isRenderable(kind, frame.data, settings)}
		<HighDimFallback summary={summaryOf(frame.data, frame.meta)} />
	{/if}
{:else if isStringFrame(frame)}
	<StringViewer {frame} {settings} />
{:else if isTableFrame(frame)}
	<TableViewer {frame} {settings} />
{/if}
