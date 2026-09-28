<!-- The viewer body: a lazily-subscribed Data frame, drawn in the worker on the host's plot
     surface for every array kind and by a component here for the text kinds. Padding is the caller's. -->
<script lang="ts">
	import { bindViewer } from '$lib/api/frames';
	import { createDrawing, type DrawingHandle } from '$lib/api/drawings';
	import { viewSpecsForKind } from './capacity';
	import type { DataFrame } from '$lib/codec/decode';
	import ViewerSurface from './ViewerSurface.svelte';
	import HighDimFallback from './HighDimFallback.svelte';
	import type { ViewBinding } from './viewBinding';
	import { EmptyState } from '$lib/ui';
	import { untrack } from 'svelte';
	import { offsetIn, useAnchor, useSurface } from './plotHost';
	import type { DrawnState } from './drawing';
	import type { Hover } from './hover';
	import { portal } from 'panelty';
	import { drawsOnSurface, isTrajectory } from './kind';

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
	const trajectory = $derived(isTrajectory(kind, settings));

	// What a text kind shows: its frame, read on this thread.
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

	// The drawing of an array kind, in the worker, and what it reports for the DOM.
	const NOTHING: DrawnState = { has: false, fallback: null, labels: [], texts: [], message: null, drag: false };
	let drawing = $state.raw<DrawingHandle | null>(null);
	let drawn = $state.raw<DrawnState>(NOTHING);
	let dragging: { x: number; y: number } | null = null;
	// Whether the feed holds the pointer: a finger reading it, or a drag on the drawing.
	let held = false;
	let hover = $state.raw<Hover | null>(null);
	let pointer = $state.raw<{ x: number; y: number } | null>(null);
	// The pointer in viewport px: the readout is portalled, so the window edge is its only bound.
	let client = $state.raw<{ x: number; y: number } | null>(null);
	let readoutW = $state(0);
	let readoutH = $state(0);
	const GAP = 6;
	// Up and left of the pointer, away from where a hand or a pen sits; a side flips only where
	// the window would cut it off.
	const readoutPos = $derived.by(() => {
		if (!client) return null;
		let left = client.x - GAP - readoutW;
		if (left < 0) left = client.x + GAP;
		let top = client.y - GAP - readoutH;
		if (top < 0) top = client.y + GAP;
		return { left, top };
	});

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
		if (!visible || !slot) return;
		// Kind is not part of the stream's identity, but it IS part of what this viewer needs.
		const specs = capW > 0 && capH > 0 ? viewSpecsForKind(kind, capW, capH, settings) : null;
		// A text kind reads its frames here; an array kind's drawing takes them in the worker.
		return bindViewer(node, slot, token, specs, onSurface ? null : (f: DataFrame) => (frame = f));
	});

	$effect(() => {
		const s = host?.surface;
		const el = container;
		if (!onSurface || !s || !el) return;
		const d = createDrawing(
			s,
			kind,
			trajectory,
			(state) => (drawn = state),
			(h) => (hover = h)
		);
		d.setBackground(getComputedStyle(el).getPropertyValue('--bg').trim());
		drawing = d;
		return () => {
			d.remove();
			drawing = null;
			drawn = NOTHING;
			hover = null;
		};
	});

	// The drawing follows the stream while the body is on screen; zoomed out past legibility it
	// lets the stream go and keeps its last picture as a thumbnail. Decided from the settled state,
	// never in a teardown, which sees the values of the run before.
	$effect(() => {
		const d = drawing;
		if (!d) return;
		if (frozen) d.detach(true);
		else if (!visible || !slot) d.detach(false);
		else d.attach(node, slot);
	});

	// The rect follows the card: its flow position, and this body's offset inside it.
	$effect(() => {
		const d = drawing;
		const a = anchor;
		void layout;
		if (!d || !a?.el || !container) return;
		const o = offsetIn(container, a.el);
		d.place(a.x + o.x, a.y + o.y, o.w, o.h, a.z);
	});

	// The settings and the body's size, under which the worker draws each frame.
	$effect(() => {
		const d = drawing;
		if (!d || !capW) return; // unmeasured: nothing draws before the first size
		d.settings(settings, { w: boxW, h: boxH, cols: capW, zoom });
	});

	/** The pointer in layout px: the box is measured under the flow zoom, the pointer is not. */
	function layoutPoint(e: PointerEvent): { x: number; y: number } | null {
		if (!container) return null;
		const r = container.getBoundingClientRect();
		const scale = r.width > 0 ? boxW / r.width : 1;
		return { x: (e.clientX - r.left) * scale, y: (e.clientY - r.top) * scale };
	}

	function probeBox(): { w: number; h: number; tol: number } {
		return { w: boxW, h: boxH, tol: 12 / zoom };
	}

	function onPointerDown(e: PointerEvent): void {
		if (e.button !== 0) return;
		// A finger is the hover a touch screen does not have: held down, it reads the picture and
		// follows, and the card is not carried by it. A mouse only reads by resting over the body.
		const finger = e.pointerType === 'touch';
		if (!drawn.drag && !finger) return;
		if (drawn.drag) dragging = layoutPoint(e);
		held = true;
		container?.setPointerCapture(e.pointerId);
		if (finger) onPointerMove(e);
		// The card must not take this press as the start of a node drag, nor a menu as a dismissal.
		e.stopPropagation();
	}

	/** The node's drag and the pane's pan start on touch events, which a finger reading a viewer
	 * keeps to itself; they have not started, so a still finger is no long press on the pane. */
	function keepTouch(e: TouchEvent): void {
		e.stopPropagation();
	}

	function onPointerMove(e: PointerEvent): void {
		const at = layoutPoint(e);
		if (!at) return;
		if (dragging && drawn.drag) {
			drawing?.drag(at.x - dragging.x, at.y - dragging.y, probeBox());
			dragging = at;
		}
		pointer = at;
		client = { x: e.clientX, y: e.clientY };
		if (boxW && boxH) drawing?.pointer({ ...at, box: probeBox() });
	}

	function onPointerUp(e: PointerEvent): void {
		dragging = null;
		held = false;
		if (container?.hasPointerCapture(e.pointerId)) container.releasePointerCapture(e.pointerId);
		// A lifted finger hovers nothing.
		if (e.pointerType === 'touch') onPointerLeave(e);
	}

	function onPointerLeave(e: PointerEvent): void {
		// A captured pointer leaves the box and comes back; only its release ends the hold.
		if (held && e.type === 'pointerleave') return;
		dragging = null;
		held = false;
		pointer = null;
		client = null;
		hover = null;
		drawing?.pointer(null);
	}
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="viewer-feed"
	class:nodrag={drawn.drag}
	bind:this={container}
	onpointerdown={onPointerDown}
	onpointermove={onPointerMove}
	onpointerup={onPointerUp}
	onpointerleave={onPointerLeave}
	onpointercancel={onPointerLeave}
	ontouchstart={keepTouch}
	ontouchmove={keepTouch}
