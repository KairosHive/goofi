<!-- One transition's settings, open inside its state's transition list or in the arc's popover:
     the buttons that fire it by hand and delete it, the condition that takes it on its own, and
     how long the move takes; each attribute eases over it by its own curve. -->
<script lang="ts">
	import type { Machine } from '$lib/api/generated';
	import { ContextMenu, type MenuItem } from 'panelty';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import { Button, Field, Icon } from '$lib/ui';
	import { numberDescriptor } from './attributes';

	let { name, m, id }: { name: string; m: Machine; id: string } = $props();

	const g = graph();
	const t = $derived(m.transitions[id]);
	const playheads = $derived(Object.keys(m.playheads));
	watchVariables(() => playheads.map((p) => `${p}.state`));

	const call = (op: MachineOp, patch: Record<string, unknown>): void => void g.machine(op, { machine: name, id, ...patch }).catch((e) => notify().failure(op, e));
	const edit = (patch: Record<string, unknown>): void => call('machine transition edit', patch);
	// ---- firing by hand: the playheads resting in `from` (anywhere, for `*`); one goes at once, several ask.
	const candidates = $derived(playheads.filter((p) => t && (t.from === '*' || String(variableValue(`${p}.state`) ?? m.playheads[p].start) === t.from)));
	let picker = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function fire(e: MouseEvent): void {
		const go = (p: string): void => void g.machine('machine fire', { machine: name, playhead: p, transition: id }).catch((err) => notify().failure('Fire', err));
		if (candidates.length === 1) return go(candidates[0]);
		const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
		picker = { x: r.left, y: r.bottom, items: candidates.map((p) => ({ label: p, icon: 'circle-dot', action: () => go(p) })) };
	}
</script>

{#if picker}
	<ContextMenu x={picker.x} y={picker.y} items={picker.items} onClose={() => (picker = null)} />
{/if}

{#if t}
	<div class="form" data-testid="machine-transition">
		<div class="head">
			<span class="id">{id}</span>
			<Button variant="ghost" size="sm" disabled={candidates.length === 0}
				title={candidates.length === 0 ? 'No playhead is in its from state' : candidates.length === 1 ? `Fire it for ${candidates[0]}` : 'Fire it for one of the playheads there'}
				data-testid="transition-fire" onclick={fire}><Icon name="play" />fire</Button>
			<Button variant="danger" size="sm" title="Delete the transition" data-testid="transition-remove" onclick={() => call('machine transition remove', {})}>delete</Button>
		</div>
		<Field label="condition" doc="Taken the instant this turns true, for each playhead resting here; empty, the decision and the fire button alone take it">
			<ExprEditor machine={name} label="condition" value={t.condition ?? ''} testid="transition-condition" onCommit={(condition) => edit({ condition })} />
		</Field>
		<ParamField paramName="duration" descriptor={numberDescriptor(t.duration, 0, 10, 0)} literal viewKey={`${name}/${id}/duration`}
			data-testid="transition-duration" onCommit={(v) => edit({ duration: Math.max(0, Number(v)) })} />
	</div>
{/if}

<style>
	.form {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		min-width: 0;
	}
	.head {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}
	.id {
		flex: 1 1 auto;
		color: var(--text-muted);
		font-family: var(--font-mono);
		font-size: var(--fs-small);
	}
</style>
