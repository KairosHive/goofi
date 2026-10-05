<!-- Every playhead as a coloured dot: at rest on its state's dock, in arrival order; in flight
     along the edge from `.prev` to `.state` at `.progress`. Rendered inside a front ViewportPortal,
     so flow units are its pixels; a glide between frames keeps it smooth at the cap. -->
<script lang="ts">
	import type { Node } from '@xyflow/svelte';
	import { untrack } from 'svelte';
	import type { Machine } from '$lib/api/generated';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { DOCK, DOT, cardBox, dotColor } from './layout';

	let { m, nodes }: { m: Machine; nodes: Node[] } = $props();
	const names = $derived(Object.keys(m.playheads));
	watchVariables(() => names.flatMap((p) => [`${p}.state`, `${p}.prev`, `${p}.progress`]));

	const where = $derived(
		names.map((p, i) => {
			const state = String(variableValue(`${p}.state`) ?? m.playheads[p].start);
			const prev = String(variableValue(`${p}.prev`) ?? '');
			const progress = Number(variableValue(`${p}.progress`) ?? 1);
			return { p, color: dotColor(m.playheads[p].color, i), state, prev, progress, flying: prev !== '' && prev !== state && progress < 1 };
		})
	);

	// Who rests where, in arrival order: a playhead joins its state's list as it comes to rest.
	let order = $state<Record<string, string[]>>({});
	$effect(() => {
		const resting = where.filter((w) => !w.flying);
		const held = untrack(() => order);
		const next: Record<string, string[]> = {};
		for (const w of resting) {
			const kept = (held[w.state] ?? []).filter((p) => resting.some((r) => r.p === p && r.state === w.state));
			next[w.state] = kept.includes(w.p) ? kept : [...kept, w.p];
		}
		if (JSON.stringify(next) !== JSON.stringify(held)) order = next;
	});

	function at(w: (typeof where)[number]): { x: number; y: number } | null {
		const to = cardBox(nodes, w.state);
		if (!to) return null;
		if (!w.flying) {
			const i = Math.max(0, (order[w.state] ?? []).indexOf(w.p));
			return { x: to.x + DOCK.x + i * DOCK.step, y: to.y + DOCK.y };
		}
		const from = cardBox(nodes, w.prev);
		const a = from ? { x: from.x + from.w, y: from.y + from.h / 2 } : { x: to.x, y: to.y + to.h / 2 };
		const b = { x: to.x, y: to.y + to.h / 2 };
		return { x: a.x + (b.x - a.x) * w.progress, y: a.y + (b.y - a.y) * w.progress };
	}
</script>

<div class="dots" data-testid="playhead-dots">
	{#each where as w (w.p)}
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
