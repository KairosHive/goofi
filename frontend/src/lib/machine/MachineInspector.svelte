<!-- What the machine panel's side pane shows: the selected states. One state is edited whole —
     its name, how it decides, every attribute it sets and the envelope each lands by, the playheads
     that start in it, and the transitions in and out of it, each row opening on its own settings
     and each row out carrying its share of the state's draw. Several states show what they share,
     and an edit writes to each. Laid out as the node inspector lays out a node. -->
<script lang="ts">
	import type { Attribute, Decision, Machine } from '$lib/api/generated';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import IdentityBar from '$lib/inspector/IdentityBar.svelte';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import RenameField from '$lib/inspector/RenameField.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { history } from '$lib/stores/history.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { ColorPicker, Disclosure, ENVELOPE_BORN, Field, Icon, IconButton, ScrollArea, Segmented, Select, Slider, TextInput } from '$lib/ui';
	import AttributeForm from './AttributeForm.svelte';
	import { FACES, descriptorFor, envelopeDescriptor, literalOf, numberDescriptor } from './attributes';
	import { editEach, held, mixed } from './batch';
	import { DECISIONS, outgoing as outgoingOf, relink, shares, weightsOf } from './decision';
	import { dotColor, hexToRgba, rgbaToHex } from './layout';
	import type { Subject } from './subject';
	import TransitionForm from './TransitionForm.svelte';
	import { summary as triggerSummary } from './triggers';

	let { name, m, subject, onClose }: { name: string; m: Machine; subject: Subject; onClose: () => void } = $props();

	const g = graph();
	function call(op: MachineOp, payload: Record<string, unknown>): void {
		void g.machine(op, { machine: name, ...payload }).catch((e) => notify().failure(op, e));
	}

	const ids = $derived(subject.ids.filter((id) => id in m.states));
	const one = $derived(ids.length === 1 ? ids[0] : null);
	const s = $derived(one ? m.states[one] : null);
	const transitions = $derived(Object.entries(m.transitions));
	const incoming = $derived(one ? transitions.filter(([, t]) => t.to === one) : []);
	const outgoing = $derived(one ? transitions.filter(([, t]) => t.from === one || t.from === '*') : []);
	const arrow = (t: Machine['transitions'][string]): string => `${t.from === '*' ? 'any' : t.from} → ${t.to}`;

	// ---- the selected states, edited as one.
	const states = $derived(ids.map((id) => m.states[id]));
	const editStates = (patch: Record<string, unknown>): void => void editEach(name, 'machine state edit', 'name', ids, patch);
	const preview = (patch: Parameters<typeof g.previewState>[2]): void => ids.forEach((id) => g.previewState(name, id, patch));
	const tag = (label: string, values: unknown[]): string => (mixed(values) ? `${label} (mixed)` : label);
	const SEGMENTS = DECISIONS.map((d) => ({ id: d.id, label: d.label, title: d.title, testid: `state-mode-${d.id}` }));

	// ---- one state's draw: a dragged share moves the others in their own ratio, and the release
	// lands all of them as one edit. The drag reads from the shares it started on, so nothing drifts.
	const out = $derived(one ? outgoingOf(m, one) : []);
	let draft = $state<{ base: number[]; now: number[] } | null>(null);
	const committed = $derived(s ? shares(s, out) : [1]);
	const shown = $derived(draft?.now ?? committed);
	function drag(k: number, v: number): void {
		const base = draft?.base ?? committed;
		draft = { base, now: relink(base, k, v) };
		preview(weightsOf(draft.now, out));
	}
	function land(k: number, v: number): void {
		const p = relink(draft?.base ?? committed, k, v);
		draft = null;
		editStates(weightsOf(p, out));
	}
	const percent = (p: number): string => `${Math.round(p * 100)}%`;

	// ---- the attributes: a name typed makes one, set in every selected state, as ONE step; a set
	// number may play an envelope on arrival, drawn under its value.
	let fresh = $state('');
	const creatable = $derived(isValidIdentifier(fresh.trim()) && !(fresh.trim() in m.attributes));
	async function create(): Promise<void> {
		const attr = fresh.trim();
		if (!creatable) return;
		try {
			await history().transaction(`Add attribute ${attr}`, async (step) => {
				await g.machine('machine attribute add', { machine: name, name: attr, value: FACES[0].born, kind: FACES[0].kind }, step);
				for (const id of ids) await g.machine('machine state edit', { machine: name, name: id, values: { [attr]: FACES[0].born } }, step);
			});
			fresh = '';
		} catch (e) {
			notify().failure('Add attribute', e);
		}
	}
	let defining = $state<string | null>(null);
	const shapable = (a: Attribute): boolean => a.kind.type === 'num';
	/** The envelope the selected states give `attr`: the first's, or none. */
	const envelopeOf = (attr: string): number[] | null => ids.map((id) => m.states[id]?.envelopes?.[attr]).find((e) => e !== undefined) ?? null;
	/** The longest time an envelope's knobs reach, in seconds. */
	const LONGEST = 10;

	// ---- the playheads that start in the one state, and where each is now.
	const playheads = $derived(Object.keys(m.playheads));
	const here = $derived(one ? playheads.filter((p) => m.playheads[p].start === one) : []);
	watchVariables(() => here.map((p) => `${p}.state`));

	// ---- a transition out, added here rather than dragged: to a state picked from the list.
	function addTransition(to: string): void {
		if (one && to) void g.machine('machine transition add', { machine: name, from: one, to, triggers: [{ kind: 'manual' }] }).catch((e) => notify().failure('Add transition', e));
	}
