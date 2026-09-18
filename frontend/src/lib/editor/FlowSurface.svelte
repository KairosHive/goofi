<script lang="ts">
	/** The editor's plot surface: one canvas under the node cards, following the camera in flow
	 * units so a flow-unit rect lands 1:1 on device pixels. Render inside <SvelteFlow>. */
	import { useStore } from '@xyflow/svelte';
	import { createSurface, type Surface } from 'glance';

	let { surface = $bindable(null) }: { surface: Surface | null } = $props();
	const store = useStore();
	let canvas: HTMLCanvasElement | null = null;

	$effect(() => {
		const layer = store.domNode?.querySelector<HTMLElement>('.svelte-flow__nodes');
		if (!layer) return;
		const c = document.createElement('canvas');
		c.className = 'plot-surface';
		c.style.cssText = 'position:absolute;pointer-events:none;';
		layer.prepend(c);
		let s: Surface | null = null;
		try {
			s = createSurface(c);
		} catch (err) {
			console.warn(err);
		}
		canvas = c;
		surface = s;
		return () => {
			s?.dispose();
			c.remove();
			canvas = null;
			surface = null;
		};
	});

	$effect(() => {
		const { x, y, zoom } = store.viewport;
		const { width, height } = store;
		const s = surface;
		if (!s || !canvas) return;
		canvas.style.left = `${-x / zoom}px`;
		canvas.style.top = `${-y / zoom}px`;
		canvas.style.width = `${width / zoom}px`;
		canvas.style.height = `${height / zoom}px`;
		s.setView({ x, y, zoom, width, height, dpr: window.devicePixelRatio || 1 });
	});
</script>
