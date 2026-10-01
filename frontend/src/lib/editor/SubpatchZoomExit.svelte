<!-- Zoom out far enough to leave a sub-patch. Must live INSIDE <SvelteFlow> for the hooks to
     resolve, and renders nothing. The threshold is derived live from the content's fit zoom. -->
<script lang="ts">
	import { useViewport, useSvelteFlow, useStore, getViewportForBounds } from '@xyflow/svelte';

	// `options` and `minZoom` are the editor's fit framing, which the exit zoom is measured against.
	type Props = { entered: string | null; options: { maxZoom: number; padding: number }; minZoom: number; onExit: () => void };
	let { entered, options, minZoom, onExit }: Props = $props();

	const EXIT_RATIO = 0.5; // pop at half the all-node-fit zoom ("50% past fit")

	const vp = useViewport();
	const store = useStore();
	const { getNodesBounds } = useSvelteFlow();
	let armed = false; // plain, not reactive: only a zoom change drives the check

	$effect(() => {
		// Arm shortly after descending, so the enter-fit settles first.
		const cur = entered;
		armed = false;
		if (!cur) return;
		const t = setTimeout(() => {
			armed = true;
		}, 220);
		return () => clearTimeout(t);
	});

	// On the nodes and the pane size, not on the zoom: a pinch must not re-measure every node per step.
	const fitZoom = $derived.by(() => {
		const w = store.width;
		const h = store.height;
		// The bounds read the store untracked, so what moves them is read here: a node's measure
		// (it is listed before it is measured) and its position.
		const ids = store.nodes.filter((n) => n.measured?.width && Number.isFinite(n.position.x + n.position.y)).map((n) => n.id);
		if (!w || !h || ids.length === 0) return null;
		const bounds = getNodesBounds(ids);
		if (!bounds.width || !bounds.height) return null;
		return getViewportForBounds(bounds, w, h, minZoom, options.maxZoom, options.padding).zoom;
	});

	$effect(() => {
		const z = vp.current.zoom;
		if (!entered || !armed || fitZoom === null) return;
		if (z < fitZoom * EXIT_RATIO) {
			armed = false; // one-shot; the exit re-fit restores a higher zoom
			onExit();
		}
	});
</script>
