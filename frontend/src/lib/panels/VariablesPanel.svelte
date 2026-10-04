<script lang="ts">
	import { tick } from 'svelte';
	import type { PanelProps } from 'panelty';
	import { graph } from '$lib/stores/graph.svelte';
	import type { Literal } from '$lib/api/generated';
	import { effectiveLock, groupedVariables, isValidIdentifier, type VariableView } from '$lib/crdt/graphDoc';
	import { variableForm, variableValue, watchVariables, type Form } from '$lib/stores/variableValues.svelte';
	import { Button, Icon, IconButton, MODE_ATTRS, NumberInput, ScrollArea, Select, TextInput } from '$lib/ui';

	let {}: PanelProps = $props();
	const g = graph();
	const groups = $derived(groupedVariables(g.variables, g.variableGroups));
	// Values come over the data plane, one stream per variable shown.
	watchVariables(() => g.variables.map((entry) => entry.name));
	let panel: HTMLDivElement;
	let open = $state<Record<string, boolean>>({});
	let editing = $state<string | null>(null);
	let groupName = $state('');
	let focusEntry = $state<string | null>(null);
	let busy = $state(false);
	let error = $state('');

	$effect(() => {
		const name = focusEntry;
		if (name && g.variables.some((entry) => entry.name === name)) {
			focusEntry = null;
			void tick().then(() => {
				const input = panel.querySelector<HTMLInputElement>(`[data-name="${name}"] [data-testid="variable-name"]`);
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

	async function guarded(op: () => Promise<void>): Promise<void> {
		busy = true;
		error = '';
		try {
			await op();
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
			await g.renameVariableGroup(from, to);
			open[to] = open[from] === true;
			delete open[from];
			editing = null;
			error = '';
		} catch (reason) {
			report(reason);
		}
	}

	function shown(value: Literal | null): string {
		return value === null ? '' : typeof value === 'string' ? value : JSON.stringify(value);
	}
	function commitValue(entry: VariableView, raw: string | number): void {
		const form = variableForm(entry.name);
		let value: Literal = Number(raw);
		if (form === 'text') value = String(raw);
		else if (form === 'list' || form === 'image') {
			try {
				value = JSON.parse(String(raw));
			} catch {
				return;
			}
			if (!Array.isArray(value)) return;
		} else if (!Number.isFinite(value)) return;
		void g.setVariableValue(entry.name, value).catch(report);
	}
	/** Switching the form starts the value over: 0, an empty text, or a list of one. */
	function setForm(entry: VariableView, form: Form): void {
		if (form === variableForm(entry.name)) return;
		void g.setVariableValue(entry.name, form === 'text' ? '' : form === 'list' ? [0] : 0).catch(report);
	}

	function commitName(entry: VariableView, raw: string): void {
		const name = raw.trim();
		if (name !== entry.element) void g.renameVariable(entry.name, `${entry.group}.${name}`).catch(report);
	}
</script>

<div class="panel-wrap" data-testid="variables-panel" bind:this={panel}>
	<ScrollArea>
		<div class="gp-body">
			{#each groups as grp (grp.group)}
				{@const controlled = grp.entries.some((entry) => entry.control)}
				{@const lock = { ...grp.lock, config: grp.lock.config || controlled }}
				<section class="grp" data-testid="variable-group" data-group={grp.group}
					data-lock-config={lock.config} data-lock-value={grp.lock.value}>
					<div class="grp-head">
						<button class="grp-toggle" aria-label={`${open[grp.group] ? 'Collapse' : 'Expand'} ${grp.group}`}
							aria-expanded={open[grp.group] === true} data-testid="variable-group-toggle"
							onclick={() => (open[grp.group] = !open[grp.group])}></button>
						<span class="grp-caret"><Icon name={open[grp.group] ? 'chevron-down' : 'chevron-right'} /></span>
						{#if editing === grp.group}
							<input {...MODE_ATTRS.search} class="group-name-input" data-testid="variable-group-name"
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
									class="grp-edit" data-testid="variable-group-edit" onclick={() => editGroup(grp.group)}><Icon name="pencil" /></IconButton>
							{/if}
						{/if}
						<span class="grp-tags">
							{#if controlled}
								<span class="grp-control" role="img" aria-label="Control panel" title="Control panel"><Icon name="sliders-horizontal" /></span>
							{/if}
							{#if g.midiGroups[grp.group]}
								<span class="grp-control" role="img" aria-label="MIDI device" title={`MIDI device ${g.midiGroups[grp.group].port}`}><Icon name="piano" /></span>
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
								{@const value = variableValue(entry.name)}
								{@const form = variableForm(entry.name)}
								<div class="entry" data-testid="variable-row" data-name={entry.name}
									data-lock-config={held.config} data-lock-value={held.value}
									data-control={entry.control?.kind}>
									<div class="entry-name">
										{#if held.config}
											<span class="fixed">{entry.element}</span>
										{:else}
											<TextInput inputmode="search" data-testid="variable-name" aria-label="Entry name"
												value={entry.element} autocomplete="off" onChange={(value) => commitName(entry, value)} />
										{/if}
									</div>
									<div class="entry-value">
										{#if held.value || entry.source || value === null}
											<span class="ro-value" data-testid="variable-value">{shown(value)}</span>
										{:else if typeof value === 'number'}
											<NumberInput data-testid="variable-value" aria-label="Entry value" {value}
												onChange={(value) => commitValue(entry, value)} />
										{:else}
											<TextInput inputmode="search" data-testid="variable-value" aria-label="Entry value"
												value={shown(value)} autocomplete="off" onChange={(value) => commitValue(entry, value)} />
										{/if}
									</div>
									<Select data-testid="variable-type" aria-label="Entry form" value={form}
										disabled={held.config || held.value || !!entry.source}
										options={form === 'number' || form === 'text' ? ['number', 'text'] : ['number', 'text', form]}
										onChange={(value) => setForm(entry, value as Form)} />
									{#if !held.config}
										<IconButton variant="ghost" size="sm" data-testid="variable-delete" title="Delete entry"
											label={`Delete ${entry.name}`} onclick={() => void g.removeVariable(entry.name).catch(report)}><Icon name="x" /></IconButton>
									{/if}
								</div>
							{/each}
							{#if !lock.config}
								<Button class="add-row" variant="ghost" size="sm" data-testid="variable-add-in" disabled={busy}
									onclick={() => void guarded(async () => { focusEntry = await g.addVariableEntry(grp.group); })}><Icon name="plus" />entry</Button>
							{/if}
						</div>
					{/if}
				</section>
			{/each}
			<Button class="add-row new-group" variant="ghost" size="sm" data-testid="variable-add-group-btn"
				disabled={busy} onclick={() => void guarded(async () => editGroup(await g.addVariableGroup()))}><Icon name="plus" />group</Button>
			{#if error}<p class="error-text" role="alert">{error}</p>{/if}
		</div>
	</ScrollArea>
</div>

<style>
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
	@container (max-width: 280px) {
		.entry { grid-template-columns: minmax(0, 1fr) auto var(--hit); }
		.entry-name { grid-column: 1 / -1; }
	}
	@media (hover: none) and (pointer: coarse) {
		.group-name-input { font-size: 16px; }
	}
</style>
