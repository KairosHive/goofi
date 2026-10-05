<!-- One param row, every part of it named by `rowPlan`: the face control, disabled when a source
     drives it, and the open region with the list or the entries, the source editor and the foot. -->
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
	import { isNumeric, rowPlan } from './controlKind';
	import { numValue, numValues } from '$lib/api/types';
	import { MODE_FACE, sourceForMode } from './paramSeed';
	import ExprEditor from './expr/ExprEditor.svelte';
	import MidiLearn from './MidiLearn.svelte';

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
		literal = false,
		...rest
	}: HTMLAttributes<HTMLDivElement> & {
		paramName: string;
		/** What the row is called; a slot of a list wears its name inside the section. */
		label?: string;
		descriptor: ParamDescriptor;
		onCommit: (value: unknown) => unknown;
		/** A step of a drag on the value, ahead of its commit. */
		onPreview?: (value: number | number[]) => unknown;
		/** Absent on a literal row, which takes no source. */
		onSetSource?: (source: SourcePatch) => void;
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
		/** A value alone — a machine attribute's — with no source to pick, trigger or learn. */
		literal?: boolean;
	} = $props();

	const learnId = $props.id();
	const uiStore = ui();
	let row = $state<HTMLDivElement>();
	const over = $derived(
		(dropZone !== null && uiStore.nodeDragOver === dropZone) ||
		(uiStore.variableDrag !== null && uiStore.variableDrag.target === row)
	);

	function acceptVariable(el: HTMLDivElement): { destroy(): void } {
		const drop = (event: Event): void => onSetSource?.({ expression: (event as CustomEvent<string>).detail });
		el.addEventListener('variable-expression-drop', drop);
		return { destroy: () => el.removeEventListener('variable-expression-drop', drop) };
	}

	/** Every part the row shows, from the descriptor and the reader's view; see `rowPlan`. */
	const plan = $derived(rowPlan(descriptor, { individual: uiStore.paramView[viewKey] === 'individual', sources: !literal }));
	/** Whether the caret has anything to open. */
	const openable = $derived(plan.list || plan.elements || plan.source || plan.foot);
	const dims = $derived(isNumeric(descriptor) ? numValues(descriptor).length : 1);
	/** Each element's own source, which an entry row shows beside its number. */
	const elements = $derived(descriptor.elements ?? []);
	const elementDisabled = (i: number): boolean => driven || (elements[i]?.mode ?? 'constant') !== 'constant';

	const elementName = (i: number): string => (isNumeric(descriptor) && descriptor.color ? ['R', 'G', 'B', 'A'][i] : `[${i}]`);
	function chooseElement(i: number, mode: ParamMode): void {
		const el = elements[i];
		if (!el || mode === el.mode) return;
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

	// `step` uses the declared bounds, so the arrow keys move in rungs that fit the range.
	const num = $derived(isNumeric(descriptor) ? descriptor : null);
	const step = $derived(num ? (num.int ? 1 : stepOf(num.vmin, num.vmax)) : 1);
	/** The vector with one dimension replaced, which is what a dimension's control commits. */
	const withDim = (i: number, v: number): number[] => numValues(num!).map((held, k) => (k === i ? v : held));

	const options = $derived(descriptor.type === 'string' ? (descriptor.options ?? []) : []);

	const driven = $derived(descriptor.mode !== 'constant');

	const MODE_TITLE: Record<ParamMode, string> = {
		constant: 'a value set here by hand, unchanging until you edit it',
		expression: "a bare nd('node') or variables.group.element copied as it comes, or Python over them"
	};

	function choose(mode: ParamMode): void {
		if (mode !== descriptor.mode) onSetSource?.(sourceForMode(descriptor, mode));
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
	<!-- Which face a vector wears, L or I: one list with one source (a colour's picker over it), or
	     one row per entry with a source each. It sits at the row's end in both, so it stays put. -->
	{#snippet viewSwitch()}
		<Segmented
			class="pf-view"
			value={plan.elements ? 'individual' : 'list'}
			segments={[
				{ id: 'list', label: 'L', name: 'List', title: 'List — the values as one list, with one source for all', testid: 'param-view-list' },
				{ id: 'individual', label: 'I', name: 'Individual', title: 'Individual — one row per entry, each with a source of its own', testid: 'param-view-individual' }
			]}
			onChange={(v) => {
				if (v === 'individual') uiStore.paramView[viewKey] = 'individual';
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
		onExpand={openable ? () => (open = !open) : undefined}
	>
		<!-- `display: contents` so the face inherits WITHOUT laying out: Field requires paired controls to
		     be its direct children, and a real box would take them out of the @container column-flip. -->
		<div class="pf-value">
			{#if num && plan.face === 'color'}
				<ColorPicker
					value={numValues(num)}
					onChange={onCommit}
					onInput={onPreview}
					disabled={plan.disabled}
					title={plan.disabled && !driven ? 'An entry has a source of its own; see the individual view' : undefined}
					data-param-edit
					data-testid="param-color"
				/>
				{@render viewSwitch()}
			{:else if num && plan.face === 'vector'}
				<!-- One number per dimension, sharing the row's width as the picker would; each commits
				     the whole vector. -->
				<div class="pf-vector" data-testid="param-vector">
					{#each numValues(num) as held, i (i)}
						<NumberInput
							class="pf-fill"
							value={held}
							onChange={(v) => onCommit(withDim(i, v))}
							{step}
							disabled={elementDisabled(i)}
							data-param-edit
							data-testid={`param-number-${i}`}
						/>
					{/each}
				</div>
				{#if plan.viewSwitch}{@render viewSwitch()}{/if}
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
					{step}
					disabled={driven}
					data-param-edit
					data-testid="param-number"
				/>
			{:else if plan.face === 'toggle'}
				<Toggle
					value={Boolean(descriptor.value)}
					onChange={onCommit}
					disabled={driven}
					data-param-edit
					data-testid="param-toggle"
				/>
			{:else if plan.face === 'select'}
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
			{:else if plan.face === 'text'}
				<TextInput
					value={String(descriptor.value)}
					onChange={onCommit}
					disabled={driven}
					data-param-edit
					data-testid="param-text"
				/>
			{:else if plan.face === 'pulse'}
				<!-- A pulse holds no value to read out, so a driven one keeps its button: firing one by
				     hand is a request, and the source fires on its own edges. -->
				<Button class="pf-pulse" title="Fire one pulse" onclick={onPulse} data-param-edit data-testid="param-pulse">
					pulse
				</Button>
			{:else if plan.face === 'unknown'}
				<code class="unknown" data-testid="param-unknown">{JSON.stringify(descriptor.value)}</code>
			{/if}
		</div>
	</Field>

	{#if open}
		<div class="pf-more" data-testid="param-more">
			{#if num && plan.elements}
				<!-- One row per element: its number, and a source of its own that drives that dimension. -->
				<div class="pf-elements" data-testid="param-elements">
					{#each numValues(num) as held, i (i)}
						{@const el = elements[i]}
						<div class="pf-element" data-testid={`param-element-${i}`}>
							<div class="pf-element-row">
								<span class="pf-element-name">{elementName(i)}</span>
								<NumberInput
									class="pf-fill"
									value={el?.value ?? held}
									onChange={(v) => onCommitElement?.(i, v)}
									{step}
									disabled={elementDisabled(i)}
									title={driven ? 'A source drives the whole list; see the list view' : undefined}
									data-param-edit
									data-testid={`param-element-number-${i}`}
								/>
								<Segmented
									value={el?.mode ?? 'constant'}
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
								<MidiLearn
									label={`${paramName} ${elementName(i)}`}
									target={`param:${learnId}:${i}`}
									onLearn={(reference, index) => onSetElementSource?.(i, { expression: `${reference}[${index}]` })}
									testid={`param-element-learn-${i}`}
								/>
							</div>
							{#if el?.mode === 'expression'}
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
							{#if el?.error}
								<div class="src-error" title={el.error}>
									<span class="prefix"><Icon name="triangle-alert" /></span>
									<span class="msg">{el.error}</span>
								</div>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
			{#if num && plan.list}
				<!-- The whole list, typed as one: what an expression would hand the param. -->
				<TextInput
					class="pf-list"
					value={`[${numValues(num).map((v) => Number(v.toFixed(4))).join(', ')}]`}
					onChange={(text) => {
						const list = parseList(text);
						if (list) onCommit(list);
					}}
					disabled={plan.disabled}
					spellcheck={false}
					aria-label={`${paramName} as a list`}
					data-testid="param-list"
				/>
			{/if}
			{#if plan.source}
				<div class="src-region">
					<ExprEditor
						{selfName}
						value={descriptor.expression ?? ''}
						error={descriptor.error}
						onCommit={(expression) => onSetSource?.({ expression })}
						label={`${paramName} expression`}
						placeholder="nd('oscillator0').out.data.mean()"
						testid="param-expr-input"
					/>
				</div>
			{/if}
			{#if driven && plan.foot}
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
					onChange={() => onSetSource?.({ triggers: !descriptor.triggers })}
				/>
			{/if}
			{#if plan.foot}
			<Segmented
				value={descriptor.mode}
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
					onLearn={(reference, index) => onSetSource?.({ expression: `${reference}[${index}]` })} />
			{/if}
			{/if}
		</div>
	{/if}
	<!-- Shown whether or not the source is unfolded: the value beside it is the literal standing in,
	     and a failure is not a readout. -->
	{#if driven && descriptor.error}
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
	.pf-vector {
		flex: 1 1 auto;
		min-width: 0;
		display: flex;
		gap: var(--space-2);
		--number-width: 100%;
	}
	/* A number that shares its row: the picker's bar fills the row, so its numbers do too. */
	.pf-vector :global(.pf-fill),
	.pf-element-row :global(.pf-fill) {
		flex: 1 1 0;
		min-width: 0;
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
		--number-width: 100%;
	}
	.pf-element-name {
		flex: 0 0 1.5rem;
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		color: var(--text-muted);
	}
	/* The list shares its row with the source foot, and takes its own line only when the pane is
	   too narrow for both. */
	.pf-more :global(.pf-list) {
		flex: 1 1 10rem;
		min-width: 0;
		font-family: var(--font-mono);
	}
	.unknown {
		font-size: var(--fs-micro);
		color: var(--text-muted);
	}
</style>
