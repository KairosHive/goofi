<!-- The frame→component dispatch for the kinds a component draws; lines and images go to the
     plot surface in ViewerFeed. Subscription is the caller's concern. -->
<script lang="ts">
	import { isArrayFrame, isStringFrame, isTableFrame, type DataFrame } from '$lib/codec/decode';
	import { isRenderable, isTrajectory, type ViewerKind } from './kind';
	import { summaryOf } from './viewMeta';
	import type { SettingsMap } from './settingsSchema';
	import { EmptyState } from '$lib/ui';
	import TrajectoryViewer from './TrajectoryViewer.svelte';
	import BrainViewer from './BrainViewer.svelte';
	import StringViewer from './StringViewer.svelte';
	import TableViewer from './TableViewer.svelte';
	import HighDimFallback from './HighDimFallback.svelte';
	import type { Drag, Probe } from './hover';

	let {
		frame,
		kind,
		settings = {},
		probe = $bindable(null),
		drag = $bindable(null)
	}: {
		frame: DataFrame | null;
		kind: ViewerKind;
		settings?: SettingsMap;
		probe?: Probe | null;
		drag?: Drag | null;
	} = $props();

	const arraySpec = $derived(frame && isArrayFrame(frame) ? frame.data : null);
	const renderable = $derived(isRenderable(kind, arraySpec, settings));
	// A shape this kind cannot draw resolves to the text fallback.
	const summary = $derived.by(() => {
		if (!frame || !arraySpec) return null;
		if (!renderable) return summaryOf(arraySpec, frame.meta);
		return null;
	});
	// Only a brain answers a hover or a drag here; the plot surface kinds answer in the feed.
	$effect(() => {
		if (!(kind === 'brain' && renderable && arraySpec)) {
			probe = null;
			drag = null;
		}
	});
</script>

{#if !frame}
	<EmptyState>
		{#snippet hint()}no data yet{/snippet}
	</EmptyState>
{:else if summary}
	<HighDimFallback {summary} />
{:else if isArrayFrame(frame)}
	{#if isTrajectory(kind, settings)}
		<TrajectoryViewer {frame} {settings} />
	{:else if kind === 'brain'}
		<BrainViewer {frame} {settings} bind:probe bind:drag />
	{/if}
{:else if isStringFrame(frame)}
	<StringViewer {frame} {settings} />
{:else if isTableFrame(frame)}
	<TableViewer {frame} {settings} />
{/if}
