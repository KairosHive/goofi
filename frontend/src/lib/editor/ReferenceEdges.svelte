<!-- The dashed lines between the selected node and the nodes whose expressions read an output of
     it, or that it reads, in either direction. Drawn from the graph store; not cables, and not
     interactive. -->
<script lang="ts">
	import type { Node } from '@xyflow/svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import { isSlotExpanded } from '$lib/viewers/inlineView';
	import { NODE, outputTops } from './nodeMetrics';

	let { nodes, selected }: { nodes: Node[]; selected: string | null } = $props();
	const g = graph();

	/** Every `nd('name')` term of an expression, with the slot label behind `.out`, if any. */
	const ND = /nd\(\s*(['"])(.+?)\1\s*\)(?:\s*\.\s*out\s*\.\s*([A-Za-z_][A-Za-z0-9_]*))?/g;

	const paths = $derived.by(() => {
		if (!selected) return [];
		const drawn = new Map(nodes.map((n) => [n.id, n]));
		const byName = new Map(nodes.flatMap((n) => (g.nodeById(n.id) ? [[g.nodeById(n.id)!.name, n.id] as const] : [])));
		// The slot a term names: the label behind `.out`, or a one-output node's only slot.
		const slotOf = (uid: string, label: string | undefined): string | undefined => {
			const info = g.nodeById(uid);
			const keys = Object.keys(info?.output_slots ?? {});
			if (label === undefined) return keys.length === 1 ? keys[0] : undefined;
			return keys.find((k) => (info?.slot_labels?.[k] ?? k) === label);
		};
		const out = new Map<string, string>();
		for (const n of nodes) {
			for (const group of Object.values(g.nodeById(n.id)?.params ?? {})) {
				for (const d of Object.values(group)) {
					const sources = [d, ...(d.elements ?? [])].filter((s) => s.mode === 'expression' && s.expression);
					for (const s of sources) {
						for (const m of s.expression!.matchAll(ND)) {
							const uid = byName.get(m[2]);
							const slot = uid === undefined ? undefined : slotOf(uid, m[3]);
							if (!uid || !slot || uid === n.id || (uid !== selected && n.id !== selected)) continue;
							const key = `${uid}/${slot}>${n.id}`;
							const a = drawn.get(uid)!;
							const info = g.nodeById(uid)!;
							const top = outputTops(Object.keys(info.output_slots), (k) => isSlotExpanded(info, k)).find((p) => p.slot === slot)!.top;
							const [x1, y1] = [a.position.x + (a.measured?.width ?? NODE.width), a.position.y + top];
							const [x2, y2] = [n.position.x, n.position.y + NODE.header / 2];
							const bend = Math.max(40, Math.abs(x2 - x1) / 2);
							out.set(key, `M${x1},${y1} C${x1 + bend},${y1} ${x2 - bend},${y2} ${x2},${y2}`);
						}
					}
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
