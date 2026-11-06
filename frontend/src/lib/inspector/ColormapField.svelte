<!-- ColormapField — a list section drawn as one colormap: the row is the map as a bar, and it
     opens the configurator in a popover, as a colour's swatch opens its picker. The configurator
     holds the presets to start from, the bar with a handle per stop to drag or a place to press
     for a new one, each stop's colour, place and the blend to the next, and the picker of the
     stop in hand. A drag previews; its release stores the whole list, in order of place. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import { Button, IconButton, Icon, Field, NumberInput, Popover, Segmented, ColorPanel, colorCss } from '$lib/ui';
	import { claimFieldControlId } from '$lib/ui/field';
	import { BLENDS, PRESETS, gradient, inserted, preset, sorted, widestGap, type Blend, type Stop } from './colormap';

	let {
		section,
		doc,
		stops,
		min,
		max,
		disabled = false,
		onWrite,
		onPreview,
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		/** The list's name, which the row wears. */
		section: string;
		doc?: string;
		/** The open slots, as stored. */
		stops: Stop[];
		/** How many stops the list keeps at the least and the most. */
		min: number;
		max: number;
		/** The count or a member has a source of its own; the widget then only shows. */
		disabled?: boolean;
		/** The whole list as it is to be stored, already in order of place. */
		onWrite: (stops: Stop[]) => void;
		/** One slot's member as a drag has it, before the release writes the list. */
		onPreview: (slot: number, base: 'at' | 'colour', value: number | number[]) => void;
	} = $props();

	const ownId = $props.id();
	const fieldId = claimFieldControlId(ownId);

	let open = $state(false);
	let bar = $state<HTMLButtonElement | null>(null);
	let track = $state<HTMLDivElement | null>(null);
	/** The stop in hand: the one the picker edits. */
	let selected = $state(0);
	/** The list as a drag has it; null while nothing is dragged, when the stored list shows. */
	let draft = $state<Stop[] | null>(null);
	const shown = $derived(draft ?? stops);
	const current = $derived(shown[Math.min(selected, shown.length - 1)] ?? null);
	const bg = $derived(gradient(shown));

	/** Where a pointer sits across the track, from 0 to 1. */
	function along(e: PointerEvent | MouseEvent): number {
		if (!track) return 0;
		const box = track.getBoundingClientRect();
		return Math.min(1, Math.max(0, (e.clientX - box.left) / Math.max(box.width, 1)));
	}

	let dragging = $state<{ index: number; pointer: number; moved: boolean } | null>(null);
	function lift(e: PointerEvent, index: number): void {
		e.stopPropagation();
		selected = index;
		if (disabled) return;
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		dragging = { index, pointer: e.pointerId, moved: false };
		draft = stops.map((s) => ({ ...s }));
	}
	function slide(e: PointerEvent): void {
		if (!dragging || !draft) return;
		const at = along(e);
		draft[dragging.index] = { ...draft[dragging.index], at };
		dragging.moved = true;
		onPreview(dragging.index, 'at', at);
	}
	function drop(): void {
		const d = dragging;
		dragging = null;
		if (!d || !draft) return;
		const moved = draft[d.index];
		const next = sorted(draft);
		draft = null;
		if (d.moved) {
			selected = next.indexOf(moved);
			onWrite(next);
		}
	}
	/** A press on the bar where no handle is: a stop opens there, in the colour the map has. */
	function press(e: MouseEvent): void {
		if (disabled || stops.length >= max) return;
		const next = inserted(stops, along(e));
		selected = next.findIndex((s) => !stops.includes(s));
		onWrite(next);
	}
	function add(): void {
		if (stops.length >= max) return;
		const next = inserted(stops, widestGap(stops));
		selected = next.findIndex((s) => !stops.includes(s));
		onWrite(next);
	}
	function remove(index: number): void {
		if (stops.length <= min) return;
		onWrite(stops.filter((_, i) => i !== index));
		selected = Math.min(selected, stops.length - 2);
	}
	function patch(index: number, change: Partial<Stop>): void {
		const next = stops.map((s, i) => (i === index ? { ...s, ...change } : s));
		const moved = next[index];
		const ordered = sorted(next);
		selected = ordered.indexOf(moved);
		onWrite(ordered);
	}
	function seed(name: string): void {
		const next = preset(name);
		if (!next || disabled) return;
		selected = 0;
		onWrite(next.slice(0, max));
	}
	/** An arrow on a handle moves its stop a hundredth. */
	function nudge(e: KeyboardEvent, index: number): void {
		const step = e.key === 'ArrowRight' ? 0.01 : e.key === 'ArrowLeft' ? -0.01 : 0;
		if (!step || disabled) return;
		e.preventDefault();
		patch(index, { at: Math.min(1, Math.max(0, stops[index].at + step)) });
	}