>
	{#if !slot}
		<EmptyState>
			{#snippet hint()}node has no output slots{/snippet}
		</EmptyState>
	{:else if onSurface && !host?.surface}
		<EmptyState>
			{#snippet hint()}WebGL2 is not available{/snippet}
		</EmptyState>
	{:else if !onSurface}
		<ViewerSurface {frame} {settings} />
	{:else if !drawn.has}
		<EmptyState>
			{#snippet hint()}no data yet{/snippet}
		</EmptyState>
	{:else}
		{#if drawn.fallback}
			<HighDimFallback summary={drawn.fallback} />
		{/if}
		{#each drawn.labels as text, i (i)}
			{#if text}<span class="tick tick-{i}">{text}</span>{/if}
		{/each}
		{#each drawn.texts as t, i (i)}
			<span
				class="placed"
				style:left="{t.x}px"
				style:top="{t.y}px"
				style:transform="rotate({t.angle}rad) translate({t.align === 'right' ? '-100%' : '0'}, -50%)"
				style:color={t.color}>{t.text}</span
			>
		{/each}
		{#if drawn.message}
			<span class="message">{drawn.message}</span>
		{/if}
		{#if hover && pointer}
			{#if hover.mark}
				<span
					class="mark"
					style:left="{hover.mark.x}px"
					style:top="{hover.mark.y}px"
					style:width="{2 * hover.mark.r}px"
					style:height="{2 * hover.mark.r}px"
				></span>
			{/if}
			{#if readoutPos}
				<span
					class="viewer-hover-readout"
					style:left="{readoutPos.left}px"
					style:top="{readoutPos.top}px"
					bind:offsetWidth={readoutW}
					bind:offsetHeight={readoutH}
					use:portal
				>
					{#each hover.lines as line, i (i)}
						<span class="row">{#each line as part, j (j)}<span>{part}</span>{/each}</span>
					{/each}
				</span>
			{/if}
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
	/* A finger on a viewer reads it, so the page gets no scroll or pan from it, and a held one
	   selects no text. */
	.viewer-feed {
		touch-action: none;
		user-select: none;
		-webkit-user-select: none;
		-webkit-touch-callout: none;
	}
	.viewer-feed > :global(*) {
		flex: 1;
		min-width: 0;
		min-height: 0;
	}
	/* The range labels: the surface draws no text, so its corners are annotated here — on hover,
	   on a selected card, and always in a docked panel. */
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
	.viewer-feed:hover .tick,
	:global(.svelte-flow__node.selected) .tick,
	:global(.vp-body) .tick {
		opacity: 1;
	}
	/* No hover on a touch screen: the labels rest visible there. */
	@media (hover: none) and (pointer: coarse) {
		.tick {
			opacity: 1;
		}
	}
	/* Text a drawing places on its picture, turned about its anchor; a message sits in the middle. */
	.placed,
	.message {
		position: absolute;
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		line-height: 1;
		color: var(--text-dim);
		white-space: nowrap;
		pointer-events: none;
	}
	.placed {
		transform-origin: 0 50%;
	}
	.message {
		left: 50%;
		top: 50%;
		transform: translate(-50%, -50%);
	}
	/* The mark sits on the probed point; the readout follows the pointer, on <body> so a card's
	   edge never clips it. */
	.mark {
		position: absolute;
		pointer-events: none;
		transform: translate(-50%, -50%);
		border-radius: 50%;
		background: var(--text);
		box-shadow: 0 0 0 1px var(--bg);
	}
	:global(.viewer-hover-readout) {
		position: fixed;
		z-index: var(--z-menu);
		pointer-events: none;
		display: flex;
		flex-direction: column;
		gap: 1px;
		padding: var(--space-1);
		border-radius: var(--radius-sm);
		background: var(--surface-2);
		border: 1px solid var(--border-strong);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		line-height: 1.2;
		color: var(--text);
		white-space: nowrap;
	}
	:global(.viewer-hover-readout .row) {
		display: flex;
		gap: var(--space-2);
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
