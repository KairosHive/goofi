<script lang="ts">
	import { untrack } from 'svelte';
	import { on } from 'svelte/events';
	import { ViewportPortal, useSvelteFlow } from '@xyflow/svelte';
	import SnapGuides from './SnapGuides.svelte';
	import { computeSnapDelta, makeBounds, type Bounds } from './snap';
	import { createTouchPlacement, ghostOrigin, type GhostAnchor } from './touchPlacement';

	/** The pointer side of a placement: where the ghost goes, snapped, and when it is committed.
	 *  The ghost itself is a flow node the editor draws with the same card as every other. */
	interface Props {
		/** Mouse position in client coords, used until the first mousemove. */
		initialClient: { x: number; y: number };
		/** The ghost's footprint, which the snap holds against `targets`. */
		size: { width: number; height: number };
		/** Snap-target bounds in flow coords: the same set the node drag snaps against. */
		targets: Bounds[];
		onMove: (pos: [number, number]) => void;
		onCommit: (pos: [number, number]) => void;
		onCancel: () => void;
	}

	let { initialClient, size, targets, onMove, onCommit, onCancel }: Props = $props();

	const { screenToFlowPosition } = useSvelteFlow();

	let mouseClient = $state<{ x: number; y: number }>(untrack(() => ({ x: initialClient.x, y: initialClient.y })));
	let altKey = $state(false);
	/** Which point of the ghost the input holds; asked of the EVENT, so a hybrid device stays right. */
	let anchor = $state<GhostAnchor>('top-left');

	const flowPos = $derived(screenToFlowPosition({ x: mouseClient.x, y: mouseClient.y }));
	// Spend the anchor once, here, so the snap bounds, the transform and the commit cannot disagree.
	const origin = $derived(ghostOrigin(flowPos, { w: size.width, h: size.height }, anchor));

	const snap = $derived.by(() => {
		const dragged = [makeBounds(origin.x, origin.y, size.width, size.height)];
		return computeSnapDelta(dragged, targets, altKey);
	});

	const snappedX = $derived(origin.x + snap.dx);
	const snappedY = $derived(origin.y + snap.dy);
	$effect(() => onMove([Math.round(snappedX), Math.round(snappedY)]));

	function onMouseMove(e: MouseEvent): void {
		mouseClient = { x: e.clientX, y: e.clientY };
		altKey = e.altKey;
		anchor = 'top-left';
	}

	function onKeyDown(e: KeyboardEvent): void {
		if (e.key === 'Escape') {
			e.preventDefault();
			e.stopPropagation();
			onCancel();
		}
	}

	function inCanvas(target: EventTarget | null): boolean {
		return Boolean((target as HTMLElement | null)?.closest?.('.canvas-wrap'));
	}

	function onWindowClick(e: MouseEvent): void {
		if (e.button !== 0) return;
		if (!inCanvas(e.target)) {
			onCancel();
			return;
		}
		// Block SF's pane-click / node-click from also firing.
		e.stopPropagation();
		e.preventDefault();
		onCommit([Math.round(snappedX), Math.round(snappedY)]);
	}

	function onWindowMouseDown(e: MouseEvent): void {
		if (e.button !== 0) return;
		if (!inCanvas(e.target)) return;
		// Stop SF from starting a pan/select/drag while placement is pending.
		e.stopPropagation();
		e.preventDefault();
	}

	const touch = createTouchPlacement();

	function onPointerDown(e: PointerEvent): void {
		const at = touch.down(e, inCanvas(e.target));
		if (!at) return;
		// `down` answers non-null for a touch and nothing else, so this IS the modality gate.
		anchor = 'centre';
		mouseClient = at;
	}

	function onPointerMove(e: PointerEvent): void {
		const at = touch.move(e);
		if (at) mouseClient = at;
	}

	function onPointerUp(e: PointerEvent): void {
		const at = touch.up(e);
		if (!at) return;
		mouseClient = at;
		onCommit([Math.round(snappedX), Math.round(snappedY)]);
	}

	/** The pan block and the synthetic-click suppression: SvelteFlow pans off `touchstart`, and
	 * cancelling it also suppresses the compat mouse cascade after the commit. */
	function onTouchStart(e: TouchEvent): void {
		if (!touch.active) return;
		e.stopPropagation();
		e.preventDefault();
	}

	$effect(() => {
		const capture = { capture: true };
		const offs = [
			on(window, 'mousemove', onMouseMove),
			on(window, 'keydown', onKeyDown, capture),
			on(window, 'click', onWindowClick, capture),
			on(window, 'mousedown', onWindowMouseDown, capture),
			on(window, 'pointerdown', onPointerDown, capture),
			on(window, 'pointermove', onPointerMove, capture),
			on(window, 'pointerup', onPointerUp, capture),
			on(window, 'pointercancel', touch.cancel, capture),
			// `passive: false` is load-bearing: Chromium makes window-level touchstart passive by
			// default, which would drop the `preventDefault` above and the click suppression with it.
			on(window, 'touchstart', onTouchStart, { capture: true, passive: false })
		];
		return () => offs.forEach((off) => off());
	});
</script>

<ViewportPortal target="front">
	{#if snap.guides.length > 0}
		<SnapGuides guides={snap.guides} testid="placement-snap-guides" />
	{/if}
</ViewportPortal>
