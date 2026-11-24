<!-- The app-wide paint rate in the TopBar, ticked off a timer rather than a perpetual rAF. -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { paints as p } from '$lib/api/frames';

	onMount(() => {
		const id = setInterval(() => p.tick(), 250);
		return () => clearInterval(id);
	});
</script>

<span
	class="hud"
	data-testid="perf-hud"
	title="Paints per second: how often the page draws a new batch of frames, whatever the stream count. Streams served together paint together, so this reads the viewer cap at rest."
>
	<span class="fps">{p.rate.toFixed(0)} fps</span>
</span>

<style>
	.hud {
		display: inline-flex;
		align-items: center;
		gap: var(--space-3);
		font-size: var(--fs-chrome);
		/* Tabular figures hold the column still as the counter ticks. */
		font-variant-numeric: tabular-nums;
		color: var(--text-dim);
		white-space: nowrap;
	}
</style>
