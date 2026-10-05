<!-- What the machine panel's side pane shows: the selected state (its name, its values, the
     transitions in and out of it), the selected transition, or — with nothing selected — the
     machine itself: its name, its seed, its attributes and its playheads. An attribute is drawn as a
     node's param is, with the widget of its kind. -->
<script lang="ts">
	import type { Attribute, Literal, Machine } from '$lib/api/generated';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import IdentityBar from '$lib/inspector/IdentityBar.svelte';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import RenameField from '$lib/inspector/RenameField.svelte';
	import VariableGhost from '$lib/panels/VariableGhost.svelte';
	import { createVariableLift } from '$lib/panels/variableLift.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { Button, Chip, ColorPicker, Disclosure, Icon, IconButton, NumberInput, ScrollArea, Select, TextInput, Toggle } from '$lib/ui';
	import { FACES, descriptorFor, faceOf, literalOf, type Face } from './attributes';
	import { dotColor, hexToRgba, rgbaToHex } from './layout';
	import type { Subject } from './subject';
	import TransitionForm from './TransitionForm.svelte';
	import { summary as triggerSummary } from './triggers';

	let {
		name,
		m,
		subject,
		onFire,
		onClose
	}: {
		name: string;
		m: Machine;
		subject: Subject;
		onFire: (id: string) => void;
		onClose: () => void;
	} = $props();

	const g = graph();
	const states = $derived(Object.keys(m.states));
	const playheads = $derived(Object.keys(m.playheads));
	const attrs = $derived(Object.entries(m.attributes));
	watchVariables(() => playheads.flatMap((p) => [`${p}.state`, ...attrs.map(([a]) => `${p}.${a}`)]));
	const lift = createVariableLift();

	function call(op: MachineOp, payload: Record<string, unknown>): void {
		void g.machine(op, { machine: name, ...payload }).catch((e) => notify().failure(op, e));
	}
	function rename(op: MachineOp, from: string, to: string): void {
		if (to !== from && isValidIdentifier(to)) call(op, { name: from, to });
	}

	// ---- the machine: its attributes and playheads.
	const fresh = (stem: string, taken: string[]): string => {
		let i = 0;
		while (taken.includes(`${stem}${i}`)) i += 1;
		return `${stem}${i}`;
	};
	/** A new face starts the default over, so the widget can always draw what it is handed. */
	function setFace(attr: string, face: string): void {
		const f = FACES.find((f) => f.id === (face as Face))!;
		call('machine attribute edit', { name: attr, kind: f.kind, value: f.born });
	}
	function editKind(attr: string, a: Attribute, patch: Record<string, unknown>): void {
		call('machine attribute edit', { name: attr, kind: { ...a.kind, ...patch } });
	}
	/** The options typed as one line; a default they no longer hold moves to the first. */
	function setOptions(attr: string, a: Attribute, raw: string): void {
		const options = raw.split(',').map((o) => o.trim()).filter(Boolean);
		if (options.length === 0) return;
		const value = options.includes(String(a.default)) ? {} : { value: options[0] };
		call('machine attribute edit', { name: attr, kind: { type: 'string', options }, ...value });
	}
	function shown(v: Literal | null): string {
		return v === null ? '…' : typeof v === 'number' ? String(Math.round(v * 1000) / 1000) : typeof v === 'string' ? v : JSON.stringify(v);
	}

	// ---- the state: its values, and the transitions into and out of it.
	const state = $derived(subject.kind === 'state' ? m.states[subject.id] : undefined);
	const values = $derived(state?.values ?? {});
	const editState = (patch: Record<string, Literal | null>): void => {
		if (subject.kind === 'state') call('machine state edit', { name: subject.id, values: patch });
	};
	const transitions = $derived(Object.entries(m.transitions));
	const incoming = $derived(subject.kind === 'state' ? transitions.filter(([, t]) => t.to === subject.id) : []);
	const outgoing = $derived(subject.kind === 'state' ? transitions.filter(([, t]) => t.from === subject.id || t.from === '*') : []);
	const transition = $derived(subject.kind === 'transition' ? m.transitions[subject.id] : undefined);
</script>

