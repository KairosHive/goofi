<!-- Shared inspector content for the editor side pane and the Inspector panel. -->
<script lang="ts">
	import ParamForm from './ParamForm.svelte';
	import MetadataPanel from '$lib/editor/MetadataPanel.svelte';
	import { Button, ScrollArea } from '$lib/ui';
	import { graph } from '$lib/stores/graph.svelte';
	import type { NodeInstanceInfo } from '$lib/api/control';

	let { node, onClose }: {
		node: NodeInstanceInfo | null;
		onClose?: () => void;
	} = $props();

	function restart(): void {
		if (!node) return;
		void graph()
			.restartNode(node.uid)
			.catch((e) => console.warn('restart failed', e));
	}
</script>

<ScrollArea>
	<ParamForm {node} {onClose} />
	{#if node}
		<MetadataPanel {node} />
		{#if node.error}
			<section class="node-error" data-testid="inspector-error">
				<div class="err-head">
					<header>Error</header>
					<Button
						variant="danger"
						size="sm"
						onclick={restart}
						title="Restart this node (respawn with the same params + links)"
						data-testid="inspector-restart">↻ Restart</Button
					>
				</div>
				<pre>{node.error}</pre>
			</section>
		{/if}
	{/if}
</ScrollArea>

<style>
	.node-error {
		padding: var(--space-6);
		border-top: 1px solid var(--border);
		background: var(--surface-1);
	}
	.err-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-5);
		margin-bottom: var(--space-3);
	}
	.node-error header {
		font-weight: 600;
		font-size: var(--fs-small);
		color: var(--danger);
	}
	/* Stated, not inherited: a bare <pre> takes app.css's `font: inherit`, which is the chrome face. */
	.node-error pre {
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		color: var(--text-dim);
		white-space: pre-wrap;
		word-break: break-word;
		margin: 0;
	}
</style>
