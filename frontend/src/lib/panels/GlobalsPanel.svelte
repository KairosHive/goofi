<script lang="ts">
	import { tick } from 'svelte';
	import type { PanelProps } from 'panelty';
	import { graph } from '$lib/stores/graph.svelte';
	import { effectiveLock, groupedGlobals, isValidIdentifier, type GlobalType, type GlobalView } from '$lib/crdt/graphDoc';
	import { Button, Icon, IconButton, MODE_ATTRS, NumberInput, ScrollArea, Select, TextInput, Toggle } from '$lib/ui';

	let {}: PanelProps = $props();
	const g = graph();
	const globals = $derived(g.globals);
	const groups = $derived(groupedGlobals(globals, g.globalGroups));
	let panel: HTMLDivElement;
	let open = $state<Record<string, boolean>>({});
	let editing = $state<string | null>(null);
	let groupName = $state('');
	let focusEntry = $state<string | null>(null);
	let busy = $state(false);
	let error = $state('');

	$effect(() => {
		const name = focusEntry;
		if (name && globals.some((entry) => entry.name === name)) {
			focusEntry = null;
			void tick().then(() => {
				const input = panel.querySelector<HTMLInputElement>(`[data-name="${name}"] [data-testid="global-name"]`);
				input?.focus();
				input?.select();
			});
		}
	});

	function report(reason: unknown): void {
		error = String(reason);
	}

	function editGroup(group: string): void {
		editing = group;
		groupName = group;
		error = '';
	}

	function selectName(input: HTMLInputElement): void {
		input.focus();
		input.select();
	}

	async function addGroup(): Promise<void> {
		busy = true;
		error = '';
		try {
			editGroup(await g.addGlobalGroup());
		} catch (reason) {
			report(reason);
		} finally {
			busy = false;
		}
	}

	async function renameGroup(from: string): Promise<void> {
		if (editing !== from) return;
		const to = groupName.trim();
		if (to === from) {
			editing = null;
			return;
		}
		if (!isValidIdentifier(to)) {
			error = 'Use letters, digits and underscores. Start with a letter or underscore.';
			return;
		}
		try {
			await g.renameGlobalGroup(from, to);
			open[to] = open[from] === true;
			delete open[from];
			editing = null;
			error = '';
		} catch (reason) {
			report(reason);
		}
	}

	async function addEntry(group: string): Promise<void> {
		busy = true;
		error = '';
		try {
			focusEntry = await g.addGlobalEntry(group);
		} catch (reason) {
			report(reason);
		} finally {
			busy = false;
		}
	}

	function commitValue(entry: GlobalView, raw: string | number | boolean): void {
		let value: number | string | boolean;
		if (entry.type === 'bool') value = raw === true;
		else if (entry.type === 'string') value = String(raw);
		else {
			const number = Number(raw);
			if (!Number.isFinite(number)) return;
			value = entry.type === 'int' ? Math.round(number) : number;
		}
		void g.setGlobalValue(entry.name, value).catch(report);
	}

	function commitName(entry: GlobalView, raw: string): void {
		const name = raw.trim();
		if (name !== entry.element) void g.renameGlobal(entry.name, `${entry.group}.${name}`).catch(report);
	}
</script>

