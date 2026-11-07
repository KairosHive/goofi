<!-- EnvelopePicker — a bar that draws an ADSR's shape and opens its panel (EnvelopePanel) in a
     popover, as a colour's swatch opens its picker. A turn previews through `onInput`; its release
     commits through `onChange`. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import Popover from './Popover.svelte';
	import EnvelopePanel from './EnvelopePanel.svelte';
	import { ADSR_SHORT, envelopePoints, pathOf, readout, withStage } from './envelope';
	import { claimFieldControlId } from './field';

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
		onInput?: (adsr: number[]) => unknown;
		/** The longest time a knob reaches, in seconds. */
		max?: number;
		/** Whether the release may hold the sustain for ever. */
		hold?: boolean;
		disabled?: boolean;
		label?: string;
	} = $props();

	const ownId = $props.id();
	const fieldId = claimFieldControlId(ownId);
	const adsr = $derived(withStage(value, -1, 0));
	const W = 100;
	const H = 20;
	const line = $derived(pathOf(envelopePoints(adsr, W, H)));
	const title = $derived(ADSR_SHORT.map((s, i) => `${s} ${readout(i, adsr[i])}`).join('  '));

	let open = $state(false);
	let bar = $state<HTMLButtonElement | null>(null);
</script>

<div {...rest} class={`ui-envelope-pick ${klass}`.trim()}>
	<button id={fieldId} type="button" class="ui-envelope-bar" bind:this={bar} {disabled} aria-haspopup="dialog" aria-expanded={open} aria-label={label} {title}
		onclick={() => (open = !open)}>
		<svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden="true">
			<path class="ui-envelope-bar-fill" d={`${line} Z`} />
			<path class="ui-envelope-bar-line" d={line} vector-effect="non-scaling-stroke" />
		</svg>
	</button>
</div>

<Popover anchor={bar} {open} onDismiss={() => (open = false)} class="ui-envelope-pop" role="dialog" aria-label="envelope" style="--popover-min-width: 16rem; --popover-max-width: 20rem">
	<EnvelopePanel {value} {onChange} {onInput} {max} {hold} {disabled} {label} />
</Popover>

<style>
	/* The bar is the row's whole value: the shape across the width, as a colour's swatch is. */
	.ui-envelope-pick {
		display: flex;
		flex: 1 1 auto;
		min-width: 0;
	}
	.ui-envelope-bar {
		flex: 1 1 auto;
		min-width: var(--chrome-control-h);
		height: var(--chrome-control-h);
		padding: 0;
		border-radius: var(--radius-sm);
		border: 1px solid var(--border);
		background: var(--surface-2);
		overflow: hidden;
		cursor: pointer;
	}
	.ui-envelope-bar:hover {
		border-color: var(--border-strong);
	}
	.ui-envelope-bar:disabled {
		cursor: default;
		opacity: var(--disabled-opacity);
	}
	.ui-envelope-bar svg {
		display: block;
		width: 100%;
		height: 100%;
	}
	.ui-envelope-bar-fill {
		fill: var(--accent-fill);
	}
	.ui-envelope-bar-line {
		fill: none;
		stroke: var(--accent);
		stroke-width: 1.5;
		stroke-linejoin: round;
	}
</style>
