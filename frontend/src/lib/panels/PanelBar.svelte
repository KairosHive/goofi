<!-- The bar a panel that shows one named thing wears: the name in mono (or a `start` of its own),
     the panel's actions at the end. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Bar } from '$lib/ui';

	let { title = '', start, end }: { title?: string; start?: Snippet; end?: Snippet } = $props();
</script>

<Bar style="--bar-bg: transparent; --bar-border: 1px solid var(--border)">
	{#snippet start()}{#if start}{@render start()}{:else}<span class="title">{title}</span>{/if}{/snippet}
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
