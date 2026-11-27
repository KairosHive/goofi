<!-- FlowCanvas — the shell every flow canvas has: the host its side pane sits beside, the grid, the
     zoom cluster, a camera kept per panel and framed once per load, the double-tap zoom and the
     held finger on the bare pane. A panel gives it the nodes, the edges and its own handlers; every
     other prop is SvelteFlow's. -->
<script lang="ts" module>
	import type { FitViewOptions } from '@xyflow/svelte';

	/** Framing for every programmatic fit. */
	export const FIT_OPTIONS = { maxZoom: 1, padding: 0.18 } satisfies FitViewOptions;
	/** How far a canvas may be zoomed; the double-tap zoom clamps itself to the same pair. */
	export const MIN_ZOOM = 0.05;
	export const MAX_ZOOM = 4;
</script>

<script lang="ts">
	import { Background, Controls, SvelteFlow, SvelteFlowProvider, type Edge, type Node, type SvelteFlowProps, type Viewport } from '@xyflow/svelte';
	import { createLongPress } from 'panelty';
	import { untrack, type Snippet } from 'svelte';
	import { on } from 'svelte/events';
	import { graph } from '$lib/stores/graph.svelte';
	import { camera } from './camera';
	import { bindTapZoom } from './doubleTapZoom';
	import FlowApi from './FlowApi.svelte';
	import { GRID } from './nodeMetrics';

	type Point = { x: number; y: number };

	let {
		panelId,
		nodes = $bindable(),
		edges = $bindable(),
		root = $bindable(null),
		screenToFlow = $bindable(),
		getViewport = $bindable(),
		fit = $bindable(),
		zoom = 1,
		fitWhen,
		holdAllowed = () => true,
		onHold,
		empty,
		hint = true,
		controls,
		inside,
		before,
		children,
		class: klass = '',
		style,
		...rest
	}: Omit<SvelteFlowProps, 'nodes' | 'edges' | 'viewport' | 'children' | 'class' | 'style'> & {
		panelId: string;
		nodes: Node[];
		edges: Edge[];
		/** The host: the canvas and whatever stands beside it. */
		root?: HTMLDivElement | null;
		screenToFlow?: (p: Point) => Point;
		getViewport?: () => Viewport;
		/** Frame the canvas; with no options, Flow's own framing. */
		fit?: (o?: FitViewOptions) => Promise<boolean>;
		/** The zoom a camera opens at before it has been framed or moved. */
		zoom?: number;
		/** Whether a load has anything to frame: an empty one must not arm a fit that the first
		 * card placed would then satisfy. */
		fitWhen: () => boolean;
		/** Whether the bare pane takes a held finger and a double-tap zoom right now. */
		holdAllowed?: () => boolean;
		/** A finger held on the bare pane. A held mouse button is the start of a desktop pan. */
		onHold: (at: { clientX: number; clientY: number }) => void;
		/** What a canvas with no card says, while `hint` allows; it never takes the press on it. */
		empty?: Snippet;
		hint?: boolean;
		/** The panel's own buttons in the zoom cluster. */
		controls?: Snippet;
		/** What the panel draws inside the flow. */
		inside?: Snippet;
		/** What stands in the host before the canvas, and after it. */
		before?: Snippet;
		children?: Snippet;
		class?: string;
		style?: string;
	} = $props();

	const g = graph();
	let setViewport = $state<((v: Viewport) => void) | undefined>(undefined);

	// A layout reshape or a page switch DESTROYS this component, so the camera outliving it is what
	// carries the framing across.
	const cam = camera(untrack(() => panelId));
	let viewport = $state<Viewport>(cam.viewport ?? { x: 0, y: 0, zoom: untrack(() => zoom) });

	// Frame the whole canvas after a wholesale load, never on an interactive add.
	$effect(() => {
		const epoch = g.loadEpoch;
		if (!fit || epoch === cam.fittedEpoch || !g.loadSettled) return;
		cam.fittedEpoch = epoch;
		if (fitWhen()) void fit(FIT_OPTIONS);
	});

	const bare = (target: EventTarget | null): boolean =>
		holdAllowed() && Boolean((target as HTMLElement | null)?.classList.contains('svelte-flow__pane'));
	const press = createLongPress((at) => onHold(at));

	// The double-tap-and-drag zoom, beside pinch; the seam is `zoomOnDoubleClick={false}` below. A
	// double tap held still stands the long press down.
	$effect(() => {
		const el = root;
		if (!el) return;
		const offs = [
			bindTapZoom(el, {
				allowed: bare,
				onStart: press.cancel,
				getViewport: () => getViewport?.(),
				screenToFlow: (p) => screenToFlow?.(p),
				setViewport: (v) => setViewport?.(v),
				min: MIN_ZOOM,
				max: MAX_ZOOM
			}),
			on(el, 'pointerdown', (e: PointerEvent) => {
				if (e.pointerType === 'touch' && bare(e.target)) press.start(e);
			}),
			on(el, 'pointermove', press.move),
			on(el, 'pointerup', press.cancel),
			on(el, 'pointercancel', press.cancel)
		];
		return () => {
			offs.forEach((off) => off());
			press.cancel(); // a press in flight must not fire into an unmounted canvas
		};
	});
