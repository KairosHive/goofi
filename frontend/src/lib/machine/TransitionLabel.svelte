<!-- A transition label owns its rendered size, for connected and provisional routes. -->
<script lang="ts">
	import { LABEL_W, LABEL_H, type LabelSize } from './geometry';
	let { summary, selected = false, preview = false, testid, onPick, onMeasure }: {
		summary: string;
		selected?: boolean;
		preview?: boolean;
		testid?: string;
		onPick?: (event: MouseEvent) => void;
		onMeasure: (size: LabelSize) => void;
	} = $props();
	function measure(el: HTMLButtonElement): { destroy: () => void } {
		const update = (): void => {
			if (el.isConnected && el.offsetWidth > 0 && el.offsetHeight > 0) onMeasure({ w: el.offsetWidth, h: el.offsetHeight });
		};
		const observer = new ResizeObserver(update);
		observer.observe(el);
		update();
		return { destroy: () => observer.disconnect() };
	}
</script>

<button type="button" class="label nodrag nopan" class:picked={selected} class:preview
	data-testid={testid} style={`--label-width: ${LABEL_W}px; --label-height: ${LABEL_H}px`}
	title="Select the transition" tabindex={preview ? -1 : 0} use:measure
	onclick={(event) => { event.stopPropagation(); onPick?.(event); }}>{summary}</button>

<style>
	.label {
		height: var(--label-height);
		padding: var(--space-1) var(--space-3);
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: 999px;
		color: var(--text);
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		white-space: nowrap;
		max-width: var(--label-width);
		overflow: hidden;
		text-overflow: ellipsis;
		cursor: pointer;
		pointer-events: all;
	}
	.label:hover, .label.picked { border-color: var(--accent); }
	.label.preview { pointer-events: none; opacity: 0.6; }
</style>
