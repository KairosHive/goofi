<script lang="ts">
	import { onMount } from 'svelte';
	import { createLongPress } from 'panelty';
	import { Popover } from '$lib/ui';
	import { nearestTitle } from './titleTip';

	const LINGER_MS = 5000;

	let tip = $state<{ el: HTMLElement; text: string } | null>(null);

	let armed: { el: HTMLElement; text: string } | null = null;
	let swallowClick = false;
	let lingerTimer: ReturnType<typeof setTimeout> | null = null;

	const press = createLongPress(() => {
		if (!armed) return;
		tip = armed;
		swallowClick = true;
		lingerTimer = setTimeout(hide, LINGER_MS);
	});

	function hide(): void {
		tip = null;
		if (lingerTimer) clearTimeout(lingerTimer);
		lingerTimer = null;
	}

	function onPointerDown(e: PointerEvent): void {
		hide();
		press.cancel();
		armed = null;
		swallowClick = false;
		if (e.pointerType === 'mouse') return;
		const hit = nearestTitle(e.target as HTMLElement | null);
		if (!hit) return;
		armed = { el: hit.el as HTMLElement, text: hit.text };
		press.start(e);
	}

	function onClick(e: MouseEvent): void {
		if (!swallowClick) return;
		swallowClick = false;
		e.preventDefault();
		e.stopPropagation();
	}

	onMount(() => {
		// Capture throughout: a control that stops propagation on pointerdown would otherwise never
		// be askable, and the click must be caught before its target to be swallowed at all.
		const opts = { capture: true } as const;
		window.addEventListener('pointerdown', onPointerDown, opts);
		window.addEventListener('pointermove', press.move, opts);
		window.addEventListener('pointerup', press.cancel, opts);
		window.addEventListener('pointercancel', press.cancel, opts);
		window.addEventListener('click', onClick, opts);
		window.addEventListener('scroll', hide, opts);
		return () => {
			window.removeEventListener('pointerdown', onPointerDown, opts);
			window.removeEventListener('pointermove', press.move, opts);
			window.removeEventListener('pointerup', press.cancel, opts);
			window.removeEventListener('pointercancel', press.cancel, opts);
			window.removeEventListener('click', onClick, opts);
			window.removeEventListener('scroll', hide, opts);
			press.cancel(); // a press in flight must not fire into an unmounted layer
			if (lingerTimer) clearTimeout(lingerTimer);
		};
	});
</script>

<Popover anchor={tip?.el ?? null} open={!!tip} onDismiss={hide} flip role="tooltip" data-testid="title-tip" class="title-tip"
	>{tip?.text}</Popover
>

<style>
	:global(.title-tip) {
		--popover-z: var(--z-toast);
		--popover-min-width: 0;
		--popover-max-width: min(24rem, 90vw);
		--popover-pad: var(--space-3) var(--space-5);
		--popover-radius: var(--radius-sm);
		pointer-events: none;
		white-space: pre-wrap;
	}
</style>
