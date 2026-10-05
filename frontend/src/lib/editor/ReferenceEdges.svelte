<!-- The dashed lines between the selected node and the nodes whose expressions read an output of
     it, or that it reads, in either direction. Drawn from the graph store; not cables, and not
     interactive. -->
<script lang="ts">
	import type { Node } from '@xyflow/svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import { isSlotExpanded } from '$lib/viewers/inlineView';
	import { NODE, outputTops } from './nodeMetrics';

	let { nodes, selected, drawEndpoint }: {
		nodes: Node[];
		selected: string | null;
		drawEndpoint: (node: string, slot: string) => { node: string; handle: string } | null;
	} = $props();
	const g = graph();
	const paths = $derived.by(() => {
		if (!selected) return null;
		const drawn = new Map(nodes.map((n) => [n.id, n]));
		const out = new Map<string, string>();
		const variables = new Map<string, { x: number; y: number; name: string }>();
		for (const record of g.nodes) {
			const consumer = drawEndpoint(record.uid, '');
			const n = consumer && drawn.get(consumer.node);
			if (!n) continue;
			for (const group of Object.values(record.params ?? {})) {
				for (const d of Object.values(group)) {
					const sources = [d, ...(d.elements ?? [])].filter((s) => s.mode === 'expression' && s.expression);
					for (const s of sources) {
						for (const dependency of s.dependencies ?? []) {
							if (dependency.kind === 'variable') {
								if (n.id !== selected) continue;
								const key = `variables.${dependency.name}>${n.id}`;
								if (variables.has(key)) continue;
								const x = n.position.x;
								const y = n.position.y - 28 - variables.size * 24;
								variables.set(key, { x, y, name: dependency.name });
								out.set(key, `M${x},${y + 4} L${x},${n.position.y + NODE.header / 2}`);
								continue;
							}
							const producer = drawEndpoint(dependency.node, dependency.kind === 'output' ? dependency.slot : '');
							if (!producer) continue;
							const { node: uid, handle: slot } = producer;
							if (uid === n.id || (uid !== selected && n.id !== selected)) continue;
							const a = drawn.get(uid);
							const info = g.nodeById(uid);
							if (!a || !info) continue;
							const key = `${uid}/${dependency.kind === 'param' ? `${dependency.group}/${dependency.name}` : slot}>${n.id}`;
							const top = dependency.kind === 'param' || uid !== dependency.node ? NODE.header / 2
								: outputTops(Object.keys(info.output_slots), (k) => isSlotExpanded(info, k)).find((p) => p.slot === slot)?.top;
							if (top === undefined) continue;
							const [x1, y1] = [a.position.x + (a.measured?.width ?? NODE.width), a.position.y + top];
							const [x2, y2] = [n.position.x, n.position.y + NODE.header / 2];
							const bend = Math.max(40, Math.abs(x2 - x1) / 2);
							out.set(key, `M${x1},${y1} C${x1 + bend},${y1} ${x2 - bend},${y2} ${x2},${y2}`);
						}
					}
				}
			}
		}
		return { edges: [...out], variables: [...variables] };
	});
</script>

{#if paths && paths.edges.length}
	<svg class="reference-edges" data-testid="reference-edges">
		{#each paths.edges as [key, d] (key)}
			<path {d} data-testid="reference-edge" data-edge={key} />
		{/each}
		{#each paths.variables as [key, variable] (key)}
			<text x={variable.x} y={variable.y} data-testid="variable-dependency">variables.{variable.name}</text>
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
	text {
		fill: var(--text-muted);
		font-size: var(--fs-small);
		font-family: var(--font-mono);
	}
</style>
