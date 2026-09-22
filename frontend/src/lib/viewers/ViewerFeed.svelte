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
	import { isRenderable } from './kind';
	import { SERIES } from './palette';
	import { makeLUTCache } from './colormaps';
	import { formatTick } from './format';

	let { node, slot, binding }: { node: string; slot: string | null; binding: ViewBinding } =
		$props();

	const kind = $derived(binding.kind);
	const settings = $derived(binding.settings);
	const host = useSurface();
	const anchor = useAnchor();
	const onSurface = $derived(kind === 'line' || kind === 'image');

	let frame = $state.raw<DataFrame | null>(null);
	let visible = $state(false);
	let container: HTMLDivElement | null = $state(null);
	// Quantized to 32-px steps so a 1-px resize does not renegotiate the reduction.
	let capW = $state(0);
	let capH = $state(0);
	let layout = $state(0);
	// Stable per-instance token so multiple viewers of one slot collect (not evict).
	const token =
		typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : `vf-${Math.random()}`;

	// Surface kinds: the plot, and the little that still reaches the DOM.
	let plot = $state.raw<Plot | null>(null);
	let labels = $state<string[]>([]);
	let labelKey = '';
	let cursor = $state<{ x: number; values: number[] } | null>(null);
	const lutFor = makeLUTCache();

	function quantize(px: number): number {
		const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
		return Math.max(32, Math.round((px * dpr) / 32) * 32);
	}
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
			capW = quantize(el.clientWidth);
			capH = quantize(el.clientHeight);
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
		if (p instanceof LinePlot) p.setColors(SERIES);
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
		if (!p || !isArrayFrame(f) || !isRenderable(kind, f.data)) return;
		if (p instanceof LinePlot) {
			pushLine(p, f, capW, Boolean(settings.logX));
			if (pointerX !== null) cursor = p.valueAt(pointerX);
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

	// POINTER, not mouse, so a finger gets a readout too; a tap is a press with no motion. The
	// readout follows the frames while the pointer rests, so it stays true on a live plot.
	let pointerX: number | null = null;
	function onPointer(e: PointerEvent): void {
		const p = plot;
		if (!(p instanceof LinePlot) || !container) return;
		const r = container.getBoundingClientRect();
		pointerX = r.width > 0 ? ((e.clientX - r.left) / r.width) * container.offsetWidth : null;
		cursor = pointerX === null ? null : p.valueAt(pointerX);
	}
	function onLeave(e: PointerEvent): void {
		// A touch pointer is destroyed on release, so retracting on its `pointerleave` would flash the readout away.
		if (e.type === 'pointerleave' && e.pointerType !== 'mouse') return;
		pointerX = null;
		cursor = null;
	}
</script>

<!-- The pointer only feeds the readout; the body is not a control. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="viewer-feed"
	bind:this={container}
	onpointermove={onPointer}
	onpointerdown={onPointer}
	onpointerleave={onLeave}
	onpointercancel={onLeave}
>
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
		{#if cursor}
			<div class="cursor-chip" data-testid="cursor-chip">
				<span class="cursor-x">x={formatTick(cursor.x)}</span>
				{#each cursor.values as v, i (i)}
					<span class="cursor-y" style="color: {SERIES[i % SERIES.length]};">{formatTick(v) || '—'}</span>
				{/each}
			</div>
		{/if}
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
	/* The range labels: the surface draws no text, so its corners are annotated here. */
	.tick {
		position: absolute;
		padding: var(--space-1);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		line-height: 1;
		color: var(--text-dim);
		pointer-events: none;
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
	.cursor-chip {
		/* Bottom-center: the corners are taken by the range labels. */
		position: absolute;
		bottom: 4px;
		left: 50%;
		transform: translateX(-50%);
		display: flex;
		gap: var(--space-3);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		padding: var(--space-1) var(--space-3);
		background: color-mix(in srgb, var(--bg) 78%, transparent);
		border: 1px solid color-mix(in srgb, var(--text-muted) 30%, transparent);
		border-radius: 3px;
		color: var(--text-dim);
		pointer-events: none;
		max-width: calc(100% - 8px);
		overflow: hidden;
		white-space: nowrap;
		text-overflow: ellipsis;
		z-index: 2;
	}
	.cursor-x {
		color: var(--text-muted);
	}
	.cursor-y {
		font-variant-numeric: tabular-nums;
	}
</style>