</script>

<div {...rest} class="cm-field">
	<Field label={section} {doc}>
		<div class="cm-value">
			<button
				id={fieldId}
				type="button"
				class="cm-bar"
				bind:this={bar}
				aria-haspopup="dialog"
				aria-expanded={open}
				title={`${stops.length} stops`}
				style={`--map: ${bg}`}
				onclick={() => (open = !open)}
				data-testid="param-colormap"
			>
				<span class="cm-ink"></span>
				{#each stops as stop, i (i)}
					<span class="cm-tick" style={`left: ${stop.at * 100}%`}></span>
				{/each}
			</button>
		</div>
	</Field>
</div>

<Popover anchor={bar} {open} onDismiss={() => (open = false)} class="cm-pop" role="dialog" aria-label={`${section} colormap`} data-testid="colormap-pop">
	<div class="cm-presets" aria-label="presets">
		{#each PRESETS as name (name)}
			<button
				type="button"
				class="cm-preset"
				title={`Start from ${name}`}
				{disabled}
				style={`--map: ${gradient(preset(name) ?? [], 16)}`}
				onclick={() => seed(name)}
				data-testid={`colormap-preset-${name}`}
			></button>
		{/each}
	</div>
	<!-- A press on the track is the pointer's own act; the keyboard adds a stop by the button below. -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<div
		class="cm-track"
		bind:this={track}
		style={`--map: ${bg}`}
		class:disabled
		title={stops.length < max ? 'Press to add a stop here' : undefined}
		onclick={press}
		data-testid="colormap-bar"
	>
		<span class="cm-ink"></span>
		{#each shown as stop, i (i)}
			<button
				type="button"
				class="cm-handle"
				class:selected={i === selected}
				style={`left: ${stop.at * 100}%; --ink: ${colorCss(stop.colour)}`}
				aria-label={`stop ${i + 1} at ${stop.at.toFixed(2)}`}
				title={`${stop.at.toFixed(2)}`}
				onpointerdown={(e) => lift(e, i)}
				onpointermove={slide}
				onpointerup={drop}
				onpointercancel={drop}
				onclick={(e) => e.stopPropagation()}
				onkeydown={(e) => nudge(e, i)}
				data-testid={`colormap-stop-${i}`}
			></button>
		{/each}
	</div>
	<div class="cm-stops">
		{#each stops as stop, i (i)}
			{#if i > 0}
				<Segmented
					class="cm-blend"
					value={stops[i - 1].blend}
					segments={BLENDS.map((id) => ({ id, label: id, title: `From stop ${i} to ${i + 1}: ${id}`, testid: `colormap-blend-${i - 1}-${id}` }))}
					onChange={(id) => patch(i - 1, { blend: id as Blend })}
					{disabled}
					aria-label={`blend from stop ${i} to ${i + 1}`}
					data-testid={`colormap-blend-${i - 1}`}
					fill
				/>
			{/if}
			<div class="cm-stop" class:selected={i === selected} data-testid={`colormap-row-${i}`}>
				<button
					type="button"
					class="cm-swatch"
					style={`--ink: ${colorCss(stop.colour)}`}
					aria-label={`stop ${i + 1}`}
					aria-pressed={i === selected}
					onclick={() => (selected = i)}
					data-testid={`colormap-swatch-${i}`}
				></button>
				<NumberInput
					class="cm-at"
					value={stop.at}
					min={0}
					max={1}
					step={0.01}
					{disabled}
					aria-label={`stop ${i + 1} place`}
					onChange={(v) => patch(i, { at: Math.min(1, Math.max(0, v)) })}
					data-testid={`colormap-at-${i}`}
				/>
				<IconButton
					label={`Remove stop ${i + 1}`}
					variant="ghost"
					size="sm"
					disabled={disabled || stops.length <= min}
					onclick={() => remove(i)}
					data-testid={`colormap-remove-${i}`}
				>
					<Icon name="x" />
				</IconButton>
			</div>
		{/each}
	</div>
	<Button variant="ghost" size="sm" disabled={disabled || stops.length >= max} onclick={add} data-testid="colormap-add">
		<Icon name="plus" /> Add stop
	</Button>
	{#if current}
		<ColorPanel
			value={current.colour}
			{disabled}
			onInput={(c) => onPreview(selected, 'colour', c)}
			onChange={(c) => patch(selected, { colour: c })}
			data-testid="colormap-picker"
		/>
	{/if}
</Popover>

<style>
	.cm-field {
		display: contents;
	}
	.cm-value {
		display: contents;
	}
	/* The checkerboard every map sits on, so a thinned stop shows. */
	.cm-bar,
	:global(.cm-track),
	:global(.cm-preset),
	:global(.cm-swatch) {
		--check: repeating-conic-gradient(var(--surface-3) 0 25%, var(--surface-1) 0 50%) 0 0 / 10px 10px;
	}
	/* The bar is the row's whole value, as a colour's swatch is. */
	.cm-bar {
		position: relative;
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
	.cm-ink,
	:global(.cm-track .cm-ink) {
		position: absolute;
		inset: 0;
		background: var(--map);
		pointer-events: none;
	}
	.cm-tick {
		position: absolute;
		top: 0;
		bottom: 0;
		width: 1px;
		transform: translateX(-50%);
		background: rgba(255, 255, 255, 0.7);
		box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.4);
		pointer-events: none;
	}
	:global(.cm-pop) {
		--popover-min-width: 0;
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		width: 18rem;
		max-height: 85vh;
		overflow-y: auto;
	}
	/* The surface scrolls; its parts keep their height rather than shrinking to fit. */
	:global(.cm-pop > *) {
		flex-shrink: 0;
	}
	:global(.cm-presets) {
		display: grid;
		grid-template-columns: repeat(6, minmax(0, 1fr));
		gap: var(--space-2);
	}
	:global(.cm-preset) {
		height: var(--chrome-control-h);
		padding: 0;
		border-radius: var(--radius-sm);
		border: 1px solid var(--border);
		background:
			var(--map),
			var(--check);
		cursor: pointer;
	}
	:global(.cm-preset:disabled) {
		cursor: default;
		opacity: 0.6;
	}
	/* A handle at 0 or 1 overhangs the track by half, as a strip's thumb does. */
	:global(.cm-track) {
		--handle: 14px;
		position: relative;
		height: 28px;
		border: 1px solid var(--border);
		background: var(--check);
		cursor: copy;
		touch-action: none;
	}
	:global(.cm-track.disabled) {
		cursor: default;
	}
	:global(.cm-handle) {
		position: absolute;
		top: -3px;
		bottom: -3px;
		width: var(--handle);
		padding: 0;
		transform: translateX(-50%);
		border: 2px solid #fff;
		border-radius: var(--radius-sm);
		box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6);
		background: var(--ink);
		cursor: ew-resize;
		touch-action: none;
	}
	:global(.cm-handle.selected) {
		border-color: var(--accent);
	}
	:global(.cm-stops) {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		--number-width: 5ch;
	}
	:global(.cm-blend) {
		margin: 0 var(--space-6);
	}
	:global(.cm-stop) {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		padding: var(--space-1);
		border-radius: var(--radius-sm);
	}
	:global(.cm-stop.selected) {
		background: var(--surface-3);
	}
	:global(.cm-swatch) {
		flex: 1 1 auto;
		min-width: var(--chrome-control-h);
		height: var(--chrome-control-h);
		padding: 0;
		border-radius: var(--radius-sm);
		border: 1px solid var(--border);
		background:
			linear-gradient(var(--ink), var(--ink)),
			var(--check);
		cursor: pointer;
	}
	:global(.cm-stop.selected .cm-swatch) {
		border-color: var(--accent);
	}
	/* A finger's floor: the presets in three columns, the track as tall and each handle as wide. */
	@media (hover: none) and (pointer: coarse) {
		:global(.cm-presets) {
			grid-template-columns: repeat(3, minmax(0, 1fr));
		}
		:global(.cm-preset) {
			height: var(--hit);
		}
		:global(.cm-track) {
			height: var(--hit);
			--handle: var(--hit);
		}
	}
</style>
