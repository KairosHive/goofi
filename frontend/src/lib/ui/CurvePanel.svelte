<!-- CurvePanel — an easing curve's surface: the presets as a strip, the curve drawn over the unit
     square, and for a custom curve its two bezier handles, dragged on the square or typed under
     it. A drag previews through `onInput`; its release commits through `onChange`. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import type { Curve } from '$lib/api/generated';
	import NumberInput from './NumberInput.svelte';
	import Segmented from './Segmented.svelte';
	import { PRESETS, curvePath, handlesOf, isBezier, type Handles } from './curve';

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
		/** Each step of a handle's drag, before the release commits it. */
		onInput?: (curve: Curve) => unknown;
		disabled?: boolean;
	} = $props();

	/** The handles under a drag in flight, so the curve follows the hand before the commit. */
	let draft = $state<Handles | null>(null);
	const curve = $derived<Curve>(draft ? { bezier: draft } : value);
	const handles = $derived(draft ?? handlesOf(value));
	const custom = $derived(isBezier(curve));

	// The square shows y from -0.25 to 1.25, so an overshooting handle stays in reach.
	const W = 100;
	const PAD = 25;
	const H = W + 2 * PAD;
	const px = (x: number): number => x * W;
	const py = (y: number): number => H - PAD - y * W;
	const path = $derived(curvePath(curve, W, W).replace(/(-?\d+\.?\d*) (-?\d+\.?\d*)/g, (_, x, y) => `${x} ${Number(y) + PAD}`));

	const SEGMENTS = [...PRESETS.map((id) => ({ id, label: id === 'in_out' ? 'in·out' : id, title: `The ${id} preset` })), { id: 'custom', label: 'custom', title: 'A bezier of your own, from the shape the preset left' }];
	function choose(id: string): void {
		draft = null;
		onChange(id === 'custom' ? { bezier: handlesOf(value) } : (id as Curve));
	}

	let square = $state<SVGSVGElement | null>(null);
	let held: 0 | 1 | null = $state(null);
	function at(e: PointerEvent): [number, number] {
		const box = square!.getBoundingClientRect();
		const x = Math.min(1, Math.max(0, (e.clientX - box.left) / box.width));
		const y = Math.min(1.25, Math.max(-0.25, 1.25 - ((e.clientY - box.top) / box.height) * (H / W)));
		return [Number(x.toFixed(3)), Number(y.toFixed(3))];
	}
	function down(i: 0 | 1, e: PointerEvent): void {
		if (disabled || !custom) return;
		held = i;
		draft = [...handles] as Handles;
		square?.setPointerCapture(e.pointerId);
		e.preventDefault();
	}
	function move(e: PointerEvent): void {
		if (held === null || !draft) return;
		const [x, y] = at(e);
		draft = draft.map((v, k) => (k === held! * 2 ? x : k === held! * 2 + 1 ? y : v)) as Handles;
		onInput?.({ bezier: draft });
	}
	function up(e: PointerEvent): void {
		if (held === null) return;
		square?.releasePointerCapture(e.pointerId);
		const next = draft!;
		held = null;
		draft = null;
		onChange({ bezier: next });
	}
	function type(k: number, v: number): void {
		const next = handles.map((h, i) => (i === k ? (k % 2 === 0 ? Math.min(1, Math.max(0, v)) : v) : h)) as Handles;
		onChange({ bezier: next });
	}
</script>

<div {...rest} class={`ui-curve-panel ${klass}`.trim()} class:disabled>
	<Segmented value={custom ? 'custom' : (curve as string)} segments={SEGMENTS} onChange={choose} {disabled} aria-label="curve" data-testid="curve-preset" fill />
	<svg class="ui-curve-square" class:custom bind:this={square} viewBox={`0 0 ${W} ${H}`} role="img" aria-label="the curve" data-testid="curve-square"
		onpointermove={move} onpointerup={up} onpointercancel={up}>
		<rect class="ui-curve-unit" x="0" y={PAD} width={W} height={W} />
		<line class="ui-curve-diagonal" x1="0" y1={py(0)} x2={W} y2={py(1)} />
		<path class="ui-curve-line" d={path} vector-effect="non-scaling-stroke" />
		{#if custom}
			<line class="ui-curve-arm" x1={px(0)} y1={py(0)} x2={px(handles[0])} y2={py(handles[1])} />
			<line class="ui-curve-arm" x1={px(1)} y1={py(1)} x2={px(handles[2])} y2={py(handles[3])} />
			{#each [0, 1] as i (i)}
				<circle class="ui-curve-handle" class:held={held === i} cx={px(handles[i * 2])} cy={py(handles[i * 2 + 1])} r="5"
					role="slider" aria-label={`handle ${i + 1}`} aria-valuenow={handles[i * 2]} tabindex={disabled ? -1 : 0} data-testid={`curve-handle-${i}`}
					onpointerdown={(e) => down(i as 0 | 1, e)} />
			{/each}
		{/if}
	</svg>
	{#if custom}
		<div class="ui-curve-handles">
			{#each ['x1', 'y1', 'x2', 'y2'] as name, k (name)}
				<label class="ui-curve-number">
					<span>{name}</span>
					<NumberInput value={handles[k]} min={k % 2 === 0 ? 0 : undefined} max={k % 2 === 0 ? 1 : undefined} step={0.01} {disabled} onChange={(v) => type(k, v)} data-testid={`curve-${name}`} />
				</label>
			{/each}
		</div>
	{/if}
</div>

<style>
	.ui-curve-panel {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		width: 100%;
	}
	.ui-curve-square {
		display: block;
		width: 100%;
		aspect-ratio: 100 / 150;
		border-radius: var(--radius-sm);
		background: var(--surface-1);
		touch-action: none;
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
	}
	.ui-curve-arm {
		stroke: var(--text-muted);
		stroke-width: 0.6;
	}
	.ui-curve-handle {
		fill: var(--surface-3);
		stroke: var(--accent);
		stroke-width: 1.5;
		cursor: grab;
	}
	.ui-curve-handle.held {
		fill: var(--accent);
		cursor: grabbing;
	}
	.ui-curve-handle:focus-visible {
		outline: var(--focus-width) solid var(--focus-ink);
	}
	.ui-curve-handles {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: var(--space-2);
		--number-width: 100%;
	}
	.ui-curve-number {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		min-width: 0;
		color: var(--text-muted);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		text-align: center;
	}
	.ui-curve-panel.disabled .ui-curve-line {
		opacity: var(--disabled-opacity);
	}
</style>