</script>

{#snippet transitionList(title: string, list: [string, Machine['transitions'][string]][], testid: string, addable: boolean)}
	<section class="section" data-testid={testid}>
		<div class="section-head">
			<span class="section-name">{title}</span>
			{#if addable}
				<span class="add-to">
					<Select aria-label="Add a transition to" density="chrome" value="" options={['', ...Object.keys(m.states)]} labels={{ '': '+ to…' }}
						data-testid="transition-add-to" onChange={addTransition} />
				</span>
			{/if}
		</div>
		{#if list.length === 0}
			<div class="empty">None</div>
		{/if}
		<!-- A random state's draw: the wait leads the rows out, and each row out carries its share;
		     a drag moves the others in their own ratio, so the shares always sum to one. -->
		{#if addable && s?.mode === 'random' && list.length > 0}
			<div class="line" data-testid="state-share-wait">
				<span class="to muted">wait</span>
				<Slider value={shown[0]} min={0} max={1} step={0.01} bounds={false} aria-label="wait" onInput={(v) => drag(0, v)} onChange={(v) => land(0, v)} />
				<span class="pct">{percent(shown[0])}</span>
			</div>
		{/if}
		{#each list as [id, t] (id)}
			{@const k = out.indexOf(id)}
			<Disclosure class="transition-row" open={subject.open.includes(id)} data-testid={`transition-row-${id}`}>
				{#snippet summary()}
					<span class="arrow">{arrow(t)}</span>
					<span class="when">{triggerSummary(m, id)}</span>
				{/snippet}
				{#snippet aside()}
					{#if addable && s?.mode === 'random' && k >= 0}
						<span class="share" data-testid={`state-share-${id}`}>
							<Slider value={shown[k + 1]} min={0} max={1} step={0.01} bounds={false} aria-label={`share of ${id}`} onInput={(v) => drag(k + 1, v)} onChange={(v) => land(k + 1, v)} />
						</span>
					{/if}
				{/snippet}
				{#snippet children()}
					<TransitionForm {name} {m} {id} />
				{/snippet}
			</Disclosure>
		{/each}
	</section>
{/snippet}

<div class="inspector" data-testid="machine-inspector" data-subject="state" data-count={ids.length} data-open={subject.open.join(' ')}>
	{#if ids.length > 0}
		{#key one}
			<IdentityBar type={one ? 'state' : 'states'} {onClose}>
				{#snippet title()}
					{#if one}
						<RenameField value={one} valid={isValidIdentifier} label="State name" testid="state-name" onRename={(to) => call('machine state rename', { name: one, to })} />
					{:else}
						<span class="mono" data-testid="state-name">{ids.length} states</span>
					{/if}
				{/snippet}
			</IdentityBar>
		{/key}
		<ScrollArea>
			<div class="rows">
				<section class="section" data-testid="state-decision">
					<div class="section-head"><span class="section-name">decision</span></div>
					<div class="fields">
						<Field label={tag('mode', states.map((s) => s.mode))} row>
							<Segmented value={states[0].mode} segments={SEGMENTS} onChange={(mode) => editStates({ mode: mode as Decision })} aria-label="decision" data-testid="state-mode" fill />
						</Field>
					</div>
					<!-- Below 0 the interval is off: -1 on the wire, so the slider's low end lands there. -->
					<ParamField paramName="interval" label={tag('interval', states.map((s) => s.interval))} descriptor={numberDescriptor(states[0].interval, -1, 10, 1)} literal
						viewKey={`${name}/states/interval`} data-testid="state-interval" onPreview={(v) => preview({ interval: Number(v) })} onCommit={(v) => editStates({ interval: Number(v) < 0 ? -1 : Number(v) })} />
					{#if s?.mode === 'expression'}
						<ExprEditor value={s.expression} onCommit={(e) => editStates({ expression: e })} label="decision" placeholder="index, e.g. int(variables.desk.pick)" testid="state-expression" />
					{:else if s}
						<div class="line"><span class="muted">{s.mode === 'random' ? 'a draw by the shares on the transitions out' : 'the transitions out in turn, by their number'}</span></div>
					{/if}
				</section>
				<hr class="rule" />
				<section class="section" data-testid="state-attributes">
					<div class="section-head">
						<span class="section-name">attributes</span>
						<span class="add-attr">
							<TextInput class="add-attr-name" inputmode="search" placeholder="name a new attribute…" aria-label="New attribute" data-testid="attribute-name-new"
								value={fresh} onInput={(v) => (fresh = v)} onChange={(v) => { fresh = v; void create(); }} />
							<IconButton variant="ghost" size="sm" label="Add the attribute" title="Add an attribute of that name, set in the selected states" disabled={!creatable}
								data-testid="attribute-create" onclick={() => void create()}><Icon name="plus" /></IconButton>
						</span>
					</div>
					{#each Object.entries(m.attributes) as [attr, a], k (attr)}
						{@const h = held(m, ids, attr)}
						{@const env = envelopeOf(attr)}
						<div class="slot" class:alt={k % 2 === 1} class:unset={!h.set} data-testid={`state-attribute-${attr}`} data-set={h.set} data-mixed={h.mixed} data-envelope={env !== null}>
							<div class="slot-head">
								<ParamField paramName={attr} label={h.mixed ? `${attr} (mixed)` : attr} descriptor={descriptorFor(a, h.value)} literal viewKey={`${name}/selection/${attr}`}
									onCommit={(v) => editStates({ values: { [attr]: literalOf(v) } })}
									onPreview={(v) => preview({ values: { [attr]: v } })} />
								<!-- The row keeps its buttons set or not, so the value's track never changes width under
								     a pointer; unset, they wait for the value. -->
								{#if shapable(a)}
									<IconButton variant="ghost" size="sm" label={env ? `Land ${attr} plain` : `Shape ${attr}`} aria-pressed={env !== null} data-testid="state-envelope" disabled={!h.set}
										title={env ? 'Remove the envelope: the value lands plain' : 'Add an envelope: on arrival the value plays an attack, decay, sustain and release'}
										onclick={() => editStates({ envelopes: { [attr]: env ? null : ENVELOPE_BORN } })}><Icon name="activity" /></IconButton>
								{/if}
								<IconButton variant="ghost" size="sm" label={`Unset ${attr}`} title="Unset: a playhead entering keeps what it holds" data-testid="state-clear" disabled={!h.set}
									onclick={() => editStates({ values: { [attr]: null } })}><Icon name="minus" /></IconButton>
								<IconButton variant="ghost" size="sm" label={`Define ${attr}`} title="Its name, kind, range and default, for every state" data-testid="attribute-edit"
									aria-expanded={defining === attr} onclick={() => (defining = defining === attr ? null : attr)}><Icon name="pencil" /></IconButton>
							</div>
							{#if env}
								<ParamField paramName={`${attr} envelope`} label="envelope" descriptor={envelopeDescriptor(env, LONGEST)} literal viewKey={`${name}/selection/${attr}/envelope`}
									data-testid="state-attribute-envelope"
									onCommit={(v) => editStates({ envelopes: { [attr]: v } })}
									onPreview={(v) => preview({ envelopes: { [attr]: v as number[] } })} />
							{/if}
							{#if defining === attr}<AttributeForm {name} {attr} {a} />{/if}
						</div>
					{:else}
						<div class="empty">No attributes yet: name one above.</div>
					{/each}
				</section>
				{#if one}
					<hr class="rule" />
					<section class="section" data-testid="state-playheads">
						<div class="section-head">
							<span class="section-name">playheads starting here</span>
							<IconButton variant="ghost" size="sm" label="Add a playhead here" title="Add a playhead that starts in this state" data-testid="machine-add-playhead"
								onclick={() => call('machine playhead add', { start: one, color: dotColor('', playheads.length) })}><Icon name="plus" /></IconButton>
						</div>
						{#each here as p, k (p)}
							{@const ph = m.playheads[p]}
							<div class="slot playhead" class:alt={k % 2 === 1} data-testid={`playhead-${p}`}>
								<span class="swatch"><ColorPicker value={hexToRgba(dotColor(ph.color, playheads.indexOf(p)))} aria-label={`${p} colour`} onChange={(rgba) => call('machine playhead edit', { name: p, color: rgbaToHex(rgba) })} /></span>
								<span class="label"><RenameField value={p} valid={isValidIdentifier} label="Playhead name" testid="playhead-name" onRename={(to) => call('machine playhead rename', { name: p, to })} /></span>
								<span class="now mono" title="Where it is now" data-testid="playhead-state">{String(variableValue(`${p}.state`) ?? ph.start)}</span>
								<Select data-testid="playhead-start" aria-label="start state" density="chrome" value={ph.start} options={Object.keys(m.states)} onChange={(v) => call('machine playhead edit', { name: p, start: v })} />
								<IconButton variant="ghost" size="sm" label={`Remove ${p}`} title="Remove the playhead and its variables" data-testid="playhead-remove" onclick={() => call('machine playhead remove', { name: p })}><Icon name="x" /></IconButton>
							</div>
						{:else}
							<div class="empty">A playhead travels the states; its values are the variables a param reads.</div>
						{/each}
					</section>
					<hr class="rule" />
					{@render transitionList('transitions out', outgoing, 'transitions-out', true)}
					<hr class="rule" />
					{@render transitionList('transitions in', incoming, 'transitions-in', false)}
				{/if}
			</div>
		</ScrollArea>
	{/if}
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
		gap: var(--space-3);
		min-height: var(--chrome-control-h);
	}
	.rows :global(.section-name) {
		color: var(--text-muted);
		font-size: var(--fs-small);
		white-space: nowrap;
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
	.slot-head > :global(.pf-param),
	.slot > :global(.pf-param) {
		flex: 1 1 auto;
		min-width: 0;
		margin-inline: 0;
	}
	/* A value no selected state sets: the default, until a turn sets it in each. */
	.slot.unset > .slot-head > :global(.pf-param) {
		opacity: 0.55;
	}
	.slot.unset:focus-within > .slot-head > :global(.pf-param) {
		opacity: 1;
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
	.mono {
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	/* The decision's line, and the wait that leads the transitions out. */
	.line {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--chrome-control-h);
		min-width: 0;
		font-size: var(--fs-small);
	}
	.to {
		flex: 0 1 auto;
		width: 6em;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
	}
	.share {
		display: inline-flex;
		flex: 1 1 auto;
		min-width: 0;
	}
	.pct {
		flex: 0 0 auto;
		font-family: var(--font-mono);
		font-variant-numeric: tabular-nums;
		color: var(--text-muted);
	}
	.pct {
		width: 2.8em;
		text-align: right;
	}
	.muted {
		color: var(--text-muted);
	}
	.add-attr {
		display: flex;
		align-items: center;
		gap: var(--space-1);
		flex: 1 1 auto;
		min-width: 0;
		justify-content: flex-end;
	}
	.add-attr > :global(.add-attr-name) {
		flex: 1 1 8rem;
		min-width: 0;
	}
	.add-to {
		display: inline-flex;
	}
	/* A playhead's row: swatch, name, where it is, where it starts, and its removal. */
	.rows :global(.slot.playhead) {
		flex-direction: row;
		align-items: center;
		gap: var(--space-2);
	}
	.playhead .label {
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
