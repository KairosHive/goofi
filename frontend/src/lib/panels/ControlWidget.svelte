<!-- One control widget, drawn by its kind: what a control panel's cell, a state card's row and an
     attribute's default all put on screen. Values arrive as literals; a pad reads its own array. -->
<script lang="ts">
	import type { Literal } from '$lib/api/generated';
	import type { ControlView } from '$lib/crdt/graphDoc';
	import type { ArrayData } from '$lib/codec/decode';
	import { ColorPicker, Knob, NumberInput, PaintPad, Select, Slider, TextInput, Toggle } from '$lib/ui';

	let {
		control: c,
		value,
		label,
		onChange,
		onInput,
		image = null,
		onStroke,
		disabled = false
	}: {
		control: ControlView;
		value: Literal | null;
		label: string;
		onChange: (v: Literal) => unknown;
		onInput?: (v: Literal) => unknown;
		/** A pad's sheet; null when the value is not one. */
		image?: ArrayData | null;
		/** A pad's strokes; a pad with none to give is read-only. */
		onStroke?: (ops: string) => void;
		disabled?: boolean;
	} = $props();

	function num(v: Literal | null): number {
		return typeof v === 'number' ? v : Array.isArray(v) && typeof v[0] === 'number' ? v[0] : 0;
	}
	/** A list as its numbers, one deep; a lone number as a list of one. */
	function nums(v: Literal | null): number[] {
		if (typeof v === 'number') return [v];
		return Array.isArray(v) ? v.map((x) => (typeof x === 'number' ? x : 0)) : [];
	}
	/** The one truth rule, as `control::truth` reads it: a non-empty text, or any number above zero. */
	function truth(v: Literal | null): boolean {
		return typeof v === 'string' ? v !== '' : typeof v === 'number' ? v > 0 : Array.isArray(v) ? v.some(truth) : v === true;
	}
	const withAt = (i: number, n: number): number[] => nums(value).map((held, k) => (k === i ? n : held));
</script>

{#if c.kind === 'knob'}
	<Knob {label} value={num(value)} min={c.min ?? 0} max={c.max ?? 1} step={c.step ?? 0} {disabled} {onChange} {onInput} />
{:else if c.kind === 'slider'}
	<Slider value={num(value)} min={c.min ?? 0} max={c.max ?? 1} step={c.step} {disabled} {onChange} {onInput} />
{:else if c.kind === 'number'}
	<NumberInput value={num(value)} min={c.min} max={c.max} step={c.step ?? 1} {disabled} aria-label={label} {onChange} />
{:else if c.kind === 'toggle'}
	<Toggle value={truth(value)} {disabled} onChange={(v) => onChange(v ? 1 : 0)} />
{:else if c.kind === 'dropdown'}
	<Select value={String(value ?? '')} options={c.options ?? []} {disabled} aria-label={label} {onChange} />
{:else if c.kind === 'paint'}
	<PaintPad value={image} disabled={disabled || !onStroke} onStroke={(ops) => onStroke?.(ops)} />
{:else if c.kind === 'vector'}
	<div class="vector" role="group" aria-label={label}>
		{#each nums(value) as v, i (i)}
			<NumberInput value={v} min={c.min} max={c.max} step={c.step ?? 0.01} {disabled} aria-label={`${label}[${i}]`} onChange={(n) => onChange(withAt(i, n))} />
		{/each}
	</div>
{:else if c.kind === 'color'}
	<ColorPicker value={nums(value)} {disabled} aria-label={label} {onChange} {onInput} />
{:else}
	<TextInput multiline value={String(value ?? '')} {disabled} aria-label={label} {onChange} {onInput} />
{/if}

<style>
	.vector {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-1);
		width: 100%;
		min-width: 0;
		--number-width: 0;
	}
	.vector > :global(*) {
		flex: 1 1 3rem;
		min-width: 0;
	}
</style>
