<!-- A button opening a Popover of settings rows, one under the other: a viewer's settings, or a
     state's attributes behind a search field. Its role is `group`, not a menu: no row is a menuitem.
     Up and down move between the search and each `data-menu-row`; left and right stay the row's. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Icon, IconButton } from 'panelty';
	import type { IconName } from './icons';
	import { MODE_ATTRS } from './inputMode';
	import Popover from './Popover.svelte';
	import ScrollArea from './ScrollArea.svelte';

	let {
		label,
		icon = 'settings',
		testid,
		menuTestid = `${testid}-menu`,
		class: klass = '',
		query = $bindable(''),
		placeholder,
		onEnter,
		width = 232,
		children
	}: {
		label: string;
		icon?: IconName;
		/** Lands on the button. */
		testid: string;
		menuTestid?: string;
		/** The button's own class, `on` added while open. */
		class?: string;
		/** The search field's text; the field shows only with a `placeholder`. */
		query?: string;
		placeholder?: string;
		/** Enter in the search field. */
		onEnter?: (query: string) => void;
		/** The menu's width in px. */
		width?: number;
		children: Snippet;
	} = $props();

	let open = $state(false);
	let anchor = $state<HTMLElement | null>(null);

	function toggle(e: MouseEvent): void {
		// Not also a click on what hosts the button: a node header's collapse, a card's selection.
		e.stopPropagation();
		open = !open;
		if (!open) query = '';
	}
	function dismiss(): void {
		open = false;
		query = '';
	}
	function onKeydown(e: KeyboardEvent): void {
		if (e.key === 'Escape') {
			e.preventDefault();
			dismiss();
		} else if (e.key === 'Enter') {
			e.preventDefault();
			onEnter?.(query);
		}
	}
	const focus = (el: HTMLInputElement): void => void requestAnimationFrame(() => el.focus({ preventScroll: true }));

	const FOCUSABLE = 'input, select, textarea, button, [tabindex]:not([tabindex="-1"]), [contenteditable="true"]';
	let body = $state<HTMLDivElement | null>(null);
	/** Up and down step through the stops: the search, then each row's first control. */
	function onArrow(e: KeyboardEvent): void {
		if ((e.key !== 'ArrowDown' && e.key !== 'ArrowUp') || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || !body) return;
		const search = body.querySelector<HTMLElement>('.sm-search');
		const rows = [...body.querySelectorAll<HTMLElement>('[data-menu-row]')].map((row) => ({ row, stop: row.querySelector<HTMLElement>(FOCUSABLE) }));
		const stops = [...(search ? [{ row: search, stop: search }] : []), ...rows].filter((s) => s.stop !== null);
		if (stops.length < 2) return;
		const at = Math.max(0, stops.findIndex((s) => s.row.contains(document.activeElement)));
		const next = stops[(at + (e.key === 'ArrowDown' ? 1 : stops.length - 1)) % stops.length].stop!;
		e.preventDefault();
		next.focus();
		next.scrollIntoView({ block: 'nearest' });
	}
</script>

<span class="sm-anchor" bind:this={anchor}>
	<IconButton variant="ghost" size="sm" class={`${klass} ${open ? 'on' : ''}`.trim()} {label} aria-expanded={open} data-testid={testid} onclick={toggle}>
		<Icon name={icon} />
	</IconButton>
</span>

<Popover {anchor} {open} onDismiss={dismiss} catcher role="group" aria-label={label} class="sm-menu" data-testid={menuTestid}>
	<!-- Popover owns the surface's style, so the width rides a body inside it. -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="sm-body" style={`width: ${width}px`} bind:this={body} onkeydown={onArrow}>
		{#if placeholder !== undefined}
			<!-- Native, not `TextInput`: it filters per keystroke and owns Enter and Escape. -->
			<input {...MODE_ATTRS.search} class="sm-search" bind:value={query} {placeholder} autocomplete="off" aria-label={placeholder}
				data-testid={`${testid}-search`} onkeydown={onKeydown} use:focus />
		{/if}
		<ScrollArea>
			{@render children()}
		</ScrollArea>
	</div>
</Popover>

<style>
	.sm-anchor {
		display: inline-flex;
	}
	.sm-search {
		width: 100%;
		box-sizing: border-box;
		border: none;
		border-bottom: 1px solid var(--border);
		border-radius: 0;
		background: transparent;
		padding: var(--space-4) var(--space-5);
		font-size: var(--fs-body);
	}
	/* The field keeps the focus while the menu is open; the menu's frame is its ring. */
	.sm-search:focus-visible {
		outline: none;
	}
	/* iOS force-zooms an input under 16px on focus; 16px is a threshold, not a type rung. */
	@media (hover: none) and (pointer: coarse) {
		.sm-search {
			font-size: 16px;
		}
	}
	:global(.sm-menu) {
		--popover-min-width: 0;
		/* The menu overhangs a small button, which Popover's default --radius-md rounds too much. */
		--popover-radius: var(--radius-sm);
		max-height: 70dvh;
		display: flex;
		flex-direction: column;
	}
	.sm-body {
		display: flex;
		flex-direction: column;
		min-height: 0;
		max-width: calc(100dvw - 2 * var(--space-6));
	}
</style>
