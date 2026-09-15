<!-- RecoverDialog — the offer a manager makes at the start: what an earlier goofi left unsaved.
     Each row opens its autosave; its × removes it. Dismissing keeps every recovery on disk. -->
<script lang="ts">
	import { tick } from 'svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { Button, ConfirmDialog, Icon, IconButton } from '$lib/ui';

	const g = graph();
	let dismissed = $state(false);
	const open = $derived(!dismissed && g.recoveries.length > 0);
	let later = $state<HTMLElement | null>(null);

	// A modal focuses its first control, which would be the first patch in the list; the safe
	// answer takes it instead, once the dialog is open — an `autofocus` fires at mount, when it
	// is still closed.
	$effect(() => {
		if (!open) return;
		void tick().then(() => later?.querySelector('button')?.focus());
	});

	// Once per SERVER session, on the startup hook: a page that stayed open across a restart is
	// offered what that restart found, without a reload. A demo has no host to have crashed on.
	$effect(() => {
		if (g.sessionEpoch === 0 || g.demo) return;
		dismissed = false;
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
					<button class="entry" title={r.home ?? undefined} onclick={() => recover(r.workspace)}>
						<span class="nm">{name(r.home)}</span>
						<span class="at">{when(r.at)}</span>
					</button>
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
	<span bind:this={later}><Button variant="ghost" onclick={() => (dismissed = true)}>Later</Button></span>
</ConfirmDialog>

<style>
	.rows {
		list-style: none;
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		margin: var(--space-6) 0 0;
		padding: 0;
		max-height: 40dvh;
		overflow-y: auto;
	}
	.row {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		padding: 0 var(--space-2) 0 0;
		border-radius: var(--radius-sm);
		background: var(--surface-3);
	}
	.entry {
		font: inherit;
		display: flex;
		align-items: baseline;
		gap: var(--space-4);
		flex: 1;
		min-width: 0;
		text-align: left;
		background: transparent;
		border: none;
		border-radius: var(--radius-sm);
		color: var(--text);
		padding: var(--space-3) var(--space-4);
		cursor: pointer;
	}
	.row:hover {
		background: var(--surface-4);
	}
	/* Inside the row, or the scrolling list clips the ring to a line along one edge. */
	.entry:focus-visible {
		outline-offset: calc(-1 * var(--focus-width));
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
