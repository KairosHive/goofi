<!-- The viewer body: a lazily-subscribed Data frame, drawn on the host's plot surface for lines and
     images and by a component for every other kind. Padding is the caller's. -->
<script lang="ts">
	import { bindViewer } from '$lib/api/frames';
	import { viewSpecsForKind } from './capacity';
	import { isArrayFrame, type DataFrame } from '$lib/codec/decode';
	import ViewerSurface from './ViewerSurface.svelte';
	import type { ViewBinding } from './viewBinding';
	import { EmptyState } from '$lib/ui';
	import { untrack } from 'svelte';
	import { LinePlot, type Plot } from 'glance';
	import { offsetIn, useAnchor, useSurface } from './plotHost';
	import { pushImage, pushLine } from './plotFeed';
	import { drawsOnSurface, isRenderable } from './kind';
	import { makeLUTCache } from './colormaps';
	import { formatTick } from './format';

	/** `zoom` is the flow zoom the viewer is drawn under; a docked panel draws at 1. */
	let {
		node,
		slot,
		binding,
		zoom = 1
	}: { node: string; slot: string | null; binding: ViewBinding; zoom?: number } = $props();

	const kind = $derived(binding.kind);
	const settings = $derived(binding.settings);
	const host = useSurface();
	const anchor = useAnchor();
	const onSurface = $derived(drawsOnSurface(kind));

	let frame = $state.raw<DataFrame | null>(null);
	let visible = $state(false);
	let container: HTMLDivElement | null = $state(null);
	// The content box in CSS px; the device box below is derived from it, the DPR and the zoom.
	let boxW = $state(0);
	let boxH = $state(0);
	// Quantized to 32-px steps so a 1-px resize does not renegotiate the reduction.
	let capW = $state(0);
	let capH = $state(0);
	// Bumped when this body or its card moved, so the plot rect is measured again.
	let layout = $state(0);
	// Below a readable zoom the viewer unsubscribes and keeps its last frame as a thumbnail.
	let frozen = $state(false);
	// Stable per-instance token so multiple viewers of one slot collect (not evict).
	const token =
		typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : `vf-${Math.random()}`;

	// Surface kinds: the plot, and the little that still reaches the DOM.
	let plot = $state.raw<Plot | null>(null);
	let labels = $state<string[]>([]);
	let labelKey = '';
	const lutFor = makeLUTCache();

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

	// Subscribe only while in the viewport; the ResizeObserver tracks the pixel budget and the rect.
	$effect(() => {
		const el = container;
		if (!el) return;
		const io = new IntersectionObserver(
			(entries) => {
				for (const e of entries) visible = e.isIntersecting;
			},
			{ rootMargin: '64px' }
		);
		io.observe(el);
		const ro = new ResizeObserver(() => {
			boxW = el.clientWidth;
			boxH = el.clientHeight;
			layout++;
		});
		ro.observe(el);
		if (anchor?.el) ro.observe(anchor.el);
		return () => {
			io.disconnect();
			ro.disconnect();
		};
	});

	/** ONE hook for this viewer's stream, visibility and reduction; the registry settles what is sent. */
	$effect(() => {
		if (frozen) return;
		frame = null;
		labels = [];
		labelKey = '';
		untrack(() => plot?.clear());
		if (!visible || !slot) return;
		// Kind is not part of the stream's identity, but it IS part of what this viewer needs.
		const specs = capW > 0 && capH > 0 ? viewSpecsForKind(kind, capW, capH) : null;
		const draw = onSurface;
		// A joiner is replayed the current frame at once, so the delivery must not become a dependency.
		return bindViewer(node, slot, token, specs, (f: DataFrame) =>
			untrack(() => {
				frame = f;
				if (draw) drawFrame(f);
			})
		);
	});

	$effect(() => {
		const s = host?.surface;
		const el = container;
		if (!onSurface || !s || !el) return;
		const p = kind === 'line' ? s.addLine() : s.addImage();
		p.setBackground(getComputedStyle(el).getPropertyValue('--bg').trim());
		plot = p;
		return () => {
			p.remove();
			plot = null;
		};
	});

	// The rect follows the card: its flow position, and this body's offset inside it.
	$effect(() => {
		const p = plot;
		const a = anchor;
		void layout;
		if (!p || !a?.el || !container) return;
		const o = offsetIn(container, a.el);
		p.setRect(a.x + o.x, a.y + o.y, o.w, o.h);
		p.setOrder(a.z);
	});

	$effect(() => {
		const p = plot;
		const s = settings;
		if (!p) return;
		if (p instanceof LinePlot) {
			p.setSettings({
				logX: Boolean(s.logX),
				logY: Boolean(s.logY),
				yAuto: s.yAuto !== false,
				yMin: Number(s.yMin ?? -1),
				yMax: Number(s.yMax ?? 1),
				points: Boolean(s.points)
			});
		} else {
			p.setSettings({ lut: lutFor(String(s.colormap ?? 'gray')), stretch: s.stretch === true });
		}
		untrack(() => frame && drawFrame(frame));
	});

	function drawFrame(f: DataFrame): void {
		const p = plot;
		if (!p || !isArrayFrame(f)) return;
		// A frame this kind cannot draw takes the fallback text; the trace before it must not stay under it.
		if (!isRenderable(kind, f.data)) {
			p.clear();
			labels = [];
			labelKey = '';
			return;
		}
		if (capW === 0) return; // unmeasured: the re-bind on the first size replays the frame
		if (p instanceof LinePlot) {
			pushLine(p, f, capW, Boolean(settings.logX));
			const r = p.range();
			if (!r) return;
			const next = r.scalar
				? ['', formatTick(r.xMin), formatTick(r.xMax)]
				: [formatTick(r.yMax), formatTick(r.yMin), formatTick(r.xMax)];
			const key = next.join('|');
			if (key !== labelKey) {
				labelKey = key;
				labels = next;
			}
		} else {
			pushImage(p, f, settings);
		}
	}

</script>

<div class="viewer-feed" bind:this={container}>
	{#if !slot}
		<EmptyState>
			{#snippet hint()}node has no output slots{/snippet}
		</EmptyState>
	{:else if onSurface && !host?.surface}
		<EmptyState>
			{#snippet hint()}WebGL2 is not available{/snippet}
		</EmptyState>
	{:else}
		<ViewerSurface {frame} {kind} {settings} />
		{#each labels as text, i (i)}
			{#if text}<span class="tick tick-{i}">{text}</span>{/if}
		{/each}
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
	/* The range labels: the surface draws no text, so its corners are annotated here, on hover. */
	.tick {
		position: absolute;
		opacity: 0;
		transition: opacity var(--dur-slow) var(--ease);
		padding: var(--space-1);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		line-height: 1;
		color: var(--text-dim);
		pointer-events: none;
	}
	.viewer-feed:hover .tick {
		opacity: 1;
	}
	/* No hover on a touch screen: the labels rest visible there. */
	@media (hover: none) and (pointer: coarse) {
		.tick {
			opacity: 1;
		}
	}
	.tick-0 {
		top: 0;
		left: 0;
	}
	.tick-1 {
		bottom: 0;
		left: 0;
	}
	.tick-2 {
		bottom: 0;
		right: 0;
	}
</style>
