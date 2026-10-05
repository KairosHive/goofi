<!-- Every playhead as a coloured dot: at rest on its state's top edge, in arrival order; in flight
     along the transition's course from `.prev` to `.state` at `.progress`. Rendered inside a front ViewportPortal,
     so flow units are its pixels. Each position is the latest runtime sample. -->
<script lang="ts">
	import type { Node } from '@xyflow/svelte';
	import type { Machine } from '$lib/api/generated';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { flightCourse, type RouteGeometry } from './geometry';
	import { DOCK, DOT, cardBox, dotColor } from './layout';

	let { m, nodes, geometry }: { m: Machine; nodes: Node[]; geometry: Map<string, RouteGeometry> } = $props();
	const names = $derived(Object.keys(m.playheads));
	watchVariables(() => names.flatMap((p) => [`${p}.state`, `${p}.prev`, `${p}.progress`, `${p}.transition`, `${p}.arrived`]));

	const where = $derived(
		names.map((p, i) => {
			const state = String(variableValue(`${p}.state`) ?? m.playheads[p].start);
			const prev = String(variableValue(`${p}.prev`) ?? '');
			const progress = Number(variableValue(`${p}.progress`) ?? 1);
			const transition = String(variableValue(`${p}.transition`) ?? '');
			const arrived = BigInt(String(variableValue(`${p}.arrived`) ?? '0'));
			return { p, color: dotColor(m.playheads[p].color, i), state, prev, progress, transition, arrived, flying: transition !== '' && progress < 1 };
		})
	);

	const residents = $derived(where.filter((w) => !w.flying).toSorted((a, b) => a.arrived < b.arrived ? -1 : a.arrived > b.arrived ? 1 : 0));

	function at(w: (typeof where)[number]): { x: number; y: number } | null {
		const to = cardBox(nodes, w.state);
		if (!to) return null;
		if (!w.flying) {
			const docked = residents.filter((r) => r.state === w.state);
			const i = Math.max(0, docked.findIndex((r) => r.p === w.p));
			const step = Math.min(DOCK.step, Math.max(0, to.w - 2 * DOCK.x) / Math.max(1, docked.length - 1));
			return { x: to.x + DOCK.x + i * step, y: to.y };
		}
		const route = flightCourse(geometry, w.transition, w.prev, w.state, (id) => cardBox(nodes, id));
		return route?.at(w.progress) ?? { x: to.x + DOCK.x, y: to.y };
	}
</script>

<div class="dots" data-testid="playhead-dots">
	{#each where as w (w.p)}
		{@const p = at(w)}
		{#if p}
			<div
				class="dot"
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
</style>
