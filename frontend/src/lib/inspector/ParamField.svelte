<!-- One param row: the control controlKind chooses, disabled when a source drives it; the expander
     holds the source switch and editor. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import { PARAM_MODES, type ParamDescriptor, type ParamMode, type SourcePatch } from '$lib/api/types';
	import {
		Field,
		Slider,
		ColorPicker,
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
	import { numValue, numValues } from '$lib/api/types';
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
		onCommitElement,
		onSetElementSource,
		onRefresh,
		onPulse,
		refreshing = false,
		selfName,
		dropZone = null,
		viewKey = '',
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		paramName: string;
		/** What the row is called; a slot of a list wears its name inside the section. */
		label?: string;
		descriptor: ParamDescriptor;
		onCommit: (value: unknown) => unknown;
		/** A step of a drag on the value, ahead of its commit. */
		onPreview?: (value: number | number[]) => unknown;
		onSetSource: (source: SourcePatch) => void;
		/** One element of a vector: its literal into its dimension, and its own source. */
		onCommitElement?: (index: number, value: number) => unknown;
		onSetElementSource?: (index: number, source: SourcePatch) => void;
		onRefresh?: () => void;
		onPulse?: () => void;
		refreshing?: boolean;
		/** The node's display name, handed to the expression editor as `me`. */
		selfName?: string;
		/** This row's key as a node-drop target; null while it is not one. */
		dropZone?: string | null;
		/** `uid/group/name`, under which a colour's view (picker or vector) is remembered. */
		viewKey?: string;
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
	/** A colour opened as a vector shows and expands as one; the value under both is the same list. */
	const asVector = $derived(kind === 'color' && uiStore.paramView[viewKey] === 'vector');
	const dims = $derived(isNumeric(descriptor) ? numValues(descriptor).length : 1);
	/** Each element's own source, which a vector row shows beside its number. */
	const elements = $derived(descriptor.elements ?? []);
	const elementDriven = (i: number): boolean => (elements[i]?.mode ?? 'constant') !== 'constant';
	// A colour with ANY element driven is driven as a whole: the picker would write a literal the
	// driven channel ignores, and snap to what came back. The vector rows show which one it is.
	const anyElementDriven = $derived(elements.some((_, i) => elementDriven(i)));
	const elementName = (i: number): string => (kind === 'color' ? ['R', 'G', 'B', 'A'][i] : `[${i}]`);
	// The element a reference is being picked for, before one is retained.
	let pickingElement = $state<number | null>(null);
	function chooseElement(i: number, mode: ParamMode): void {
		pickingElement = null;
		const el = elements[i];
		if (!el || mode === el.mode) return;
		if (mode === 'reference' && !el.reference) {
			pickingElement = i;
			return;
		}
		const seed = { ...descriptor, value: numValues(num!)[i], expression: el.expression } as ParamDescriptor;
		onSetElementSource?.(i, sourceForMode(seed, mode));
	}
	/** A typed `[a, b, c, d]`, or bare numbers apart; short lists fill with zero, long ones are cut. */
	function parseList(text: string): number[] | null {
		const parts = text.replace(/[[\]]/g, '').split(/[,\s]+/).filter(Boolean).map(Number);
		if (parts.length === 0 || parts.some((n) => !Number.isFinite(n))) return null;
		return Array.from({ length: dims }, (_, i) => parts[i] ?? 0);
	}

	let open = $state(false);

	// `step` uses the declared bounds; a native `'any'` would
	// NaN the NumberInput's scrub arithmetic.
	const num = $derived(isNumeric(descriptor) ? descriptor : null);
	const step = $derived(num ? (num.int ? 1 : stepOf(num.vmin, num.vmax)) : 1);
	/** The vector with one dimension replaced, which is what a dimension's control commits. */
	const withDim = (i: number, v: number): number[] => numValues(num!).map((held, k) => (k === i ? v : held));

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
	<!-- Which face a colour wears, C or V: the picker over its list, or one number and one source
	     per element. It sits at the row's end in both, so the switch back is where the switch was. -->
	{#snippet viewSwitch()}
		<Segmented
			class="pf-view"
			value={asVector ? 'vector' : 'color'}
			segments={[
				{ id: 'color', label: 'C', name: 'Colour', title: 'Colour — the picker, over the four values as one list', testid: 'param-view-color' },
				{ id: 'vector', label: 'V', name: 'Vector', title: 'Vector — one row per element, each with a source of its own', testid: 'param-view-vector' }
			]}
			onChange={(v) => {
				if (v === 'vector') uiStore.paramView[viewKey] = 'vector';
				else delete uiStore.paramView[viewKey];
			}}
			aria-label={`${paramName} view`}
			data-testid="param-view"
		/>
	{/snippet}

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
			{#if num && kind === 'color' && !asVector}
				<ColorPicker
					value={numValues(num)}
					onChange={onCommit}
					onInput={onPreview}
					disabled={driven || anyElementDriven}
					title={anyElementDriven ? 'An element has a source of its own; see the vector view' : undefined}
					data-param-edit
					data-testid="param-color"
				/>
				{@render viewSwitch()}
			{:else if num && (kind === 'vector' || asVector)}
				<!-- One number per dimension; each commits the whole vector. -->
				{#each numValues(num) as held, i (i)}
					<NumberInput
						value={held}
						onChange={(v) => onCommit(withDim(i, v))}
						{step}
						scrub
						disabled={driven || elementDriven(i)}
						data-param-edit
						data-testid={`param-number-${i}`}
					/>
				{/each}
				{#if kind === 'color'}{@render viewSwitch()}{/if}
			{:else if num}
				<!-- SOFT bounds → Slider only; the NumberInput is UNBOUNDED (the engine does not clamp on set). -->
				{#if num.int && num.options?.length}
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
					value={numValue(num)}
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
					value={numValue(num)}
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
			{#if num && dims > 1 && (asVector || kind === 'vector')}
				<!-- One row per element: its number, and a source of its own that drives that dimension. -->
				<div class="pf-elements" data-testid="param-elements">
					{#each numValues(num) as held, i (i)}
						{@const el = elements[i]}
						{@const driving = el != null && el.mode !== 'constant'}
						{@const picking = pickingElement === i}
						<div class="pf-element" data-testid={`param-element-${i}`}>
							<div class="pf-element-row">
								<span class="pf-element-name">{elementName(i)}</span>
								<NumberInput
									value={el?.value ?? held}
									onChange={(v) => onCommitElement?.(i, v)}
									{step}
									scrub
									disabled={driven || driving}
									data-param-edit
									data-testid={`param-element-number-${i}`}
								/>
								<Segmented
									value={picking ? 'reference' : (el?.mode ?? 'constant')}
									bad={!!el?.error}
									segments={PARAM_MODES.map((id) => ({
										id,
										...MODE_FACE[id],
										title: `${MODE_FACE[id].name} — ${MODE_TITLE[id]}`,
										testid: `param-element-mode-${id}-${i}`
									}))}
									onChange={(m) => chooseElement(i, m as ParamMode)}
									aria-label={`${paramName} ${elementName(i)} source`}
								/>
							</div>
							{#if picking || el?.mode === 'reference'}
								<RefPicker
									value={el?.reference ?? null}
									paramType="num"
									onCommit={(reference) => {
										pickingElement = null;
										onSetElementSource?.(i, { reference });
									}}
									testid={`param-element-ref-${i}`}
								/>
							{:else if el?.mode === 'expression'}
								<ExprEditor
									{selfName}
									value={el.expression ?? ''}
									error={el.error}
									onCommit={(expression) => onSetElementSource?.(i, { expression })}
									label={`${paramName} ${elementName(i)} expression`}
									placeholder="nd('oscillator0').out.data.mean()"
									testid={`param-element-expr-${i}`}
								/>
							{/if}
							{#if el?.error && !picking}
								<div class="src-error" title={el.error}>
									<span class="prefix"><Icon name="triangle-alert" /></span>
									<span class="msg">{el.error}</span>
								</div>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
			{#if num && dims > 1 && !asVector && kind !== 'vector'}
				<!-- The whole list, typed as one: what an expression or a reference hands the param. -->
				<TextInput
					class="pf-list"
					value={`[${numValues(num).map((v) => Number(v.toFixed(4))).join(', ')}]`}
					onChange={(text) => {
						const list = parseList(text);
						if (list) onCommit(list);
					}}
					disabled={driven || anyElementDriven}
					spellcheck={false}
					aria-label={`${paramName} as a list`}
					data-testid="param-list"
				/>
			{/if}
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
	.pf-value :global(.pf-view) {
		flex: 0 0 auto;
	}
	.pf-elements {
		flex: 1 1 100%;
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}
	.pf-element {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
	}
	.pf-element-row {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		--number-width: 5rem;
	}
	.pf-element-name {
		flex: 0 0 1.5rem;
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		color: var(--text-muted);
	}
	.pf-more :global(.pf-list) {
		flex: 1 1 100%;
		font-family: var(--font-mono);
	}
	.unknown {
		font-size: var(--fs-micro);
		color: var(--text-muted);
	}
</style>
