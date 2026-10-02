<!-- The dashed lines between the selected node and the nodes whose params follow an output slot,
     in either direction. Drawn from the graph store; not cables, and not interactive. -->
<script lang="ts">
	import type { Node } from '@xyflow/svelte';
	import { graph, slotReference } from '$lib/stores/graph.svelte';
	import { isSlotExpanded } from '$lib/viewers/inlineView';
	import { NODE, outputTops } from './nodeMetrics';

	let { nodes, selected }: { nodes: Node[]; selected: string | null } = $props();
	const g = graph();

	const paths = $derived.by(() => {
		if (!selected) return [];
		const drawn = new Map(nodes.map((n) => [n.id, n]));
		const slots = new Map<string, { uid: string; slot: string }>();
		for (const n of nodes) {
			const info = g.nodeById(n.id);
			for (const slot of Object.keys(info?.output_slots ?? {})) if (info) slots.set(slotReference(info, slot), { uid: n.id, slot });
		}
		const out = new Map<string, string>();
		for (const n of nodes) {
			for (const group of Object.values(g.nodeById(n.id)?.params ?? {})) {
				for (const d of Object.values(group)) {
					const src = d.mode === 'reference' && d.reference ? slots.get(d.reference) : undefined;
					if (!src || src.uid === n.id || (src.uid !== selected && n.id !== selected)) continue;
					const key = `${src.uid}/${src.slot}>${n.id}`;
					const a = drawn.get(src.uid)!;
					const info = g.nodeById(src.uid)!;
					const top = outputTops(Object.keys(info.output_slots), (s) => isSlotExpanded(info, s)).find((p) => p.slot === src.slot)!.top;
					const [x1, y1] = [a.position.x + (a.measured?.width ?? NODE.width), a.position.y + top];
					const [x2, y2] = [n.position.x, n.position.y + NODE.header / 2];
					const bend = Math.max(40, Math.abs(x2 - x1) / 2);
					out.set(key, `M${x1},${y1} C${x1 + bend},${y1} ${x2 - bend},${y2} ${x2},${y2}`);
				}
			}
		}
		return [...out];
	});
</script>

{#if paths.length}
	<svg class="reference-edges" data-testid="reference-edges">
		{#each paths as [key, d] (key)}
			<path {d} data-testid="reference-edge" data-edge={key} />
		{/each}
	</svg>
{/if}

<style>
	.reference-edges {
		position: absolute;
		left: 0;
		top: 0;
		width: 1px;
		height: 1px;
		overflow: visible;
		pointer-events: none;
	}
	path {
		fill: none;
		stroke: var(--text-muted);
		stroke-width: 1.5;
		stroke-dasharray: 5 4;
	}
</style>
