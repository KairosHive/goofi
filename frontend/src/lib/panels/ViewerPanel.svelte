<!-- Viewer panel — visualizes an output slot of the bound node. Each panel is an independent
     viewer instance; the data stream is still shared (one WS per node+slot). -->
<script lang="ts">
	import type { PanelProps } from 'panelty';
	import type { NodeInstanceInfo } from '$lib/api/control';
	import NodeLinkedPanel from './NodeLinkedPanel.svelte';
	import ViewerFeed from '$lib/viewers/ViewerFeed.svelte';
	import ViewerControls from '$lib/viewers/ViewerControls.svelte';
	import { viewBinding, type ViewBinding } from '$lib/viewers/viewBinding';
	import type { SlotView } from '$lib/viewers/inlineView';
	import { asStateObject } from 'panelty';
	import { workspace } from 'panelty';
	import { Select } from '$lib/ui';
	import type { SurfaceHandle } from '$lib/api/drawings';
	import { mountSurface, provideAnchor, provideSurface } from '$lib/viewers/plotHost';

	let props: PanelProps = $props();
	const ws = workspace();

	// This panel's own plot surface, at zoom 1, covering the body under the feed.
	let body = $state<HTMLElement | null>(null);
	let canvas = $state<HTMLCanvasElement | null>(null);
	let surface = $state.raw<SurfaceHandle | null>(null);
	provideSurface({
		get surface() {
			return surface;
		}
	});
	provideAnchor({
		x: 0,
		y: 0,
		z: 0,
		get el() {
			return body;
		}
	});
	$effect(() => {
		const c = canvas;
		const b = body;
		if (!c || !b) return;
		const m = mountSurface(c, () => (surface = null));
		surface = m.surface;
		const ro = new ResizeObserver(() =>
			m.setView({ x: 0, y: 0, zoom: 1, width: b.clientWidth, height: b.clientHeight })
		);
		ro.observe(b);
		return () => {
			ro.disconnect();
			m.dispose();
			surface = null;
		};
	});

	// One place, so the controls and content snippets (separate scopes) don't each re-derive it.
	function view(node: NodeInstanceInfo): { slot: string | null; dtype: string | null; binding: ViewBinding } {
		const want = asStateObject(props.state).slot;
		const slot = typeof want === 'string' && node.output_slots[want] ? want : (Object.keys(node.output_slots)[0] ?? null);
		const dtype = slot ? node.output_slots[slot] : null;
		const raw = () => asStateObject(props.state) as SlotView;
		const binding = viewBinding(dtype, raw, (patch, label) => {
			const settings = { ...raw().settings, ...patch.settings };
			props.setState({ ...raw(), ...patch, settings }, 'authored', label);
		});
		return { slot, dtype, binding };
	}
</script>

<NodeLinkedPanel {...props} label="data">
	{#snippet controls(node)}
		{@const { slot, dtype, binding } = view(node)}
		<Select
			density="chrome"
			value={slot ?? ''}
			onChange={(v) => ws.setPanelSlot(props.panelId, v)}
			options={Object.keys(node.output_slots)}
			labels={Object.fromEntries(
				Object.entries(node.output_slots).map(([name, dt]) => [
					name,
					`${node.slot_labels?.[name] ?? name} · ${dt.toLowerCase()}`
				])
			)}
			data-testid="viewer-slot"
		/>
		{#if slot && dtype}
			<ViewerControls {dtype} {binding} />
		{/if}
	{/snippet}

	{#snippet content(node)}
		{@const { slot, binding } = view(node)}
		<div class="vp-body" bind:this={body}>
			<canvas class="plot-surface" bind:this={canvas}></canvas>
			<ViewerFeed node={node.uid} {slot} {binding} />
		</div>
	{/snippet}
</NodeLinkedPanel>

<style>
	.vp-body {
		position: relative;
		flex: 1;
		min-height: 0;
		display: flex;
		padding: var(--space-3);
	}
	.plot-surface {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
	}
</style>
