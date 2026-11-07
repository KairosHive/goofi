<!-- The settings of one state, or of several edited as one: its name, how it decides and how
     often, the playheads that start in it, and the transitions out of and into it, each row
     opening on its own settings and each row out carrying its share of the state's draw. Opened
     from a column's cog in the score; the attributes are the score's own rows. -->
<script lang="ts">
	import type { Decision, Machine } from '$lib/api/generated';
	import { ContextMenu, type MenuItem } from 'panelty';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import RenameField from '$lib/inspector/RenameField.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import { Badge, Button, ColorPicker, Field, Icon, IconButton, Segmented, Select } from '$lib/ui';
	import { numberDescriptor } from './attributes';
	import { dotColor, hexToRgba, rgbaToHex } from './layout';
	import { DECISIONS, editEach, mixed, outgoing as outgoingOf, relink, shares, summary as transitionSummary, weightsOf } from './model';
	import TransitionForm from './TransitionForm.svelte';

	let { name, m, ids, open = [] }: { name: string; m: Machine; ids: string[]; open?: string[] } = $props();

	const g = graph();
	function call(op: MachineOp, payload: Record<string, unknown>): void {
		void g.machine(op, { machine: name, ...payload }).catch((e) => notify().failure(op, e));
	}
	const one = $derived(ids.length === 1 ? ids[0] : null);
	const s = $derived(one ? m.states[one] : null);
	const transitions = $derived(Object.entries(m.transitions));
	const incoming = $derived(one ? transitions.filter(([, t]) => t.to === one) : []);
	const outgoing = $derived(one ? transitions.filter(([, t]) => t.from === one || t.from === '*') : []);
	const arrow = (t: Machine['transitions'][string]): string => `${t.from === '*' ? 'any' : t.from} → ${t.to}`;

	// ---- the states, edited as one.
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
	let opens = $state<Record<string, boolean>>({});
	$effect(() => {
		opens = Object.fromEntries(open.map((id) => [id, true]));
	});

	// ---- the playheads: those that start in the one state, and those resting in it now.
	const playheads = $derived(Object.keys(m.playheads));
	const here = $derived(one ? playheads.filter((p) => m.playheads[p].start === one) : []);
	watchVariables(() => playheads.map((p) => `${p}.state`));
	const whereNow = (p: string): string => String(variableValue(`${p}.state`) ?? m.playheads[p].start);
	const resting = $derived(one ? playheads.filter((p) => whereNow(p) === one) : []);

	// ---- a transition out, added here rather than dragged: to a state picked from a menu under the +.
	let picker = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function pickTransition(e: MouseEvent): void {
		const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
		const add = (to: string): void => void g.machine('machine transition add', { machine: name, from: one, to }).catch((err) => notify().failure('Add transition', err));
		picker = { x: r.left, y: r.bottom, items: Object.keys(m.states).map((to) => ({ label: to, action: () => add(to) })) };
	}
	/** The one delete path: a playhead starting here refuses it, as the manager says. */
	const remove = (): void => ids.forEach((id) => call('machine state remove', { name: id }));
</script>

