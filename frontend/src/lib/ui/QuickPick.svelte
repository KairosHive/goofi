<!-- QuickPick — a short list to pick one option from, hung at a point and portalled to <body>: the
     output a dropped node is read by, the attribute a state offers. Arrow keys walk it, Enter takes
     the lit row, Escape and a press outside let it go without a pick. -->
<script lang="ts" module>
	export interface PickOption {
		id: string;
		label: string;
		/** A muted note after the label: the option's kind, where it comes from. */
		hint?: string;
	}
</script>

<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import { untrack } from 'svelte';
	import { portal, clampToViewport, overlayViewport } from 'panelty';
	import { reveal, stepWrapped } from './listNav';

	let {
		x,
		y,
		options,
		onPick,
		onClose,
		label,
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		/** The point the list hangs at, in client pixels. */
		x: number;
		y: number;
		options: PickOption[];
		onPick: (id: string) => void;
		onClose: () => void;
		/** What is being picked, for the list's accessible name. */
		label: string;
	} = $props();

	const id = $props.id();
	let listEl = $state<HTMLDivElement | null>(null);
	let pos = $state(untrack(() => ({ x, y })));
	let active = $state(0);

	$effect(() => {
		if (!listEl) return;
		const r = listEl.getBoundingClientRect();
		// The spawn point is a degenerate anchor rect, as the context menu clamps its own.
		const p = clampToViewport({ left: x, top: y, right: x, bottom: y, width: 0, height: 0 }, { width: r.width, height: r.height }, overlayViewport());
		pos = { x: p.left, y: p.top };
		listEl.focus();
	});

	function onKeydown(e: KeyboardEvent): void {
		const n = options.length;
		if (n === 0) return;
		switch (e.key) {
			case 'ArrowDown':
				active = stepWrapped(active, 1, n);
				break;
			case 'ArrowUp':
				active = stepWrapped(active, -1, n);
				break;
			case 'Home':
				active = 0;
				break;
			case 'End':
				active = n - 1;
				break;
			case 'Enter':
			case ' ':
				onPick(options[active].id);
				break;
			default:
				return;
		}
		e.preventDefault();
		e.stopPropagation();
	}
	function onWindowPointerDown(e: PointerEvent): void {
		if (!listEl?.contains(e.target as Node)) onClose();
	}
	function onWindowKeydown(e: KeyboardEvent): void {
		if (e.key !== 'Escape') return;
		// Consumed at capture: the open surface is topmost, as every anchored surface here holds.
		e.stopPropagation();
		onClose();
	}
</script>

<svelte:window onpointerdown={onWindowPointerDown} onkeydowncapture={onWindowKeydown} />

<div
	{...rest}
	bind:this={listEl}
	class="ui-quickpick thin-scrollbar"
	style="left:{pos.x}px; top:{pos.y}px"
	use:portal
	role="listbox"
	tabindex="-1"
	aria-label={label}
	aria-activedescendant={`${id}-${active}`}
	onkeydown={onKeydown}
>
	{#each options as o, i (o.id)}
		<button
			type="button"
			class="row"
			class:active={i === active}
			use:reveal={i === active}
			id={`${id}-${i}`}
			role="option"
			aria-selected={i === active}
			tabindex="-1"
			onpointermove={() => (active = i)}
			onclick={() => onPick(o.id)}
		>
			<span class="label">{o.label}</span>
			{#if o.hint}<span class="hint">{o.hint}</span>{/if}
		</button>
	{/each}
</div>

<style>
	.ui-quickpick {
		position: fixed;
		z-index: var(--z-menu);
		min-width: 10rem;
		max-width: 90vw;
		max-height: calc(100dvh - 12px);
		overflow-y: auto;
		overscroll-behavior: contain;
		display: flex;
		flex-direction: column;
		gap: 1px;
		padding: var(--space-2);
		background: var(--surface-2);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-md);
		box-shadow: var(--shadow-2);
		color: var(--text);
		font-size: var(--fs-small);
		user-select: none;
		outline: none;
	}
	/* A list row, not a Button: full-bleed, left-aligned, lit by the keyboard as by the pointer. */
	.row {
		font: inherit;
		display: flex;
		align-items: baseline;
		gap: var(--space-3);
		width: 100%;
		padding: var(--space-3) var(--space-4);
		background: transparent;
		border: none;
		border-radius: var(--radius-sm);
		color: inherit;
		text-align: left;
		cursor: pointer;
	}
	.row.active {
		background: var(--accent-fill);
	}
	.label {
		font-family: var(--font-mono);
	}
	.hint {
		margin-left: auto;
		color: var(--text-muted);
	}
	@media (hover: none) and (pointer: coarse) {
		.row {
			min-height: var(--hit);
		}
	}
</style>
