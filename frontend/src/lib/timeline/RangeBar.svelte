<!-- RangeBar — a scroll bar as long as an axis from 0 to `span`, whose thumb is the part of the
     axis in view: a view of the whole axis fills the bar. A drag of the thumb pans, and a drag of
     the handle at either end of it moves that end alone, which zooms. A view that reaches past an
     end of the axis folds there: the thumb stays at the bar's end, and that end of it takes a
     colour, as far as the view is past the axis at first and then less and less, so a view has
     no widest and the fold never leaves the bar. -->
<script lang="ts">
	import { beginDrag } from 'panelty';
	import { barPlace, barPoint } from './model';

	let {
		view,
		span,
		onChange,
		vertical = false
	}: {
		/** What is in view, from and to. */
		view: [number, number];
		/** Where the axis ends; it begins at 0. */
		span: number;
		onChange: (view: [number, number]) => void;
		/** The axis runs down. */
		vertical?: boolean;
	} = $props();
	/** The narrowest a view gets. */
	const LEAST = 0.05;

	const clamp = (v: number): number => Math.min(1, Math.max(0, v));
	const [from, to] = $derived([barPlace(view[0], span), barPlace(view[1], span)]);
	/** How much of the thumb each fold takes: its share of the bar, told in the thumb's own length. */
	const shown = $derived(Math.max(clamp(to) - clamp(from), 1e-6));
	const folds = $derived([Math.min(1, Math.max(0, -from) / shown), Math.min(1, Math.max(0, to - 1) / shown)]);

	let bar = $state<HTMLElement | null>(null);
	let drag: (() => void) | undefined;
	/** A drag of the thumb (`end` null), which moves the view as the bar measures the axis, or of
	 * one end, which goes where the pointer is on the bar, folds and all. */
	function grab(e: PointerEvent, end: 0 | 1 | null): void {
		e.stopPropagation();
		const box = bar!.getBoundingClientRect();
		const length = vertical ? box.height : box.width;
		const [was, held] = [view, [from, to]];
		drag = beginDrag(e.currentTarget as HTMLElement, e.pointerId, {
			move: (p) => {
				const d = (vertical ? p.clientY - e.clientY : p.clientX - e.clientX) / length;
				if (end === null) onChange([was[0] + d * span, was[1] + d * span]);
				else if (end === 0) onChange([Math.min(barPoint(held[0] + d, span), was[1] - LEAST), was[1]]);
				else onChange([was[0], Math.max(barPoint(held[1] + d, span), was[0] + LEAST)]);
			},
			commit: () => {},
			cancel: () => onChange(was)
		});
	}
	$effect(() => () => drag?.());
</script>

<div class="bar" class:vertical bind:this={bar} data-testid="range-bar">
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div
		class="thumb"
		style:--from={`${100 * clamp(from)}%`}
		style:--to={`${100 * clamp(to)}%`}
		style:--before={`${100 * folds[0]}%`}
		style:--after={`${100 * folds[1]}%`}
		data-folded={folds[0] > 0 || folds[1] > 0}
		onpointerdown={(e) => grab(e, null)}
	>
		<span class="fold before"></span>
		<span class="fold after"></span>
		<span class="end" data-testid="range-from" onpointerdown={(e) => grab(e, 0)}></span>
		<span class="end" data-testid="range-to" onpointerdown={(e) => grab(e, 1)}></span>
	</div>
</div>

<style>
	.bar {
		--thick: var(--space-6);
		position: relative;
		height: var(--thick);
		touch-action: none;
		user-select: none;
	}
	.bar.vertical {
		width: var(--thick);
		height: auto;
	}
	.thumb {
		position: absolute;
		/* A view too narrow to show keeps both its ends on the bar. */
		inset: 0 calc(100% - var(--to)) 0 min(var(--from), calc(100% - 2 * var(--thick)));
		display: flex;
		justify-content: space-between;
		min-width: calc(var(--thick) * 2);
		border-radius: 999px;
		background: var(--surface-4);
		cursor: grab;
	}
	/* A fold is an end of the thumb itself, in the accent, under the handle there. */
	.fold {
		position: absolute;
		inset: 0 auto 0 0;
		width: var(--before);
		border-radius: 999px 0 0 999px;
		background: var(--accent);
		pointer-events: none;
	}
	.fold.after {
		inset: 0 0 0 auto;
		width: var(--after);
		border-radius: 0 999px 999px 0;
	}
	.vertical .fold {
		inset: 0 0 auto 0;
		width: auto;
		height: var(--before);
		border-radius: 999px 999px 0 0;
	}
	.vertical .fold.after {
		inset: auto 0 0 0;
		height: var(--after);
		border-radius: 0 0 999px 999px;
	}
	.vertical .thumb {
		inset: min(var(--from), calc(100% - 2 * var(--thick))) 0 calc(100% - var(--to)) 0;
		flex-direction: column;
		min-width: 0;
		min-height: calc(var(--thick) * 2);
	}
	.end {
		position: relative;
		width: var(--thick);
		height: var(--thick);
		cursor: ew-resize;
	}
	/* The dot is drawn smaller than the press it takes. */
	.end::before {
		content: '';
		position: absolute;
		inset: 20%;
		border-radius: 50%;
		background: var(--text-muted);
	}
	.vertical .end {
		cursor: ns-resize;
	}
	@media (hover: none) and (pointer: coarse) {
		.bar {
			--thick: var(--space-8);
		}
		.end::after {
			content: '';
			position: absolute;
			inset: calc((100% - var(--hit)) / 2);
		}
	}
</style>
