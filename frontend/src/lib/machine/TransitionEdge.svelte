<!-- A transition on the machine canvas: a line from box to box, or a loop off the side when it
     re-enters its own state, with chevrons along it for the way it goes — one stroke, so hover and
     selection reach the whole of it. Two between one pair of boxes run side by side. A click on the
     line or on its label, which carries the trigger summary, selects it. -->
<script lang="ts">
	import { EdgeLabel, type EdgeProps } from '@xyflow/svelte';
	import type { LabelSize, RouteGeometry } from './geometry';
	import TransitionLabel from './TransitionLabel.svelte';
	import TransitionStroke from './TransitionStroke.svelte';

	let { id, selected, data }: EdgeProps = $props();
	const d = $derived(data as { summary: string; geometry: RouteGeometry; onPick: (event: MouseEvent) => void; onMeasure: (size: LabelSize) => void });
	const geometry = $derived(d.geometry);
</script>

{#if geometry}
	<TransitionStroke {id} course={geometry.course} />
	<EdgeLabel x={geometry.label.x} y={geometry.label.y} transparent>
		<TransitionLabel summary={d.summary} {selected} testid={`transition-${id}`} onPick={d.onPick} onMeasure={d.onMeasure} />
	</EdgeLabel>
{/if}
