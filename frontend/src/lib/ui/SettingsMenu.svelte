<!-- A button opening a Popover of settings rows, one under the other: a viewer's settings, or a
     state's attributes behind a search field. Its role is `group`, not a menu: no row is a menuitem.
     Up and down move between the search and each `data-menu-row`; left and right stay the row's.
     With an `anchor` it has no button: it hangs from that element, and its owner opens it. -->
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
		open = $bindable(false),
		anchor,
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
		/** Whether the menu shows; an owner closes it when a row in it ends the menu's business. */
		open?: boolean;
		/** The element the menu hangs from where no button of its own opens it: what was
		 * right-clicked, say. */
		anchor?: HTMLElement | null;
		children: Snippet;
	} = $props();

	let button = $state<HTMLElement | null>(null);

	function toggle(e: MouseEvent): void {
		// Not also a click on what hosts the button: a node header's collapse, a card's selection.
		e.stopPropagation();
		open = !open;
		if (!open) query = '';
	}
	function dismiss(): void {
		// The focus that was in the menu goes back to its button, not to nowhere.
		if (body?.contains(document.activeElement)) button?.querySelector('button')?.focus({ preventScroll: true });
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
	const focus = (el: HTMLElement): void => void requestAnimationFrame(() => el.focus({ preventScroll: true }));
	/** A menu with no search field takes the focus itself when it opens, so Tab and the arrows go
	 * into its rows; no control is focused for it, which would raise a keyboard under a finger. */
	const land = (el: HTMLElement): void => {
		if (placeholder === undefined) focus(el);
	};

	const FOCUSABLE = 'input, select, textarea, button, [tabindex]:not([tabindex="-1"]), [contenteditable="true"]';
	let body = $state<HTMLDivElement | null>(null);

	/** Up and down step through the stops: the search, then each row's first control. */
	function onArrow(e: KeyboardEvent): void {
		if ((e.key !== 'ArrowDown' && e.key !== 'ArrowUp') || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || !body) return;
		const search = body.querySelector<HTMLElement>('.sm-search');
		const rows = [...body.querySelectorAll<HTMLElement>('[data-menu-row]')].map((row) => ({ row, stop: row.querySelector<HTMLElement>(FOCUSABLE) }));
		const stops = [...(search ? [{ row: search, stop: search }] : []), ...rows].filter((s) => s.stop !== null);
		// From the menu itself, which holds the focus when it opens, down is the first stop and up
		// the last.
		const at = stops.findIndex((s) => s.row.contains(document.activeElement));
		if (stops.length === 0 || (at >= 0 && stops.length < 2)) return;
		const to = at < 0 ? (e.key === 'ArrowDown' ? 0 : stops.length - 1) : (at + (e.key === 'ArrowDown' ? 1 : stops.length - 1)) % stops.length;
		const next = stops[to].stop!;
		e.preventDefault();
		next.focus();
		next.scrollIntoView({ block: 'nearest' });
	}
</script>

{#if anchor === undefined}
	<span class="sm-anchor" bind:this={button}>
		<IconButton variant="ghost" size="sm" class={`${klass} ${open ? 'on' : ''}`.trim()} {label} aria-expanded={open} data-testid={testid} onclick={toggle}>
			<Icon name={icon} />
		</IconButton>
	</span>
{/if}

<Popover anchor={anchor ?? button} {open} onDismiss={dismiss} role="group" aria-label={label} class="sm-menu" data-testid={menuTestid}>
	<!-- Popover owns the surface's style, so the width rides a body inside it. -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="sm-body" style={`width: ${width}px`} tabindex="-1" bind:this={body} onkeydown={onArrow} use:land>
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
	.sm-body:focus {
		outline: none;
	}
	.sm-body {
		display: flex;
		flex-direction: column;
		min-height: 0;
		max-width: calc(100dvw - 2 * var(--space-6));
	}
</style>
