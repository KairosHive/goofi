<!-- An attribute of the machine, for every state: its name, kind, range or options, default, the
     curve it eases by on a transition, and its removal. Opened from its row's cog in the score,
     dressed by the inspector's `.fields` grid. -->
<script lang="ts">
	import type { Attribute } from '$lib/api/generated';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { Button, CurvePicker, Field, NumberInput, Select, TextInput, Toggle } from '$lib/ui';
	import { FACES, descriptorFor, faceOf, literalOf, type Face } from './attributes';

	let { name, attr, a }: { name: string; attr: string; a: Attribute } = $props();

	const g = graph();
	function call(op: MachineOp, payload: Record<string, unknown>): void {
		void g.machine(op, { machine: name, name: attr, ...payload }).catch((e) => notify().failure(op, e));
	}
	/** A new face starts the default over, so the widget can always draw what it is handed. */
	function setFace(face: string): void {
		const f = FACES.find((f) => f.id === (face as Face))!;
		call('machine attribute edit', { kind: f.kind, value: f.born });
	}
	const editKind = (patch: Record<string, unknown>): void => call('machine attribute edit', { kind: { ...a.kind, ...patch } });
	/** The options typed as one line; a default they no longer hold moves to the first. */
	function setOptions(raw: string): void {
		const options = raw.split(',').map((o) => o.trim()).filter(Boolean);
		if (options.length === 0) return;
		const value = options.includes(String(a.default)) ? {} : { value: options[0] };
		call('machine attribute edit', { kind: { type: 'string', options }, ...value });
	}
</script>

<div class="attribute-form" data-testid={`attribute-${attr}`} data-face={faceOf(a)}>
	<div class="form-head">
		<span class="form-name">definition</span>
		<Button variant="danger" size="sm" title="Remove the attribute from every state and every playhead" data-testid="attribute-remove" onclick={() => call('machine attribute remove', {})}>remove</Button>
	</div>
	<div class="fields">
		<Field label="name" row>
			<TextInput inputmode="search" value={attr} aria-label="Attribute name" data-testid="attribute-name" autocomplete="off"
				onChange={(to) => { if (to.trim() !== attr && isValidIdentifier(to.trim())) call('machine attribute rename', { to: to.trim() }); }} />
		</Field>
		<Field label="kind" row>
			<Select data-testid="attribute-kind" aria-label="kind" value={faceOf(a)} options={FACES.map((f) => f.id)} onChange={setFace} />
		</Field>
		{#if a.kind.type === 'num'}
			<Field label="curve" doc="How the value eases over a transition's duration, from wherever it stands to the next state's" row>
				<CurvePicker value={a.curve} data-testid="attribute-curve" onChange={(curve) => call('machine attribute edit', { curve })} />
			</Field>
		{/if}
		{#if a.kind.type === 'num' && a.kind.face === 'plain'}
			<Field label="range" doc="The slider's bounds; a typed value may pass them" row>
				<span class="pair">
					<NumberInput value={a.kind.vmin} aria-label="min" onChange={(v) => editKind({ vmin: v })} />
					<NumberInput value={a.kind.vmax} aria-label="max" onChange={(v) => editKind({ vmax: v })} />
				</span>
			</Field>
			<Field label="integer" row>
				<Toggle value={a.kind.int} aria-label="integer" onChange={(v) => editKind({ int: v })} />
			</Field>
		{:else if a.kind.type === 'num' && a.kind.face === 'envelope'}
			<Field label="longest" doc="The seconds a time knob reaches" row>
				<NumberInput value={a.kind.vmax} min={0.01} aria-label="max" onChange={(v) => editKind({ vmax: Math.max(0.01, v) })} />
			</Field>
		{:else if a.kind.type === 'string' && a.kind.options}
			<Field label="options" doc="The choices, comma-separated" row>
				<TextInput inputmode="search" value={a.kind.options.join(', ')} aria-label="options" onChange={setOptions} />
			</Field>
		{/if}
	</div>
	<ParamField paramName={attr} label="default" descriptor={descriptorFor(a, a.default)} literal viewKey={`${name}/${attr}`}
		onCommit={(v) => call('machine attribute edit', { value: literalOf(v) })} />
</div>

<style>
	.attribute-form {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
	}
	.form-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
	}
	.form-name {
		color: var(--text-muted);
		font-size: var(--fs-small);
	}
	.pair {
		display: flex;
		gap: var(--space-2);
	}
</style>
