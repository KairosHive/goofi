<!-- A transition on the machine canvas: a straight edge, or a loop at the card's corner when it
     re-enters its own state. Its label carries the trigger summary; a tap fires it, a long press or
     a right-click picks it for the pane. -->
<script lang="ts">
	import { BaseEdge, EdgeLabel, getStraightPath, type EdgeProps } from '@xyflow/svelte';
	import { createLongPress } from 'panelty';

	let { id, source, target, sourceX, sourceY, targetX, targetY, markerEnd, data }: EdgeProps = $props();
	const d = $derived(data as { summary: string; picked: boolean; onFire: () => void; onPick: () => void });

	/** A loop's radius in flow units. */
	const R = 26;
	const geometry = $derived.by(() => {
		if (source === target) {
			const [x, y] = [sourceX, sourceY];
			const path = `M ${x} ${y} C ${x + 2 * R} ${y + R * 0.2}, ${x + 2 * R} ${y - 2.6 * R}, ${x} ${y - 2 * R} C ${x - R} ${y - 1.8 * R}, ${x - R} ${y - 0.4 * R}, ${x} ${y}`;
			return { path, x: x + 1.3 * R, y: y - 1.4 * R };
		}
		const [path, x, y] = getStraightPath({ sourceX, sourceY, targetX, targetY });
		return { path, x, y };
	});

	// The touch door onto the pane: a held label picks and the lift's click is then not a fire.
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

<BaseEdge {id} path={geometry.path} {markerEnd} class={d.picked ? 'transition picked' : 'transition'} interactionWidth={24} />
<EdgeLabel x={geometry.x} y={geometry.y} transparent>
	<button
		type="button"
		class="label nodrag nopan"
		class:picked={d.picked}
		data-testid={`transition-${id}`}
		title="Tap to fire; hold or right-click to edit"
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

<style>
	:global(.svelte-flow__edge-path.transition) {
		stroke: var(--border-strong);
		stroke-width: 2;
	}
	:global(.svelte-flow__edge-path.transition.picked) {
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
