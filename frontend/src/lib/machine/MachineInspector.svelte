<!-- What the machine panel's side pane shows: the selected state (its name, its values, the
     transitions in and out of it), the selected transition, or — with nothing selected — the
     machine itself: its name, its seed, its attributes and its playheads. Laid out as the node
     inspector lays out a node: Field rows, slot blocks for the lists, a rule between sections. -->
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
	import { Button, Chip, ColorPicker, Disclosure, Field, Icon, IconButton, NumberInput, ScrollArea, Select, TextInput, Toggle } from '$lib/ui';
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
	const rename = (op: MachineOp, from: string, to: string): void => call(op, { name: from, to });

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
	const arrow = (t: Machine['transitions'][string]): string => `${t.from === '*' ? 'any' : t.from} → ${t.to}`;
</script>

{#snippet sectionHead(title: string, add?: { label: string; testid: string; disabled?: boolean; title?: string; onclick: () => void })}
	<div class="section-head">
		<span class="section-name">{title}</span>
		{#if add}
			<IconButton variant="ghost" size="sm" label={add.label} title={add.title ?? add.label} disabled={add.disabled} data-testid={add.testid} onclick={add.onclick}><Icon name="plus" /></IconButton>
		{/if}
	</div>
{/snippet}

{#snippet transitionList(title: string, list: [string, Machine['transitions'][string]][], testid: string)}
	<hr class="rule" />
	<section class="section" data-testid={testid}>
		{@render sectionHead(title)}
		{#if list.length === 0}
			<div class="empty">None</div>
		{/if}
		{#each list as [id, t] (id)}
			<Disclosure class="transition-row" data-testid={`transition-row-${id}`}>
				{#snippet summary()}
					<span class="arrow">{arrow(t)}</span>
					<span class="when">{triggerSummary(t)}</span>
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
			<div class="rows">
				<section class="section" data-testid="state-values">
					{@render sectionHead('values')}
					{#if attrs.length === 0}
						<div class="empty">No attributes yet. Select nothing to add them to the machine.</div>
					{/if}
					{#each attrs as [attr, a] (attr)}
						{#if attr in values}
							<div class="value" data-testid={`state-value-${attr}`}>
								<ParamField paramName={attr} descriptor={descriptorFor(a, values[attr])} literal viewKey={`${name}/${id}/${attr}`}
									onCommit={(v) => editState({ [attr]: literalOf(v) })}
									onPreview={(v) => g.previewState(name, id, { values: { [attr]: v } })} />
								<IconButton variant="ghost" size="sm" label={`Clear ${attr}`} title="Leave it unset: a playhead entering keeps what it holds" data-testid="state-clear" onclick={() => editState({ [attr]: null })}><Icon name="minus" /></IconButton>
							</div>
						{:else}
							<div class="fields">
								<Field label={attr} row doc="Unset: a playhead entering keeps what it holds">
									<Button variant="ghost" size="sm" data-testid={`state-unset-${attr}`} onclick={() => editState({ [attr]: a.default })}>set here</Button>
								</Field>
							</div>
						{/if}
					{/each}
				</section>
				{@render transitionList('incoming', incoming, 'transitions-in')}
				{@render transitionList('outgoing', outgoing, 'transitions-out')}
			</div>
		</ScrollArea>
	{:else if subject.kind === 'transition' && transition}
		<IdentityBar type={`transition ${subject.id}`} {onClose}>
			{#snippet title()}<span class="mono">{arrow(transition)}</span>{/snippet}
		</IdentityBar>
		<ScrollArea>
			<div class="rows">
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
			<div class="rows">
				<div class="fields">
					<Field label="seed" doc="Starts the random draws, so a run repeats" row>
						<NumberInput value={m.seed ?? 0} min={0} step={1} aria-label="seed" data-testid="machine-seed" onChange={(v) => call('machine edit', { seed: Math.max(0, Math.round(v)) })} />
					</Field>
				</div>

				<hr class="rule" />
				<section class="section" data-testid="machine-attributes">
					{@render sectionHead('attributes', { label: 'Add an attribute', testid: 'machine-add-attribute', onclick: () => call('machine attribute add', { name: fresh('attr', attrs.map(([a]) => a)), value: 0, kind: FACES[0].kind }) })}
					{#if attrs.length === 0}
						<div class="empty">A value every state may set, and every playhead carries as a variable.</div>
					{/if}
					{#each attrs as [attr, a], i (attr)}
						{@const face = faceOf(a)}
						<div class="slot" class:alt={i % 2 === 1} data-testid={`attribute-${attr}`} data-face={face}>
							<div class="slot-head">
								<span class="slot-name"><RenameField value={attr} valid={isValidIdentifier} label="Attribute name" testid="attribute-name" onRename={(to) => rename('machine attribute rename', attr, to)} /></span>
								<Select data-testid="attribute-kind" aria-label="kind" density="chrome" value={face} options={FACES.map((f) => f.id)} onChange={(v) => setFace(attr, v)} />
								<IconButton variant="ghost" size="sm" label={`Remove ${attr}`} title="Remove the attribute from every state and playhead" data-testid="attribute-remove" onclick={() => call('machine attribute remove', { name: attr })}><Icon name="x" /></IconButton>
							</div>
							{#if a.kind.type === 'num' && !a.kind.color}
								<div class="fields">
									<Field label="range" doc="The slider's bounds; a typed value may pass them" row>
										<NumberInput value={a.kind.vmin} aria-label="min" onChange={(v) => editKind(attr, a, { vmin: v })} />
										<NumberInput value={a.kind.vmax} aria-label="max" onChange={(v) => editKind(attr, a, { vmax: v })} />
									</Field>
									<Field label="integer" row>
										<Toggle value={a.kind.int} aria-label="integer" onChange={(v) => editKind(attr, a, { int: v })} />
									</Field>
								</div>
							{:else if a.kind.type === 'string' && a.kind.options}
								<div class="fields">
									<Field label="options" doc="The choices, comma-separated" row>
										<TextInput inputmode="search" value={a.kind.options.join(', ')} aria-label="options" onChange={(raw) => setOptions(attr, a, raw)} />
									</Field>
								</div>
							{/if}
							<ParamField paramName={attr} label="default" descriptor={descriptorFor(a, a.default)} literal viewKey={`${name}/${attr}`}
								onCommit={(v) => call('machine attribute edit', { name: attr, value: literalOf(v) })} />
						</div>
					{/each}
				</section>

				<hr class="rule" />
				<section class="section" data-testid="machine-playheads">
					{@render sectionHead('playheads', { label: 'Add a playhead', title: states.length === 0 ? 'Add a state first' : 'Add a playhead in the first state', testid: 'machine-add-playhead', disabled: states.length === 0, onclick: () => call('machine playhead add', { start: states[0], color: dotColor('', playheads.length) }) })}
					{#if playheads.length === 0}
						<div class="empty">A playhead travels the states; its values are the variables a param reads.</div>
					{/if}
					{#each playheads as p, i (p)}
						{@const ph = m.playheads[p]}
						<div class="slot" class:alt={i % 2 === 1} data-testid={`playhead-${p}`}>
							<div class="slot-head">
								<span class="swatch"><ColorPicker value={hexToRgba(dotColor(ph.color, i))} aria-label={`${p} colour`} onChange={(rgba) => call('machine playhead edit', { name: p, color: rgbaToHex(rgba) })} /></span>
								<span class="slot-name"><RenameField value={p} valid={isValidIdentifier} label="Playhead name" testid="playhead-name" onRename={(to) => rename('machine playhead rename', p, to)} /></span>
								<IconButton variant="ghost" size="sm" label={`Remove ${p}`} title="Remove the playhead and its variables" data-testid="playhead-remove" onclick={() => call('machine playhead remove', { name: p })}><Icon name="x" /></IconButton>
							</div>
							<div class="fields">
								<Field label="start" doc="Where a reset puts it" row>
									<Select data-testid="playhead-start" aria-label="start state" value={ph.start} options={states} onChange={(v) => call('machine playhead edit', { name: p, start: v })} />
								</Field>
								<Field label="now" row>
									<span class="mono" data-testid="playhead-state">{String(variableValue(`${p}.state`) ?? ph.start)}</span>
								</Field>
							</div>
							{#if attrs.length > 0}
								<div class="chips">
									{#each attrs as [attr] (attr)}
										<Chip
											density="chrome"
											data-testid={`playhead-chip-${p}-${attr}`}
											title={`variables.${p}.${attr} — drag onto a param to read it there`}
											onpointerdown={(e) => lift.start(e, `${p}.${attr}`)}
											onpointermove={lift.move}
											onpointerup={(e) => lift.drop(e)}
											onpointercancel={lift.cancel}
											onlostpointercapture={lift.cancel}>{attr} <b>{shown(variableValue(`${p}.${attr}`))}</b></Chip
										>
									{/each}
								</div>
							{/if}
						</div>
					{/each}
				</section>

				<hr class="rule" />
				<div class="actions">
					<Button variant="danger" size="sm" data-testid="machine-remove" onclick={() => call('machine remove', {})}>delete machine</Button>
				</div>
			</div>
		</ScrollArea>
	{/if}
	<VariableGhost {lift} />
</div>

<style>
	.inspector {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
	}
	/* The body, as the node inspector's rows: the pane's own ground, one padding, rows in a column. */
	.rows {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		padding: var(--space-6);
		background: var(--surface-1);
	}
	.section {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
	}
	/* `:global`, so the TransitionForm inside wears the same rows, slots and heads: it is drawn
	   nowhere else. Field rows sit on a grid, so the labels of one block share a column. */
	.rows :global(.fields) {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		column-gap: var(--space-5);
		row-gap: var(--space-3);
		--number-width: 100%;
	}
	.rows :global(.section-head) {
		display: flex;
		align-items: center;
		justify-content: space-between;
		min-height: var(--chrome-control-h);
	}
	.rows :global(.section-name) {
		color: var(--text-muted);
		font-size: var(--fs-small);
	}
	.rows :global(.rule) {
		margin: var(--space-3) 0;
		border: none;
		border-top: 1px solid var(--border);
	}
	.rows :global(.empty) {
		color: var(--text-muted);
		font-size: var(--fs-small);
	}
	/* An attribute, a playhead or a trigger is a slot of its list, on the surface steps above the
	   body's own, alternating, so each reads as one. */
	.rows :global(.slot) {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: var(--space-2) var(--space-4) var(--space-3);
		border-radius: var(--radius-sm);
		background: var(--surface-2);
	}
	.rows :global(.slot.alt) {
		background: var(--surface-3);
	}
	.rows :global(.slot-head) {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		min-height: var(--hit);
	}
	.rows :global(.slack) {
		flex: 1 1 auto;
	}
	.rows :global(.actions) {
		display: flex;
		justify-content: flex-end;
		gap: var(--space-3);
	}
	.slot-name {
		flex: 1 1 auto;
		min-width: 0;
		font-weight: 600;
	}
	.swatch {
		display: inline-flex;
		flex: 0 0 auto;
		width: var(--chrome-control-h);
	}
	/* A set value is a param row, with the clear beside it; the row's own negative margin is undone. */
	.value {
		display: flex;
		align-items: flex-start;
		gap: var(--space-2);
	}
	.value > :global(.pf-param) {
		flex: 1 1 auto;
		min-width: 0;
		margin-inline: 0;
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
		gap: var(--space-2);
		touch-action: none;
	}
	.chips b {
		font-weight: 400;
		font-family: var(--font-mono);
		color: var(--text-muted);
	}
	.rows :global(.transition-row) {
		--disclosure-surface: var(--surface-2);
		--disclosure-hover: var(--surface-3);
	}
	.arrow {
		font-family: var(--font-mono);
		font-weight: 600;
	}
	.when {
		margin-inline-start: var(--space-3);
		color: var(--text-muted);
		font-weight: 400;
	}
</style>
