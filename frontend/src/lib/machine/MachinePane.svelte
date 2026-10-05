<!-- The machine's side pane: its name, its attributes, its playheads with their live values as
     draggable chips, and the picked transition's triggers, duration, curve and weight. -->
<script lang="ts">
	import type { Attribute, Literal, Machine, Policy, Trigger } from '$lib/api/generated';
	import { CONTROL_KINDS, type ControlKindId } from '$lib/api/vocab';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import { bornValue } from '$lib/panels/controlLayout';
	import ControlWidget from '$lib/panels/ControlWidget.svelte';
	import VariableGhost from '$lib/panels/VariableGhost.svelte';
	import { createVariableLift } from '$lib/panels/variableLift.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { Button, Chip, ColorPicker, Field, Icon, IconButton, NumberInput, ScrollArea, Select, TextInput } from '$lib/ui';
	import { dotColor, hexToRgba, rgbaToHex } from './layout';
	import { CURVES, POLICIES, TRIGGER_KINDS, blankTrigger, type TriggerKind } from './triggers';

	let {
		name,
		m,
		picked,
		onPick,
		onClose
	}: {
		name: string;
		m: Machine;
		/** The transition the pane edits, or null. */
		picked: string | null;
		onPick: (id: string | null) => void;
		onClose: () => void;
	} = $props();

	const g = graph();
	const states = $derived(Object.keys(m.states));
	const playheads = $derived(Object.keys(m.playheads));
	const attrs = $derived(Object.entries(m.attributes));
	const transition = $derived(picked !== null ? m.transitions[picked] : undefined);
	watchVariables(() => playheads.flatMap((p) => [`${p}.state`, ...attrs.map(([a]) => `${p}.${a}`)]));
	const lift = createVariableLift();

	function call(op: MachineOp, payload: Record<string, unknown>): Promise<unknown> {
		return g.machine(op, { machine: name, ...payload }).catch((e) => notify().failure(op, e));
	}
	function rename(op: MachineOp, from: string, raw: string, key = 'name'): void {
		const to = raw.trim();
		if (to !== from && isValidIdentifier(to)) void call(op, { [key]: from, to });
	}

	/** The widget kinds an attribute may wear: every one but a paint pad, which only strokes can fill. */
	const KINDS = CONTROL_KINDS.filter((k) => k.id !== 'paint');
	const fresh = (stem: string, taken: string[]): string => {
		let i = 0;
		while (taken.includes(`${stem}${i}`)) i += 1;
		return `${stem}${i}`;
	};
	/** A widget record for `kind`, with the range the number kinds carry. */
	function controlFor(kind: ControlKindId): Record<string, unknown> {
		const k = KINDS.find((k) => k.id === kind)!;
		return k.draws === 'number' || k.draws === 'vector' ? { kind, min: 0, max: 1, step: 0.01 } : kind === 'dropdown' ? { kind, options: [] } : { kind };
	}
	function setKind(attr: string, a: Attribute, kind: string): void {
		if (kind === 'none') {
			void call('machine attribute edit', { name: attr, control: null });
			return;
		}
		const k = KINDS.find((k) => k.id === kind)!;
		// A new kind starts the default over, so the widget can always draw what it is handed.
		const same = a.control && KINDS.find((c) => c.id === a.control!.kind)?.draws === k.draws;
		void call('machine attribute edit', { name: attr, control: controlFor(k.id), ...(same ? {} : { value: bornValue(k) }) });
	}

	function shown(v: Literal | null): string {
		return v === null ? '…' : typeof v === 'number' ? String(Math.round(v * 1000) / 1000) : typeof v === 'string' ? v : JSON.stringify(v);
	}

	function setTriggers(triggers: Trigger[]): void {
		if (picked !== null) void call('machine transition edit', { id: picked, triggers });
	}
	function editTransition(patch: Record<string, unknown>): void {
		if (picked !== null) void call('machine transition edit', { id: picked, ...patch });
	}
	const replaceAt = (list: Trigger[], i: number, t: Trigger): Trigger[] => list.map((held, k) => (k === i ? t : held));
