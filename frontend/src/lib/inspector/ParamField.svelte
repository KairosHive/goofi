<!-- One param row: the control controlKind chooses, disabled when a source drives it; the expander
     holds the source switch and editor. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import { PARAM_MODES, type ParamDescriptor, type ParamMode, type SourcePatch } from '$lib/api/types';
	import {
		Field,
		Slider,
		NumberInput,
		Toggle,
		Select,
		TextInput,
		Button,
		Icon,
		Segmented
	} from '$lib/ui';
	import { ui } from '$lib/stores/ui.svelte';
	import { stepOf } from '$lib/ui/knob';
	import { controlKind, isNumeric } from './controlKind';
	import { MODE_FACE, sourceForMode } from './paramSeed';
	import ExprEditor from './expr/ExprEditor.svelte';
	import MidiLearn from './MidiLearn.svelte';
	import RefPicker from './RefPicker.svelte';

	let {
		paramName,
		label = paramName,
		descriptor,
		onCommit,
		onPreview,
		onSetSource,
		onRefresh,
		onPulse,
		refreshing = false,
		selfName,
		dropZone = null,
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		paramName: string;
		/** What the row is called; a slot of a list wears its name inside the section. */
		label?: string;
		descriptor: ParamDescriptor;
		onCommit: (value: unknown) => unknown;
		/** A step of a drag on the value, ahead of its commit. */
		onPreview?: (value: number) => unknown;
		onSetSource: (source: SourcePatch) => void;
		onRefresh?: () => void;
		onPulse?: () => void;
		refreshing?: boolean;
		/** The node's display name, handed to the expression editor as `me`. */
		selfName?: string;
		/** This row's key as a node-drop target; null while it is not one. */
		dropZone?: string | null;
	} = $props();

	const learnId = $props.id();
	const uiStore = ui();
	let row = $state<HTMLDivElement>();
	const over = $derived(
		(dropZone !== null && uiStore.nodeDragOver === dropZone) ||
		(uiStore.variableDrag !== null && uiStore.variableDrag.target === row)
	);

	function acceptVariable(el: HTMLDivElement): { destroy(): void } {
		const drop = (event: Event): void => {
			picking = false;
			onSetSource({ expression: (event as CustomEvent<string>).detail });
		};
		el.addEventListener('variable-expression-drop', drop);
		return { destroy: () => el.removeEventListener('variable-expression-drop', drop) };
	}

	const kind = $derived(controlKind(descriptor));

	let open = $state(false);

	// `step` uses the declared bounds; a native `'any'` would
	// NaN the NumberInput's scrub arithmetic.
	const num = $derived(isNumeric(descriptor) ? descriptor : null);
	const step = $derived(num ? (num.type === 'int' ? 1 : stepOf(num.vmin, num.vmax)) : 1);

	const options = $derived(descriptor.type === 'string' ? (descriptor.options ?? []) : []);

	const driven = $derived(descriptor.mode !== 'constant');
	// A reference chosen before one is retained shows the picker without a record to show yet.
	let picking = $state(false);
	// The MODE, not the kind: a pulse is a button in every mode, so its kind cannot name its source.
	const showPicker = $derived(descriptor.mode === 'reference' || picking);
	const showSource = $derived(showPicker || descriptor.mode === 'expression');
	// The error and preview belong to a source that IS live: a picker over a retained expression
	// shows neither.
	const shown = $derived(descriptor.mode === 'reference' || (descriptor.mode === 'expression' && !picking));
	$effect(() => {
		if (descriptor.mode === 'reference') picking = false;
	});

	const MODE_TITLE: Record<ParamMode, string> = {
		constant: 'a value set here by hand, unchanging until you edit it',
		expression: 'Python over nd(), variables and me, evaluated at control rate',
		reference: "one node's output slot, followed at that node's rate"
	};

	function choose(mode: ParamMode): void {
		picking = false;
		if (mode === descriptor.mode) return;
		if (mode === 'reference' && !descriptor.reference) {
			picking = true;
		} else {
			onSetSource(sourceForMode(descriptor, mode));
		}
	}
</script>

<div
	class="pf-param"
	bind:this={row}
	use:acceptVariable
	data-variable-drop
	class:armed={dropZone !== null || uiStore.variableDrag !== null}
	class:over
	class:open
	data-node-drop={dropZone}
	{...rest}
