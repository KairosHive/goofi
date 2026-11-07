<!-- CurvePicker — a button that shows an easing curve, as a glyph and by name, and opens its
     panel (CurvePanel) in a popover. A handle drag previews through `onInput`; its release commits
     through `onChange`. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import type { Curve } from '$lib/api/generated';
	import Popover from './Popover.svelte';
	import CurvePanel from './CurvePanel.svelte';
	import { curveName, curvePath } from './curve';
	import { claimFieldControlId } from './field';

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
		onInput?: (curve: Curve) => unknown;
		disabled?: boolean;
	} = $props();

	const ownId = $props.id();
	const fieldId = claimFieldControlId(ownId);
	let open = $state(false);
	let button = $state<HTMLButtonElement | null>(null);
	const W = 24;
	const H = 16;
</script>

<div {...rest} class={`ui-curve ${klass}`.trim()}>
	<button id={fieldId} type="button" class="ui-curve-button" bind:this={button} {disabled} aria-haspopup="dialog" aria-expanded={open}
		title="How the attribute eases over a transition" onclick={() => (open = !open)}>
		<svg class="ui-curve-glyph" viewBox={`-1 -1 ${W + 2} ${H + 2}`} aria-hidden="true">
			<path d={curvePath(value, W, H, 24)} vector-effect="non-scaling-stroke" />
		</svg>
		<span class="ui-curve-name">{curveName(value)}</span>
	</button>
</div>

<Popover anchor={button} {open} onDismiss={() => (open = false)} class="ui-curve-pop" role="dialog" aria-label="curve" style="--popover-min-width: 14rem; --popover-max-width: 16rem">
	<CurvePanel {value} {onChange} {onInput} {disabled} />
</Popover>

<style>
	.ui-curve {
		display: flex;
		flex: 1 1 auto;
		min-width: 0;
	}
	.ui-curve-button {
		display: inline-flex;
		align-items: center;
		gap: var(--space-2);
		flex: 1 1 auto;
		min-width: 0;
		height: var(--chrome-control-h);
		padding: 0 var(--space-3);
		border-radius: var(--radius-sm);
		border: 1px solid var(--border);
		background: var(--surface-2);
		color: var(--text);
		font: inherit;
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		cursor: pointer;
	}
	.ui-curve-button:hover {
		border-color: var(--border-strong);
	}
	.ui-curve-button:disabled {
		cursor: default;
		opacity: var(--disabled-opacity);
	}
	.ui-curve-glyph {
		flex: 0 0 auto;
		width: 1.5em;
		height: 1em;
		fill: none;
		stroke: var(--accent);
		stroke-width: 1.5;
	}
	.ui-curve-name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
</style>
