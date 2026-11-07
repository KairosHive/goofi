<!-- The score's transition lane: every transition as an arc from the column it leaves to the one
     it enters, a loop over its own column, and a chip near its end that sums it up — its share or
     turn, then its duration — so the chips of arcs into different columns stand apart. A `*`
     transition hangs over the column it enters. An arc lights with the pointer on its line on the
     canvas, and a click selects it there too. -->
<script lang="ts">
	import type { Machine } from '$lib/api/generated';
	import { summary } from './model';
	import { machineHover } from './hover.svelte';
	import { SCORE } from './layout';

	let {
		name,
		m,
		order,
		centres,
		width,
		selected,
		onPick
	}: {
		name: string;
		m: Machine;
		order: string[];
		/** Each column's centre in px, in column order. */
		centres: number[];
		width: number;
		selected: Set<string>;
		onPick: (id: string, e: MouseEvent) => void;
	} = $props();

	const hover = machineHover();
	const LANE_H = SCORE.lane;
	const BASE = LANE_H - 4;
	const at = (state: string): number => centres[order.indexOf(state)] ?? -1;

	/** Each drawn transition: its path, where its chip sits, and whether it is a `*`. */
	const arcs = $derived.by(() => {
		const out: { id: string; d: string; x: number; y: number; any: boolean }[] = [];
		const seen = new Map<string, number>();
		for (const [id, t] of Object.entries(m.transitions)) {
			const to = at(t.to);
			if (to < 0) continue;
			if (t.from === '*') {
				out.push({ id, d: `M ${to} 4 L ${to} ${BASE - 6}`, x: to, y: 10, any: true });
				continue;
			}
			const from = at(t.from);
			if (from < 0) continue;
			// Parallel arcs between one pair climb a little further each, so their chips clear.
			const key = [t.from, t.to].sort().join('|');
			const k = seen.get(key) ?? 0;
			seen.set(key, k + 1);
			if (t.from === t.to) {
				const r = 10 + k * 6;
				out.push({ id, d: `M ${from - r} ${BASE} A ${r} ${r} 0 1 1 ${from + r} ${BASE}`, x: from, y: BASE - r - 4, any: false });
				continue;
			}
			const span = Math.abs(to - from);
			const h = Math.min(BASE - 8, 12 + span * 0.12 + k * 8);
			const mid = (from + to) / 2;
			const dir = to > from ? 1 : -1;
			const tip = `M ${to - dir * 6} ${BASE - 5} L ${to} ${BASE} L ${to - dir * 6} ${BASE + 2}`;
			// The chip sits seven tenths of the way along the quadratic, toward the column it enters.
			const s = 0.7;
			const cx = (1 - s) * (1 - s) * from + 2 * (1 - s) * s * mid + s * s * to;
			const cy = (1 - s) * (1 - s) * BASE + 2 * (1 - s) * s * (BASE - 2 * h) + s * s * BASE;
			out.push({ id, d: `M ${from} ${BASE} Q ${mid} ${BASE - 2 * h} ${to} ${BASE} ${tip}`, x: cx, y: cy, any: false });
		}
		return out;
	});
</script>

<div class="lane" style={`width: ${width}px; height: ${LANE_H}px`} data-testid="transition-lane">
	<svg {width} height={LANE_H} aria-hidden="true">
		{#each arcs as a (a.id)}
			<path class="arc" class:picked={selected.has(a.id)} class:lit={hover.litTransition(name, a.id)} d={a.d} />
		{/each}
	</svg>
	{#each arcs as a (a.id)}
		<button type="button" class="chip" class:picked={selected.has(a.id)} class:lit={hover.litTransition(name, a.id)} class:any={a.any}
			style={`left: ${a.x}px; top: ${a.y}px`} title={`${m.transitions[a.id].from} → ${m.transitions[a.id].to}: its settings`} data-testid={`arc-${a.id}`}
			onpointerenter={() => hover.overTransition(name, a.id)} onpointerleave={() => hover.clear()}
			onclick={(e) => onPick(a.id, e)}>{a.any ? 'any' : summary(m, a.id)}</button>
	{/each}
</div>

<style>
	.lane {
		position: relative;
		flex: 0 0 auto;
	}
	svg {
		display: block;
		overflow: visible;
	}
	.arc {
		fill: none;
		stroke: var(--text-muted);
		stroke-width: 1.5;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.arc.lit {
		stroke: var(--ring-accent);
	}
	.arc.picked {
		stroke: var(--accent);
		stroke-width: 2;
	}
	.chip {
		position: absolute;
		transform: translate(-50%, -50%);
		max-width: 6rem;
		padding: 0 var(--space-2);
		border: 1px solid var(--border);
		border-radius: 999px;
		background: var(--surface-2);
		color: var(--text);
		font: inherit;
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		line-height: 1.4;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		cursor: pointer;
	}
	.chip.any {
		color: var(--text-muted);
	}
	.chip:hover,
	.chip.lit,
	.chip.picked {
		border-color: var(--accent);
	}
	.chip.picked {
		background: var(--accent-fill);
	}
</style>
