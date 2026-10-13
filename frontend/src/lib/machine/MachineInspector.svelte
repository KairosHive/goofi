<!-- What the machine panel's side pane shows: the selected states — the name and the transitions in
     and out of one, the decision of several, and the attributes of either, each value written to
     every selected state and each attribute's definition under its pencil — or the selected
     transitions. One state's decision and playheads are its card's. Laid out as the node inspector
     lays out a node: Field rows, slot blocks, rules. -->
<script lang="ts">
	import type { Decision, Machine } from '$lib/api/generated';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import IdentityBar from '$lib/inspector/IdentityBar.svelte';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import RenameField from '$lib/inspector/RenameField.svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { Disclosure, Field, Icon, IconButton, ScrollArea, Segmented } from '$lib/ui';
	import AttributeForm from './AttributeForm.svelte';
	import { descriptorFor, literalOf, numberDescriptor } from './attributes';
	import { editEach, held, mixed } from './batch';
	import { DECISIONS } from './decision';
	import type { Subject } from './subject';
	import TransitionForm from './TransitionForm.svelte';
	import { summary as triggerSummary } from './triggers';

	let { name, m, subject, onClose }: { name: string; m: Machine; subject: Subject; onClose: () => void } = $props();

	const g = graph();
	function rename(from: string, to: string): void {
		void g.machine('machine state rename', { machine: name, name: from, to }).catch((e) => notify().failure('Rename state', e));
	}

	const ids = $derived(subject.ids.filter((id) => (subject.kind === 'state' ? id in m.states : id in m.transitions)));
	const one = $derived(ids.length === 1 ? ids[0] : null);
	const transitions = $derived(Object.entries(m.transitions));
	const incoming = $derived(subject.kind === 'state' && one ? transitions.filter(([, t]) => t.to === one) : []);
	const outgoing = $derived(subject.kind === 'state' && one ? transitions.filter(([, t]) => t.from === one || t.from === '*') : []);
	const arrow = (t: Machine['transitions'][string]): string => `${t.from === '*' ? 'any' : t.from} → ${t.to}`;

	// ---- the selected states, edited as one.
	const states = $derived(subject.kind === 'state' ? ids.map((id) => m.states[id]) : []);
	const editStates = (patch: Record<string, unknown>): void => void editEach(name, 'machine state edit', 'name', ids, patch);
	const previewValue = (attr: string, v: number | number[]): void => ids.forEach((id) => g.previewState(name, id, { values: { [attr]: v } }));
	const tag = (label: string, values: unknown[]): string => (mixed(values) ? `${label} (mixed)` : label);
	let defining = $state<string | null>(null);
	const SEGMENTS = DECISIONS.map((d) => ({ id: d.id, label: d.label, title: d.title }));
</script>

{#snippet transitionList(title: string, list: [string, Machine['transitions'][string]][], testid: string)}
	<section class="section" data-testid={testid}>
		<div class="section-head"><span class="section-name">{title}</span></div>
		{#if list.length === 0}
			<div class="empty">None</div>
		{/if}
		{#each list as [id, t] (id)}
			<Disclosure class="transition-row" data-testid={`transition-row-${id}`}>
				{#snippet summary()}
					<span class="arrow">{arrow(t)}</span>
					<span class="when">{triggerSummary(m, id)}</span>
				{/snippet}
				{#snippet children()}
					<TransitionForm {name} {m} ids={[id]} />
				{/snippet}
			</Disclosure>
		{/each}
	</section>
{/snippet}

<div class="inspector" data-testid="machine-inspector" data-subject={subject.kind} data-count={ids.length}>
	{#if subject.kind === 'state' && ids.length > 0}
		{#key one}
			<IdentityBar type={one ? 'state' : 'states'} {onClose}>
				{#snippet title()}
					{#if one}
						<RenameField value={one} valid={isValidIdentifier} label="State name" testid="state-name" onRename={(to) => rename(one, to)} />
					{:else}
						<span class="mono" data-testid="state-name">{ids.length} states</span>
					{/if}
				{/snippet}
			</IdentityBar>
		{/key}
		<ScrollArea>
			<div class="rows">
				{#if !one}
					<section class="section" data-testid="states-decision">
						<div class="section-head"><span class="section-name">decision</span></div>
						<div class="fields">
							<Field label={tag('mode', states.map((s) => s.mode))} row>
								<Segmented value={states[0].mode} segments={SEGMENTS} onChange={(mode) => editStates({ mode: mode as Decision })} aria-label="decision" data-testid="states-mode" fill />
							</Field>
						</div>
						<ParamField paramName="interval" label={tag('interval', states.map((s) => s.interval))} descriptor={numberDescriptor(states[0].interval, 0, 10, 1)} literal
							viewKey={`${name}/states/interval`} data-testid="states-interval" onCommit={(v) => editStates({ interval: Math.max(0, Number(v)) })} />
					</section>
					<hr class="rule" />
				{/if}
				<section class="section" data-testid="state-attributes">
					<div class="section-head"><span class="section-name">attributes</span></div>
					{#each Object.entries(m.attributes) as [attr, a], k (attr)}
						{@const h = held(m, ids, attr)}
						<div class="slot" class:alt={k % 2 === 1} class:unset={!h.set} data-testid={`state-attribute-${attr}`} data-set={h.set} data-mixed={h.mixed}>
							<div class="slot-head">
								<ParamField paramName={attr} label={h.mixed ? `${attr} (mixed)` : attr} descriptor={descriptorFor(a, h.value)} literal viewKey={`${name}/selection/${attr}`}
									onCommit={(v) => editStates({ values: { [attr]: literalOf(v) } })}
									onPreview={(v) => previewValue(attr, v)} />
								{#if h.set}
									<IconButton variant="ghost" size="sm" label={`Unset ${attr}`} title="Unset: a playhead entering keeps what it holds" data-testid="state-clear"
										onclick={() => editStates({ values: { [attr]: null } })}><Icon name="minus" /></IconButton>
								{/if}
								<IconButton variant="ghost" size="sm" label={`Define ${attr}`} title="Its name, kind, range and default, for every state" data-testid="attribute-edit"
									aria-expanded={defining === attr} onclick={() => (defining = defining === attr ? null : attr)}><Icon name="pencil" /></IconButton>
							</div>
							{#if defining === attr}<AttributeForm {name} {attr} {a} />{/if}
						</div>
					{:else}
						<div class="empty">No attributes yet: name one in a state's menu.</div>
					{/each}
				</section>
				{#if one}
					<hr class="rule" />
					{@render transitionList('incoming', incoming, 'transitions-in')}
					<hr class="rule" />
					{@render transitionList('outgoing', outgoing, 'transitions-out')}
				{/if}
			</div>
		</ScrollArea>
	{:else if subject.kind === 'transition' && ids.length > 0}
		<IdentityBar type={one ? `transition ${one}` : 'transitions'} {onClose}>
			{#snippet title()}<span class="mono">{one ? arrow(m.transitions[one]) : `${ids.length} transitions`}</span>{/snippet}
		</IdentityBar>
		<ScrollArea>
			<div class="rows">
				<TransitionForm {name} {m} {ids} />
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
	.slot-head > :global(.pf-param) {
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
