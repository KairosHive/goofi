<!-- One transition's controls: how the move goes, with the button that fires it by hand, then its
     triggers. The selected transition has one editor in the inspector. -->
<script lang="ts">
	import type { Edge, Machine, Policy, Trigger } from '$lib/api/generated';
	import { ContextMenu, type MenuItem } from 'panelty';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { Button, Field, Icon, IconButton, Segmented, Select, TextInput, Toggle } from '$lib/ui';
	import Assignments from './Assignments.svelte';
	import SecondsField from './SecondsField.svelte';
	import { numberDescriptor } from './attributes';
	import { CURVES, POLICIES, TRIGGER_KINDS, blankTrigger, type TriggerKind } from './triggers';

	let { name, m, id }: { name: string; m: Machine; id: string } = $props();

	const g = graph();
	const t = $derived(m.transitions[id]);
	const playheads = $derived(Object.keys(m.playheads));
	const triggers = $derived(t?.triggers ?? []);
	watchVariables(() => playheads.flatMap((p) => [`${p}.state`, `${p}.transition`]));

	function call(op: MachineOp, payload: Record<string, unknown>): void {
		void g.machine(op, { machine: name, id, ...payload }).catch((e) => notify().failure(op, e));
	}
	const edit = (patch: Record<string, unknown>): void => call('machine transition edit', patch);
	const setTriggers = (triggers: Trigger[]): void => edit({ triggers });
	const setTrigger = (i: number, trigger: Trigger): void => setTriggers(triggers.map((held, k) => k === i ? trigger : held));

	function moveTrigger(i: number, delta: number): void {
		const list = [...triggers];
		const [trigger] = list.splice(i, 1);
		list.splice(i + delta, 0, trigger);
		setTriggers(list);
	}

	// ---- firing by hand: the playheads resting in `from` (anywhere, for `*`); one goes at once, several ask.
	const candidates = $derived(playheads.filter((p) => t && (!t.internal || !variableValue(`${p}.transition`))
		&& (t.from === '*' || String(variableValue(`${p}.state`) ?? m.playheads[p].start) === t.from)));
	let picker = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function fire(e: MouseEvent): void {
		const go = (p: string): void => void g.machine('machine fire', { machine: name, playhead: p, transition: id }).catch((err) => notify().failure('Fire', err));
		if (candidates.length === 1) return go(candidates[0]);
		const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
		picker = { x: r.left, y: r.bottom, items: candidates.map((p) => ({ label: p, icon: 'circle-dot', action: () => go(p) })) };
	}

	const POLICY_SEGMENTS = POLICIES.map((id) => ({ id, label: id }));
</script>

