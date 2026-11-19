<script lang="ts">
	// Bridges the SvelteFlow API up to the panel's <script>, which cannot call useSvelteFlow()
	// itself, and holds the one rule every flow canvas has for a selection. Render this inside
	// <SvelteFlow> and bind the functions you need.
	import { useStore, useSvelteFlow, type FitViewOptions, type Viewport } from '@xyflow/svelte';

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

	// A selection is the cards' own, however it was made. Flow lays a rectangle over the cards a
	// box has picked, which takes every press on them: a click on one of them then picks nothing.
	// With no rectangle a press on a picked card is that card's, as it is after a pick by click:
	// a click picks it alone, and a drag moves every card picked.
	const store = useStore();
	$effect(() => {
		if (store.selectionRectMode === 'nodes') store.selectionRectMode = null;
	});
</script>