<div class="wrap" data-testid="globals-panel" bind:this={panel}>
	<ScrollArea>
		<div class="gp-body">
			{#each groups as grp (grp.group)}
				{@const controlled = grp.entries.some((entry) => entry.control)}
				{@const lock = { ...grp.lock, config: grp.lock.config || controlled }}
				<section class="grp" data-testid="global-group" data-group={grp.group}
					data-lock-config={lock.config} data-lock-value={grp.lock.value}>
					<div class="grp-head">
						<button class="grp-toggle" aria-label={`${open[grp.group] ? 'Collapse' : 'Expand'} ${grp.group}`}
							aria-expanded={open[grp.group] === true} data-testid="global-group-toggle"
							onclick={() => (open[grp.group] = !open[grp.group])}></button>
						<span class="grp-caret"><Icon name={open[grp.group] ? 'chevron-down' : 'chevron-right'} /></span>
						{#if editing === grp.group}
							<input {...MODE_ATTRS.search} class="group-name-input" data-testid="global-group-name"
								aria-label="Group name" bind:value={groupName} use:selectName
								onblur={() => void renameGroup(grp.group)}
								onkeydown={(event) => {
									if (event.key === 'Enter') event.currentTarget.blur();
									if (event.key === 'Escape') { editing = null; error = ''; }
								}} />
						{:else}
							<span class="grp-name">{grp.group}</span>
							{#if !lock.config}
								<IconButton variant="ghost" size="sm" label={`Rename ${grp.group}`} title="Rename group"
									class="grp-edit" data-testid="global-group-edit" onclick={() => editGroup(grp.group)}><Icon name="pencil" /></IconButton>
							{/if}
						{/if}
						<span class="grp-tags">
							{#if controlled}
								<span class="grp-control" role="img" aria-label="Control panel" title="Control panel"><Icon name="sliders-horizontal" /></span>
							{/if}
							{#if lock.config || lock.value}
								<span class="grp-lock" role="img" aria-label="Built-in lock" title="Built-in lock"><Icon name="lock" /></span>
							{/if}
							<span class="grp-count">{grp.entries.length}</span>
						</span>
					</div>
					{#if open[grp.group]}
						<div class="grp-body">
							{#each grp.entries as entry (entry.name)}
								{@const held = effectiveLock(entry, lock)}
								<div class="entry" data-testid="global-row" data-name={entry.name}
									data-lock-config={held.config} data-lock-value={held.value}
									data-control={entry.control?.kind}>
									<div class="entry-name">
										{#if held.config}
											<span class="fixed">{entry.element}</span>
										{:else}
											<TextInput inputmode="search" data-testid="global-name" aria-label="Entry name"
												value={entry.element} autocomplete="off" onChange={(value) => commitName(entry, value)} />
										{/if}
									</div>
									<div class="entry-value">
										{#if held.value || entry.source}
											<span class="ro-value" data-testid="global-value">{String(entry.value)}</span>
										{:else if entry.type === 'bool'}
											<Toggle data-testid="global-value" value={entry.value === true} onChange={(value) => commitValue(entry, value)} />
										{:else if entry.type === 'string'}
											<TextInput inputmode="search" data-testid="global-value" aria-label="Entry value"
												value={String(entry.value)} autocomplete="off" onChange={(value) => commitValue(entry, value)} />
										{:else}
											<NumberInput data-testid="global-value" aria-label="Entry value" value={Number(entry.value)}
												onChange={(value) => commitValue(entry, value)} />
										{/if}
									</div>
									<Select data-testid="global-type" aria-label="Entry type" value={entry.type}
										disabled={held.config || held.value || !!entry.source}
										options={['float', 'int', 'bool', 'string']}
										onChange={(value) => void g.setGlobalType(entry.name, value as GlobalType).catch(report)} />
									{#if !held.config}
										<IconButton variant="ghost" size="sm" data-testid="global-delete" title="Delete entry"
											label={`Delete ${entry.name}`} onclick={() => void g.removeGlobal(entry.name).catch(report)}><Icon name="x" /></IconButton>
									{/if}
								</div>
							{/each}
							{#if !lock.config}
								<Button class="add-row" variant="ghost" size="sm" data-testid="global-add-in" disabled={busy}
									onclick={() => void addEntry(grp.group)}><Icon name="plus" />entry</Button>
							{/if}
						</div>
					{/if}
				</section>
			{/each}
			<Button class="add-row new-group" variant="ghost" size="sm" data-testid="global-add-group-btn"
				disabled={busy} onclick={() => void addGroup()}><Icon name="plus" />group</Button>
			{#if error}<p class="error" role="alert">{error}</p>{/if}
		</div>
	</ScrollArea>
</div>

<style>
	.wrap {
		height: 100%;
		min-height: 0;
		container-type: inline-size;
	}
	.gp-body {
		padding: var(--space-3) var(--space-5) var(--space-6);
	}
	.grp:last-of-type {
		border-bottom: 1px solid var(--border);
	}
	.grp-head {
		background: var(--surface-2);
		border-radius: var(--radius-sm);
		position: relative;
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--hit);
		padding: var(--space-2) var(--space-3);
	}
	.grp-toggle {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		border: none;
		border-radius: var(--radius-sm);
		background: transparent;
		cursor: pointer;
	}
	.grp-toggle:hover { background: color-mix(in srgb, var(--text) 5%, transparent); }
	.grp-toggle:focus-visible {
		outline: var(--focus-width) solid var(--focus-ink);
		outline-offset: -2px;
	}
	.grp-caret, .grp-name, .grp-tags {
		position: relative;
		pointer-events: none;
	}
	.grp-caret {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: var(--hit);
		flex-shrink: 0;
		color: var(--text-muted);
	}
	.grp-head :global(.grp-edit), .group-name-input { position: relative; }
	.grp-name, .group-name-input {
		min-width: 0;
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		color: var(--text);
	}
	.grp-name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		background: none;
		border: none;
		padding: 0;
	}
	.group-name-input {
		width: 12ch;
		flex: 1;
		background: var(--surface-1);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
	}
	.grp-tags {
		display: inline-flex;
		align-items: center;
		gap: var(--space-3);
		flex-shrink: 0;
		margin-left: auto;
		color: var(--text-muted);
	}
	.grp-count { font-size: var(--fs-micro); }
	.grp-control, .grp-lock { display: inline-flex; }
	.grp-body {
		background: var(--surface-1);
		padding: var(--space-2) var(--space-3);
	}
	.entry {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto var(--hit);
		gap: var(--space-2);
		align-items: center;
		padding: var(--space-2) 0;
		font-size: var(--fs-small);
	}
	.entry + .entry {
		border-top: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
	}
	.entry-name, .entry-value {
		min-width: 0;
		font-family: var(--font-mono);
		--number-width: 100%;
	}
	.fixed, .ro-value { overflow-wrap: anywhere; }
	.ro-value { color: var(--text-muted); }
	.gp-body :global(.add-row) {
		width: 100%;
		justify-content: center;
	}
	.gp-body :global(.new-group) { margin-top: var(--space-4); }
	.error { color: var(--danger); font-size: var(--fs-small); overflow-wrap: anywhere; }
	@container (max-width: 280px) {
		.entry { grid-template-columns: minmax(0, 1fr) auto var(--hit); }
		.entry-name { grid-column: 1 / -1; }
	}
	@media (hover: none) and (pointer: coarse) {
		.group-name-input { font-size: 16px; }
	}
</style>
