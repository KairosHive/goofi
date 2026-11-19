<!-- RangeBar — a scroll bar as long as an axis from 0 to `span`, whose thumb is the part of the
     axis in view: a view of the whole axis fills the bar. A drag of the thumb pans, and a drag of
     the handle at either end of it moves that end alone, which zooms; a double click asks for the
     view to be set again. A view that reaches past an end of the axis folds there: the thumb
     stays at the bar's end, and that end of it takes a colour, in proportion to how far the view
     is past the axis for about the axis's own length, and then less and less, so a view has no
     widest and the fold never leaves the bar. A `bounded` bar has no fold: its view stays on the
     axis. -->
<script lang="ts">
	import { beginDrag } from 'panelty';
	import { NARROWEST_VIEW, barPlace, barPoint } from './model';

	let {
		view,
		span,
		onChange,
		onCommit,
		onReset,
		vertical = false,
		bounded = false,
		least = NARROWEST_VIEW,
		step = 0,
		testid = 'range'
	}: {
		/** What is in view, from and to. */
		view: [number, number];
		/** Where the axis ends; it begins at 0. */
		span: number;
		/** The view as a drag has it, at each move. */
		onChange: (view: [number, number]) => void;
		/** The view a drag ends on, where its owner sends that as one edit. */
		onCommit?: (view: [number, number]) => void;
		/** A double click on the bar. */
		onReset?: () => void;
		/** The axis runs down. */
		vertical?: boolean;
		/** The view stays on the axis: a pan stops at an end, as long as it was. */
		bounded?: boolean;
		/** The narrowest a view gets. */
		least?: number;
		/** The ends of a view stand on whole steps of this, where it is more than 0. */
		step?: number;
		/** The bar is `<testid>-bar`, its ends `<testid>-from` and `<testid>-to`. */
		testid?: string;
	} = $props();

	const clamp = (v: number, lo = 0, hi = 1): number => Math.min(hi, Math.max(lo, v));
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
		const stepped = (v: number): number => (step > 0 ? Math.round(v / step) * step : v);
		let now = was;
		drag = beginDrag(e.currentTarget as HTMLElement, e.pointerId, {
			move: (p) => {
				const d = (vertical ? p.clientY - e.clientY : p.clientX - e.clientX) / length;
				if (end === null) {
					const by = stepped(bounded ? clamp(d * span, -was[0], span - was[1]) : d * span);
					now = [was[0] + by, was[1] + by];
				} else {
					const at = stepped(barPoint(held[end] + d, span));
					const on = bounded ? clamp(at, 0, span) : at;
					now = end === 0 ? [Math.min(on, was[1] - least), was[1]] : [was[0], Math.max(on, was[0] + least)];
				}
				onChange(now);
			},
			commit: () => onCommit?.(now),
			cancel: () => onChange(was)
		});
	}
	$effect(() => () => drag?.());
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="bar" class:vertical bind:this={bar} data-testid={`${testid}-bar`} ondblclick={() => onReset?.()}>
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
		<span class="fold before" class:on={folds[0] > 0}></span>
		<span class="fold after" class:on={folds[1] > 0}></span>
		<span class="end" class:folded={folds[0] > 0} data-testid={`${testid}-from`} onpointerdown={(e) => grab(e, 0)}></span>
		<span class="end" class:folded={folds[1] > 0} data-testid={`${testid}-to`} onpointerdown={(e) => grab(e, 1)}></span>
	</div>
</div>

<style>
	/* The bar is a faint channel on whatever it lies on, as long as its axis. */
	.bar {
		--thick: var(--space-6);
		position: relative;
		height: var(--thick);
		border-radius: 999px;
		background: var(--hover-fill);
		touch-action: none;
		user-select: none;
	}
	.bar.vertical {
		width: var(--thick);
		height: auto;
	}
	.thumb {
		position: absolute;
		/* A view too narrow to show keeps both its ends on the bar, with a middle between them
		   as long as an end, to drag the view by. */
		inset: 0 calc(100% - var(--to)) 0 min(var(--from), calc(100% - 3 * var(--thick)));
		display: flex;
		justify-content: space-between;
		min-width: calc(var(--thick) * 3);
		border-radius: 999px;
		background: var(--surface-4);
		cursor: grab;
	}
	/* A fold is an end of the thumb itself, in the accent held back, round at both its ends and
	   never shorter than the handle it is under. */
	.fold {
		position: absolute;
		inset: 0 auto 0 0;
		display: none;
		width: var(--before);
		min-width: var(--thick);
		border-radius: 999px;
		background: var(--accent-muted);
		pointer-events: none;
	}
	.fold.on {
		display: block;
	}
	.fold.after {
		inset: 0 0 0 auto;
		width: var(--after);
	}
	.vertical .fold {
		inset: 0 0 auto 0;
		width: auto;
		min-width: 0;
		height: var(--before);
		min-height: var(--thick);
	}
	.vertical .fold.after {
		inset: auto 0 0 0;
		height: var(--after);
	}
	.vertical .thumb {
		inset: min(var(--from), calc(100% - 3 * var(--thick))) 0 calc(100% - var(--to)) 0;
		flex-direction: column;
		min-width: 0;
		min-height: calc(var(--thick) * 3);
	}
	.end {
		position: relative;
		width: var(--thick);
		height: var(--thick);
		cursor: ew-resize;
	}
	/* The dot is drawn smaller than the press it takes: light on the thumb's grey, and dark where
	   a fold is around it. */
	.end::before {
		content: '';
		position: absolute;
		inset: 20%;
		border-radius: 50%;
		background: var(--text-muted);
	}
	.end.folded::before {
		background: var(--surface-1);
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
