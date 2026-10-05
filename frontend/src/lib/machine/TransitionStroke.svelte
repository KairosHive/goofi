<!-- One stroke for a transition and the transition being drawn. -->
<script lang="ts">
	import { BaseEdge } from '@xyflow/svelte';
	import { marked, type Course } from './geometry';
	let { id, course, preview = false }: { id: string; course: Course; preview?: boolean } = $props();
</script>

<BaseEdge {id} path={marked(course)} class="transition" interactionWidth={preview ? 0 : 24} />

<style>
	:global(.svelte-flow__edge-path.transition) { stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
	:global(.svelte-flow__edge:hover .svelte-flow__edge-path.transition) { stroke: var(--ring-accent); }
	:global(.svelte-flow__edge.selected .svelte-flow__edge-path.transition) { stroke: var(--accent); }
</style>
