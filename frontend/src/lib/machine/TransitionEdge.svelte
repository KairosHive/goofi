<!-- A transition on the machine canvas: a line from box to box, or a loop off the side when it
     re-enters its own state, with chevrons along it for the way it goes — one stroke, so hover and
     selection reach the whole of it. A click on the line selects it; its label carries the trigger
     summary, and a tap on the label fires it. -->
<script lang="ts">
	import { BaseEdge, EdgeLabel, useStore, type EdgeProps, type InternalNode } from '@xyflow/svelte';
	import { createLongPress } from 'panelty';
	import { course, marked, type Box } from './geometry';
	import { CARD_W, FALLBACK_H } from './layout';

	let { id, source, target, selected, data }: EdgeProps = $props();
	const d = $derived(data as { summary: string; onFire: () => void; onPick: () => void });

	const store = useStore();
	const boxOf = (n: InternalNode | undefined): Box | null =>
		n ? { x: n.internals.positionAbsolute.x, y: n.internals.positionAbsolute.y, w: n.measured.width ?? CARD_W, h: n.measured.height ?? FALLBACK_H } : null;
	const geometry = $derived.by(() => {
		// Read through `nodes`, as the hook does: a measurement lands there before the lookup moves.
		void store.nodes;
		const a = boxOf(store.nodeLookup.get(source));
		const b = boxOf(store.nodeLookup.get(target));
		if (!a || !b) return null;
		const c = course(a, source === target ? a : b);
		return { d: marked(c), label: c.at(0.5) };
	});

	// The touch door onto the inspector: a held label picks, and the lift's click is then not a fire.
	let held = false;
	const press = createLongPress(() => {
		held = true;
		d.onPick();
	});
	function tap(): void {
		if (held) {
			held = false;
			return;
		}
		d.onFire();
	}
</script>

{#if geometry}
	<BaseEdge {id} path={geometry.d} class="transition" interactionWidth={24} />
	<EdgeLabel x={geometry.label.x} y={geometry.label.y} transparent>
		<button
			type="button"
			class="label nodrag nopan"
			class:picked={selected}
			data-testid={`transition-${id}`}
			title="Tap to fire; hold or right-click to inspect"
			onclick={tap}
			oncontextmenu={(e) => {
				e.preventDefault();
				d.onPick();
			}}
			onpointerdown={(e) => {
				if (e.pointerType !== 'mouse') press.start(e);
			}}
			onpointermove={press.move}
			onpointerup={press.cancel}
			onpointercancel={press.cancel}>{d.summary}</button
		>
	</EdgeLabel>
{/if}

<style>
	/* The stroke alone: its ink on hover and selection is the editor's, from app.css. */
	:global(.svelte-flow__edge-path.transition) {
		stroke-width: 2;
		stroke-linecap: round;
		stroke-linejoin: round;
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
