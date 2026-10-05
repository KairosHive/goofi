<!-- The same authored duration and modes for a timer and a transition's travel. -->
<script lang="ts">
	import type { Seconds } from '$lib/api/generated';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import { Field, NumberInput, Segmented } from '$lib/ui';
	import { useLiveValue } from '$lib/ui/liveValue.svelte';
	import { numberDescriptor } from './attributes';

	let { label, value: source, viewKey, onCommit, fallback = 1, testid }: {
		label: string; value: Seconds; viewKey: string; onCommit: (value: Seconds) => void;
		fallback?: number; testid?: string;
	} = $props();
	const live = useLiveValue(() => source, (v) => onCommit(v));
	const value = $derived(live.value);
	const mode = $derived(typeof value === 'number' ? 'fixed' : typeof value === 'string' ? 'expression' : 'range');
</script>

<div class="fields">
	<Field {label} doc="Seconds, an expression sampled once, or a seeded uniform random range" row>
		<Segmented value={mode} segments={[{ id: 'fixed', label: 'fixed' }, { id: 'expression', label: 'expression' }, { id: 'range', label: 'random range' }]}
			onChange={(mode) => onCommit(mode === 'fixed' ? fallback : mode === 'expression' ? String(fallback) : { min: fallback, max: Math.max(1, fallback * 2) })}
			aria-label={`${label} mode`} fill />
	</Field>
</div>
{#if typeof value === 'number'}
	<ParamField paramName={label} descriptor={numberDescriptor(value, 0, Math.max(10, value), fallback)} literal {viewKey}
		data-testid={testid} onCommit={(v) => live.commit(Math.max(0, Number(v)))} />
{:else if typeof value === 'string'}
	<ExprEditor {value} onCommit={live.commit} {label} placeholder="variables.desk.seconds" testid={testid ?? 'duration-expression'} />
{:else}
	<div class="fields" data-testid={testid}>
		<Field label="minimum" row><NumberInput value={value.min} min={0} max={value.max} step={0.01} aria-label={`${label} minimum`}
			onChange={(min) => { if (typeof value === 'object') live.commit({ min, max: Math.max(min, value.max) }); }} /></Field>
		<Field label="maximum" row><NumberInput value={value.max} min={value.min} step={0.01} aria-label={`${label} maximum`}
			onChange={(max) => { if (typeof value === 'object') live.commit({ min: value.min, max: Math.max(value.min, max) }); }} /></Field>
	</div>
{/if}
