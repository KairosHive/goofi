<!-- Shared chrome for the node-linked panels (Inspector / Viewer). -->
<script lang="ts">
	import type { PanelProps } from 'panelty';
	import type { NodeInstanceInfo } from '$lib/api/control';
	import { graph } from '$lib/stores/graph.svelte';
	import { ui } from '$lib/stores/ui.svelte';
	import { linkedNodeName } from 'panelty';
	import { Bar, StatusDot, EmptyState } from '$lib/ui';
	import { nodeHealth } from '$lib/editor/nodeHealth';
	import NodeSelect from './NodeSelect.svelte';
	import type { Snippet } from 'svelte';

	let {
		panelId,
		state: linkState,
		label,
		content,
		controls
	}: PanelProps & {
		label: string;
		content: Snippet<[NodeInstanceInfo]>;
		/** Panel-specific controls rendered inline in the header bar. */
		controls?: Snippet<[NodeInstanceInfo]>;
	} = $props();

	const g = graph();
	const uiStore = ui();

	const linkedName = $derived(linkedNodeName(linkState));
	const node = $derived(linkedName ? g.nodeById(linkedName) : null);
	const dragActive = $derived(uiStore.nodeDrag !== null);
	const over = $derived(uiStore.nodeDragTarget === panelId);
</script>

<div class="linked" role="group" data-testid="node-linked-panel">
	<Bar class="nlp-bar">
		{#snippet start()}
			<!-- ONE scrolling group, the picker and dot included, so a narrow panel slides the whole
			     strip rather than clipping the controls behind a fixed picker. -->
			<div class="controls thin-scrollbar" data-testid="panel-strip">
				{#if node}
					{@const health = nodeHealth(node)}
					<StatusDot
						tone={health.tone}
						size="sm"
						pulse={health.kind === 'dead'}
						title={health.title}
					/>
				{/if}
				<NodeSelect {panelId} state={linkState} emptyLabel="<empty>" />
				{#if node && controls}
					{@render controls(node)}
				{/if}
			</div>
		{/snippet}
	</Bar>
	{#if node}
		<div class="body">
			{@render content(node)}
		</div>
	{:else}
		<div class="empty">
			<EmptyState>
				{#snippet title()}No node picked{/snippet}
				{#snippet hint()}Pick one above, or drag a node here, to show its {label}{/snippet}
			</EmptyState>
		</div>
	{/if}

	{#if dragActive}
		<div class="node-drop-hint" class:active={over} data-testid="node-drop-hint"></div>
	{/if}
</div>

<style>
	.linked {
		position: relative;
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	/* `gap: inherit` — the bar group's own gap, so the picker and these read as one row of siblings. */
	.controls {
		flex: 0 1 auto;
		min-width: 0;
		display: flex;
		align-items: center;
		gap: inherit;
		overflow-x: auto;
		overflow-y: hidden;
		/* A scroll box clips what it holds, and a picker's focus ring sits outside its border, so
		   the group pads for the ring on every side and pulls the same back out of the row. */
		--ring: calc(var(--focus-width) + 1px);
		padding: var(--ring);
		margin: calc(-1 * var(--ring));
		scroll-padding: var(--ring);
	}
	/* Touch: the strip's cog and bar take a shorter floor than the 44px hit, following the rem
	   up to a maximum, so a phone's viewer keeps its height for the data. `.nlp-bar` travels to
	   `Bar` as a prop; `.linked` keeps the rule scoped. */
	@media (hover: none) and (pointer: coarse) {
		.linked {
			--strip-control: min(32px, 2rem);
			--vs-cog-box: var(--strip-control);
			--bar-height: min(36px, 2.25rem);
		}
		.linked :global(.nlp-bar button) {
			min-height: var(--strip-control);
		}
	}
	.body {
		flex: 1;
		min-height: 0;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}
	.body > :global(*) {
		flex: 1;
		min-height: 0;
	}
	.empty {
		flex: 1;
		display: grid;
		place-items: center;
		padding: var(--space-7);
		text-align: center;
		color: var(--text-muted);
	}
</style>
