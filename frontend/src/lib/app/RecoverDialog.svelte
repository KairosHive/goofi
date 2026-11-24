<!-- RecoverDialog — the offer a manager makes at the start: what an earlier goofi left unsaved.
     Each row opens its autosave; its × removes it, Remove all removes every one. Skipping keeps
     every recovery on disk. The list keeps its height as rows go, so the next × lands under a
     finger that stays put, and a row leaves on the click, so a repeat never asks twice. -->
<script lang="ts">
	import { tick } from 'svelte';
	import { getControl, type Recovery } from '$lib/api/control';
	import { patchName } from '$lib/api/patchFile';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { Button, ConfirmDialog, Icon, IconButton } from '$lib/ui';

	const g = graph();
	const ctl = getControl();
	let recoveries = $state<Recovery[]>([]);
	let dismissed = $state(false);
	const open = $derived(!dismissed && recoveries.length > 0);
	let skip = $state<HTMLElement | null>(null);

	// A modal focuses its first control (a patch row); the safe answer takes focus instead, once
	// open, since an `autofocus` fires at mount while the dialog is still closed.
	$effect(() => {
		if (!open) return;
		void tick().then(() => skip?.querySelector('button')?.focus());
	});

	// Once per SERVER session, on the startup hook: a page that stayed open across a restart is
	// offered what that restart found, without a reload. A demo has no host to have crashed on.
	$effect(() => {
		if (g.sessionEpoch === 0 || g.demo) return;
		dismissed = false;
		ctl.call<{ recoveries: Recovery[] }>('session recoverable', {}).then(
			(r) => (recoveries = r.recoveries),
			(e) => notify().failure('Recover', e)
		);
	});

	function when(at: number | null): string {
		if (at === null) return '';
		const ago = Math.max(0, Date.now() / 1000 - at);
		if (ago < 90) return 'just now';
		if (ago < 5400) return `${Math.round(ago / 60)} min ago`;
		if (ago < 129600) return `${Math.round(ago / 3600)} h ago`;
		return new Date(at * 1000).toLocaleString();
	}

	/** Take a row out of the list; the row comes back when the call it stood for is refused. */
	function take(workspace: string): Recovery | undefined {
		const row = recoveries.find((r) => r.workspace === workspace);
		recoveries = recoveries.filter((r) => r.workspace !== workspace);
		return row;
	}

	function restore(row: Recovery | undefined): void {
		if (row) recoveries = [...recoveries, row];
	}

	async function recover(workspace: string): Promise<void> {
		const row = take(workspace);
		try {
			await ctl.call('session recover', { workspace });
			dismissed = true;
		} catch (e) {
			restore(row);
			notify().failure('Recover', e);
		}
	}

	async function discard(workspace: string): Promise<void> {
		const row = take(workspace);
		try {
			await ctl.call('session discard', { workspace });
		} catch (e) {
			restore(row);
			notify().failure('Discard', e);
		}
	}

	function discardAll(): void {
		for (const r of recoveries) void discard(r.workspace);
	}
</script>

<ConfirmDialog
	{open}
	question="Recover unsaved work?"
	detail="A goofi that did not shut down cleanly left these patches behind. Open one to continue from its last autosave, or remove it. Skipping keeps them for next time."
	onClose={() => (dismissed = true)}
	data-testid="recover-dialog"
>
	{#snippet body()}
		<ul class="rows">
			{#each recoveries as r (r.workspace)}
				<li class="row" data-testid="recover-entry">
					<button class="entry" title={r.home ?? undefined} onclick={() => recover(r.workspace)}>
						<span class="nm">{r.home ? patchName(r.home) : 'Unsaved patch'}</span>
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
	<Button variant="danger" data-testid="recover-discard-all" onclick={discardAll}>Remove all</Button>
	<span bind:this={skip}><Button data-testid="recover-skip" onclick={() => (dismissed = true)}>Skip</Button></span>
</ConfirmDialog>

<style>
	.rows {
		list-style: none;
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		margin: var(--space-6) 0 0;
		padding: 0;
		/* A fixed size: the answers stay where they are as rows go, whatever the names. */
		width: min(80vw, 28rem);
		height: min(16rem, 40dvh);
		overflow-y: auto;
	}
	.row {
		flex: 0 0 auto;
		display: flex;
		align-items: center;
		gap: var(--space-2);
		padding: 0 var(--space-2) 0 0;
		border-radius: var(--radius-sm);
		background: var(--surface-3);
	}
	.entry {
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
