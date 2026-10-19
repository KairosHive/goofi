<!-- The controls of one transition: the buttons that fire it by hand and delete it, how the move
     goes, then the triggers that fire it besides its state's decision. Shown open inside a state's
     transition lists, and dressed by the inspector's own rules (`.fields`, `.slot`, `.section-head`). -->
<script lang="ts">
	import type { Machine, Policy, Trigger } from '$lib/api/generated';
	import { ContextMenu, type MenuItem } from 'panelty';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { Button, Field, Icon, IconButton, Segmented, Select } from '$lib/ui';
	import { numberDescriptor } from './attributes';
	import { CURVES, POLICIES, TRIGGER_KINDS, blankTrigger, type TriggerKind } from './triggers';

	let { name, m, id }: { name: string; m: Machine; id: string } = $props();

	const g = graph();
	const t = $derived(m.transitions[id]);
	const playheads = $derived(Object.keys(m.playheads));
	watchVariables(() => playheads.map((p) => `${p}.state`));

	const call = (op: MachineOp, patch: Record<string, unknown>): void => void g.machine(op, { machine: name, id, ...patch }).catch((e) => notify().failure(op, e));
	const edit = (patch: Record<string, unknown>): void => call('machine transition edit', patch);
	const setTriggers = (triggers: Trigger[]): void => edit({ triggers });
	const replaceAt = (list: Trigger[], i: number, tr: Trigger): Trigger[] => list.map((held, k) => (k === i ? tr : held));
	/** A trigger of another kind in the same place, keeping what the two kinds share. */
	function switchKind(i: number, tr: Trigger, kind: TriggerKind): void {
		const next = blankTrigger(kind);
		if ('weight' in tr && 'weight' in next) next.weight = tr.weight;
		setTriggers(replaceAt(t.triggers ?? [], i, next));
	}

	// ---- firing by hand: the playheads resting in `from` (anywhere, for `*`); one goes at once, several ask.
	const candidates = $derived(playheads.filter((p) => t && (t.from === '*' || String(variableValue(`${p}.state`) ?? m.playheads[p].start) === t.from)));
	let picker = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function fire(e: MouseEvent): void {
		const go = (p: string): void => void g.machine('machine fire', { machine: name, playhead: p, transition: id }).catch((err) => notify().failure('Fire', err));
		if (candidates.length === 1) return go(candidates[0]);
		const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
		picker = { x: r.left, y: r.bottom, items: candidates.map((p) => ({ label: p, icon: 'circle-dot', action: () => go(p) })) };
	}

	const KIND_SEGMENTS = TRIGGER_KINDS.map((id) => ({ id, label: id, testid: `trigger-kind-${id}` }));
	const POLICY_SEGMENTS = POLICIES.map((id) => ({ id, label: id }));
</script>

{#if picker}
	<ContextMenu x={picker.x} y={picker.y} items={picker.items} onClose={() => (picker = null)} />
{/if}

{#if t}
	<div class="form" data-testid="machine-transition">
		<div class="section-head">
			<Button variant="ghost" size="sm" disabled={candidates.length === 0}
				title={candidates.length === 0 ? 'No playhead is in its from state' : candidates.length === 1 ? `Fire it for ${candidates[0]}` : 'Fire it for one of the playheads there'}
				data-testid="transition-fire" onclick={fire}><Icon name="play" />fire</Button>
			<Button variant="danger" size="sm" data-testid="transition-remove" onclick={() => call('machine transition remove', {})}>delete</Button>
		</div>
		<ParamField paramName="duration" descriptor={numberDescriptor(t.duration, 0, 10, 0)} literal viewKey={`${name}/${id}/duration`}
			data-testid="transition-duration" onCommit={(v) => edit({ duration: Math.max(0, Number(v)) })} />
		<div class="fields">
			<Field label="curve" doc="How a value eases on the way, where no envelope carries it" row>
				<Select aria-label="curve" value={t.curve} options={[...CURVES]} onChange={(v) => edit({ curve: v })} />
			</Field>
		</div>

		<div class="section-head">
			<span class="section-name">triggers</span>
			<IconButton variant="ghost" size="sm" label="Add a trigger" title="Add a trigger" data-testid="transition-add-trigger" onclick={() => setTriggers([...(t.triggers ?? []), blankTrigger('manual')])}><Icon name="plus" /></IconButton>
		</div>
		{#if (t.triggers ?? []).length === 0}
			<div class="empty">Taken when its state's decision picks it, or by the fire button.</div>
		{/if}
		{#each t.triggers ?? [] as tr, i (i)}
			<div class="slot" class:alt={i % 2 === 1} data-testid="trigger-row">
				<div class="slot-head">
					<Segmented value={tr.kind} segments={KIND_SEGMENTS} onChange={(k) => switchKind(i, tr, k as TriggerKind)} aria-label="trigger kind" data-testid="trigger-kind" fill />
					<IconButton variant="ghost" size="sm" label="Remove trigger" title="Remove the trigger" onclick={() => setTriggers((t.triggers ?? []).filter((_, k) => k !== i))}><Icon name="x" /></IconButton>
				</div>
				{#if tr.kind === 'when'}
					<ExprEditor value={tr.expression} onCommit={(e) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, expression: e }))} label="when" placeholder="variables.sensor.level > 0.5" testid="trigger-when" />
					<ParamField paramName="weight" descriptor={numberDescriptor(tr.weight, 0, 10, 1)} literal viewKey={`${name}/${id}/${i}/weight`}
						onCommit={(v) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, weight: Math.max(0, Number(v)) }))} />
				{:else if tr.kind === 'meet'}
					<div class="fields">
						<Field label="policy" doc="Who goes when a second playhead arrives: the longest resident, the newest, or all" row>
							<Segmented value={tr.policy} segments={POLICY_SEGMENTS} onChange={(p) => setTriggers(replaceAt(t.triggers ?? [], i, { ...tr, policy: p as Policy }))} aria-label="policy" fill />
						</Field>
					</div>
				{/if}
			</div>
		{/each}
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
