<!--
  ParamField — one inspector row: the name, and the one control `controlKind(descriptor)` chooses.
  That control is the row in EVERY mode — driven, it is disabled and reads out what the source
  produces, since the param's own face is what a reader recognises. The name opens a second row
  holding the three-way switch — constant, expression, reference — and the editor of whichever
  source is active. `vmin/vmax` are SOFT bounds: they scope only the Slider's track, and the
  NumberInput beside it commits what is typed.
-->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import type { ParamDescriptor, ParamMode, SourcePatch } from '$lib/api/types';
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
	import { controlKind } from './controlKind';
	import { literalFor } from './paramSeed';
	import ExprEditor from './expr/ExprEditor.svelte';
	import RefPicker from './RefPicker.svelte';

	let {
		paramName,
		descriptor,
		onCommit,
		onSetSource,
		onRefresh,
		onPulse,
		refreshing = false,
		selfName,
		dropZone = null,
		class: klass = '',
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		paramName: string;
		descriptor: ParamDescriptor;
		onCommit: (value: unknown) => void;
		onSetSource: (source: SourcePatch) => void;
		onRefresh?: () => void;
		onPulse?: () => void;
		refreshing?: boolean;
		/** The node's display name, handed to the expression editor as `me`. */
		selfName?: string;
		/** This row's key as a node-drop target; null while it is not one. */
		dropZone?: string | null;
	} = $props();

	const uiStore = ui();
	let row = $state<HTMLDivElement>();
	const over = $derived(
		(dropZone !== null && uiStore.nodeDragZone === dropZone) ||
		(uiStore.globalDrag !== null && uiStore.globalDrag.target === row)
	);

	function acceptGlobal(el: HTMLDivElement): { destroy(): void } {
		const drop = (event: Event): void => {
			picking = false;
			onSetSource({ expression: (event as CustomEvent<string>).detail });
		};
		el.addEventListener('global-expression-drop', drop);
		return { destroy: () => el.removeEventListener('global-expression-drop', drop) };
	}

	const kind = $derived(controlKind(descriptor));

	let open = $state(false);

	// `step` is computed against the SAME auto-extended bounds the Slider uses; a native `'any'` would
	// NaN the NumberInput's scrub arithmetic.
	const num = $derived(descriptor.type === 'float' || descriptor.type === 'int' ? descriptor : null);
	const lo = $derived(num ? Math.min(num.vmin, num.value) : 0);
	const hi = $derived(num ? Math.max(num.vmax, num.value) : 1);
	const step = $derived(num ? (num.type === 'int' ? 1 : Math.max((hi - lo) / 200, 1e-6)) : 1);

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

	function choose(mode: ParamMode): void {
		picking = false;
		if (mode === descriptor.mode) return;
		if (mode === 'expression' && !descriptor.expression) {
			onSetSource({ expression: literalFor(descriptor) });
		} else if (mode === 'reference' && !descriptor.reference) {
			picking = true;
		} else {
			onSetSource({ mode });
		}
	}
</script>

<div
	class={`pf-param ${klass}`.trim()}
	bind:this={row}
	use:acceptGlobal
	data-global-drop
	class:armed={dropZone !== null || uiStore.globalDrag !== null}
	class:over
	data-node-drop={dropZone}
	{...rest}
>
	<Field
		label={paramName}
		doc={descriptor.doc ?? undefined}
		expanded={open}
		onExpand={() => (open = !open)}
	>
		<!-- `display: contents` so the face inherits WITHOUT laying out: Field requires paired controls to
		     be its direct children, and a real box would take them out of the @container column-flip. -->
		<div class="pf-value">
			{#if num}
				<!-- SOFT bounds → Slider only; the NumberInput is UNBOUNDED (the engine does not clamp on set). -->
				<Slider
					value={num.value}
					onChange={onCommit}
					min={num.vmin}
					max={num.vmax}
					{step}
					disabled={driven}
					data-testid="param-slider"
				/>
				<NumberInput
					value={num.value}
					onChange={onCommit}
					{step}
					scrub
					disabled={driven}
					data-testid="param-number"
				/>
			{:else if kind === 'toggle'}
				<Toggle
					value={Boolean(descriptor.value)}
					onChange={onCommit}
					disabled={driven}
					data-testid="param-toggle"
				/>
			{:else if kind === 'select'}
				<!-- A non-refreshable dropdown passes no `onRefresh`, so the Select renders no ⟳. -->
				<Select
					{options}
					value={String(descriptor.value)}
					onChange={onCommit}
					onRefresh={descriptor.refreshable ? onRefresh : undefined}
					{refreshing}
					disabled={driven}
					refreshTestid="param-refresh"
					data-testid="param-select"
				/>
			{:else if kind === 'text'}
				<TextInput
					value={String(descriptor.value)}
					onChange={onCommit}
					disabled={driven}
					data-testid="param-text"
				/>
			{:else if kind === 'pulse'}
				<!-- A pulse holds no value to read out, so a driven one keeps its button: firing one by
				     hand is a request, and the source fires on its own edges. -->
				<Button class="pf-pulse" title="Fire one pulse" onclick={onPulse} data-testid="param-pulse">
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
			<!-- `picking` lights the reference segment before one is committed, whatever the mode says. -->
			<Segmented
				value={picking ? 'reference' : descriptor.mode}
				bad={!!descriptor.error}
				segments={[
					{
						id: 'constant',
						label: 'C',
						name: 'Constant',
						title: 'Constant — a value set here by hand, unchanging until you edit it',
						testid: 'param-mode-constant'
					},
					{
						id: 'expression',
						label: 'E',
						name: 'Expression',
						title: 'Expression — Python over nd(), globals and me, evaluated at control rate',
						testid: 'param-mode-expression'
					},
					{
						id: 'reference',
						label: 'R',
						name: 'Reference',
						title: "Reference — one node's output slot, followed at that node's rate",
						testid: 'param-mode-reference'
					}
				]}
				onChange={(m) => choose(m as ParamMode)}
				aria-label={`${paramName} source`}
				data-testid="param-mode"
			/>
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
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		min-width: 0;
	}
	/* An `outline` rather than a border: a row that becomes a target must not move the rows under it. */
	.pf-param.armed {
		outline: 1px dashed var(--border-strong);
		outline-offset: var(--space-2);
		border-radius: var(--radius-sm);
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
