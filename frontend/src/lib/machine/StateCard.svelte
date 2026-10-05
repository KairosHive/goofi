<!-- A state's card on the machine canvas: its name, and one row per attribute — a widget where the
     state sets it, a dimmed name where it does not. Its top strip is the dock the playhead dots sit on. -->
<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import type { Attribute, Literal, State } from '$lib/api/generated';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import ControlWidget from '$lib/panels/ControlWidget.svelte';
	import { Icon, IconButton, TextInput } from '$lib/ui';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import { CARD_W } from './layout';

	let { id, data }: NodeProps = $props();
	const g = graph();
	const machine = $derived(data.machine as string);
	const st = $derived(data.state as State);
	const attributes = $derived(data.attributes as Record<string, Attribute>);
	const values = $derived(st.values ?? {});
	let renaming = $state(false);

	function edit(values: Record<string, Literal | null>): void {
		void g.machine('machine state edit', { machine, name: id, values }).catch((e) => notify().failure('Edit state', e));
	}
	function rename(raw: string): void {
		renaming = false;
		const to = raw.trim();
		if (to === id || !isValidIdentifier(to)) return;
		void g.machine('machine state rename', { machine, name: id, to }).catch((e) => notify().failure('Rename state', e));
	}
	function remove(): void {
		void g.machine('machine state remove', { machine, name: id }).catch((e) => notify().failure('Remove state', e));
	}
	/** A value typed where the attribute has no widget: a number, or JSON for a list or a text. */
	function parse(raw: string): Literal | null {
		const n = Number(raw);
		if (raw.trim() !== '' && Number.isFinite(n)) return n;
		try {
			return JSON.parse(raw) as Literal;
		} catch {
			return raw;
		}
	}
</script>

<div class="card" style={`width: ${CARD_W}px`} data-testid={`state-card-${id}`} data-state={id}>
	<Handle type="target" position={Position.Left} class="port" />
	<Handle type="source" position={Position.Right} class="port" />
	<div class="dock"></div>
	<div class="head">
		{#if renaming}
			<div class="nodrag grow">
				<TextInput inputmode="search" data-testid="state-rename" value={id} autocomplete="off" onChange={rename}
					onkeydown={(e) => { if (e.key === 'Escape') renaming = false; }} />
			</div>
		{:else}
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<span class="name" title="Double-click to rename" ondblclick={() => (renaming = true)}>{id}</span>
		{/if}
		<span class="nodrag">
			<IconButton variant="ghost" size="sm" label={`Remove ${id}`} title="Remove this state" data-testid="state-remove" onclick={remove}><Icon name="x" /></IconButton>
		</span>
	</div>
	{#each Object.entries(attributes) as [attr, a] (attr)}
		{@const set = attr in values}
		{#if set}
			<div class="row nodrag" data-testid={`state-value-${attr}`}>
				<span class="label" title={attr}>{attr}</span>
				<div class="widget">
					{#if a.control}
						<ControlWidget control={a.control} value={values[attr]} label={attr}
							onChange={(v) => edit({ [attr]: v })}
							onInput={(v) => g.previewState(machine, id, { values: { [attr]: v } })} />
					{:else}
						<TextInput inputmode="search" value={JSON.stringify(values[attr])} aria-label={attr} onChange={(raw) => edit({ [attr]: parse(raw) })} />
					{/if}
				</div>
				<IconButton variant="ghost" size="sm" label={`Clear ${attr}`} title="Clear: a playhead entering keeps what it holds" data-testid="state-clear" onclick={() => edit({ [attr]: null })}><Icon name="minus" /></IconButton>
			</div>
		{:else}
			<button type="button" class="row unset nodrag" data-testid={`state-unset-${attr}`} title="Tap to set it here" onclick={() => edit({ [attr]: a.default })}>
				<span class="label">{attr}</span>
			</button>
		{/if}
	{/each}
</div>

<style>
	.card {
		position: relative;
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		padding: 0 var(--space-2) var(--space-2);
		background: var(--surface-1);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-md);
		box-sizing: border-box;
		font-size: var(--fs-small);
	}
	:global(.svelte-flow__node.selected) .card {
		outline: var(--focus-width) solid var(--accent);
	}
	/* Where resting dots sit: the overlay draws them at DOCK, so the strip only reserves the room. */
	.dock {
		height: 22px;
	}
	.head {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--hit);
	}
	.name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
		font-weight: 600;
	}
	.grow {
		flex: 1;
		min-width: 0;
	}
	.row {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--hit);
		padding: 0 var(--space-1);
		border-radius: var(--radius-sm);
		background: var(--surface-2);
		--number-width: 100%;
	}
	.row.unset {
		width: 100%;
		border: 1px dashed var(--border);
		background: transparent;
		color: var(--text-muted);
		cursor: pointer;
		text-align: left;
	}
	.row.unset:hover {
		border-color: var(--accent);
	}
	.label {
		flex: 0 0 auto;
		max-width: 40%;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
		color: var(--text-muted);
	}
	.widget {
		flex: 1;
		min-width: 0;
		display: flex;
		align-items: center;
	}
	/* Above the card's own controls, so a drag that starts on a port is a connection. */
	.card :global(.port) {
		width: 12px;
		height: 12px;
		z-index: 3;
		background: var(--accent);
		border: 2px solid var(--surface-1);
	}
	/* A finger needs a --hit port; the head leaves it the corner it covers. */
	@media (hover: none) and (pointer: coarse) {
		.card :global(.port) {
			width: var(--hit);
			height: var(--hit);
			border: none;
			background: var(--ring-accent);
		}
		.head {
			margin-right: var(--space-6);
		}
	}
</style>