{#if picker}
	<ContextMenu x={picker.x} y={picker.y} items={picker.items} onClose={() => (picker = null)} />
{/if}

{#if t}
	<div class="form" data-testid="machine-transition">
		<div class="section-head">
			<span class="section-name">transition</span>
			<Button variant="ghost" size="sm" disabled={candidates.length === 0}
				title={candidates.length === 0 ? 'No playhead is in its from state' : candidates.length === 1 ? `Fire it for ${candidates[0]}` : 'Fire it for one of the playheads there'}
				data-testid="transition-fire" onclick={fire}><Icon name="play" />fire</Button>
		</div>
		<div class="fields">
			<Field label="from" row><Select aria-label="from state" value={t.from} options={['*', ...Object.keys(m.states)]} onChange={(from) => edit({ from })} /></Field>
			<Field label="to" row><Select aria-label="to state" value={t.to} options={Object.keys(m.states)} onChange={(to) => edit({ to })} /></Field>
		</div>
		<SecondsField label="duration" value={t.duration} viewKey={`${name}/${id}/duration`} fallback={0} testid="transition-duration" onCommit={(duration) => edit({ duration })} />
		<div class="fields">
			<Field label="curve" doc="How the values ease on the way" row>
				<Select aria-label="curve" value={t.curve} options={[...CURVES]} onChange={(curve) => edit({ curve })} />
			</Field>
			<Field label="internal" doc="A zero-duration self-transition without exit, entry, or dwell reset" row>
				<Toggle value={t.internal} disabled={t.from !== t.to || t.duration !== 0} aria-label="internal transition" onChange={(internal) => edit({ internal })} />
			</Field>
		</div>
		<ParamField paramName="chance" descriptor={numberDescriptor(t.chance, 0, 1, 1)} literal viewKey={`${name}/${id}/chance`}
			data-testid="transition-chance" onCommit={(v) => edit({ chance: Math.min(1, Math.max(0, Number(v))) })} />
		<ParamField paramName="weight" descriptor={numberDescriptor(t.weight, 0, Math.max(10, t.weight), 1)} literal viewKey={`${name}/${id}/weight`}
			data-testid="transition-weight" onCommit={(v) => edit({ weight: Math.max(0, Number(v)) })} />
		<ExprEditor value={t.guard ?? ''} onCommit={(guard) => edit({ guard })} label="guard" placeholder="Optional condition; empty allows the transition" testid="transition-guard" />

		<div class="section-head">
			<span class="section-name">triggers</span>
			<IconButton variant="ghost" size="sm" label="Add a trigger" title="Add a trigger" data-testid="transition-add-trigger" onclick={() => setTriggers([...triggers, blankTrigger('manual')])}><Icon name="plus" /></IconButton>
		</div>
		{#if triggers.length === 0}
			<div class="empty">Never fires on its own; the fire button or `machine fire` moves it.</div>
		{/if}
		{#each triggers as tr, i (i)}
			<div class="slot" class:alt={i % 2 === 1} data-testid="trigger-row">
				<div class="slot-head">
					<Select value={tr.kind} options={[...TRIGGER_KINDS]} onChange={(k) => setTrigger(i, blankTrigger(k as TriggerKind))} aria-label="trigger kind" data-testid="trigger-kind" />
					<IconButton variant="ghost" size="sm" label="Move trigger up" disabled={i === 0} onclick={() => moveTrigger(i, -1)}><Icon name="chevron-down" style="transform: rotate(180deg)" /></IconButton>
					<IconButton variant="ghost" size="sm" label="Move trigger down" disabled={i === triggers.length - 1} onclick={() => moveTrigger(i, 1)}><Icon name="chevron-down" /></IconButton>
					<IconButton variant="ghost" size="sm" label="Remove trigger" title="Remove the trigger" onclick={() => setTriggers(triggers.filter((_, k) => k !== i))}><Icon name="x" /></IconButton>
				</div>
				{#if tr.kind === 'after'}
					<SecondsField label="after" value={tr.seconds} viewKey={`${name}/${id}/${i}/seconds`} testid="trigger-seconds"
						onCommit={(seconds) => setTrigger(i, { ...tr, seconds })} />
				{:else if tr.kind === 'when'}
					<ExprEditor value={tr.expression} onCommit={(e) => setTrigger(i, { ...tr, expression: e })} label="when" placeholder="variables.sensor.level > 0.5" testid="trigger-when" />
					<div class="fields"><Field label="condition" row>
						<Select aria-label="condition edge" value={tr.edge} options={['rising', 'falling', 'change', 'level']}
							onChange={(edge) => setTrigger(i, { ...tr, edge: edge as Edge })} />
					</Field></div>
				{:else if tr.kind === 'event'}
					<div class="fields"><Field label="event" row><TextInput value={tr.name} aria-label="event name"
						onChange={(name) => setTrigger(i, { ...tr, name })} /></Field></div>
				{:else if tr.kind === 'meet'}
					<div class="fields">
						<Field label="policy" doc="Who goes when a second playhead arrives: the longest resident, the newest, or all" row>
							<Segmented value={tr.policy} segments={POLICY_SEGMENTS} onChange={(p) => setTrigger(i, { ...tr, policy: p as Policy })} aria-label="policy" fill />
						</Field>
					</div>
				{/if}
			</div>
		{/each}

		<div class="section-head"><span class="section-name">transition values</span></div>
		<Assignments attributes={m.attributes} values={t.values ?? {}} viewKey={`${name}/${id}/values`} onCommit={(values) => edit({ values })} />
		<div class="actions">
			<Button variant="danger" size="sm" data-testid="transition-remove" onclick={() => call('machine transition remove', {})}>delete</Button>
		</div>
	</div>
{/if}

<style>
	.form {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		min-width: 0;
	}
</style>