</script>

<SvelteFlowProvider>
	<!-- `canvas-wrap` is the marker PlacementPreview uses to tell a commit click from a cancel. -->
	<div class={`flow-canvas canvas-wrap ${klass}`.trim()} {style} bind:this={root}>
		{@render before?.()}
		<SvelteFlow
			bind:nodes
			bind:edges
			fitViewOptions={FIT_OPTIONS}
			minZoom={MIN_ZOOM}
			maxZoom={MAX_ZOOM}
			bind:viewport={() => viewport, (v) => (viewport = cam.viewport = v)}
			zoomOnDoubleClick={false}
			autoPanOnNodeDrag={false}
			{...rest}
		>
			<Background gap={GRID} size={1} patternColor="var(--border)" />
			<!-- `showLock` off: goofi has no read-only mode, so Flow's lock reads as breakage. -->
			<Controls showLock={false}>{@render controls?.()}</Controls>
			<FlowApi bind:screenToFlowPosition={screenToFlow} bind:getViewport bind:setViewport bind:fitView={fit} />
			{@render inside?.()}
		</SvelteFlow>
		{#if empty && hint && nodes.length === 0}
			<div class="empty-hint" data-testid="empty-hint">{@render empty()}</div>
		{/if}
		{@render children?.()}
	</div>
</SvelteFlowProvider>

<style>
	/* The canvas and the side pane side by side: the pane takes its width from the canvas, so the
	   zoom cluster in the canvas's corner is never under it. */
	.flow-canvas {
		position: relative;
		display: flex;
		width: 100%;
		height: 100%;
		min-width: 0;
		min-height: 0;
	}
	.flow-canvas > :global(.svelte-flow) {
		flex: 1 1 0;
		min-width: 0;
		min-height: 0;
	}
	@media all {
		@container (orientation: portrait) {
			.flow-canvas {
				flex-direction: column;
			}
		}
	}
	/* `margin: 0` is load-bearing: Flow's own `.svelte-flow__panel` sets `margin: 15px`, which STACKS
	   on these offsets. The cluster shows while the pointer is in the panel; a touch screen, with no
	   pointer to wait for, shows it always. */
	.flow-canvas :global(.svelte-flow__controls) {
		margin: 0;
		bottom: var(--flow-controls-inset, var(--space-6));
		left: var(--flow-controls-inset, var(--space-6));
		opacity: 0;
		transition: opacity var(--dur-fast) var(--ease);
	}
	.flow-canvas:hover :global(.svelte-flow__controls),
	.flow-canvas :global(.svelte-flow__controls:focus-within) {
		opacity: 1;
	}
	/* On touch each button is floored to --hit in both axes, so the cluster is a slab and takes the
	   small inset whatever the panel asked. */
	@media (hover: none) and (pointer: coarse) {
		.flow-canvas :global(.svelte-flow__controls) {
			bottom: var(--space-6);
			left: var(--space-6);
			opacity: 1;
		}
		/* A portrait sheet's grab band leans --hit over the canvas's bottom edge; the cluster sits above it. */
		@container (orientation: portrait) {
			.flow-canvas:has(> :global(.side-panel.open)) :global(.svelte-flow__controls) {
				bottom: calc(var(--space-6) + var(--hit));
			}
		}
	}
	/* Non-interactive, so it never eats the double-click that lands on the pane under it. */
	.empty-hint {
		position: absolute;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		pointer-events: none;
		z-index: 1;
	}
</style>
