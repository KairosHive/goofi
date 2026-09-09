<script lang="ts">
	import type { NodeInstanceInfo } from '$lib/api/control';
	import { bindViewer, dropRate } from '$lib/api/frames';
	import type { DataFrame } from '$lib/codec/decode';
	import { metaEntries, formatMetaValue, formatMetaInline } from './metaFormat';
	import MetadataField from './MetadataField.svelte';
	import { nodeStatsRows } from './nodeStats';
	import { Select, EmptyState } from '$lib/ui';

	type Props = {
		node: NodeInstanceInfo;
	};
	const { node }: Props = $props();

	const slots = $derived(Object.keys(node.output_slots ?? {}));
	let internalSlot = $state<string | null>(null);
	let lastFrame = $state<DataFrame | null>(null);
	/** This panel's identity in the slot's viewer registry; it binds with a null spec, so it
	 *  constrains nothing a real viewer asked for. */
	const token =
		typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : `md-${Math.random()}`;
	$effect(() => {
		const fst = slots[0] ?? null;
		if (internalSlot === null || !slots.includes(internalSlot)) internalSlot = fst;
	});

	$effect(() => {
		lastFrame = null;
		const slot = internalSlot;
		if (!slot) return;
		return bindViewer(node.uid, slot, token, null, (f: DataFrame) => {
			lastFrame = f;
		});
	});

	// Format once per frame, not per render: this panel re-renders at the data rate.
	const fields = $derived(
		metaEntries(lastFrame?.meta).map(([key, value]) => ({
			key,
			body: formatMetaValue(value),
			inline: formatMetaInline(value)
		}))
	);

	// Polled, not derived: a rate must keep falling when frames stop, and only a frame re-renders.
	let drops = $state<number | null>(null);
	$effect(() => {
		const slot = internalSlot;
		drops = null;
		if (!slot) return;
		const id = setInterval(() => (drops = dropRate(node.uid, slot)), 250);
		return () => clearInterval(id);
	});

	const statsRows = $derived(nodeStatsRows(node.stats, drops));
</script>

<section class="panel">
	<header>
		<span>Metadata</span>
		{#if slots.length > 0}
			<Select
				class="slot-select"
				value={internalSlot ?? ''}
				onChange={(v) => (internalSlot = v)}
				options={slots}
				labels={node.slot_labels}
			/>
		{/if}
	</header>

	{#if statsRows.length > 0}
		<dl class="stats" data-testid="node-stats">
			{#each statsRows as row (row.label)}
				<div class="stat">
					<dt>{row.label}</dt>
					<dd>{row.value}</dd>
				</div>
			{/each}
		</dl>
	{/if}

	{#if lastFrame}
		{#if fields.length === 0}
			<EmptyState>
				{#snippet hint()}No metadata{/snippet}
			</EmptyState>
		{:else}
			<div class="meta-tree">
				{#each fields as f (f.key)}
					<MetadataField name={f.key} body={f.body} inline={f.inline} />
				{/each}
			</div>
		{/if}
	{:else}
		<EmptyState>
			{#snippet hint()}Waiting for data…{/snippet}
		</EmptyState>
	{/if}
</section>

<style>
	.panel {
		padding: var(--space-6);
		border-top: 1px solid var(--border);
	}
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		font-weight: 600;
		margin-bottom: var(--space-5);
	}
	header :global(.slot-select) {
		flex: 0 0 auto;
	}
	.stats {
		margin: 0 0 var(--space-5);
		padding: 0 0 var(--space-5);
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-1) var(--space-7);
		border-bottom: 1px solid var(--border);
	}
	.stats .stat {
		display: flex;
		gap: var(--space-3);
		align-items: baseline;
		font-family: var(--font-mono);
	}
	.stats dt {
		font-size: var(--fs-micro);
		color: var(--text-dim);
	}
	.stats dd {
		margin: 0;
		font-size: var(--fs-small);
		color: var(--text);
		font-variant-numeric: tabular-nums;
	}
	.meta-tree {
		display: flex;
		flex-direction: column;
		gap: 1px;
	}
</style>
