<!-- RecoverDialog — the offer a manager makes at the start: what an earlier goofi left unsaved.
     Each row opens its autosave; its × removes it. Dismissing keeps every recovery on disk. -->
<script lang="ts">
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { Button, ConfirmDialog, Icon, IconButton } from '$lib/ui';

	const g = graph();
	let asked = $state(false);
	let dismissed = $state(false);
	const open = $derived(!dismissed && g.recoveries.length > 0);

	// Once per page, after the first hello: a recovery is an offer made at the start, and a demo
	// has no host to have crashed on.
	$effect(() => {
		if (asked || !g.hadHello || g.demo) return;
		asked = true;
		void g.refreshRecoveries().catch((e) => notify().failure('Recover', e));
	});

	function name(home: string | null): string {
		return home ? (home.split('/').pop() ?? home) : 'Unsaved patch';
	}

	function when(at: number | null): string {
		if (at === null) return '';
		const ago = Math.max(0, Date.now() / 1000 - at);
		if (ago < 90) return 'just now';
		if (ago < 5400) return `${Math.round(ago / 60)} min ago`;
		if (ago < 129600) return `${Math.round(ago / 3600)} h ago`;
		return new Date(at * 1000).toLocaleString();
	}

	async function recover(workspace: string): Promise<void> {
		try {
			await g.recover(workspace);
			dismissed = true;
		} catch (e) {
			notify().failure('Recover', e);
		}
	}

	async function discard(workspace: string): Promise<void> {
		try {
			await g.discardRecovery(workspace);
		} catch (e) {
			notify().failure('Discard', e);
		}
	}
</script>

<ConfirmDialog
	{open}
	question="Recover unsaved work?"
	detail="A goofi that did not shut down cleanly left these patches behind. Open one to continue from its last autosave, or remove it."
	onClose={() => (dismissed = true)}
	data-testid="recover-dialog"
>
	{#snippet body()}
		<ul class="rows">
			{#each g.recoveries as r (r.workspace)}
				<li class="row" data-testid="recover-entry">
					<Button
						variant="ghost"
						class="entry"
						title={r.home ?? r.workspace}
						onclick={() => recover(r.workspace)}
					>
						<span class="nm">{name(r.home)}</span>
						<span class="at">{when(r.at)}</span>
					</Button>
					<IconButton
						variant="ghost"
						size="sm"
						label="Remove"
						data-testid="recover-discard"
						onclick={() => discard(r.workspace)}><Icon name="x" /></IconButton
					>
				</li>
			{/each}
		</ul>
	{/snippet}
	<Button variant="ghost" onclick={() => (dismissed = true)}>Later</Button>
</ConfirmDialog>

<style>
	.rows {
		list-style: none;
		margin: var(--space-4) 0 0;
		padding: 0;
		max-height: 40dvh;
		overflow: auto;
	}
	.row {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}
	/* `:global` because the class travels to `Button` as a prop; `.row` keeps it scoped. */
	.row :global(.entry) {
		flex: 1;
		min-width: 0;
		justify-content: space-between;
		gap: var(--space-4);
	}
	.nm {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.at {
		flex: 0 0 auto;
		color: var(--text-dim);
		font-size: var(--fs-small);
	}
</style>
