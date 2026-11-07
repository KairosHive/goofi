<!-- Popover — an anchored, self-dismissing overlay portalled to <body> and clamped to the
     viewport. It imposes no role of its own; the consumer declares one through `...rest`. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';
	import { portal, clampToViewport, overlayViewport } from 'panelty';

	let {
		anchor,
		open,
		onDismiss,
		flip = false,
		catcher = false,
		class: klass = '',
		children,
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		/** The element the popover hangs beneath; its rect drives the clamp. */
		anchor: HTMLElement | null;
		open: boolean;
		onDismiss: () => void;
		/** Let the surface flip ABOVE a bottom-anchored trigger rather than shift up and cover it. */
		flip?: boolean;
		/** Render a full-screen catcher under the surface so a dismissing click ONLY dismisses. */
		catcher?: boolean;
		children?: Snippet;
	} = $props();

	let menuEl = $state<HTMLDivElement | null>(null);
	let pos = $state<{ left: number; top: number }>({ left: 0, top: 0 });
	// Hidden until the first measurement lands, so nothing flashes at (0,0).
	let placed = $state(false);

	$effect(() => {
		if (!open || !menuEl || !anchor) {
			placed = false;
			return;
		}
		const el = menuEl;
		const a = anchor;
		const place = (): void => {
			const ar = a.getBoundingClientRect();
			const m = el.getBoundingClientRect();
			pos = clampToViewport(ar, { width: m.width, height: m.height }, overlayViewport(), { flip });
			placed = true;
		};
		// Re-measured on every resize, not once per open: the consumer's content can grow while open,
		// and so can the anchor.
		const ro = new ResizeObserver(place);
		ro.observe(el);
		ro.observe(a);
		// …and on every visual-viewport change: a soft keyboard shrinks the space without resizing us.
		const vv = window.visualViewport;
		vv?.addEventListener('resize', place);
		place();
		return () => {
			ro.disconnect();
			vv?.removeEventListener('resize', place);
		};
	});

	function onWindowPointerDown(e: PointerEvent): void {
		const t = e.target as Element | null;
		// The anchor is not "outside": its own onclick toggles, so a dismiss here would reopen.
		if (t && menuEl?.contains(t)) return;
		if (t && anchor?.contains(t)) return;
		// Nor is a surface opened from this one — a picker's popover, a menu — which is portalled
		// beside it rather than inside it.
		if (t?.closest('.ui-popover, .context-menu')) return;
		onDismiss();
	}
	function onWindowKeydown(e: KeyboardEvent): void {
		if (e.key !== 'Escape') return;
		// A key from inside reaches its element first: an editor's open completion takes the
		// Escape; the one it lets through dismisses, from `onInnerKeydown`.
		if (menuEl?.contains(e.target as Node)) return;
		// The topmost surface alone answers: one opened from this one is portalled after it.
		const open = document.querySelectorAll('.ui-popover');
		if (open[open.length - 1] !== menuEl) return;
		// Consumed, and capture-phase for it: no window-bubble listener beneath may also act on it.
		e.stopPropagation();
		onDismiss();
	}
	function onInnerKeydown(e: KeyboardEvent): void {
		if (e.key !== 'Escape' || e.defaultPrevented) return;
		e.stopPropagation();
		onDismiss();
	}
</script>

<svelte:window
	onpointerdown={open ? onWindowPointerDown : undefined}
	onkeydowncapture={open ? onWindowKeydown : undefined}
/>

<!-- One portalled element per block: Svelte tears a block down by walking siblings from its first
     node to its last, so a moved LAST node would take the anchor's neighbours instead. -->
{#if open && catcher}
	<!-- Handler-free by design: the layer only ABSORBS the pointer event, so a dismissing click
	     does not also act on what it landed on. The window listener above dismisses. -->
	<div class="ui-popover-catcher" use:portal></div>
{/if}
{#if open}
	<div
		{...rest}
		bind:this={menuEl}
		class={`ui-popover ${klass}`.trim()}
		style="left:{pos.left}px; top:{pos.top}px; visibility:{placed ? 'visible' : 'hidden'}"
		onkeydown={onInnerKeydown}
		use:portal
	>
		{@render children?.()}
	</div>
{/if}

<style>
	.ui-popover-catcher {
		position: fixed;
		inset: 0;
		z-index: calc(var(--popover-z, var(--z-menu)) - 1);
	}
	.ui-popover {
		position: fixed;
		z-index: var(--popover-z, var(--z-menu));
		min-width: var(--popover-min-width, 12rem);
		max-width: var(--popover-max-width, 90vw);
		padding: var(--popover-pad, var(--space-4));
		background: var(--popover-bg, var(--surface-2));
		border: var(--popover-border, 1px solid var(--border-strong));
		border-radius: var(--popover-radius, var(--radius-md));
		box-shadow: var(--popover-shadow, var(--shadow-2));
		color: var(--text);
		font-size: var(--fs-small);
	}
</style>
