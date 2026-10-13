<!-- A state's settings menu: a search over the machine's attributes, each drawn as its param — the
     value this state sets, or the default, dimmed, which a turn sets here — with a clear on a set one;
     a name no attribute has yet makes one. Below, the playheads that start here. What an attribute
     is, and several states at once, are the inspector's. -->
<script lang="ts">
	import type { Machine } from '$lib/api/generated';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import RenameField from '$lib/inspector/RenameField.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { history } from '$lib/stores/history.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { Button, ColorPicker, Icon, IconButton, Select, SettingsMenu } from '$lib/ui';
	import { FACES, descriptorFor, literalOf } from './attributes';
	import { dotColor, hexToRgba, rgbaToHex } from './layout';

	let { name, m, id }: { name: string; m: Machine; id: string } = $props();

	const g = graph();
	let query = $state('');
	let list = $state<HTMLDivElement | null>(null);

	function call(op: MachineOp, payload: Record<string, unknown>): void {
		void g.machine(op, { machine: name, ...payload }).catch((e) => notify().failure(op, e));
	}
	const values = $derived(m.states[id]?.values ?? {});
	const matches = (attr: string): boolean => attr.toLowerCase().includes(query.trim().toLowerCase());
	const attrs = $derived(Object.entries(m.attributes).filter(([a]) => matches(a)));
	const fresh = $derived(query.trim());
	const creatable = $derived(isValidIdentifier(fresh) && !(fresh in m.attributes));
	const playheads = $derived(Object.keys(m.playheads));
	const here = $derived(playheads.filter((p) => m.playheads[p].start === id));
	watchVariables(() => here.map((p) => `${p}.state`));

	const editState = (patch: Record<string, unknown>): void => call('machine state edit', { name: id, values: patch });
	/** A new attribute, set here, as ONE step. */
	async function create(attr: string): Promise<void> {
		try {
			await history().transaction(`Add attribute ${attr}`, async (step) => {
				await g.machine('machine attribute add', { machine: name, name: attr, value: 0, kind: FACES[0].kind }, step);
				await g.machine('machine state edit', { machine: name, name: id, values: { [attr]: 0 } }, step);
			});
			query = '';
		} catch (e) {
			notify().failure('Add attribute', e);
		}
	}
	/** Enter: a new name makes the attribute; else the first match takes the keys. */
	function enter(): void {
		if (creatable) void create(fresh);
		else list?.querySelector<HTMLElement>('[data-menu-row] input, [data-menu-row] select, [data-menu-row] button')?.focus();
	}
</script>

<SettingsMenu label="Attributes and playheads" icon="sliders-horizontal" testid={`state-menu-${id}`} class="state-menu-button" width={300}
	bind:query placeholder="Search or name an attribute…" onEnter={enter}>
	<div class="menu" data-testid="state-menu" bind:this={list}>
		{#each attrs as [attr, a] (attr)}
			{@const set = attr in values}
			<div class="row" class:unset={!set} data-menu-row data-testid={`state-value-${attr}`} data-set={set}>
				<ParamField paramName={attr} descriptor={descriptorFor(a, set ? values[attr] : a.default)} literal viewKey={`${name}/${id}/${attr}`}
					onCommit={(v) => editState({ [attr]: literalOf(v) })}
					onPreview={(v) => g.previewState(name, id, { values: { [attr]: v } })} />
				{#if set}
					<IconButton variant="ghost" size="sm" label={`Unset ${attr}`} title="Unset: a playhead entering keeps what it holds" data-testid="state-clear" onclick={() => editState({ [attr]: null })}><Icon name="minus" /></IconButton>
				{/if}
			</div>
		{:else}
			{#if !creatable}
				<div class="empty">{query ? 'No attribute matches; a name makes one.' : 'Type a name to make an attribute.'}</div>
			{/if}
		{/each}
		{#if creatable}
			<div class="row" data-menu-row>
				<Button variant="ghost" size="sm" data-testid="attribute-create" onclick={() => void create(fresh)}><Icon name="plus" />new attribute <b>{fresh}</b></Button>
			</div>
		{/if}

		<div class="head split">
			<span>playheads starting here</span>
			<IconButton variant="ghost" size="sm" label="Add a playhead here" title="Add a playhead that starts in this state" data-testid="machine-add-playhead"
				onclick={() => call('machine playhead add', { start: id, color: dotColor('', playheads.length) })}><Icon name="plus" /></IconButton>
		</div>
		{#each here as p (p)}
			{@const ph = m.playheads[p]}
			<div class="row" data-menu-row data-testid={`playhead-${p}`}>
				<span class="swatch"><ColorPicker value={hexToRgba(dotColor(ph.color, playheads.indexOf(p)))} aria-label={`${p} colour`} onChange={(rgba) => call('machine playhead edit', { name: p, color: rgbaToHex(rgba) })} /></span>
				<span class="label"><RenameField value={p} valid={isValidIdentifier} label="Playhead name" testid="playhead-name" onRename={(to) => call('machine playhead rename', { name: p, to })} /></span>
				<span class="now mono" title="Where it is now" data-testid="playhead-state">{String(variableValue(`${p}.state`) ?? ph.start)}</span>
				<Select data-testid="playhead-start" aria-label="start state" density="chrome" value={ph.start} options={Object.keys(m.states)} onChange={(v) => call('machine playhead edit', { name: p, start: v })} />
				<IconButton variant="ghost" size="sm" label={`Remove ${p}`} title="Remove the playhead and its variables" data-testid="playhead-remove" onclick={() => call('machine playhead remove', { name: p })}><Icon name="x" /></IconButton>
			</div>
		{:else}
			<div class="empty">A playhead travels the states; its values are the variables a param reads.</div>
		{/each}
	</div>
</SettingsMenu>

<style>
	.menu {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: var(--space-3) var(--space-4) var(--space-4);
		font-size: var(--fs-small);
	}
	.head {
		margin-top: var(--space-3);
		color: var(--text-muted);
		font-size: var(--fs-micro);
	}
	.head.split {
		display: flex;
		align-items: center;
		justify-content: space-between;
		border-top: 1px solid var(--border);
		padding-top: var(--space-3);
	}
	.row {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--chrome-control-h);
	}
	.row > :global(.pf-param) {
		flex: 1 1 auto;
		min-width: 0;
		margin-inline: 0;
	}
	/* A value this state leaves alone: the default, until a turn sets it here. */
	.row.unset > :global(.pf-param) {
		opacity: 0.55;
	}
	.row.unset:focus-within > :global(.pf-param) {
		opacity: 1;
	}
	.label {
		flex: 1 1 auto;
		min-width: 0;
		font-family: var(--font-mono);
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.swatch {
		display: inline-flex;
		flex: 0 0 auto;
		width: var(--chrome-control-h);
	}
	.now {
		color: var(--text-muted);
	}
	.mono {
		font-family: var(--font-mono);
	}
	.empty {
		color: var(--text-muted);
	}
</style>
