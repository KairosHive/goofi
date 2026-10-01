<!-- Slider — a dumb range control: `value` in, `onChange` out. The latch is released on pointer-up
     OR pointer-cancel, because a touch pan the UA claims fires cancel and never up. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import { useLiveValue } from './liveValue.svelte';
	import { claimFieldControlId } from './field';
	import { stepOf } from './knob';

	let {
		value,
		onChange,
		onInput,
		min = 0,
		max = 1,
		step,
		disabled = false,
		class: klass = '',
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		value: number;
		onChange: (v: number) => unknown;
		/** Each step of a drag, before the release commits it. */
		onInput?: (v: number) => unknown;
		min?: number;
		max?: number;
		step?: number;
		/** Show the value and take no input — what a driven param's own control wears. */
		disabled?: boolean;
	} = $props();

	const ownId = $props.id();
	const fieldId = claimFieldControlId(ownId);
	const live = useLiveValue<number>(
		() => value,
		(v) => onChange(v),
		(v) => onInput?.(v)
	);

	const stp = $derived(stepOf(min, max, step));

	function fmtBound(v: number): string {
		if (!Number.isFinite(v)) return '';
		// A bound is CONTEXT beside the track, never a readout: past five digits it is printed short,
		// so a 1e6 range cannot squeeze the track it describes down to nothing.
		const a = Math.abs(v);
		if (a >= 1e5 || (a > 0 && a < 1e-3)) return v.toExponential(0).replace('e+', 'e');
		if (Number.isInteger(v)) return String(v);
		return String(Number(v.toFixed(3))); // trim trailing zeros
	}
</script>

<div {...rest} class={`ui-slider ${klass}`.trim()}>
	<span class="ui-slider-bound" aria-hidden="true">{fmtBound(min)}</span>
	<input
		id={fieldId}
		class="ui-slider-range"
		type="range"
		{disabled}
		{min}
		{max}
		step={stp}
		value={live.value}
		onpointerdown={() => live.begin()}
		onpointerup={() => live.end()}
		onpointercancel={() => live.end()}
		oninput={(e) => live.input(Number((e.currentTarget as HTMLInputElement).value))}
		onchange={(e) => live.commit(Number((e.currentTarget as HTMLInputElement).value))}
	/>
	<span class="ui-slider-bound" aria-hidden="true">{fmtBound(max)}</span>
</div>

<style>
	.ui-slider {
		display: flex;
		align-items: center;
		gap: var(--space-4);
		min-width: 0;
		flex: 1 1 auto;
	}
	.ui-slider-range {
		flex: 1 1 auto;
		min-width: 0;
		accent-color: var(--accent);
		background: transparent;
		padding: 0;
		border: none;
		/* A vertical touch gesture scrolls; a horizontal one drags the thumb. */
		touch-action: pan-y;
	}
	.ui-slider-bound {
		flex-shrink: 0;
		min-width: 1rem;
		text-align: center;
		color: var(--text-muted);
		font-size: var(--fs-micro);
		font-variant-numeric: tabular-nums;
	}
</style>
