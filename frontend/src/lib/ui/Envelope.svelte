<!-- Envelope — an ADSR as four knobs under the shape they make: attack, decay and release in
     seconds up to `max`, sustain in 0..1. A turn previews through `onInput`; its release commits. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import Knob from './Knob.svelte';
	import { ADSR, ADSR_SHORT, envelopePoints, pathOf, readout, withStage } from './envelope';

	let {
		value,
		onChange,
		onInput,
		max = 10,
		disabled = false,
		label,
		class: klass = '',
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		/** Attack, decay, sustain and release. */
		value: number[];
		onChange: (adsr: number[]) => unknown;
		/** Each step of a turn, before the release commits it. */
		onInput?: (adsr: number[]) => unknown;
		/** The longest time a knob reaches, in seconds. */
		max?: number;
		disabled?: boolean;
		label?: string;
	} = $props();

	/** The envelope under a turn in flight, so the shape follows the knob before the commit. */
	let draft = $state<number[] | null>(null);
	const adsr = $derived(draft ?? withStage(value, -1, 0));
	const W = 100;
	const H = 40;
	const points = $derived(envelopePoints(adsr, W, H));
	const line = $derived(pathOf(points));
	const area = $derived(`${line} Z`);

	function turn(i: number, v: number): void {
		draft = withStage(adsr, i, v);
		onInput?.(draft);
	}
	function land(i: number, v: number): void {
		const next = withStage(adsr, i, v);
		draft = null;
		onChange(next);
	}
</script>

<div {...rest} class={`ui-envelope ${klass}`.trim()} class:disabled role="group" aria-label={label}>
	<svg class="ui-envelope-shape" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true">
		<path class="ui-envelope-fill" d={area} />
		<path class="ui-envelope-line" d={line} vector-effect="non-scaling-stroke" />
	</svg>
	<div class="ui-envelope-knobs">
		{#each ADSR as stage, i (stage)}
			<div class="ui-envelope-knob" data-stage={stage}>
				<Knob value={adsr[i]} min={0} max={i === 2 ? 1 : max} step={0} {disabled} label={label ? `${label} ${stage}` : stage}
					onInput={(v) => turn(i, v)} onChange={(v) => land(i, v)} />
				<span class="ui-envelope-cap"><b>{ADSR_SHORT[i]}</b> {readout(i, adsr[i])}</span>
			</div>
		{/each}
	</div>
</div>

<style>
	/* The shape takes what height the box gives above the row of knobs, each a tap target. */
	.ui-envelope {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		width: 100%;
		height: 100%;
		min-width: 0;
		min-height: 0;
	}
	.ui-envelope-shape {
		flex: 1 1 1.5rem;
		min-height: 1.5rem;
		width: 100%;
		border-radius: var(--radius-sm);
		background: var(--surface-2);
	}
	.ui-envelope-fill {
		fill: var(--accent-fill);
	}
	.ui-envelope-line {
		fill: none;
		stroke: var(--accent);
		stroke-width: 2;
		stroke-linejoin: round;
	}
	.ui-envelope-knobs {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: var(--space-1);
	}
	.ui-envelope-knob {
		display: flex;
		flex-direction: column;
		align-items: center;
		min-width: 0;
		height: calc(var(--hit) + 1.2em);
	}
	.ui-envelope-cap {
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		font-variant-numeric: tabular-nums;
		color: var(--text-muted);
		white-space: nowrap;
	}
	.ui-envelope.disabled .ui-envelope-line {
		opacity: var(--disabled-opacity);
	}
</style>
