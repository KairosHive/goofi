<!-- One transition's controls: where it goes, its triggers, how long the move takes and its weight,
     with a button that fires it. Shown for a selected transition, and inside a state's lists; its
     `.row` and `.muted` are the inspector's, which is the one place it is drawn. -->
<script lang="ts">
	import type { Machine, Policy, Trigger } from '$lib/api/generated';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { Button, Chip, Field, Icon, IconButton, NumberInput, Select, TextInput } from '$lib/ui';
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
	const setTriggers = (triggers: Trigger[]): void => call('machine transition edit', { triggers });
	const replaceAt = (list: Trigger[], i: number, tr: Trigger): Trigger[] => list.map((held, k) => (k === i ? tr : held));
</script>

{#if t}
	<section class="form" data-testid="machine-transition">
		<div class="row">
			<Select aria-label="from" value={t.from} options={['*', ...states]} labels={{ '*': 'any state' }} onChange={(v) => call('machine transition edit', { from: v })} />
			<span class="muted">→</span>
			<Select aria-label="to" value={t.to} options={states} onChange={(v) => call('machine transition edit', { to: v })} />
			<Button variant="ghost" size="sm" title="Fire it for every playhead in its from state" data-testid="transition-fire" onclick={() => onFire(id)}><Icon name="play" />fire</Button>
		</div>
		<h4>triggers</h4>
		{#each t.triggers ?? [] as tr, i (i)}
			<div class="trigger" data-testid="trigger-row">
				<div class="row">
					<Select aria-label="trigger kind" data-testid="trigger-kind" value={tr.kind} options={[...TRIGGER_KINDS]} onChange={(v) => setTriggers(replaceAt(t.triggers ?? [], i, blankTrigger(v as TriggerKind)))} />
					{#if tr.kind === 'after'}
						<TextInput inputmode="search" aria-label="seconds" title="Seconds, or an expression over variables read on entry" value={String(tr.seconds)} onChange={(v) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, seconds: Number.isFinite(Number(v)) && v.trim() !== '' ? Number(v) : v }))} />
						<NumberInput aria-label="chance" title="The roll on each dwell; 1 always fires" value={tr.chance} min={0} max={1} step={0.05} onChange={(v) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, chance: v }))} />
					{:else if tr.kind === 'meet'}
						<Select aria-label="policy" value={tr.policy} options={[...POLICIES]} onChange={(v) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, policy: v as Policy }))} />
					{/if}
					<IconButton variant="ghost" size="sm" label="Remove trigger" title="Remove the trigger" onclick={() => setTriggers((t.triggers ?? []).filter((_, k) => k !== i))}><Icon name="x" /></IconButton>
				</div>
				{#if tr.kind === 'when'}
					<ExprEditor value={tr.expression} onCommit={(e) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, expression: e }))} label="when" placeholder="variables.sensor.level > 0.5" testid="trigger-when" />
				{/if}
			</div>
		{/each}
		<Button class="add-row" variant="ghost" size="sm" data-testid="transition-add-trigger" onclick={() => setTriggers([...(t.triggers ?? []), blankTrigger('manual')])}><Icon name="plus" />trigger</Button>
		<div class="row">
			<Field label="seconds" doc="How long the move takes; 0 is instant">
				<NumberInput value={t.duration} min={0} step={0.1} aria-label="duration" onChange={(v) => call('machine transition edit', { duration: Math.max(0, v) })} />
			</Field>
			<Field label="curve">
				<Select aria-label="curve" value={t.curve} options={[...CURVES]} onChange={(v) => call('machine transition edit', { curve: v })} />
			</Field>
			<Field label="weight" doc="The draw among transitions firing together; 0 is never drawn">
				<NumberInput value={t.weight} min={0} step={0.1} aria-label="weight" onChange={(v) => call('machine transition edit', { weight: Math.max(0, v) })} />
			</Field>
		</div>
		<Chip tone="danger" data-testid="transition-remove" onclick={() => call('machine transition remove', {})}>delete transition</Chip>
	</section>
{/if}

<style>
	.form {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}
	h4 {
		margin: 0;
		font-size: var(--fs-small);
		font-weight: 600;
		color: var(--text-muted);
	}
	.trigger {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		padding: var(--space-2);
		border-radius: var(--radius-sm);
		background: var(--surface-2);
	}
	.form :global(.add-row) {
		width: 100%;
		justify-content: center;
	}
</style>
