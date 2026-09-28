<script lang="ts">
	/** The editor's plot surface: one canvas under the node cards, following the camera in flow
	 * units so a flow-unit rect lands 1:1 on device pixels. Render inside <SvelteFlow>. */
	import { useStore } from '@xyflow/svelte';
	import type { Surface } from 'plotluck';
	import { mountSurface, type SurfaceMount } from '$lib/viewers/plotHost';

	let { surface = $bindable(null) }: { surface: Surface | null } = $props();
	const store = useStore();
	let canvas: HTMLCanvasElement | null = null;
	let mount = $state.raw<SurfaceMount | null>(null);

	$effect(() => {
		const layer = store.domNode?.querySelector<HTMLElement>('.svelte-flow__nodes');
		if (!layer) return;
		const c = document.createElement('canvas');
		c.className = 'plot-surface';
		c.style.cssText = 'position:absolute;pointer-events:none;';
		// Under the cards and over the cables, in the viewport's transform; not inside the nodes
		// layer, which Svelte empties wholesale with the last card and the canvas along with it.
		layer.before(c);
		const m = mountSurface(c);
		canvas = c;
		mount = m;
		surface = m.surface;
		return () => {
			m.dispose();
			c.remove();
			canvas = null;
			mount = null;
			surface = null;
		};
	});

	$effect(() => {
		const { x, y, zoom } = store.viewport;
		const { width, height } = store;
		const m = mount;
		if (!m || !canvas) return;
		canvas.style.left = `${-x / zoom}px`;
		canvas.style.top = `${-y / zoom}px`;
		canvas.style.width = `${width / zoom}px`;
		canvas.style.height = `${height / zoom}px`;
		m.setView({ x, y, zoom, width, height });
	});
</script>
