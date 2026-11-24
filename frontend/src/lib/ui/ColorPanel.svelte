<!-- ColorPanel — the RGBA picker's surface, laid out as the browser's own with the alpha it lacks:
     a saturation/value square, hue and alpha strips, an eyedropper, and the colour as hex and as
     four channels. A drag previews through `onInput`; its release commits through `onChange`.
     ColorPicker hangs it under a swatch; a configurator lays it inside a surface of its own. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import NumberInput from './NumberInput.svelte';
	import TextInput from './TextInput.svelte';
	import { IconButton, Icon } from 'panelty';
	import { css, hexOf, parseHex, rgbaOf, toHsv, toRgb, unit } from './color';

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

	const rgba = $derived(rgbaOf(value));

	/** The page's own eyedropper, where the browser has one. */
	const eyedropper = typeof window !== 'undefined' && 'EyeDropper' in window;
	async function pickFromScreen(): Promise<void> {
		try {
			const dropper = new (window as unknown as { EyeDropper: new () => { open(): Promise<{ sRGBHex: string }> } }).EyeDropper();
			const picked = parseHex((await dropper.open()).sRGBHex);
			if (picked) take([...picked.slice(0, 3), alpha]);
		} catch {
			// The pick was cancelled, which is no colour.
		}
	}
	/** One whole colour, typed rather than dragged: it commits at once. */
	function take(c: number[]): void {
		const [h, s, v] = toHsv(c);
		if (s > 0 && v > 0) hue = h;
		if (v > 0) sat = s;
		val = v;
		alpha = unit(c[3] ?? 1);
		commit();
	}

	// The hue and saturation the picker HOLDS: a grey or black colour has no hue of its own, so
	// the strips keep the one the hand last set rather than snapping to zero.
	let hue = $state(0);
	let sat = $state(0);
	let val = $state(0);
	let alpha = $state(1);
	let dragging = $state(false);

	// The picker follows the value until a drag takes over, so an echo cannot move the thumb.
	$effect(() => {
		const [h, s, v] = toHsv(rgba);
		const a = rgba[3];
		if (dragging) return;
		if (s > 0 && v > 0) hue = h;
		if (v > 0) sat = s;
		val = v;
		alpha = a;
	});

	const held = $derived([...toRgb(hue, sat, val), alpha]);
	const hueCss = $derived(css([...toRgb(hue, 1, 1), 1]));

	function preview(): void {
		onInput?.(held);
	}
	function commit(): void {
		dragging = false;
		onChange(held);
	}

	let square = $state<HTMLDivElement | null>(null);
	function place(e: PointerEvent): void {
		if (!square) return;
		// Over the pixels, not the width: the last pixel of the square reaches the full value.
		const box = square.getBoundingClientRect();
		sat = unit((e.clientX - box.left) / Math.max(box.width - 1, 1));
		val = unit(1 - (e.clientY - box.top) / Math.max(box.height - 1, 1));
	}
	function squareDown(e: PointerEvent): void {
		if (disabled) return;
		dragging = true;
		square?.setPointerCapture(e.pointerId);
		place(e);
		preview();
	}
	function squareMove(e: PointerEvent): void {
		if (!dragging) return;
		place(e);
		preview();
	}
	function squareUp(e: PointerEvent): void {
		if (!dragging) return;
		square?.releasePointerCapture(e.pointerId);
		commit();
	}
</script>

