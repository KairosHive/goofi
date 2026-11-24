<script lang="ts">
	import type { DataFrame } from '$lib/codec/decode';
	import type { SettingsMap } from './module';
	import TableTree from './TableTree.svelte';
	import { tableChildren } from './tableTree';

	type Props = { frame: DataFrame; settings: SettingsMap };
	const { frame, settings }: Props = $props();
</script>

<div class="container" data-testid="table-viewer">
	{#each tableChildren(frame) as [k, v] (k)}
		<TableTree name={k} frame={v} decimals={Number(settings.decimals)} />
	{/each}
</div>

<style>
	.container {
		width: 100%;
		height: 100%;
		min-height: 80px;
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		padding: var(--space-3) var(--space-2);
		font-size: var(--fs-micro);
		font-family: var(--font-mono);
		overflow: auto;
	}
</style>
