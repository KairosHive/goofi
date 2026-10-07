<!-- The bar a panel over one of several instances wears: the instances as tabs — a tap picks one,
     a double-click renames it, its ✕ removes it, the ＋ starts one — and the panel's own actions
     at the right end. The strip is the workspace's own tab primitive, so the two read alike. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Tabs, type TabItem } from '$lib/ui';

	let {
		items,
		active,
		onSelect,
		onAdd,
		onRename,
		onClose,
		testid,
		end
	}: {
		items: TabItem[];
		active?: string;
		onSelect: (id: string) => void;
		onAdd?: () => void;
		onRename?: (id: string, label: string) => void;
		onClose?: (id: string) => void;
		/** The strip's test id. */
		testid?: string;
		/** The panel's own actions, right-pushed. */
		end?: Snippet;
	} = $props();
</script>

<div class="bar">
	<div class="strip thin-scrollbar">
		<Tabs class="itabs" data-testid={testid} {items} {active} {onSelect} {onAdd} {onRename} {onClose} />
	</div>
	{#if end}
		<div class="end">{@render end()}</div>
	{/if}
</div>

<style>
	.bar {
		display: flex;
		align-items: stretch;
		gap: var(--space-4);
		min-height: var(--panel-header-h);
		padding: 0 var(--space-4) 0 var(--space-2);
		background: var(--surface-2);
		border-bottom: 1px solid var(--border);
		--panelty-icon-btn-size: var(--chrome-control-h);
	}
	/* The strip scrolls what the bar cannot hold; its active tab drops to the body below the bar. */
	.strip {
		flex: 1 1 auto;
		min-width: 0;
		display: flex;
		align-items: stretch;
		overflow-x: auto;
		overflow-y: hidden;
		--panelty-tab-surface: transparent;
		--panelty-tab-body: var(--surface-1);
		--panelty-tab-align: stretch;
		--panelty-tab-pad: 0;
		--panelty-tab-fs: var(--fs-chrome);
	}
	.strip :global(.itabs) {
		height: 100%;
		flex: 1 0 auto;
	}
	.end {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		flex: 0 0 auto;
	}
</style>
