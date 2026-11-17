<!-- CurvePanel — an easing curve's surface, always the one size: the presets as a strip, and the
     curve over the unit square, where it is edited. A press on the square's ground adds an anchor
     point there; a press on a point picks it, and a drag moves it; a picked point shows its two
     handles, and a drag moves one; Delete, or the ✕ in the corner, takes the picked point out. An
     edit of a preset makes it a custom curve of the preset's shape. A drag previews through
     `onInput`; its release commits through `onChange`. -->
<script lang="ts">
	import { Icon, IconButton } from 'panelty';
	import type { HTMLAttributes } from 'svelte/elements';
	import type { Curve } from '$lib/api/generated';
	import Segmented from './Segmented.svelte';
	import { PRESETS, SHORT, curveName, curvePath, fitted, pointsOf, type Point } from './curve';

	let {
		value,
		onChange,
		onInput,
		disabled = false,
		class: klass = '',
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		value: Curve;
		onChange: (curve: Curve) => unknown;
		/** Each step of a drag, before the release commits it. */
		onInput?: (curve: Curve) => unknown;
		disabled?: boolean;
	} = $props();

	/** The points under a drag in flight, so the curve follows the hand before the commit. */
	let draft = $state<Point[] | null>(null);
	const curve = $derived<Curve>(draft ? { custom: draft } : value);
	const points = $derived(draft ?? pointsOf(value));
	/** The anchor point picked: its handles show, and Delete takes it out. */
	let picked = $state<number | null>(null);
	const removable = $derived(picked !== null && picked > 0 && picked < points.length - 1);

	const W = 100;
	const px = (x: number): number => x * W;
	const py = (y: number): number => W - y * W;
	const SEGMENTS = [...PRESETS, 'custom'].map((id) => ({ id, label: SHORT[id], title: id === 'custom' ? 'A curve of your own points' : `The ${id} preset` }));
	function choose(id: string): void {
		picked = null;
		onChange(id === 'custom' ? { custom: pointsOf(value) } : (id as Curve));
	}

	let square = $state<SVGSVGElement | null>(null);
	function at(e: PointerEvent): [number, number] {
		const box = square!.getBoundingClientRect();
		return [(e.clientX - box.left) / box.width, 1 - (e.clientY - box.top) / box.height];
	}
	const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
	const fixed = (v: number): number => Number(v.toFixed(3));
	/** What a drag holds: a point, or one of its handles (2 the one before it, 4 the one after),
	 * and whether it has changed the curve yet. */
	let held: { index: number; part: 0 | 2 | 4; changed: boolean } | null = null;
	function hold(e: PointerEvent, index: number, part: 0 | 2 | 4, next: Point[], changed: boolean): void {
		e.stopPropagation();
		e.preventDefault();
		held = { index, part, changed };
		picked = index;
		draft = next;
		square?.setPointerCapture(e.pointerId);
		square?.focus();
		if (changed) onInput?.({ custom: next });
	}
	const grab = (e: PointerEvent, index: number, part: 0 | 2 | 4): void => {
		if (!disabled) hold(e, index, part, points.map((p) => [...p] as Point), false);
	};
	/** A press on the square's own ground: a new point there, picked and held. */
	function add(e: PointerEvent): void {
		if (disabled) return;
		const [x, y] = at(e);
		const next = points.map((p) => [...p] as Point);
		const index = next.findIndex((p) => p[0] > x);
		if (index < 1 || x <= next[index - 1][0] + 0.01 || x >= next[index][0] - 0.01) return;
		const reach = fixed(Math.min(x - next[index - 1][0], next[index][0] - x) / 3);
		next.splice(index, 0, [fixed(x), fixed(clamp(y, 0, 1)), -reach, 0, reach, 0]);
		hold(e, index, 0, fitted(next), true);
	}
	function move(e: PointerEvent): void {
		if (!held || !draft) return;
		const [x, y] = at(e);
		const { index, part } = held;
		const p = [...draft[index]] as Point;
		if (part !== 0) {
			p[part] = fixed(x - p[0]);
			p[part + 1] = fixed(clamp(y, -0.5, 1.5) - p[1]);
		} else if (index > 0 && index < draft.length - 1) {
			// A point between two others stays between them; the ends stay where a curve begins and ends.
			p[0] = fixed(clamp(x, draft[index - 1][0] + 0.01, draft[index + 1][0] - 0.01));
			p[1] = fixed(clamp(y, 0, 1));
		} else return;
		held.changed = true;
		draft = fitted(draft.map((q, i) => (i === index ? p : q)));
		onInput?.({ custom: draft });
	}
	/** The drag is over: a release sends what it made, and a cancel drops it. The draft stands
	 * until the value answers, so the curve does not show its old shape between the two. */
	function up(e: PointerEvent, cancelled = false): void {
		if (!held) return;
		square?.releasePointerCapture(e.pointerId);
		const [next, send] = [draft!, held.changed && !cancelled];
		held = null;
		// A press that changed nothing is a pick alone: a preset stays the preset.
		if (send) onChange({ custom: next });
		else draft = null;
	}
	$effect(() => {
		void value;
		if (!held) draft = null;
		// A point picked is one the curve still has.
		if (picked !== null && picked >= pointsOf(value).length) picked = null;
	});
	/** Take the picked point out; the ends stay. */
	function remove(): void {
		if (!removable) return;
		const next = fitted(points.filter((_, i) => i !== picked));
		picked = null;
		onChange({ custom: next });
	}
