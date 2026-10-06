<!-- Every playhead as a coloured dot: at rest on its state's top edge, in the order of `.arrived`;
     in flight along the transition's course from `.prev` to `.state` at `.progress`. Rendered inside
     a front ViewportPortal, so flow units are its pixels. A glide between two frames of ONE flight
     keeps it smooth at the cap; a new course, or coming to rest, is a new element, so a dot never
     eases across a jump. -->
<script lang="ts">
	import type { Node } from '@xyflow/svelte';
	import type { Machine } from '$lib/api/generated';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { course } from './geometry';
	import { DOCK, DOT, cardBox, dotColor } from './layout';

	let { m, nodes }: { m: Machine; nodes: Node[] } = $props();
	const names = $derived(Object.keys(m.playheads));
	watchVariables(() => names.flatMap((p) => [`${p}.state`, `${p}.prev`, `${p}.progress`, `${p}.arrived`]));

	const where = $derived(
		names.map((p, i) => {
			const state = String(variableValue(`${p}.state`) ?? m.playheads[p].start);
			const prev = String(variableValue(`${p}.prev`) ?? '');
			const progress = Number(variableValue(`${p}.progress`) ?? 1);
			const arrived = Number(variableValue(`${p}.arrived`) ?? 0);
			return { p, color: dotColor(m.playheads[p].color, i), state, prev, progress, arrived, flying: prev !== '' && prev !== state && progress < 1 };
		})
	);

	// Who rests where: the machine stamps each arrival, so a state's residents dock in that order.
	const order = $derived.by(() => {
		const docked: Record<string, string[]> = {};
		for (const w of [...where].filter((w) => !w.flying).sort((a, b) => a.arrived - b.arrived || names.indexOf(a.p) - names.indexOf(b.p))) {
			(docked[w.state] ??= []).push(w.p);
		}
		return docked;
	});

	function at(w: (typeof where)[number]): { x: number; y: number } | null {
		const to = cardBox(nodes, w.state);
		if (!to) return null;
		if (!w.flying) {
			const i = Math.max(0, (order[w.state] ?? []).indexOf(w.p));
			return { x: to.x + DOCK.x + i * DOCK.step, y: to.y };
		}
		const from = cardBox(nodes, w.prev);
		if (!from) return { x: to.x + DOCK.x, y: to.y };
		const { x, y } = course(from, to).at(w.progress);
		return { x, y };
	}
</script>

<div class="dots" data-testid="playhead-dots">
	{#each where as w (`${w.p}:${w.flying ? `${w.prev}>${w.state}` : `rest:${w.state}`}`)}
		{@const p = at(w)}
		{#if p}
			<div
				class="dot"
				class:flying={w.flying}
				style={`left: ${p.x}px; top: ${p.y}px; width: ${DOT}px; height: ${DOT}px; background: ${w.color}`}
				title={w.p}
				data-testid="playhead-dot"
				data-playhead={w.p}
				data-state={w.state}
				data-flying={w.flying}
			></div>
		{/if}
	{/each}
</div>

<style>
	.dots {
		position: absolute;
		left: 0;
		top: 0;
		pointer-events: none;
	}
	.dot {
		position: absolute;
		transform: translate(-50%, -50%);
		border-radius: 50%;
		border: 2px solid var(--surface-1);
		box-shadow: 0 0 0 1px var(--border-strong);
	}
	/* The glide between two frames of a flight; a rest is placed, not slid, so a jump does not cross the canvas. */
	.dot.flying {
		transition:
			left var(--dur-fast) linear,
			top var(--dur-fast) linear;
	}
</style>