>
	<Field
		{label}
		doc={descriptor.doc ?? undefined}
		expanded={open}
		stretchSummary
		onExpand={() => (open = !open)}
	>
		<!-- `display: contents` so the face inherits WITHOUT laying out: Field requires paired controls to
		     be its direct children, and a real box would take them out of the @container column-flip. -->
		<div class="pf-value">
			{#if num}
				<!-- SOFT bounds → Slider only; the NumberInput is UNBOUNDED (the engine does not clamp on set). -->
				{#if num.type === 'int' && num.options?.length}
					<Segmented
						value={String(num.value)}
						segments={num.options.map((value) => ({ id: String(value), label: String(value) }))}
						onChange={(value) => onCommit(Number(value))}
						disabled={driven}
						data-testid="param-options"
						fill
					/>
				{:else}
				<Slider
					value={num.value}
					onChange={onCommit}
					onInput={onPreview}
					min={num.vmin}
					max={num.vmax}
					{step}
					disabled={driven}
					data-testid="param-slider"
				/>
				{/if}
				<NumberInput
					value={num.value}
					onChange={onCommit}
					onInput={onPreview}
					{step}
					scrub
					disabled={driven}
					data-param-edit
					data-testid="param-number"
				/>
			{:else if kind === 'toggle'}
				<Toggle
					value={Boolean(descriptor.value)}
					onChange={onCommit}
					disabled={driven}
					data-param-edit
					data-testid="param-toggle"
				/>
			{:else if kind === 'select'}
				<Select
					{options}
					value={String(descriptor.value)}
					onChange={onCommit}
					onRefresh={descriptor.refreshable ? onRefresh : undefined}
					{refreshing}
					disabled={driven}
					refreshTestid="param-refresh"
					data-param-edit
					data-testid="param-select"
				/>
			{:else if kind === 'text'}
				<TextInput
					value={String(descriptor.value)}
					onChange={onCommit}
					disabled={driven}
					data-param-edit
					data-testid="param-text"
				/>
			{:else if kind === 'pulse'}
				<!-- A pulse holds no value to read out, so a driven one keeps its button: firing one by
				     hand is a request, and the source fires on its own edges. -->
				<Button class="pf-pulse" title="Fire one pulse" onclick={onPulse} data-param-edit data-testid="param-pulse">
					pulse
				</Button>
			{:else if kind === 'unknown'}
				<code class="unknown" data-testid="param-unknown">{JSON.stringify(descriptor.value)}</code>
			{/if}
		</div>
	</Field>

	{#if open}
		<div class="pf-more" data-testid="param-more">
			{#if showSource}
				<div class="src-region">
					{#if showPicker}
						<RefPicker
							value={descriptor.reference}
							paramType={descriptor.type}
							onCommit={(reference) => onSetSource({ reference })}
							testid="param-ref"
						/>
					{:else}
						<ExprEditor
							{selfName}
							value={descriptor.expression ?? ''}
							error={descriptor.error}
							onCommit={(expression) => onSetSource({ expression })}
							label={`${paramName} expression`}
							placeholder="nd('oscillator0').out.data.mean()"
							testid="param-expr-input"
						/>
					{/if}
				</div>
			{/if}
			{#if driven}
				<Segmented
					value={descriptor.triggers ? 'trig' : null}
					segments={[
						{
							id: 'trig',
							label: 'trig',
							name: 'Trigger',
							title: "Trigger — wake the node's process() each time this source changes",
							testid: 'param-triggers'
						}
					]}
					onChange={() => onSetSource({ triggers: !descriptor.triggers })}
				/>
			{/if}
			<Segmented
				value={picking ? 'reference' : descriptor.mode}
				bad={!!descriptor.error}
				segments={PARAM_MODES.map((id) => ({
					id,
					...MODE_FACE[id],
					title: `${MODE_FACE[id].name} — ${MODE_TITLE[id]}`,
					testid: `param-mode-${id}`
				}))}
				onChange={(m) => choose(m as ParamMode)}
				aria-label={`${paramName} source`}
				data-testid="param-mode"
			/>
			{#if num}
				<MidiLearn label={paramName} target={`param:${learnId}`}
					onLearn={(reference, index) => onSetSource({ reference: `${reference}[${index}]` })} />
			{/if}
		</div>
	{/if}
	<!-- Shown whether or not the source is unfolded: the value beside it is the literal standing in,
	     and a failure is not a readout. -->
	{#if shown && descriptor.error}
		<div class="src-error" title={descriptor.error} data-testid="param-source-error">
			<span class="prefix"><Icon name="triangle-alert" /></span>
			<span class="msg">{descriptor.error}</span>
		</div>
	{/if}
</div>

<style>
	.pf-param {
		position: relative;
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: var(--space-3);
		margin-inline: calc(0px - var(--space-3));
		isolation: isolate;
		border-radius: var(--radius-sm);
		min-width: 0;
	}
	.pf-param.open {
		background: var(--surface-2);
	}
	.pf-value > :global(*),
	.pf-more > :global(*) {
		position: relative;
		pointer-events: auto;
	}
	/* An `outline` rather than a border: a row that becomes a target must not move the rows under it. */
	.pf-param.armed {
		outline: 1px dashed var(--border-strong);
		outline-offset: var(--space-2);
	}
	.pf-param.over {
		outline: 1px solid var(--accent);
		background: var(--accent-fill);
	}
	/* Values are data; the label above them is chrome. Box-less, so the controls inside stay Field's
	   own direct children. */
	.pf-value {
		display: contents;
		font-family: var(--font-mono);
		/* Narrower than the primitive's default: a param row seats a slider and a number beside a name,
		   and the number is the one with slack to give. */
		--number-width: 4rem;
	}
	/* A pulse has no value beside it, so the whole row is the target — and it takes the rung above
	   the fields around it, so a press target never reads as one more box to type in. */
	.pf-value :global(.pf-pulse) {
		flex: 1 1 auto;
		background: var(--surface-3);
		border-color: var(--border-strong);
	}
	/* The switches sit at the row's end, under the widget they act on; the reading they explain
	   leads the row. */
	.pf-more {
		pointer-events: none;
		display: flex;
		align-items: center;
		justify-content: flex-end;
		flex-wrap: wrap;
		gap: var(--space-3);
	}
	/* Wide enough to type an expression in, and the first thing to take its own line when the pane
	   is narrower than that. */
	.src-region {
		flex: 1 1 14rem;
		min-width: 0;
		display: flex;
	}
	.src-error {
		display: flex;
		align-items: baseline;
		gap: var(--space-2);
		min-width: 0;
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		padding: 0 var(--space-1);
		color: var(--danger);
	}
	.src-error .prefix {
		flex-shrink: 0;
	}
	.src-error .msg {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		min-width: 0;
	}
	.unknown {
		font-size: var(--fs-micro);
		color: var(--text-muted);
	}
</style>
