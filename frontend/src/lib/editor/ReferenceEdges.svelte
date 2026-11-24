<!-- The dashed lines between the selected node and the nodes whose expressions read an output of
     it, or that it reads, in either direction: one straight line, centre to centre, per pair.
     Drawn from the graph store; not cables, and not interactive. -->
<script lang="ts">
	import type { Node } from '@xyflow/svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import { isSlotExpanded } from '$lib/viewers/inlineView';
	import { nodeSurfaceSize } from './nodeMetrics';

	let { nodes, selected }: { nodes: Node[]; selected: string | null } = $props();
	const g = graph();

	/** Every `nd('name')` term of an expression, with the slot label behind `.out`, if any. */
	const ND = /nd\(\s*(['"])(.+?)\1\s*\)(?:\s*\.\s*out\s*\.\s*([A-Za-z_][A-Za-z0-9_]*))?/g;

	// The drawn ids as one string: a rebuild of the node objects that draws the same set does not
	// re-scan every expression below.
	const drawnIds = $derived(nodes.map((n) => n.id).join(' '));

	/** The `producer>reader` pairs the selected node is one end of, from the records alone. */
	const pairs = $derived.by(() => {
		if (!selected) return [];
		const ids = drawnIds.split(' ').filter(Boolean);
		const byName = new Map(ids.flatMap((id) => (g.nodeById(id) ? [[g.nodeById(id)!.name, id] as const] : [])));
		// The slot a term names: the label behind `.out`, or a one-output node's only slot.
		const slotOf = (uid: string, label: string | undefined): string | undefined => {
			const info = g.nodeById(uid);
			const keys = Object.keys(info?.output_slots ?? {});
			if (label === undefined) return keys.length === 1 ? keys[0] : undefined;
			return keys.find((k) => (info?.slot_labels?.[k] ?? k) === label);
		};
		const out = new Set<string>();
		for (const id of ids) {
			for (const group of Object.values(g.nodeById(id)?.params ?? {})) {
				for (const d of Object.values(group)) {
					const sources = [d, ...(d.elements ?? [])].filter((s) => s.mode === 'expression' && s.expression);
					for (const s of sources) {
						for (const m of s.expression!.matchAll(ND)) {
							const uid = byName.get(m[2]);
							const slot = uid === undefined ? undefined : slotOf(uid, m[3]);
							if (!uid || !slot || uid === id || (uid !== selected && id !== selected)) continue;
							out.add(`${uid}>${id}`);
						}
					}
				}
			}
		}
		return [...out];
	});

	const paths = $derived.by(() => {
		if (!pairs.length) return [];
		const drawn = new Map(nodes.map((n) => [n.id, n]));
		const centre = (n: Node): [number, number] => {
			const info = g.nodeById(n.id);
			const fallback = nodeSurfaceSize(
				Object.keys(info?.input_slots ?? {}).length,
				Object.keys(info?.output_slots ?? {}).map((k) => (info ? isSlotExpanded(info, k) : false))
			);
			const w = n.measured?.width ?? fallback.width;
			const h = n.measured?.height ?? fallback.height;
			return [n.position.x + w / 2, n.position.y + h / 2];
		};
		const out: [string, string][] = [];
		for (const key of pairs) {
			const [from, to] = key.split('>');
			const a = drawn.get(from);
			const b = drawn.get(to);
			if (!a || !b) continue;
			const [x1, y1] = centre(a);
			const [x2, y2] = centre(b);
			out.push([key, `M${x1},${y1} L${x2},${y2}`]);
		}
		return out;
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
