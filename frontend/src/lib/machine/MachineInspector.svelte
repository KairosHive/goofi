<!-- What the machine panel's side pane shows: the selected states. One state is edited whole —
     its name, how it decides, every attribute it sets and the envelope each lands by, the playheads
     that start in it, and the transitions out of and into it, each row opening on its own settings
     and each row out carrying its share of the state's draw. Several states show what they share,
     and an edit writes to each. Laid out as the node inspector lays out a node: an identity bar,
     then sections of flat rows on the pane's own ground, a row's tools showing under the pointer. -->
<script lang="ts">
	import type { Attribute, Decision, Machine } from '$lib/api/generated';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import IdentityBar from '$lib/inspector/IdentityBar.svelte';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import RenameField from '$lib/inspector/RenameField.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { history } from '$lib/stores/history.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import { Badge, ColorPicker, Disclosure, ENVELOPE_BORN, Field, Icon, IconButton, ScrollArea, Segmented, Select, Slider, TextInput } from '$lib/ui';
	import AttributeForm from './AttributeForm.svelte';
	import { FACES, descriptorFor, envelopeDescriptor, literalOf, numberDescriptor } from './attributes';
	import { dotColor, hexToRgba, rgbaToHex } from './layout';
	import { DECISIONS, editEach, held, mixed, outgoing as outgoingOf, relink, shares, summary as transitionSummary, weightsOf, type Subject } from './model';
	import TransitionForm from './TransitionForm.svelte';

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
	const DECISION_HINT: Record<Decision, string> = {
		sequence: 'Each interval, the next transition out in its turn',
		random: 'Each interval, a draw by the shares on the transitions out; the wait is no move',
		expression: 'The index of the transition out to take, 0 first; a negative one waits'
	};

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

	// ---- the playheads: those that start in the one state, and those resting in it now.
	const playheads = $derived(Object.keys(m.playheads));
	const here = $derived(one ? playheads.filter((p) => m.playheads[p].start === one) : []);
	watchVariables(() => playheads.map((p) => `${p}.state`));
	const whereNow = (p: string): string => String(variableValue(`${p}.state`) ?? m.playheads[p].start);
	const resting = $derived(one ? playheads.filter((p) => whereNow(p) === one) : []);

	// ---- a transition out, added here rather than dragged: to a state picked from the list.
	function addTransition(to: string): void {
		if (one && to) void g.machine('machine transition add', { machine: name, from: one, to }).catch((e) => notify().failure('Add transition', e));
	}
</script>