</script>

<div {...rest} class={`ui-curve-panel ${klass}`.trim()} class:disabled>
	<Segmented value={curveName(curve)} segments={SEGMENTS} onChange={choose} {disabled} aria-label="curve" data-testid="curve-preset" fill />
	<div class="ui-curve-frame">
		<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
		<svg
			class="ui-curve-square"
			bind:this={square}
			viewBox={`0 0 ${W} ${W}`}
			role="application"
			aria-label="the curve: press to add a point, drag a point or its handles, Delete to take the picked point out"
			tabindex="0"
			data-testid="curve-square"
			onpointerdown={add}
			onpointermove={move}
			onpointerup={up}
			onpointercancel={(e) => up(e, true)}
			onkeydown={(e) => {
				if (e.key !== 'Delete' && e.key !== 'Backspace') return;
				e.preventDefault();
				remove();
			}}
		>
			<rect class="ui-curve-unit" x="0" y="0" width={W} height={W} />
			<line class="ui-curve-diagonal" x1="0" y1={py(0)} x2={W} y2={py(1)} />
			<path class="ui-curve-line" d={curvePath(curve, W, W)} vector-effect="non-scaling-stroke" />
			{#each points as p, i (i)}
				{#if picked === i}
					{#each [2, 4] as const as part (part)}
						{#if part === 2 ? i > 0 : i < points.length - 1}
							<line class="ui-curve-arm" x1={px(p[0])} y1={py(p[1])} x2={px(p[0] + p[part])} y2={py(p[1] + p[part + 1])} />
							<!-- svelte-ignore a11y_no_static_element_interactions -->
							<circle class="ui-curve-handle" cx={px(p[0] + p[part])} cy={py(p[1] + p[part + 1])} r="3" data-testid={part === 2 ? 'curve-handle-in' : 'curve-handle-out'} onpointerdown={(e) => grab(e, i, part)} />
						{/if}
					{/each}
				{/if}
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<circle class="ui-curve-point" class:picked={picked === i} cx={px(p[0])} cy={py(p[1])} r="4" data-testid="curve-point" onpointerdown={(e) => grab(e, i, 0)} />
			{/each}
		</svg>
		{#if removable}
			<span class="ui-curve-remove"><IconButton variant="ghost" size="sm" label="Delete point" data-testid="curve-remove" onclick={remove}><Icon name="x" /></IconButton></span>
		{/if}
	</div>
</div>

<style>
	.ui-curve-panel {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		width: 100%;
	}
	.ui-curve-frame {
		position: relative;
	}
	/* In the corner no curve reaches: one begins bottom left and ends top right. */
	.ui-curve-remove {
		position: absolute;
		top: 0;
		left: 0;
	}
	.ui-curve-square {
		display: block;
		width: 100%;
		aspect-ratio: 1;
		overflow: visible;
		border-radius: var(--radius-sm);
		background: var(--surface-1);
		touch-action: none;
	}
	.ui-curve-square:focus-visible {
		outline: var(--focus-width) solid var(--focus-ink);
	}
	.ui-curve-unit {
		fill: var(--surface-2);
		stroke: var(--border);
		stroke-width: 0.5;
	}
	.ui-curve-diagonal {
		stroke: var(--border);
		stroke-width: 0.5;
		stroke-dasharray: 2 2;
	}
	.ui-curve-line {
		fill: none;
		stroke: var(--accent);
		stroke-width: 2;
		stroke-linejoin: round;
		pointer-events: none;
	}
	.ui-curve-arm {
		stroke: var(--text-muted);
		stroke-width: 0.6;
		pointer-events: none;
	}
	.ui-curve-point,
	.ui-curve-handle {
		fill: var(--surface-3);
		stroke: var(--accent);
		stroke-width: 1.5;
		cursor: grab;
	}
	.ui-curve-point.picked {
		fill: var(--accent);
	}
	.ui-curve-handle {
		stroke: var(--text-muted);
	}
	.ui-curve-panel.disabled .ui-curve-line {
		opacity: var(--disabled-opacity);
	}
</style>