</script>

<aside class="pane" data-testid="machine-pane">
	<ScrollArea>
		<div class="body">
			<section class="block">
				<div class="row">
					<Field label="machine" class="grow">
						<TextInput inputmode="search" data-testid="machine-name" value={name} autocomplete="off" onChange={(v) => rename('machine rename', name, v, 'machine')} />
					</Field>
					<IconButton variant="ghost" size="sm" label="Close the pane" title="Close the pane" data-testid="machine-pane-close" onclick={onClose}><Icon name="x" /></IconButton>
				</div>
				<div class="row">
					<Field label="seed" doc="Starts the random draws, so a run repeats">
						<NumberInput value={m.seed ?? 0} min={0} step={1} aria-label="seed" onChange={(v) => void call('machine edit', { seed: Math.max(0, Math.round(v)) })} />
					</Field>
					<Chip tone="danger" data-testid="machine-remove" onclick={() => void call('machine remove', {})}>delete machine</Chip>
				</div>
			</section>

			<section class="block" data-testid="machine-attributes">
				<h3>attributes</h3>
				{#each attrs as [attr, a] (attr)}
					<div class="attr" data-testid={`attribute-${attr}`}>
						<div class="row">
							<div class="grow">
								<TextInput inputmode="search" data-testid="attribute-name" value={attr} autocomplete="off" aria-label="Attribute name" onChange={(v) => rename('machine attribute rename', attr, v)} />
							</div>
							<Select data-testid="attribute-kind" aria-label="widget" value={a.control?.kind ?? 'none'} options={['none', ...KINDS.map((k) => k.id)]} onChange={(v) => setKind(attr, a, v)} />
							<IconButton variant="ghost" size="sm" label={`Remove ${attr}`} title="Remove the attribute" data-testid="attribute-remove" onclick={() => void call('machine attribute remove', { name: attr })}><Icon name="x" /></IconButton>
						</div>
						<div class="row default">
							<span class="muted">default</span>
							<div class="grow widget">
								{#if a.control}
									<ControlWidget control={a.control} value={a.default} label={`${attr} default`} onChange={(v) => void call('machine attribute edit', { name: attr, value: v })} />
								{:else}
									<TextInput inputmode="search" value={JSON.stringify(a.default)} aria-label={`${attr} default`} onChange={(raw) => { try { void call('machine attribute edit', { name: attr, value: JSON.parse(raw) }); } catch { void call('machine attribute edit', { name: attr, value: raw }); } }} />
								{/if}
							</div>
						</div>
					</div>
				{/each}
				<Button class="add-row" variant="ghost" size="sm" data-testid="machine-add-attribute"
					onclick={() => void call('machine attribute add', { name: fresh('attr', attrs.map(([a]) => a)), value: 0, control: controlFor('knob') })}><Icon name="plus" />attribute</Button>
			</section>

			<section class="block" data-testid="machine-playheads">
				<h3>playheads</h3>
				{#each playheads as p, i (p)}
					{@const ph = m.playheads[p]}
					<div class="playhead" data-testid={`playhead-${p}`}>
						<div class="row">
							<ColorPicker value={hexToRgba(dotColor(ph.color, i))} aria-label={`${p} colour`} onChange={(rgba) => void call('machine playhead edit', { name: p, color: rgbaToHex(rgba) })} />
							<div class="grow">
								<TextInput inputmode="search" data-testid="playhead-name" value={p} autocomplete="off" aria-label="Playhead name" onChange={(v) => rename('machine playhead rename', p, v)} />
							</div>
							<IconButton variant="ghost" size="sm" label={`Remove ${p}`} title="Remove the playhead and its variables" data-testid="playhead-remove" onclick={() => void call('machine playhead remove', { name: p })}><Icon name="x" /></IconButton>
						</div>
						<div class="row">
							<span class="muted">start</span>
							<Select data-testid="playhead-start" aria-label="start state" value={ph.start} options={states} onChange={(v) => void call('machine playhead edit', { name: p, start: v })} />
							<span class="muted">in</span>
							<span class="mono" data-testid="playhead-state">{String(variableValue(`${p}.state`) ?? ph.start)}</span>
						</div>
						<div class="chips">
							{#each attrs as [attr] (attr)}
								<Chip
									density="chrome"
									data-testid={`playhead-chip-${p}-${attr}`}
									title="Drag onto a parameter to set its expression"
									onpointerdown={(e) => lift.start(e, `${p}.${attr}`)}
									onpointermove={lift.move}
									onpointerup={(e) => lift.drop(e)}
									onpointercancel={lift.cancel}
									onlostpointercapture={lift.cancel}>{attr} <b>{shown(variableValue(`${p}.${attr}`))}</b></Chip
								>
							{/each}
						</div>
					</div>
				{/each}
				<Button class="add-row" variant="ghost" size="sm" data-testid="machine-add-playhead" disabled={states.length === 0}
					title={states.length === 0 ? 'Add a state first' : 'Add a playhead in the first state'}
					onclick={() => void call('machine playhead add', { start: states[0], color: dotColor('', playheads.length) })}><Icon name="plus" />playhead</Button>
			</section>

			{#if picked !== null && transition}
				{@const t = transition}
				<section class="block" data-testid="machine-transition">
					<div class="row">
						<h3 class="grow">transition {picked}</h3>
						<IconButton variant="ghost" size="sm" label="Done" title="Done" data-testid="transition-done" onclick={() => onPick(null)}><Icon name="check" /></IconButton>
					</div>
					<div class="row">
						<Select aria-label="from" value={t.from} options={['*', ...states]} labels={{ '*': 'any state' }} onChange={(v) => editTransition({ from: v })} />
						<span class="muted">→</span>
						<Select aria-label="to" value={t.to} options={states} onChange={(v) => editTransition({ to: v })} />
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
							<NumberInput value={t.duration} min={0} step={0.1} aria-label="duration" onChange={(v) => editTransition({ duration: Math.max(0, v) })} />
						</Field>
						<Field label="curve">
							<Select aria-label="curve" value={t.curve} options={[...CURVES]} onChange={(v) => editTransition({ curve: v })} />
						</Field>
						<Field label="weight" doc="The draw among transitions firing together; 0 is never drawn">
							<NumberInput value={t.weight} min={0} step={0.1} aria-label="weight" onChange={(v) => editTransition({ weight: Math.max(0, v) })} />
						</Field>
					</div>
					<Chip tone="danger" data-testid="transition-remove" onclick={() => { const id = picked; onPick(null); void call('machine transition remove', { id }); }}>delete transition</Chip>
				</section>
			{/if}
		</div>
	</ScrollArea>
	<VariableGhost {lift} />
</aside>

<style>
	.pane {
		display: flex;
		flex-direction: column;
		min-width: 0;
		min-height: 0;
		background: var(--surface-1);
		border-left: 1px solid var(--border);
	}
	.body {
		display: flex;
		flex-direction: column;
		gap: var(--space-4);
		padding: var(--space-3);
	}
	.block {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}
	h3,
	h4 {
		margin: 0;
		font-size: var(--fs-small);
		font-weight: 600;
		color: var(--text-muted);
	}
	.row {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-width: 0;
		--number-width: 100%;
	}
	.grow {
		flex: 1;
		min-width: 0;
	}
	.pane :global(.grow) {
		flex: 1;
		min-width: 0;
	}
	.attr,
	.playhead,
	.trigger {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		padding: var(--space-2);
		border-radius: var(--radius-sm);
		background: var(--surface-2);
	}
	.widget {
		display: flex;
		align-items: center;
	}
	.muted {
		color: var(--text-muted);
		font-size: var(--fs-small);
		white-space: nowrap;
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
		gap: var(--space-1);
		touch-action: none;
	}
	.chips b {
		font-weight: 400;
		color: var(--text-muted);
	}
	.body :global(.add-row) {
		width: 100%;
		justify-content: center;
	}
</style>
