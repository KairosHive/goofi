<!-- SlotPicker — the slots a node dropped into a cable takes it by: inputs left, outputs right.
     A pick moves the cable at once; Escape and a press outside close it and keep the pick. -->
<script lang="ts">
	import { untrack } from 'svelte';
	import { portal, clampToViewport, overlayViewport } from 'panelty';
	import Catcher from '$lib/ui/Catcher.svelte';
	import { dtypeColor } from './categoryColor';
	import type { SlotChoice } from './rewire';

	let {
		x,
		y,
		inputs,
		outputs,
		input,
		output,
		labels,
		onPick,
		onClose
	}: {
		/** The point the popover hangs at, in client pixels. */
		x: number;
		y: number;
		inputs: SlotChoice[];
		outputs: SlotChoice[];
		/** The slots the cable is on now. */
		input: string;
		output: string;
		labels?: Record<string, string>;
		onPick: (side: 'input' | 'output', slot: string) => void;
		onClose: () => void;
	} = $props();

	let el = $state<HTMLDivElement | null>(null);
	let pos = $state(untrack(() => ({ x, y })));

	$effect(() => {
		if (!el) return;
		const r = el.getBoundingClientRect();
		const p = clampToViewport({ left: x, top: y, right: x, bottom: y, width: 0, height: 0 }, { width: r.width, height: r.height }, overlayViewport());
		pos = { x: p.left, y: p.top };
	});

	function onWindowPointerDown(e: PointerEvent): void {
		if (!el?.contains(e.target as Node)) onClose();
	}
	function onWindowKeydown(e: KeyboardEvent): void {
		if (e.key !== 'Escape') return;
		e.stopPropagation();
		onClose();
	}

	const title = (c: SlotChoice): string =>
		[c.dtype.toLowerCase(), c.bridge === null ? 'no converter' : c.bridge !== 'direct' ? 'through a converter' : '', c.free ? '' : 'in use']
			.filter(Boolean)
			.join(' · ');
</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydowncapture={onWindowKeydown} />

<Catcher />

{#snippet column(side: 'input' | 'output', heading: string, choices: SlotChoice[], picked: string)}
	<div class="column" role="radiogroup" aria-label={heading}>
		<span class="heading">{heading}</span>
		{#each choices as c (c.slot)}
			<button
				type="button"
				class="row"
				class:picked={c.slot === picked}
				role="radio"
				aria-checked={c.slot === picked}
				disabled={c.bridge === null}
				title={title(c)}
				data-testid="slot-pick-{side}"
				data-slot={c.slot}
				data-free={c.free}
				onclick={() => onPick(side, c.slot)}
			>
				<span class="dot" class:taken={!c.free} style="--dtype: {dtypeColor(c.dtype)}"></span>
				<span class="label">{labels?.[c.slot] ?? c.slot}</span>
				{#if c.bridge !== null && c.bridge !== 'direct'}<span class="hint">convert</span>{/if}
			</button>
		{/each}
	</div>
{/snippet}

<div bind:this={el} class="slot-picker" style="left:{pos.x}px; top:{pos.y}px" use:portal role="dialog" aria-label="Slots of the inserted node" data-testid="slot-picker">
	{@render column('input', 'Input', inputs, input)}
	{@render column('output', 'Output', outputs, output)}
</div>

<style>
	.slot-picker {
		position: fixed;
		z-index: var(--z-menu);
		max-width: 90vw;
		max-height: calc(100dvh - 12px);
		overflow-y: auto;
		display: flex;
		gap: var(--space-3);
		padding: var(--space-2);
		background: var(--surface-2);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-md);
		box-shadow: var(--shadow-2);
		color: var(--text);
		font-size: var(--fs-small);
		user-select: none;
	}
	.column {
		display: flex;
		flex-direction: column;
		gap: 1px;
		min-width: 6rem;
	}
	.heading {
		padding: var(--space-2) var(--space-4);
		color: var(--text-muted);
	}
	.row {
		font: inherit;
		display: flex;
		align-items: center;
		gap: var(--space-3);
		padding: var(--space-3) var(--space-4);
		background: transparent;
		border: none;
		border-radius: var(--radius-sm);
		color: inherit;
		text-align: left;
		cursor: pointer;
	}
	.row:hover:not(:disabled) {
		background: var(--surface-3);
	}
	.row.picked {
		background: var(--accent-fill);
	}
	.row:disabled {
		color: var(--text-muted);
		cursor: default;
	}
	/* Hollow while the slot is free, filled once a cable holds it: the pill a list input draws. */
	.dot {
		flex: 0 0 auto;
		width: 8px;
		height: 8px;
		box-sizing: border-box;
		border-radius: 50%;
		border: 2px solid var(--dtype);
	}
	.dot.taken {
		background: var(--dtype);
	}
	.label {
		font-family: var(--font-mono);
	}
	.hint {
		margin-left: auto;
		color: var(--warning);
	}
	@media (hover: none) and (pointer: coarse) {
		.row {
			min-height: var(--hit);
		}
	}
</style>
