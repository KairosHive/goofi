<!-- NumberInput — commits on blur or Enter, with arrow steps. It is
     `type=text` because `type=number` reports "" for an in-progress "5." and drops the point. -->
<script lang="ts">
	import { untrack } from 'svelte';
	import type { HTMLInputAttributes } from 'svelte/elements';
	import { useLiveValue } from './liveValue.svelte';
	import { claimFieldControlId } from './field';
	import { onStep } from './knob';

	let {
		value,
		onChange,
		min,
		max,
		step = 1,
		digits,
		class: klass = '',
		...rest
	}: Omit<HTMLInputAttributes, 'value' | 'type' | 'inputmode' | 'min' | 'max' | 'step' | 'oninput'> & {
		value: number;
		onChange: (v: number) => unknown;
		min?: number;
		max?: number;
		step?: number;
		/** The decimal places it always shows; without it, as many as the number has. */
		digits?: number;
	} = $props();

	const ownId = $props.id();
	const fieldId = claimFieldControlId(ownId);
	const live = useLiveValue<number>(
		() => value,
		(v) => onChange(v)
	);

	// Re-synced while idle only, so typing "1." is never rewritten under the cursor.
	let text = $state(untrack(() => fmt(value)));
	$effect(() => {
		if (!live.editing) text = fmt(live.value);
	});

	function fmt(n: number): string {
		return !Number.isFinite(n) ? '' : digits === undefined ? String(n) : n.toFixed(digits);
	}
	function clamp(n: number): number {
		if (min !== undefined) n = Math.max(min, n);
		if (max !== undefined) n = Math.min(max, n);
		return n;
	}

	// Each press is a complete gesture, so it commits at once, from the buffer and not the prop.
	function stepBy(e: KeyboardEvent, dir: 1 | -1): void {
		e.preventDefault();
		const held = Number(text.trim());
		const base = text.trim() !== '' && Number.isFinite(held) ? held : live.value;
		const v = clamp(onStep(base + dir * step, step));
		text = fmt(v);
		typed = false;
		live.commit(v);
	}

	// A text nobody typed is no edit: a source that moved under the focus keeps its own value.
	let typed = false;
	function commitText(): void {
		const raw = text.trim();
		const n = Number(raw);
		if (typed && raw !== '' && Number.isFinite(n)) {
			const v = clamp(n);
			text = fmt(v);
			live.commit(v);
		}
		typed = false;
		live.end();
	}

</script>

<input
	{...rest}
	id={fieldId}
	type="text"
	inputmode="decimal"
	class={`ui-number ${klass}`.trim()}
	value={text}
	onfocus={() => live.begin()}
	onblur={commitText}
	onkeydown={(e) => {
		if (e.key === 'Enter') (e.currentTarget as HTMLInputElement).blur();
		else if (e.key === 'ArrowUp') stepBy(e, 1);
		else if (e.key === 'ArrowDown') stepBy(e, -1);
	}}
	oninput={(e) => {
		typed = true;
		text = (e.currentTarget as HTMLInputElement).value;
	}}
/>

<style>
	.ui-number {
		width: var(--number-width, 6rem);
		text-align: right;
		color: var(--text);
		font-variant-numeric: tabular-nums;
	}
</style>
