<!-- The viewer body: a lazily-subscribed Data frame fed into ViewerSurface. Padding is the caller's. -->
<script lang="ts">
	import { bindViewer } from '$lib/api/frames';
	import { viewSpecsForKind } from './capacity';
	import type { DataFrame } from '$lib/codec/decode';
	import ViewerSurface from './ViewerSurface.svelte';
	import type { ViewBinding } from './viewBinding';
	import { EmptyState } from '$lib/ui';
	import { onMount, untrack } from 'svelte';

	/** `zoom` is the flow zoom the viewer is drawn under; a docked panel draws at 1. */
	let {
		node,
		slot,
		binding,
		zoom = 1
	}: { node: string; slot: string | null; binding: ViewBinding; zoom?: number } = $props();

	const kind = $derived(binding.kind);
	const settings = $derived(binding.settings);

	// Raw: a frame is an immutable payload, so a deep proxy per frame per viewer would buy nothing.
	let frame = $state.raw<DataFrame | null>(null);
	let visible = $state(false);
	let container: HTMLDivElement | null = $state(null);
	// The content box in CSS px; the device box below is derived from it, the DPR and the zoom.
	let boxW = $state(0);
	let boxH = $state(0);
	// Quantized to 32-px steps so a 1-px resize does not renegotiate the reduction.
	let capW = $state(0);
	let capH = $state(0);
	// Below a readable zoom the viewer unsubscribes and keeps its last frame as a thumbnail.
	let frozen = $state(false);
	// Stable per-instance token so multiple viewers of one slot collect (not evict).
	const token =
		typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : `vf-${Math.random()}`;

	/** The 32-px step for `px`, left where it is until `px` is a quarter step past the held one's edge. */
	function quantize(px: number, held: number): number {
		return held > 0 && Math.abs(px - held) < 24 ? held : Math.max(32, Math.round(px / 32) * 32);
	}

	$effect(() => {
		const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
		// Half-octave zoom steps, rounded up: a pinch crosses a few of them, not one per pointer
		// event, and the demand overshoots the drawn size by at most 41%.
		const scale = dpr * Math.pow(2, Math.ceil(Math.log2(zoom) * 2) / 2);
		const w = boxW;
		const h = boxH;
		if (!w || !h) return; // unmeasured: a 0 quantized to 32 would hold through the first real size
		untrack(() => {
			capW = quantize(w * scale, capW);
			capH = quantize(h * scale, capH);
			frozen = zoom < (frozen ? 0.34 : 0.3);
		});
	});

	// Subscribe only while in the viewport; the ResizeObserver tracks the pixel budget.
	onMount(() => {
		if (!container) return;
		const io = new IntersectionObserver(
			(entries) => {
				for (const e of entries) visible = e.isIntersecting;
			},
			{ rootMargin: '64px' }
		);
		io.observe(container);
		const ro = new ResizeObserver((entries) => {
			for (const e of entries) {
				boxW = e.contentRect.width;
				boxH = e.contentRect.height;
			}
		});
		ro.observe(container);
		return () => {
			io.disconnect();
			ro.disconnect();
		};
	});

	/** ONE hook for this viewer's stream, visibility and reduction; the registry settles what is sent. */
	$effect(() => {
		if (frozen) return;
		frame = null;
		if (!visible || !slot) return;
		// Kind is not part of the stream's identity, but it IS part of what this viewer needs.
		const specs = capW > 0 && capH > 0 ? viewSpecsForKind(kind, capW, capH) : null;
		return bindViewer(node, slot, token, specs, (f: DataFrame) => (frame = f));
	});
</script>

<div class="viewer-feed" bind:this={container}>
	{#if !slot}
		<EmptyState>
			{#snippet hint()}node has no output slots{/snippet}
		</EmptyState>
	{:else}
		<ViewerSurface {frame} {kind} {settings} />
	{/if}
</div>

<style>
	.viewer-feed {
		position: relative;
		flex: 1;
		min-width: 0;
		min-height: 0;
		display: flex;
		align-items: stretch;
		justify-content: stretch;
	}
	.viewer-feed > :global(*) {
		flex: 1;
		min-width: 0;
		min-height: 0;
	}
</style>
