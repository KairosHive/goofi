<!-- A provisional transition uses the connected transition's route and stroke. -->
<script lang="ts">
	import { ViewportPortal, useStore } from '@xyflow/svelte';
	import type { Machine, Transition } from '$lib/api/generated';
	import { graph } from '$lib/stores/graph.svelte';
	import { courses, type LabelSize } from './geometry';
	import { internalBox } from './layout';
	import TransitionStroke from './TransitionStroke.svelte';
	import TransitionLabel from './TransitionLabel.svelte';

	const store = useStore();
	const g = graph();
	const previewId = '$preview';
	let labelSize = $state<LabelSize | undefined>(undefined);
	const geometry = $derived.by(() => {
		const connection = store.connection;
		if (!connection.inProgress) return null;
		const source = connection.fromNode;
		const model = g.machines[String(source.data.machine)];
		if (!model) return null;
		const target = connection.toNode?.id ?? '$pointer';
		const transition: Transition = { from: source.id, to: target, duration: 0, curve: 'step', chance: 1, weight: 1, internal: false };
		const provisional: Machine = { ...model, transitions: { ...model.transitions, [previewId]: transition } };
		return courses(provisional, (id) => id === '$pointer'
			? { x: connection.to.x, y: connection.to.y, w: 0, h: 0 }
			: internalBox(store.nodeLookup.get(id)), (key) => key === previewId ? labelSize : store.edgeLookup.get(key)?.data?.labelSize as LabelSize | undefined).get(previewId);
	});
</script>

{#if geometry}
	<TransitionStroke id={previewId} course={geometry.course} preview />
	<ViewportPortal target="front">
		<div class="preview-label" style:transform={`translate(-50%, -50%) translate(${geometry.label.x}px, ${geometry.label.y}px)`}>
			<TransitionLabel summary="manual" preview onMeasure={(size) => {
				if (labelSize?.w !== size.w || labelSize?.h !== size.h) labelSize = size;
			}} />
		</div>
	</ViewportPortal>
{/if}

<style>
	.preview-label { position: absolute; top: 0; left: 0; pointer-events: none; }
</style>