{#snippet transitionList(title: string, list: [string, Machine['transitions'][string]][], testid: string)}
	<section class="block" data-testid={testid}>
		<h3>{title}</h3>
		{#if list.length === 0}
			<span class="muted">none</span>
		{/if}
		{#each list as [id, t] (id)}
			<Disclosure class="transition-row" data-testid={`transition-row-${id}`}>
				{#snippet summary()}
					<span class="tsum"><span class="mono">{t.from === '*' ? 'any' : t.from} → {t.to}</span><span class="muted">{triggerSummary(t)}</span></span>
				{/snippet}
				{#snippet children()}
					<TransitionForm {name} {m} {id} {onFire} />
				{/snippet}
			</Disclosure>
		{/each}
	</section>
{/snippet}

<div class="inspector" data-testid="machine-inspector" data-subject={subject.kind}>
	{#if subject.kind === 'state' && state}
		{@const id = subject.id}
		{#key id}
			<IdentityBar type="state" {onClose}>
				{#snippet title()}
					<RenameField value={id} valid={isValidIdentifier} label="State name" testid="state-name" onRename={(to) => rename('machine state rename', id, to)} />
				{/snippet}
			</IdentityBar>
		{/key}
		<ScrollArea>
			<div class="body">
				<section class="block" data-testid="state-values">
					<h3>values</h3>
					{#if attrs.length === 0}
						<span class="muted">No attributes yet; add them with nothing selected.</span>
					{/if}
					{#each attrs as [attr, a] (attr)}
						{#if attr in values}
							<div class="value" data-testid={`state-value-${attr}`}>
								<ParamField paramName={attr} descriptor={descriptorFor(a, values[attr])} literal viewKey={`${name}/${id}/${attr}`}
									onCommit={(v) => editState({ [attr]: literalOf(v) })}
									onPreview={(v) => g.previewState(name, id, { values: { [attr]: v } })} />
								<IconButton variant="ghost" size="sm" label={`Clear ${attr}`} title="Clear: a playhead entering keeps what it holds" data-testid="state-clear" onclick={() => editState({ [attr]: null })}><Icon name="minus" /></IconButton>
							</div>
						{:else}
							<button type="button" class="row unset" data-testid={`state-unset-${attr}`} title="Set it in this state" onclick={() => editState({ [attr]: a.default })}>
								<span class="label">{attr}</span>
								<span class="muted">not set</span>
							</button>
						{/if}
					{/each}
				</section>
				{@render transitionList('incoming', incoming, 'transitions-in')}
				{@render transitionList('outgoing', outgoing, 'transitions-out')}
			</div>
		</ScrollArea>
	{:else if subject.kind === 'transition' && transition}
		<IdentityBar type={`transition ${subject.id}`} {onClose}>
			{#snippet title()}
				<span class="mono">{transition.from === '*' ? 'any' : transition.from} → {transition.to}</span>
			{/snippet}
		</IdentityBar>
		<ScrollArea>
			<div class="body">
				<TransitionForm {name} {m} id={subject.id} {onFire} />
			</div>
		</ScrollArea>
	{:else}
		{#key name}
			<IdentityBar type="state machine" {onClose}>
				{#snippet title()}
					<RenameField value={name} valid={isValidIdentifier} label="Machine name" testid="machine-name" onRename={(to) => call('machine rename', { to })} />
				{/snippet}
			</IdentityBar>
		{/key}
		<ScrollArea>
			<div class="body">
				<section class="block">
					<div class="row">
						<span class="muted" title="Starts the random draws, so a run repeats">seed</span>
						<NumberInput value={m.seed ?? 0} min={0} step={1} aria-label="seed" data-testid="machine-seed" onChange={(v) => call('machine edit', { seed: Math.max(0, Math.round(v)) })} />
						<Chip tone="danger" data-testid="machine-remove" onclick={() => call('machine remove', {})}>delete machine</Chip>
					</div>
				</section>

				<section class="block" data-testid="machine-attributes">
					<h3>attributes</h3>
					{#each attrs as [attr, a] (attr)}
						{@const face = faceOf(a)}
						<div class="attr" data-testid={`attribute-${attr}`} data-face={face}>
							<div class="row">
								<div class="grow">
									<TextInput inputmode="search" data-testid="attribute-name" value={attr} autocomplete="off" aria-label="Attribute name" onChange={(v) => rename('machine attribute rename', attr, v.trim())} />
								</div>
								<Select data-testid="attribute-kind" aria-label="kind" value={face} options={FACES.map((f) => f.id)} onChange={(v) => setFace(attr, v)} />
								<IconButton variant="ghost" size="sm" label={`Remove ${attr}`} title="Remove the attribute" data-testid="attribute-remove" onclick={() => call('machine attribute remove', { name: attr })}><Icon name="x" /></IconButton>
							</div>
							{#if a.kind.type === 'num' && !a.kind.color}
								<div class="row">
									<span class="muted">min</span>
									<NumberInput value={a.kind.vmin} aria-label="min" onChange={(v) => editKind(attr, a, { vmin: v })} />
									<span class="muted">max</span>
									<NumberInput value={a.kind.vmax} aria-label="max" onChange={(v) => editKind(attr, a, { vmax: v })} />
									<span class="muted">int</span>
									<Toggle value={a.kind.int} aria-label="integer" onChange={(v) => editKind(attr, a, { int: v })} />
								</div>
							{:else if a.kind.type === 'string' && a.kind.options}
								<div class="row">
									<span class="muted">options</span>
									<div class="grow">
										<TextInput inputmode="search" value={a.kind.options.join(', ')} aria-label="options" title="The choices, comma-separated" onChange={(raw) => setOptions(attr, a, raw)} />
									</div>
								</div>
							{/if}
							<ParamField paramName={attr} label="default" descriptor={descriptorFor(a, a.default)} literal viewKey={`${name}/${attr}`}
								onCommit={(v) => call('machine attribute edit', { name: attr, value: literalOf(v) })} />
						</div>
					{/each}
					<Button class="add-row" variant="ghost" size="sm" data-testid="machine-add-attribute"
						onclick={() => call('machine attribute add', { name: fresh('attr', attrs.map(([a]) => a)), value: 0, kind: FACES[0].kind })}><Icon name="plus" />attribute</Button>
				</section>

				<section class="block" data-testid="machine-playheads">
					<h3>playheads</h3>
					{#each playheads as p, i (p)}
						{@const ph = m.playheads[p]}
						<div class="attr" data-testid={`playhead-${p}`}>
							<div class="row">
								<ColorPicker value={hexToRgba(dotColor(ph.color, i))} aria-label={`${p} colour`} onChange={(rgba) => call('machine playhead edit', { name: p, color: rgbaToHex(rgba) })} />
								<div class="grow">
									<TextInput inputmode="search" data-testid="playhead-name" value={p} autocomplete="off" aria-label="Playhead name" onChange={(v) => rename('machine playhead rename', p, v.trim())} />
								</div>
								<IconButton variant="ghost" size="sm" label={`Remove ${p}`} title="Remove the playhead and its variables" data-testid="playhead-remove" onclick={() => call('machine playhead remove', { name: p })}><Icon name="x" /></IconButton>
							</div>
							<div class="row">
								<span class="muted">start</span>
								<Select data-testid="playhead-start" aria-label="start state" value={ph.start} options={states} onChange={(v) => call('machine playhead edit', { name: p, start: v })} />
								<span class="muted">in</span>
								<span class="mono" data-testid="playhead-state">{String(variableValue(`${p}.state`) ?? ph.start)}</span>
							</div>
							<div class="chips">
								{#each attrs as [attr] (attr)}
									<Chip
										density="chrome"
										data-testid={`playhead-chip-${p}-${attr}`}
										title="Drag onto a parameter to set its expression"
										onpointerdown={(e) => lift.start(e, `${p}.${attr}`)}
										onpointermove={lift.move}
										onpointerup={(e) => lift.drop(e)}
										onpointercancel={lift.cancel}
										onlostpointercapture={lift.cancel}>{attr} <b>{shown(variableValue(`${p}.${attr}`))}</b></Chip
									>
								{/each}
							</div>
						</div>
					{/each}
					<Button class="add-row" variant="ghost" size="sm" data-testid="machine-add-playhead" disabled={states.length === 0}
						title={states.length === 0 ? 'Add a state first' : 'Add a playhead in the first state'}
						onclick={() => call('machine playhead add', { start: states[0], color: dotColor('', playheads.length) })}><Icon name="plus" />playhead</Button>
				</section>
			</div>
		</ScrollArea>
	{/if}
	<VariableGhost {lift} />
</div>

<style>
	.inspector {
		display: flex;
		flex-direction: column;
		min-height: 0;
		flex: 1;
	}
	.body {
		display: flex;
		flex-direction: column;
		gap: var(--space-4);
		padding: var(--space-3);
	}
	.block {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}
	h3 {
		margin: 0;
		font-size: var(--fs-small);
		font-weight: 600;
		color: var(--text-muted);
	}
	/* `:global`, so the TransitionForm inside wears the same rows. */
	.inspector :global(.row) {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-width: 0;
		--number-width: 100%;
	}
	.inspector :global(.grow) {
		flex: 1;
		min-width: 0;
	}
	.attr {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		padding: var(--space-2);
		border-radius: var(--radius-sm);
		background: var(--surface-2);
	}
	.value {
		display: flex;
		align-items: flex-start;
		gap: var(--space-1);
	}
	.value > :global(.pf-param) {
		flex: 1;
		min-width: 0;
	}
	.row.unset {
		width: 100%;
		min-height: var(--hit);
		padding: 0 var(--space-1);
		border-radius: var(--radius-sm);
		border: 1px dashed var(--border);
		background: transparent;
		color: var(--text-muted);
		cursor: pointer;
		text-align: left;
		font: inherit;
	}
	.row.unset:hover {
		border-color: var(--accent);
	}
	.label {
		font-family: var(--font-mono);
		color: var(--text-muted);
	}
	.inspector :global(.muted) {
		color: var(--text-muted);
		font-size: var(--fs-small);
		white-space: nowrap;
	}
	.mono {
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-1);
		touch-action: none;
	}
	.chips b {
		font-weight: 400;
		color: var(--text-muted);
	}
	.body :global(.add-row) {
		width: 100%;
		justify-content: center;
	}
	.body :global(.transition-row) {
		--disclosure-surface: var(--surface-2);
	}
	.tsum {
		display: flex;
		gap: var(--space-3);
		min-width: 0;
	}
</style>
