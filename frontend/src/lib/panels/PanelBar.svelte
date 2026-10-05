<!-- The bar a panel that shows one named thing wears: the name in mono, the panel's actions at the end. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Bar } from '$lib/ui';

	let { title, end }: { title: string; end?: Snippet } = $props();
</script>

<Bar style="--bar-bg: transparent; --bar-border: 1px solid var(--border)">
	{#snippet start()}<span class="title">{title}</span>{/snippet}
	{#snippet end()}{@render end?.()}{/snippet}
</Bar>

<style>
	.title {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
		font-size: var(--fs-small);
	}
</style>