<div {...rest} class={`ui-color-panel ${klass}`.trim()}>
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div
		class="ui-color-square"
		bind:this={square}
		style={`--hue: ${hueCss}`}
		data-testid="color-square"
		onpointerdown={squareDown}
		onpointermove={squareMove}
		onpointerup={squareUp}
		onpointercancel={squareUp}
	>
		<span class="ui-color-dot" style={`left: ${sat * 100}%; top: ${(1 - val) * 100}%; background: ${css([...held.slice(0, 3), 1])}`}></span>
	</div>
	<!-- Each strip is a track the thumb overhangs: the input runs a thumb's width past both ends, so
	     the thumb's centre reaches the track's edges rather than stopping a half-thumb short. -->
	<div class="ui-color-track ui-color-hue">
		<input
			class="ui-color-strip"
			type="range"
			min="0"
			max="360"
			step="1"
			{disabled}
			aria-label="hue"
			data-testid="color-hue"
			value={hue}
			onpointerdown={() => (dragging = true)}
			oninput={(e) => {
				hue = Number(e.currentTarget.value);
				preview();
			}}
			onchange={(e) => {
				hue = Number(e.currentTarget.value);
				commit();
			}}
		/>
	</div>
	<div class="ui-color-track ui-color-alpha" style={`--ink: ${css([...held.slice(0, 3), 1])}`}>
		<input
			class="ui-color-strip"
			type="range"
			min="0"
			max="1"
			step="0.01"
			{disabled}
			aria-label="alpha"
			data-testid="color-alpha"
			value={alpha}
			onpointerdown={() => (dragging = true)}
			oninput={(e) => {
				alpha = Number(e.currentTarget.value);
				preview();
			}}
			onchange={(e) => {
				alpha = Number(e.currentTarget.value);
				commit();
			}}
		/>
	</div>
	<div class="ui-color-text">
		{#if eyedropper}
			<IconButton label="Pick a colour off the screen" variant="ghost" size="sm" onclick={pickFromScreen} data-testid="color-eyedropper">
				<Icon name="pipette" />
			</IconButton>
		{/if}
		<TextInput
			class="ui-color-hex"
			value={hexOf(held)}
			onChange={(text) => {
				const c = parseHex(text);
				if (c) take(c);
			}}
			spellcheck={false}
			aria-label="hex"
			data-testid="color-hex"
		/>
	</div>
	<div class="ui-color-channels">
		{#each ['R', 'G', 'B', 'A'] as name, i (name)}
			<label class="ui-color-channel">
				<span>{name}</span>
				<NumberInput
					value={Math.round(held[i] * (i < 3 ? 255 : 100))}
					onChange={(v) => take(held.map((c, k) => (k === i ? unit(v / (i < 3 ? 255 : 100)) : c)))}
					step={1}
					data-testid={`color-channel-${i}`}
				/>
			</label>
		{/each}
	</div>
</div>

<style>
	.ui-color-panel {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		width: 100%;
	}
	/* A checkerboard under the ink, so the alpha shows on the strip. */
	.ui-color-alpha {
		--check: repeating-conic-gradient(var(--surface-3) 0 25%, var(--surface-1) 0 50%) 0 0 / 10px 10px;
	}
	/* Square-cornered: the purest colour sits in the top-right pixel, which a radius would cut off. */
	.ui-color-square {
		position: relative;
		aspect-ratio: 1;
		background:
			linear-gradient(to top, #000, transparent),
			linear-gradient(to right, #fff, var(--hue));
		cursor: crosshair;
		touch-action: none;
	}
	.ui-color-dot {
		position: absolute;
		width: 12px;
		height: 12px;
		border-radius: 50%;
		border: 2px solid #fff;
		box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6);
		transform: translate(-50%, -50%);
		pointer-events: none;
	}
	.ui-color-track {
		--thumb: 14px;
		position: relative;
		height: 12px;
		border-radius: var(--radius-sm);
		border: 1px solid var(--border);
	}
	.ui-color-strip {
		appearance: none;
		position: absolute;
		top: 0;
		left: calc(var(--thumb) / -2);
		width: calc(100% + var(--thumb));
		height: 100%;
		margin: 0;
		background: none;
		cursor: pointer;
	}
	.ui-color-strip::-webkit-slider-thumb {
		appearance: none;
		width: var(--thumb);
		height: var(--thumb);
		border-radius: 50%;
		border: 2px solid #fff;
		box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6);
		background: transparent;
	}
	.ui-color-strip::-moz-range-thumb {
		width: 10px;
		height: 10px;
		border-radius: 50%;
		border: 2px solid #fff;
		box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6);
		background: transparent;
	}
	.ui-color-text {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}
	.ui-color-text :global(.ui-color-hex) {
		flex: 1 1 auto;
		min-width: 0;
		font-family: var(--font-mono);
	}
	.ui-color-channels {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: var(--space-2);
		--number-width: 100%;
	}
	.ui-color-channel {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		min-width: 0;
		color: var(--text-muted);
		font-size: var(--fs-micro);
		text-align: center;
	}
	.ui-color-hue {
		background: linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00);
	}
	.ui-color-alpha {
		background:
			linear-gradient(to right, transparent, var(--ink)),
			var(--check);
	}
	@media (hover: none) and (pointer: coarse) {
		.ui-color-track {
			height: var(--hit);
		}
	}
</style>
