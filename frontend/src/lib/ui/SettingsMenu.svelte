<!-- A button opening a Popover of settings rows, one under the other: a viewer's settings. Its
     role is `group`, not a menu: no row is a menuitem. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Icon, IconButton } from 'panelty';
	import type { IconName } from './icons';
	import Popover from './Popover.svelte';
	import ScrollArea from './ScrollArea.svelte';

	let {
		label,
		icon = 'settings',
		testid,
		menuTestid = `${testid}-menu`,
		class: klass = '',
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
	}
</script>

<span class="sm-anchor" bind:this={anchor}>
	<IconButton variant="ghost" size="sm" class={`${klass} ${open ? 'on' : ''}`.trim()} {label} aria-expanded={open} data-testid={testid} onclick={toggle}>
		<Icon name={icon} />
	</IconButton>
</span>

<Popover {anchor} {open} onDismiss={() => (open = false)} catcher role="group" aria-label={label} class="sm-menu" data-testid={menuTestid}>
	<!-- Popover owns the surface's style, so the width rides a body inside it. -->
	<div class="sm-body" style={`width: ${width}px`}>
		<ScrollArea>
			{@render children()}
		</ScrollArea>
	</div>
</Popover>

<style>
	.sm-anchor {
		display: inline-flex;
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
