<!-- ColorPicker — a swatch that opens the RGBA picker (ColorPanel) in a popover. A drag previews
     through `onInput`; its release commits through `onChange`. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import Popover from './Popover.svelte';
	import ColorPanel from './ColorPanel.svelte';
	import { css, rgbaOf } from './color';
	import { claimFieldControlId } from './field';

	let {
		value,
		onChange,
		onInput,
		disabled = false,
		class: klass = '',
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		/** Red, green, blue and alpha, each from 0 to 1. */
		value: number[];
		onChange: (rgba: number[]) => unknown;
		/** Each step of a drag, before the release commits it. */
		onInput?: (rgba: number[]) => unknown;
		disabled?: boolean;
	} = $props();

	const ownId = $props.id();
	const fieldId = claimFieldControlId(ownId);
	const rgba = $derived(rgbaOf(value));

	let open = $state(false);
	let swatch = $state<HTMLButtonElement | null>(null);
</script>

<div {...rest} class={`ui-color ${klass}`.trim()}>
	<button
		id={fieldId}
		type="button"
		class="ui-color-swatch"
		bind:this={swatch}
		{disabled}
		aria-haspopup="dialog"
		aria-expanded={open}
		title={css(rgba)}
		onclick={() => (open = !open)}
	>
		<span class="ui-color-ink" style={`background: ${css(rgba)}`}></span>
	</button>
</div>

<Popover anchor={swatch} {open} onDismiss={() => (open = false)} class="ui-color-pop" role="dialog" aria-label="colour">
	<ColorPanel {value} {onChange} {onInput} {disabled} />
</Popover>

<style>
	/* The swatch is the row's whole value: a bar of the colour across the width, not a chip. */
	.ui-color {
		display: flex;
		flex: 1 1 auto;
		min-width: 0;
	}
	/* A checkerboard under the ink, so the alpha shows on the swatch. */
	.ui-color-swatch {
		--check: repeating-conic-gradient(var(--surface-3) 0 25%, var(--surface-1) 0 50%) 0 0 / 10px 10px;
		flex: 1 1 auto;
		min-width: var(--chrome-control-h);
		height: var(--chrome-control-h);
		padding: 0;
		border-radius: var(--radius-sm);
		border: 1px solid var(--border);
		background: var(--check);
		overflow: hidden;
		cursor: pointer;
	}
	.ui-color-swatch:disabled {
		cursor: default;
		opacity: 0.6;
	}
	.ui-color-ink {
		display: block;
		width: 100%;
		height: 100%;
	}
	:global(.ui-color-pop) {
		--popover-min-width: 0;
		width: 14rem;
	}
</style>
