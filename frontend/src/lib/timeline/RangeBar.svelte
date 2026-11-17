<!-- RangeBar — a scroll bar whose thumb is the part of an axis in view: a drag of the thumb pans,
     and a drag of the handle at either end of it moves that end alone, which zooms. A handle
     dragged past the end of the bar goes on, so a view has no widest. -->
<script lang="ts">
	import { beginDrag } from 'panelty';

	let {
		view,
		total,
		onChange,
		vertical = false,
		least = 0.05
	}: {
		/** What is in view, from and to. */
		view: [number, number];
		/** What the bar stands for: it holds the view. */
		total: [number, number];
		onChange: (view: [number, number]) => void;
		/** The axis runs down. */
		vertical?: boolean;
		/** The narrowest a view gets. */
		least?: number;
	} = $props();

	let bar = $state<HTMLElement | null>(null);
	const share = (v: number): string => `${(100 * (v - total[0])) / (total[1] - total[0])}%`;
	let drag: (() => void) | undefined;
	/** A drag of the thumb (`end` null) or of one end, at the scale the bar had at the press. */
	function grab(e: PointerEvent, end: 0 | 1 | null): void {
		e.stopPropagation();
		const box = bar!.getBoundingClientRect();
		const per = (total[1] - total[0]) / (vertical ? box.height : box.width);
		const [from, to] = view;
		drag = beginDrag(e.currentTarget as HTMLElement, e.pointerId, {
			move: (p) => {
				const d = (vertical ? p.clientY - e.clientY : p.clientX - e.clientX) * per;
				onChange(end === null ? [from + d, to + d] : end === 0 ? [Math.min(from + d, to - least), to] : [from, Math.max(to + d, from + least)]);
			},
			commit: () => {},
			cancel: () => onChange([from, to])
		});
	}
	$effect(() => () => drag?.());
</script>

<div class="bar" class:vertical bind:this={bar} data-testid="range-bar">
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="thumb" style:--from={share(view[0])} style:--to={share(view[1])} onpointerdown={(e) => grab(e, null)}>
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
		inset: 0 calc(100% - var(--to)) 0 var(--from);
		display: flex;
		justify-content: space-between;
		min-width: calc(var(--thick) * 2);
		border-radius: 999px;
		background: var(--surface-4);
		cursor: grab;
	}
	.vertical .thumb {
		inset: var(--from) 0 calc(100% - var(--to)) 0;
		flex-direction: column;
		min-width: 0;
		min-height: calc(var(--thick) * 2);
	}
	.end {
		position: relative;
		width: var(--thick);
		height: var(--thick);
		border-radius: 50%;
		background: var(--text-muted);
		scale: 0.6;
		cursor: ew-resize;
	}
	.vertical .end {
		cursor: ns-resize;
	}
	@media (hover: none) and (pointer: coarse) {
		.bar {
			--thick: var(--space-8);
		}
	}
</style>
