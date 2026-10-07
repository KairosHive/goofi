<!-- A transition on the machine canvas: a line from box to box, or a loop off the side when it
     re-enters its own state, with chevrons along it for the way it goes — one stroke, so hover and
     selection reach the whole of it. Two between one pair of boxes run side by side. A click on the
     line or on its label selects it; the label, the trigger summary, shows only while the transition
     or one of its boxes is selected. -->
<script lang="ts">
	import { BaseEdge, EdgeLabel, useStore, type EdgeProps, type InternalNode } from '@xyflow/svelte';
	import { course, labelAt, lane, marked, type Box } from './geometry';
	import { CARD_W, FALLBACK_H } from './layout';

	let { id, source, target, selected, data }: EdgeProps = $props();
	const d = $derived(data as { summary: string; k: number; n: number; labelled: boolean; onPick: () => void });

	const store = useStore();
	const boxOf = (n: InternalNode | undefined): Box | null =>
		n ? { x: n.internals.positionAbsolute.x, y: n.internals.positionAbsolute.y, w: n.measured.width ?? CARD_W, h: n.measured.height ?? FALLBACK_H } : null;
	const geometry = $derived.by(() => {
		// Read through `nodes`, as the hook does: a measurement lands there before the lookup moves.
		void store.nodes;
		const a = boxOf(store.nodeLookup.get(source));
		const b = boxOf(store.nodeLookup.get(target));
		if (!a || !b) return null;
		const l = lane(d.k, d.n, source, target);
		const c = course(a, source === target ? a : b, l.offset);
		return { d: marked(c), label: source === target ? c.at(0.5) : labelAt(c, a, b, l.at) };
	});
</script>

{#if geometry}
	<BaseEdge {id} path={geometry.d} class="transition" interactionWidth={24} />
	{#if d.labelled}
		<EdgeLabel x={geometry.label.x} y={geometry.label.y} transparent>
			<button
				type="button"
				class="label nodrag nopan"
				class:picked={selected}
				data-testid={`transition-${id}`}
				title="Select the transition"
				onclick={(e) => {
					// Stopped here: the label sits in the pane, and Flow would read the click as the pane's.
					e.stopPropagation();
					d.onPick();
				}}>{d.summary}</button
			>
		</EdgeLabel>
	{/if}
{/if}

<style>
	/* The stroke, with its hover and selection ink: Flow's own sheet, loaded after app.css, would
	   otherwise keep the selected line grey. */
	:global(.svelte-flow__edge-path.transition) {
		stroke-width: 2;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	:global(.svelte-flow__edge:hover .svelte-flow__edge-path.transition) {
		stroke: var(--ring-accent);
	}
	:global(.svelte-flow__edge.selected .svelte-flow__edge-path.transition) {
		stroke: var(--accent);
	}
	.label {
		min-height: var(--hit);
		padding: var(--space-1) var(--space-3);
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: 999px;
		color: var(--text);
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		white-space: nowrap;
		cursor: pointer;
		pointer-events: all;
	}
	.label:hover,
	.label.picked {
		border-color: var(--accent);
	}
</style>
