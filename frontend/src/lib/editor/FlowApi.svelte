<script lang="ts">
	// Bridges the SvelteFlow API up to the panel's <script>, which cannot call useSvelteFlow()
	// itself. Render this inside <SvelteFlow> and bind the functions you need.
	import { useSvelteFlow, type FitViewOptions, type Viewport } from '@xyflow/svelte';

	type ScreenToFlow = (p: { x: number; y: number }) => { x: number; y: number };

	let {
		screenToFlowPosition = $bindable(),
		getViewport = $bindable(),
		setViewport = $bindable(),
		fitView = $bindable()
	}: {
		screenToFlowPosition?: ScreenToFlow;
		/** The pan/zoom matrix and the way to write it, so no caller writes the transform directly. */
		getViewport?: () => Viewport;
		setViewport?: (v: Viewport) => void;
		fitView?: (o?: FitViewOptions) => Promise<boolean>;
	} = $props();

	const flow = useSvelteFlow();
	screenToFlowPosition = flow.screenToFlowPosition;
	getViewport = flow.getViewport;
	setViewport = flow.setViewport;
	fitView = flow.fitView;
</script>
