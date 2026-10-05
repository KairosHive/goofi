<!-- One transition's controls: where it goes, its triggers, how long the move takes and its weight,
     with a button that fires it. Shown for a selected transition, and inside a state's lists, and
     dressed by the inspector's own rules (`.fields`, `.slot`, `.section-head`), its one host. -->
<script lang="ts">
	import type { Machine, Policy, Trigger } from '$lib/api/generated';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { Button, Field, Icon, IconButton, NumberInput, Select, TextInput } from '$lib/ui';
	import { CURVES, POLICIES, TRIGGER_KINDS, blankTrigger, type TriggerKind } from './triggers';

	let {
		name,
		m,
		id,
		onFire
	}: {
		name: string;
		m: Machine;
		id: string;
		/** Fire `id` for every playhead in its `from`. */
		onFire: (id: string) => void;
	} = $props();

	const g = graph();
	const t = $derived(m.transitions[id]);
	const states = $derived(Object.keys(m.states));

	function call(op: MachineOp, payload: Record<string, unknown>): void {
		void g.machine(op, { machine: name, id, ...payload }).catch((e) => notify().failure(op, e));
	}
	const edit = (patch: Record<string, unknown>): void => call('machine transition edit', patch);
	const setTriggers = (triggers: Trigger[]): void => edit({ triggers });
	const replaceAt = (list: Trigger[], i: number, tr: Trigger): Trigger[] => list.map((held, k) => (k === i ? tr : held));
</script>

{#if t}
	<div class="form" data-testid="machine-transition">
		<div class="fields">
			<Field label="from" row>
				<Select aria-label="from" value={t.from} options={['*', ...states]} labels={{ '*': 'any state' }} onChange={(v) => edit({ from: v })} />
			</Field>
			<Field label="to" row>
				<Select aria-label="to" value={t.to} options={states} onChange={(v) => edit({ to: v })} />
			</Field>
		</div>

		<div class="section-head">
			<span class="section-name">triggers</span>
			<IconButton variant="ghost" size="sm" label="Add a trigger" title="Add a trigger" data-testid="transition-add-trigger" onclick={() => setTriggers([...(t.triggers ?? []), blankTrigger('manual')])}><Icon name="plus" /></IconButton>
		</div>
		{#if (t.triggers ?? []).length === 0}
			<div class="empty">Never fires on its own; a tap on its label or `machine fire` moves it.</div>
		{/if}
		{#each t.triggers ?? [] as tr, i (i)}
			<div class="slot" class:alt={i % 2 === 1} data-testid="trigger-row">
				<div class="slot-head">
					<Select aria-label="trigger kind" data-testid="trigger-kind" density="chrome" value={tr.kind} options={[...TRIGGER_KINDS]} onChange={(v) => setTriggers(replaceAt(t.triggers ?? [], i, blankTrigger(v as TriggerKind)))} />
					<span class="slack"></span>
					<IconButton variant="ghost" size="sm" label="Remove trigger" title="Remove the trigger" onclick={() => setTriggers((t.triggers ?? []).filter((_, k) => k !== i))}><Icon name="x" /></IconButton>
				</div>
				{#if tr.kind === 'after'}
					<div class="fields">
						<Field label="after" doc="Seconds, or an expression over variables read on entry" row>
							<TextInput inputmode="search" aria-label="seconds" value={String(tr.seconds)} onChange={(v) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, seconds: Number.isFinite(Number(v)) && v.trim() !== '' ? Number(v) : v }))} />
						</Field>
						<Field label="chance" doc="The roll on each dwell; 1 always fires" row>
							<NumberInput aria-label="chance" value={tr.chance} min={0} max={1} step={0.05} onChange={(v) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, chance: v }))} />
						</Field>
					</div>
				{:else if tr.kind === 'meet'}
					<div class="fields">
						<Field label="policy" doc="Who goes when a second playhead arrives: the longest resident, the newest, or all" row>
							<Select aria-label="policy" value={tr.policy} options={[...POLICIES]} onChange={(v) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, policy: v as Policy }))} />
						</Field>
					</div>
				{:else if tr.kind === 'when'}
					<ExprEditor value={tr.expression} onCommit={(e) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, expression: e }))} label="when" placeholder="variables.sensor.level > 0.5" testid="trigger-when" />
				{/if}
			</div>
		{/each}

		<hr class="rule" />
		<div class="fields">
			<Field label="seconds" doc="How long the move takes; 0 is instant" row>
				<NumberInput value={t.duration} min={0} step={0.1} aria-label="duration" onChange={(v) => edit({ duration: Math.max(0, v) })} />
			</Field>
			<Field label="curve" doc="How the values ease on the way" row>
				<Select aria-label="curve" value={t.curve} options={[...CURVES]} onChange={(v) => edit({ curve: v })} />
			</Field>
			<Field label="weight" doc="The draw among transitions firing together; 0 is never drawn" row>
				<NumberInput value={t.weight} min={0} step={0.1} aria-label="weight" onChange={(v) => edit({ weight: Math.max(0, v) })} />
			</Field>
		</div>

		<div class="actions">
			<Button variant="ghost" size="sm" title="Fire it for every playhead in its from state" data-testid="transition-fire" onclick={() => onFire(id)}><Icon name="play" />fire</Button>
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
