<!-- The two-line head of an inspector: what is shown (a name, renameable) over what kind of thing it
     is, with the actions on it and the ✕ that closes the pane the inspector slid in on. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Bar, Icon, IconButton } from '$lib/ui';

	let {
		title,
		type,
		end,
		onClose
	}: {
		title: Snippet;
		type: string;
		end?: Snippet;
		/** Renders the ✕; only the slide-in pane supplies one. */
		onClose?: () => void;
	} = $props();
</script>

<Bar class="pf-identity-bar">
	{#snippet start()}
		<div class="identity">
			<div class="title">{@render title()}</div>
			<div class="type">{type}</div>
		</div>
	{/snippet}
	{#snippet end()}
		{@render end?.()}
		{#if onClose}
			<IconButton
				variant="ghost"
				density="chrome"
				class="pf-close"
				label="Close inspector"
				title="Close the inspector"
				data-testid="inspector-close"
				onclick={onClose}><Icon name="x" /></IconButton
			>
		{/if}
	{/snippet}
</Bar>

<style>
	.identity {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		min-width: 0;
	}
	.title {
		font-size: var(--fs-strong);
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.type {
		color: var(--text-muted);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	/* `pf-identity-bar` is a class passed to another component, so Svelte's scoping hash never
	   reaches its markup; `:global` anchored on nothing is what reaches it. */
	:global(.pf-identity-bar) {
		/* The ✕ must never be squeezed into overflow past the pane's edge; the name is what ellipsizes. */
		--bar-end-min: max-content;
		/* Two lines tall by construction, so it takes back the padding a one-row strip has none of. */
		--bar-pad-y: var(--space-2);
	}
	:global(.pf-identity-bar .pf-close) {
		--panelty-icon-btn-size: 22px;
		color: var(--text-dim);
	}
	:global(.pf-identity-bar .pf-close:hover) {
		color: var(--text);
	}
</style>