{#snippet transitionList(title: string, list: [string, Machine['transitions'][string]][], testid: string, addable: boolean)}
	<section class="section" data-testid={testid}>
		<header>
			<span>{title}</span>
			{#if addable}
				<IconButton variant="ghost" size="sm" label="Add a transition" title="Add a transition out, to a state picked from the menu" data-testid="transition-add"
					onclick={pickTransition}><Icon name="plus" /></IconButton>
			{/if}
		</header>
		{#if list.length === 0 && !addable}
			<div class="empty">None.</div>
		{/if}
		{#if addable && s?.mode === 'random' && list.length > 0}
			<ParamField paramName="wait" descriptor={numberDescriptor(shown[0], 0, 1)} literal viewKey={`${name}/${one}/wait`} data-testid="state-share-wait"
				onPreview={(v) => drag(0, Number(v))} onCommit={(v) => land(0, Number(v))} />
		{/if}
		{#each list as [id, t] (id)}
			{@const k = out.indexOf(id)}
			{@const share = addable && s?.mode === 'random' && k >= 0}
			<ParamField paramName={id} label={arrow(t)} note={`${transitionSummary(m, id)}${t.condition ? ` · if ${t.condition}` : ''}`}
				descriptor={share ? numberDescriptor(shown[k + 1], 0, 1) : undefined} literal viewKey={`${name}/${id}/share`}
				onPreview={(v) => drag(k + 1, Number(v))} onCommit={(v) => land(k + 1, Number(v))}
				open={opens[id] === true} onToggle={() => (opens[id] = !opens[id])} data-testid={`transition-row-${id}`} data-open={opens[id] === true}>
				{#snippet more()}
					<TransitionForm {name} {m} {id} />
				{/snippet}
			</ParamField>
		{/each}
	</section>
{/snippet}

{#if picker}
	<ContextMenu x={picker.x} y={picker.y} items={picker.items} onClose={() => (picker = null)} />
{/if}

<div class="state-form" data-testid="state-form" data-count={ids.length}>
	<div class="head">
		{#if one}
			<span class="name"><RenameField value={one} valid={isValidIdentifier} label="State name" testid="state-name" onRename={(to) => call('machine state rename', { name: one, to })} /></span>
			<Badge tone={resting.length > 0 ? 'accent' : 'neutral'} title={resting.length > 0 ? `Resting here now: ${resting.join(', ')}` : 'No playhead rests here now'}>
				{resting.length > 0 ? `${resting.length} here` : 'empty'}
			</Badge>
		{:else}
			<span class="name mono" data-testid="state-name">{ids.length} states</span>
		{/if}
		<Button variant="danger" size="sm" title="Delete the state and its transitions" data-testid="state-remove" onclick={remove}>delete</Button>
	</div>
	<section class="section" data-testid="state-decision">
		<div class="fields">
			<Field label={tag('mode', states.map((s) => s.mode))} doc={DECISION_HINT[states[0].mode]} row>
				<Segmented value={states[0].mode} segments={SEGMENTS} onChange={(mode) => editStates({ mode: mode as Decision })} aria-label="decision" data-testid="state-mode" fill />
			</Field>
		</div>
		{#if states[0].mode === 'expression'}
			<Field label="index" doc={DECISION_HINT.expression}>
				<ExprEditor machine={name} label="index" value={states[0].expression} testid="state-expression"
					placeholder="0 if vars.desk.pick > 0.5 else -1" onCommit={(expression) => editStates({ expression })} />
			</Field>
		{/if}
		<!-- Below 0 the interval is off: -1 on the wire, so the slider's low end lands there. An
		     expression drives it as one drives a param, read as each beat is armed. -->
		<ParamField paramName="interval" label={tag('interval', states.map((s) => s.interval))} descriptor={numberDescriptor(states[0].interval, -1, 10, 1, states[0].interval_expression ?? '')}
			machine={name} viewKey={`${name}/states/interval`} data-testid="state-interval" onPreview={(v) => preview({ interval: Number(v) })} onCommit={(v) => editStates({ interval: Number(v) < 0 ? -1 : Number(v) })}
			onSetSource={(src) => editStates({ interval_expression: src.expression ?? (src.mode === 'constant' ? '' : states[0].interval_expression ?? '') })} />
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

<style>
	.state-form {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.head {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		min-height: var(--chrome-control-h);
	}
	.name {
		flex: 1 1 auto;
		min-width: 0;
		font-weight: 600;
	}
	.section {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: var(--space-4) 0;
	}
	.section + .section {
		border-top: 1px solid var(--border);
	}
	/* `:global`, so the forms inside wear the same rows and heads. Field rows sit on a grid, so the
	   labels of one block share a column. */
	.state-form :global(.fields) {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		column-gap: var(--space-5);
		row-gap: var(--space-3);
		--number-width: 100%;
	}
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
	.tools {
		display: inline-flex;
		align-items: center;
		flex: 0 0 auto;
		gap: var(--space-1);
	}
	.row {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		min-height: var(--hit);
		padding: 0 var(--space-3);
		margin-inline: calc(0px - var(--space-3));
		border-radius: var(--radius-sm);
	}
	.row:hover,
	.row:focus-within {
		background: var(--surface-3);
	}
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
</style>
