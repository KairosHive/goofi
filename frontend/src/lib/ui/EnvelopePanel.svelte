<!-- EnvelopePanel — an ADSR's surface: the shape it makes over four knobs, each with its number
     typed under it: attack, decay and release in seconds up to `max`, sustain in 0..1. A turn
     previews through `onInput`; its release commits. With `hold`, the release's cap is a switch
     to ∞: a release of -1, which holds the sustain. EnvelopePicker hangs it under a bar; a control
     panel lays it inside a cell of its own. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import Knob from './Knob.svelte';
	import NumberInput from './NumberInput.svelte';
	import { ADSR, ADSR_SHORT, HOLD, envelopePoints, pathOf, readout, withStage } from './envelope';

	let {
		value,
		onChange,
		onInput,
		max = 10,
		hold = false,
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
		/** Whether the release may hold the sustain for ever. */
		hold?: boolean;
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
	const held = $derived(adsr[3] < 0);
	/** A hold let go lands the release where a fresh envelope has it. */
	const toggleHold = (): void => land(3, held ? 0.3 : HOLD);
	/** A typed number lands on the stage's own bounds: a time in 0..max, the sustain in 0..1. */
	const typed = (i: number, v: number): void => land(i, Math.min(i === 2 ? 1 : max, Math.max(0, v)));
</script>

<div {...rest} class={`ui-envelope ${klass}`.trim()} class:disabled role="group" aria-label={label}>
	<svg class="ui-envelope-shape" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true">
		<path class="ui-envelope-fill" d={area} />
		<path class="ui-envelope-line" d={line} vector-effect="non-scaling-stroke" />
	</svg>
	<div class="ui-envelope-knobs">
		{#each ADSR as stage, i (stage)}
			<div class="ui-envelope-knob" data-stage={stage}>
				<span class="ui-envelope-cap"><b>{ADSR_SHORT[i]}</b> {stage}</span>
				<Knob value={adsr[i]} min={0} max={i === 2 ? 1 : max} step={0} disabled={disabled || (i === 3 && held)} label={label ? `${label} ${stage}` : stage}
					onInput={(v) => turn(i, v)} onChange={(v) => land(i, v)} />
				{#if hold && i === 3 && held}
					<button type="button" class="ui-envelope-hold" aria-pressed="true" {disabled} title="Let the release go" onclick={toggleHold}>∞</button>
				{:else}
					<NumberInput value={i === 2 ? Math.round(adsr[i] * 100) / 100 : adsr[i]} min={0} max={i === 2 ? 1 : max} step={i === 2 ? 0.01 : 0.01} {disabled}
						aria-label={`${stage} value`} title={readout(i, adsr[i])} data-testid={`envelope-${stage}`} onChange={(v) => typed(i, v)} />
				{/if}
				{#if hold && i === 3 && !held}
					<button type="button" class="ui-envelope-hold" aria-pressed="false" {disabled} title="Hold the sustain: no release" onclick={toggleHold}>hold</button>
				{/if}
			</div>
		{/each}
	</div>
</div>

<style>
	/* The shape takes what height the box gives above the row of knobs, each a tap target. */
	.ui-envelope {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		width: 100%;
		height: 100%;
		min-width: 0;
		min-height: 0;
	}
	.ui-envelope-shape {
		flex: 1 1 2rem;
		min-height: 2rem;
		width: 100%;
		border-radius: var(--radius-sm);
		background: var(--surface-1);
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
		gap: var(--space-2);
		--number-width: 100%;
	}
	.ui-envelope-knob {
		display: flex;
		flex-direction: column;
		align-items: stretch;
		gap: var(--space-1);
		min-width: 0;
	}
	.ui-envelope-knob > :global(.ui-knob) {
		height: calc(var(--hit) + var(--space-4));
	}
	.ui-envelope-cap {
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		color: var(--text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		text-align: center;
	}
	.ui-envelope-hold {
		height: var(--chrome-control-h);
		padding: 0 var(--space-2);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--surface-2);
		color: var(--text-muted);
		font: inherit;
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		cursor: pointer;
	}
	.ui-envelope-hold[aria-pressed='true'] {
		color: var(--accent);
		border-color: var(--accent);
	}
	.ui-envelope-hold:focus-visible {
		outline: var(--focus-width) solid var(--focus-ink);
		outline-offset: 2px;
	}
	.ui-envelope.disabled .ui-envelope-line {
		opacity: var(--disabled-opacity);
	}
</style>