{#snippet transitionList(title: string, list: [string, Machine['transitions'][string]][], testid: string, addable: boolean)}
	<section class="section" data-testid={testid}>
		<header>
			<span>{title}</span>
			{#if addable}
				<Select aria-label="Add a transition to" density="chrome" value="" options={['', ...Object.keys(m.states)]} labels={{ '': '+ to…' }}
					data-testid="transition-add-to" onChange={addTransition} />
			{/if}
		</header>
		{#if list.length === 0 && !addable}
			<div class="empty">None.</div>
		{/if}
		<!-- A random state's draw: the wait leads the rows out, and each row out carries its share;
		     a drag moves the others in their own ratio, so the shares always sum to one. -->
		{#if addable && s?.mode === 'random' && list.length > 0}
			<div class="row" data-testid="state-share-wait">
				<span class="hint mono">wait</span>
				<span class="share">
					<Slider value={shown[0]} min={0} max={1} step={0.01} bounds={false} aria-label="wait" onInput={(v) => drag(0, v)} onChange={(v) => land(0, v)} />
				</span>
				<span class="hint mono">{percent(shown[0])}</span>
			</div>
		{/if}
		{#each list as [id, t] (id)}
			{@const k = out.indexOf(id)}
			{@const open = subject.open.includes(id)}
			<Disclosure class="transition-row" {open} data-testid={`transition-row-${id}`} data-open={open}>
				{#snippet summary()}
					<span class="arrow">{arrow(t)}</span>
					<span class="part">{transitionSummary(m, id)}</span>
					{#if t.condition}<span class="cond" title={t.condition}>if {t.condition}</span>{/if}
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
				{#snippet end()}
					{#if one}
						<Badge tone={resting.length > 0 ? 'accent' : 'neutral'} title={resting.length > 0 ? `Resting here now: ${resting.join(', ')}` : 'No playhead rests here now'}>
							{resting.length > 0 ? `${resting.length} here` : 'empty'}
						</Badge>
					{/if}
				{/snippet}
			</IdentityBar>
		{/key}
		<ScrollArea>
			<div class="rows">
				<section class="section" data-testid="state-decision">
					<div class="fields">
						<Field label={tag('mode', states.map((s) => s.mode))} doc={DECISION_HINT[states[0].mode]} row>
							<Segmented value={states[0].mode} segments={SEGMENTS} onChange={(mode) => editStates({ mode: mode as Decision })} aria-label="decision" data-testid="state-mode" fill />
						</Field>
					</div>
					<!-- Below 0 the interval is off: -1 on the wire, so the slider's low end lands there. -->
					<ParamField paramName="interval" label={tag('interval', states.map((s) => s.interval))} descriptor={numberDescriptor(states[0].interval, -1, 10, 1)} literal
						viewKey={`${name}/states/interval`} data-testid="state-interval" onPreview={(v) => preview({ interval: Number(v) })} onCommit={(v) => editStates({ interval: Number(v) < 0 ? -1 : Number(v) })} />
					{#if s?.mode === 'expression'}
						<Field label="index" doc={DECISION_HINT.expression}>
							<ExprEditor machine={name} label="index" value={s.expression} testid="state-expression"
								placeholder="0 if variables.desk.pick > 0.5 else -1" onCommit={(expression) => editStates({ expression })} />
						</Field>
					{/if}
				</section>
				<section class="section" data-testid="state-attributes">
					<header>
						<span>Attributes</span>
						<span class="add-attr">
							<TextInput class="add-attr-name" inputmode="search" placeholder="name a new attribute…" aria-label="New attribute" data-testid="attribute-name-new"
								value={fresh} onInput={(v) => (fresh = v)} onChange={(v) => { fresh = v; void create(); }} />
							<IconButton variant="ghost" size="sm" label="Add the attribute" title="Add an attribute of that name, set in the selected states" disabled={!creatable}
								data-testid="attribute-create" onclick={() => void create()}><Icon name="plus" /></IconButton>
						</span>
					</header>
					{#each Object.entries(m.attributes) as [attr, a] (attr)}
						{@const h = held(m, ids, attr)}
						{@const env = envelopeOf(attr)}
						<div class="attr" class:unset={!h.set} data-testid={`state-attribute-${attr}`} data-set={h.set} data-mixed={h.mixed} data-envelope={env !== null}>
							<div class="attr-row">
								<ParamField paramName={attr} label={h.mixed ? `${attr} (mixed)` : attr} descriptor={descriptorFor(a, h.value)} literal viewKey={`${name}/selection/${attr}`}
									onCommit={(v) => editStates({ values: { [attr]: literalOf(v) } })}
									onPreview={(v) => preview({ values: { [attr]: v } })} />
								<!-- The tools keep their room set or not, so the value's track never changes width
								     under a pointer; they show under the pointer, and always to a finger. -->
								<span class="tools" class:pinned={env !== null || defining === attr}>
									{#if shapable(a)}
										<IconButton variant="ghost" size="sm" label={env ? `Land ${attr} plain` : `Shape ${attr}`} aria-pressed={env !== null} data-testid="state-envelope" disabled={!h.set}
											title={env ? 'Remove the envelope: the value lands plain' : 'Add an envelope: on arrival the value plays an attack, decay, sustain and release'}
											onclick={() => editStates({ envelopes: { [attr]: env ? null : ENVELOPE_BORN } })}><Icon name="activity" /></IconButton>
									{/if}
									<IconButton variant="ghost" size="sm" label={`Unset ${attr}`} title="Unset: a playhead entering keeps what it holds" data-testid="state-clear" disabled={!h.set}
										onclick={() => editStates({ values: { [attr]: null } })}><Icon name="minus" /></IconButton>
									<IconButton variant="ghost" size="sm" label={`Define ${attr}`} title="Its name, kind, range and default, for every state" data-testid="attribute-edit"
										aria-pressed={defining === attr} onclick={() => (defining = defining === attr ? null : attr)}><Icon name="pencil" /></IconButton>
								</span>
							</div>
							{#if env}
								<div class="nested">
									<ParamField paramName={`${attr} envelope`} label="envelope" descriptor={envelopeDescriptor(env, LONGEST)} literal viewKey={`${name}/selection/${attr}/envelope`}
										data-testid="state-attribute-envelope"
										onCommit={(v) => editStates({ envelopes: { [attr]: v } })}
										onPreview={(v) => preview({ envelopes: { [attr]: v as number[] } })} />
								</div>
							{/if}
							{#if defining === attr}
								<div class="definition"><AttributeForm {name} {attr} {a} /></div>
							{/if}
						</div>
					{/each}
				</section>
				{#if one}
					<section class="section" data-testid="state-playheads">
						<header>
							<span>Playheads</span>
							<IconButton variant="ghost" size="sm" label="Add a playhead here" title="Add a playhead that starts in this state" data-testid="machine-add-playhead"
								onclick={() => call('machine playhead add', { start: one, color: dotColor('', playheads.length) })}><Icon name="plus" /></IconButton>
						</header>
						{#each here as p (p)}
							{@const ph = m.playheads[p]}
							<div class="row" data-testid={`playhead-${p}`}>
								<span class="swatch"><ColorPicker value={hexToRgba(dotColor(ph.color, playheads.indexOf(p)))} aria-label={`${p} colour`} onChange={(rgba) => call('machine playhead edit', { name: p, color: rgbaToHex(rgba) })} /></span>
								<span class="mono"><RenameField value={p} valid={isValidIdentifier} label="Playhead name" testid="playhead-name" onRename={(to) => call('machine playhead rename', { name: p, to })} /></span>
								<span class="hint" title="Where it is now">in <span class="mono" data-testid="playhead-state">{whereNow(p)}</span></span>
								<span class="hint" title="Where a reset and a load put it">starts</span>
								<Select data-testid="playhead-start" aria-label="start state" density="chrome" value={ph.start} options={Object.keys(m.states)} onChange={(v) => call('machine playhead edit', { name: p, start: v })} />
								<span class="tools">
									<IconButton variant="ghost" size="sm" label={`Remove ${p}`} title="Remove the playhead and its variables" data-testid="playhead-remove" onclick={() => call('machine playhead remove', { name: p })}><Icon name="x" /></IconButton>
								</span>
							</div>
						{/each}
					</section>
					{@render transitionList('Transitions Out', outgoing, 'transitions-out', true)}
					{@render transitionList('Transitions In', incoming, 'transitions-in', false)}
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
	/* The body, as the node inspector's rows: the pane's own ground, one padding, sections in a
	   column, a hairline between them. */
	.rows {
		display: flex;
		flex-direction: column;
		padding: 0 var(--space-6);
		background: var(--surface-1);
	}
	.section {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: var(--space-5) 0;
	}
	.section + .section {
		border-top: 1px solid var(--border);
	}
	/* `:global`, so the forms inside wear the same rows and heads: they are drawn nowhere else.
	   Field rows sit on a grid, so the labels of one block share a column. */
	.rows :global(.fields) {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		column-gap: var(--space-5);
		row-gap: var(--space-3);
		--number-width: 100%;
	}
	/* A section's heading, as the node inspector's metadata wears one: its name, and its action at the end. */
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		min-height: var(--chrome-control-h);
		font-weight: 600;
		white-space: nowrap;
	}
	.empty,
	.hint {
		color: var(--text-muted);
		font-size: var(--fs-small);
	}
	.mono {
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	/* A row's tools wait under the pointer, and are always there for a finger. */
	.tools {
		display: inline-flex;
		align-items: center;
		flex: 0 0 auto;
		gap: var(--space-1);
		opacity: 0;
		transition: opacity var(--dur-fast) var(--ease);
	}
	.attr-row:hover .tools,
	.attr-row:focus-within .tools,
	.row:hover .tools,
	.row:focus-within .tools,
	.tools.pinned {
		opacity: 1;
	}
	@media (hover: none) and (pointer: coarse) {
		.tools {
			opacity: 1;
		}
	}
	/* An attribute: its value row, the envelope under it, and its definition when opened. */
	.attr {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}
	.attr-row {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-width: 0;
	}
	.attr :global(.pf-param) {
		flex: 1 1 auto;
		min-width: 0;
	}
	/* A value no selected state sets: the default, until a turn sets it in each. */
	.attr.unset > .attr-row > :global(.pf-param) {
		opacity: 0.55;
	}
	.attr.unset:focus-within > .attr-row > :global(.pf-param) {
		opacity: 1;
	}
	/* What hangs off a row sits a step in, under it. */
	.nested {
		display: flex;
		padding-inline-start: var(--space-6);
	}
	/* The definition is a form of its own, on the step above the body. */
	.definition {
		padding: var(--space-3) var(--space-4);
		border-radius: var(--radius-sm);
		background: var(--surface-2);
	}
	/* A flat row — a playhead's, or the wait that leads a random state's transitions out — lit
	   under the pointer as a transition's is; the name after the swatch takes the slack. */
	.row {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		min-height: var(--hit);
		padding: 0 var(--space-3);
		margin-inline: calc(0px - var(--space-3));
		border-radius: var(--radius-sm);
		transition: background var(--dur-fast) var(--ease);
	}
	.row:hover,
	.row:focus-within {
		background: var(--surface-2);
	}
	.row > .mono:first-child,
	.swatch + .mono {
		flex: 1 1 auto;
	}
	.hint .mono {
		color: var(--text);
	}
	.swatch {
		display: inline-flex;
		width: var(--chrome-control-h);
	}
	/* The wait's share sits where the rows' shares sit, so the column of sliders reads as one. */
	.share {
		display: inline-flex;
		flex: 1 1 auto;
		min-width: 0;
	}
	.row .share {
		flex: 0 0 40%;
	}
	/* A transition's row is flat, and lights under the pointer as the rows around it do; the open
	   one is a card on the step above, its settings inside. */
	.rows :global(.transition-row) {
		--disclosure-surface: transparent;
		--disclosure-hover: var(--surface-2);
		margin-inline: calc(0px - var(--space-3));
		border-radius: var(--radius-sm);
		transition: background var(--dur-fast) var(--ease);
	}
	.rows :global(.transition-row[data-open='true']) {
		--disclosure-hover: var(--surface-3);
		--disclosure-body-pad: var(--space-2) var(--space-4) var(--space-4);
		background: var(--surface-2);
	}
	.arrow {
		font-family: var(--font-mono);
		font-weight: 600;
	}
	.part,
	.cond {
		margin-inline-start: var(--space-3);
		color: var(--text-muted);
		font-family: var(--font-mono);
		font-weight: 400;
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
</style>
