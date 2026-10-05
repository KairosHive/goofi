<!-- Control panel: widgets over ONE group of variables. Every change is a variables op, so the
     manager owns the state and this panel owns only the drawing and the gesture in flight. -->
<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import type { PanelProps } from 'panelty';
	import { asStateObject, ContextMenu, createLongPress, type MenuItem } from 'panelty';
	import { selection } from '$lib/stores/selection.svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import type { ControlView, VariableView, LockView } from '$lib/crdt/graphDoc';
	import { effectiveLock, isValidIdentifier } from '$lib/crdt/graphDoc';
	import { ui } from '$lib/stores/ui.svelte';
	import MidiLearn from '$lib/inspector/MidiLearn.svelte';
	import { midiLearn } from '$lib/stores/midiLearn.svelte';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import {
		Bar,
		Chip,
		PaintPad,
		EmptyState,
		Field,
		Icon,
		IconButton,
		Knob,
		NumberInput,
		Popover,
		ScrollArea,
		Segmented,
		Select,
		Slider,
		TextInput,
		Toggle,
		isTextEditingTarget
	} from '$lib/ui';
	import type { Literal } from '$lib/api/generated';
	import { CONTROL_COLUMNS, CONTROL_KINDS } from '$lib/api/vocab';
	import { variableForm, variableImage, variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { KIND, cellAt, movedBy, resizedBy, sameCell, type Cell, type Kind, type Units } from './controlLayout';

	interface ControlState {
		group?: string;
	}

	type Value = Literal | null;

	let props: PanelProps = $props();
	const g = graph();
	const uiStore = ui();

	const st = $derived(asStateObject(props.state) as ControlState);
	const group = $derived(st.group ?? '');
	const named = $derived(group !== '');
	const groupLock = $derived<LockView>(g.variableGroups[group] ?? { config: false, value: false });
	const elements = $derived(g.variables.filter((gv) => gv.group === group && gv.control));
	// Values come over the data plane, one stream per widget drawn.
	watchVariables(() => elements.map((gv) => gv.name));
	/** What a widget's kind must draw for this value: the form it holds, `text` and `number` as the kinds say. */
	// What a node dropped on one of this panel's widgets means, said by the panel that draws them.
	$effect(() =>
		uiStore.onNodeDrop(props.panelId, (uid, name) => {
			void g.linkControl(name, uid).catch(() => {});
		})
	);
	// Each panel starts in edit mode and owns its mode independently.
	let editing = $state(true);
	const edit = $derived(named && editing);

	let board: HTMLDivElement | null = $state(null);
	let picked = $state<string | null>(null);
	let renaming = $state<string | null>(null);
	const pickedView = $derived(elements.find((el) => el.name === picked) ?? null);
	// The form hangs off the picked widget's own cell, and follows it wherever a drag lands it.
	const anchor = $derived.by(() => {
		if (!board || !pickedView) return null;
		return board.querySelector<HTMLElement>(`[data-testid="control-${group}-${pickedView.element}"]`);
	});

	// The two gestures in flight: a widget being moved or resized, and a chip lifted off the palette.
	let drag: { name: string; from: Cell; x: number; y: number; units: Units; resize: boolean; to: Cell | null } | null =
		$state(null);
	// `at` is the ghost's top-left: snapped to the cell it would land on while over the board, else
	// under the pointer's centre.
	let lift: { kind: Kind; at: { x: number; y: number }; snapped: boolean; w: number; h: number; from: { x: number; y: number } } | null =
		$state(null);
	// A drop's cell outlives the pointer until the document agrees, so it never flashes back.
	let pending: { name: string; to: Cell } | null = $state(null);
	$effect(() => {
		const p = pending;
		if (p && elements.some((el) => el.name === p.name && sameCell(cellOf(el), p.to))) pending = null;
	});

	function cellOf(gv: VariableView): Cell {
		const c = gv.control as ControlView;
		return { x: c.x, y: c.y, w: c.w, h: c.h };
	}

	function placed(gv: VariableView): Cell {
		if (drag?.name === gv.name && drag.to) return drag.to;
		if (pending?.name === gv.name) return pending.to;
		return cellOf(gv);
	}

	function unitsOf(el: HTMLElement): Units & { gap: number } {
		const cs = getComputedStyle(el);
		const gap = parseFloat(cs.rowGap) || 0;
		const inner = el.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
		const x = (inner + gap) / CONTROL_COLUMNS;
		const row = parseFloat(cs.gridAutoRows);
		return { x, y: Number.isFinite(row) ? row + gap : x, gap };
	}

	/** Where the pointer is on the board, in pixels into its grid, or null when it is off the
	 * board's scroll area — the whole area, since the board itself is only as tall as its widgets. */
	function onBoard(e: PointerEvent): { x: number; y: number } | null {
		const zone = board?.closest('.ui-scrollarea') ?? board;
		if (!board || !zone) return null;
		const z = zone.getBoundingClientRect();
		if (e.clientX < z.left || e.clientX > z.right || e.clientY < z.top || e.clientY > z.bottom) return null;
		const o = origin(board);
		return { x: e.clientX - o.x, y: e.clientY - o.y };
	}

	/** The board's grid origin in client pixels. */
	function origin(b: HTMLElement): { x: number; y: number } {
		const r = b.getBoundingClientRect();
		const cs = getComputedStyle(b);
		return { x: r.left + parseFloat(cs.paddingLeft), y: r.top + parseFloat(cs.paddingTop) };
	}

	function bornCell(kind: Kind, point: { x: number; y: number }, u: Units): Cell {
		return cellAt(point.x, point.y, KIND[kind].w, KIND[kind].h, u, CONTROL_COLUMNS);
	}

	/** The ghost's top-left for a pointer at `e`: the cell it would land on, in pixels, while over
	 * the board; else centred under the pointer. */
	function ghostAt(e: PointerEvent, kind: Kind, w: number, h: number): { at: { x: number; y: number }; snapped: boolean } {
		const point = onBoard(e);
		if (point && board) {
			const u = unitsOf(board);
			const cell = bornCell(kind, point, u);
			const o = origin(board);
			return { at: { x: o.x + cell.x * u.x, y: o.y + cell.y * u.y }, snapped: true };
		}
		return { at: { x: e.clientX - w / 2, y: e.clientY - h / 2 }, snapped: false };
	}

	function nameGroup(raw: string): void {
		const to = raw.trim();
		if (to === group || !isValidIdentifier(to)) return;
		void g.renameVariableGroup(group, to).catch(() => {});
	}

	function setEdit(on: boolean): void {
		picked = null;
		renaming = null;
		editing = on;
	}

	function setControl(gv: VariableView, patch: Partial<ControlView>): void {
		void g.editControl(group, gv.element, patch).catch(() => {});
	}

	function commitValue(gv: VariableView, v: Value): Promise<void> {
		return v === null ? Promise.resolve() : g.setVariableValue(gv.name, v).catch(() => {});
	}

	function num(v: Value): number {
		return typeof v === 'number' ? v : Array.isArray(v) && typeof v[0] === 'number' ? v[0] : 0;
	}
	/** The one truth rule, as `control::truth` reads it: a non-empty text, or any number above zero. */
	function truth(v: Value): boolean {
		return typeof v === 'string' ? v !== '' : typeof v === 'number' ? v > 0 : Array.isArray(v) ? v.some(truth) : v === true;
	}

	// Listened to DIRECTLY, not delegated: a finger's touch is snapped to the nearest element that
	// listens, and a delegated listener is on the root — so the corner buttons took every grab.
	function grab(el: HTMLElement, on: (e: PointerEvent) => void): { update(on: (e: PointerEvent) => void): void; destroy(): void } {
		let now = on;
		const hear = (e: PointerEvent): void => now(e);
		el.addEventListener('pointerdown', hear);
		return {
			update(next) {
				now = next;
			},
			destroy() {
				el.removeEventListener('pointerdown', hear);
			}
		};
	}

	let variableGrab: { name: string; x: number; y: number; pointer: number } | null = $state(null);

	let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function elementMenu(x: number, y: number, name: string): void {
		menu = { x, y, items: selection().referenceItems({ variable: name }, `variables.${name}`) };
	}
	// The touch door onto the right-click menu: a held label, before it moves into a drag.
	let pressName = '';
	const press = createLongPress((at) => {
		cancelVariable();
		elementMenu(at.clientX, at.clientY, pressName);
	});

	function grabVariable(e: PointerEvent, gv: VariableView): void {
		if (e.button !== 0) return;
		if (e.pointerType !== 'mouse') {
			pressName = gv.name;
			press.start(e);
		}
		variableGrab = { name: gv.name, x: e.clientX, y: e.clientY, pointer: e.pointerId };
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		e.stopPropagation();
	}

	function moveVariable(e: PointerEvent): void {
		press.move(e);
		if (!variableGrab || variableGrab.pointer !== e.pointerId) return;
		if (!uiStore.variableDrag && Math.hypot(e.clientX - variableGrab.x, e.clientY - variableGrab.y) < 4) return;
		uiStore.variableDrag = {
			name: variableGrab.name, x: e.clientX, y: e.clientY,
			target: document.elementFromPoint(e.clientX, e.clientY)?.closest('[data-variable-drop]') ?? null
		};
	}

	function cancelVariable(): void {
		press.cancel();
		variableGrab = null;
		uiStore.variableDrag = null;
	}

	function dropVariable(e: PointerEvent): void {
		if (!variableGrab || variableGrab.pointer !== e.pointerId) return;
		moveVariable(e);
		const dropped = uiStore.variableDrag;
		cancelVariable();
		if (dropped && elements.some((gv) => gv.name === dropped.name)) {
			dropped.target?.dispatchEvent(new CustomEvent('variable-expression-drop', { detail: `variables.${dropped.name}` }));
		}
	}

	onDestroy(() => { if (variableGrab) cancelVariable(); });

	function down(e: PointerEvent, gv: VariableView, resize: boolean): void {
		// The cell hears its press directly, before any delegated handler on a corner could stop it.
		if (!edit || !board || (e.target as Element).closest('.zap, .learn, .rename')) return;
		picked = gv.name;
		if (!resize) grabVariable(e, gv);
		drag = { name: gv.name, from: cellOf(gv), x: e.clientX, y: e.clientY, units: unitsOf(board), resize, to: null };
		const el = e.currentTarget as HTMLElement;
		el.setPointerCapture(e.pointerId);
		(el.closest('.cell') as HTMLElement | null)?.focus();
		e.preventDefault();
		e.stopPropagation();
	}

	function move(e: PointerEvent): void {
		if (!drag) return;
		if (!drag.resize && !onBoard(e)) {
			moveVariable(e);
			drag.to = null;
			return;
		}
		uiStore.variableDrag = null;
		const [dx, dy] = [e.clientX - drag.x, e.clientY - drag.y];
		drag.to = drag.resize
			? resizedBy(drag.from, dx, dy, drag.units, CONTROL_COLUMNS)
			: movedBy(drag.from, dx, dy, drag.units, CONTROL_COLUMNS);
	}

	// ONE op per gesture, so a drag is one undo step, the way every other frozen drag is.
	function up(e: PointerEvent): void {
		if (!drag) return;
		if (!drag.resize && !onBoard(e)) {
			dropVariable(e);
			drag = null;
			return;
		}
		cancelVariable();
		const { name, from, to } = drag;
		drag = null;
		const gv = elements.find((el) => el.name === name);
		if (!gv || !to || sameCell(to, from)) return;
		pending = { name, to };
		void g.editControl(group, gv.element, to).catch(() => (pending = null));
	}

	function zap(e: KeyboardEvent, gv: VariableView): void {
		if (!edit || isTextEditingTarget(e.target) || (e.key !== 'Delete' && e.key !== 'Backspace')) return;
		e.preventDefault();
		void g.removeControl(group, gv.element);
	}

	function liftChip(e: PointerEvent, kind: Kind): void {
		if (!board) return;
		const u = unitsOf(board);
		const born = KIND[kind];
		const [w, h] = [born.w * u.x - u.gap, born.h * u.y - u.gap];
		lift = { kind, ...ghostAt(e, kind, w, h), w, h, from: { x: e.clientX, y: e.clientY } };
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		e.preventDefault();
	}

	function driftChip(e: PointerEvent): void {
		if (!lift) return;
		const { at, snapped } = ghostAt(e, lift.kind, lift.w, lift.h);
		lift.at = at;
		lift.snapped = snapped;
	}

	// A tap bears the widget where the manager places it; a drag bears it where it was let go.
	function dropChip(e: PointerEvent): void {
		if (!lift) return;
		const { kind, from } = lift;
		lift = null;
		const dragged = Math.hypot(e.clientX - from.x, e.clientY - from.y) > 4;
		const point = dragged ? onBoard(e) : null;
		if (dragged && !point) return;
		void bear(kind, point);
	}

	async function bear(kind: Kind, point: { x: number; y: number } | null): Promise<void> {
		const cell = point && board ? bornCell(kind, point, unitsOf(board)) : undefined;
		try {
			picked = await g.addControl(group, kind, cell);
		} catch {
			/* refused */
		}
	}

	async function rename(gv: VariableView, raw: string): Promise<void> {
		renaming = null;
		const element = raw.trim();
		if (element === gv.element || !isValidIdentifier(element)) return;
		try {
			await g.editControl(group, gv.element, { name: element });
			if (picked === gv.name) picked = `${group}.${element}`;
		} catch {
			/* refused */
		}
	}

	async function startRename(gv: VariableView, cell: HTMLElement): Promise<void> {
		renaming = gv.name;
		await tick();
		const input = cell.querySelector<HTMLInputElement>('[data-testid="control-rename"]');
		input?.focus();
		input?.select();
	}

	const learn = (gv: VariableView) => (variable: string, index: number) =>
		void g.computeControl(group, gv.element, `${variable}[${index}]`).catch(() => {});

	// A widget is either set by hand or COMPUTED by an expression. `computing` is the expression
	// segment lit before one is written, so the editor shows with nothing to show yet.
	let computingFor = $state<string | null>(null);
	const computing = $derived(!!pickedView && computingFor === picked && pickedView.expression === undefined);

	function setExpression(pv: VariableView, expression: string): void {
		if (midiLearn.target === `control:${pv.name}`) midiLearn.stop();
		computingFor = null;
		void g.computeControl(group, pv.element, expression).catch(() => {});
	}

</script>

<svelte:window onkeydown={(e) => { if (e.key === 'Escape' && variableGrab) { drag = null; cancelVariable(); } }} />

{#snippet widget(c: ControlView, value: Value, label: string, onChange: (v: Value) => unknown, name = '', onInput?: (v: Value) => unknown)}
	{#if c.kind === 'knob'}
		<Knob {label} value={num(value)} min={c.min ?? 0} max={c.max ?? 1} step={c.step ?? 0} {onChange} {onInput} />
	{:else if c.kind === 'slider'}
		<Slider value={num(value)} min={c.min ?? 0} max={c.max ?? 1} step={c.step} {onChange} {onInput} />
	{:else if c.kind === 'number'}
		<NumberInput value={num(value)} min={c.min} max={c.max} step={c.step ?? 1} {onChange} />
	{:else if c.kind === 'toggle'}
		<Toggle value={truth(value)} {onChange} />
	{:else if c.kind === 'dropdown'}
		<Select value={String(value ?? '')} options={c.options ?? []} {onChange} />
	{:else if c.kind === 'paint'}
		<PaintPad value={variableImage(name)} onStroke={(ops) => name && g.paintControl(group, name.slice(group.length + 1), ops)} />
	{:else}
		<TextInput multiline value={String(value ?? '')} aria-label={label} {onChange} {onInput} />
	{/if}
{/snippet}

{#if menu}
	<ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => (menu = null)} />
{/if}

<div class="wrap" data-testid="control-panel" data-group={group} data-edit={edit}>
	<Bar style="--bar-bg: transparent; --bar-border: 1px solid var(--border)">
		{#snippet start()}<span class="title">{group}</span>{/snippet}
		{#snippet end()}
			<IconButton
				variant={edit ? 'primary' : 'ghost'}
				size="sm"
				data-testid="control-edit-toggle"
				title={edit ? 'Done editing' : 'Edit this panel'}
				label={edit ? 'Done editing' : 'Edit this panel'}
				disabled={!named}
				onclick={() => setEdit(!edit)}><Icon name={edit ? 'check' : 'pencil'} /></IconButton
			>
		{/snippet}
	</Bar>

	{#if edit}
		<div class="strip">
			<div class="grow">
				<TextInput
					inputmode="search"
					data-testid="control-group-name"
					title="The group its variables live in: variables.name.element"
					value={group}
					autocomplete="off"
					onChange={nameGroup}
				/>
			</div>
			<div class="palette" data-testid="control-palette">
				{#each CONTROL_KINDS as { id: kind } (kind)}
					<Chip
						tone={lift?.kind === kind ? 'accent' : 'neutral'}
						data-testid={`control-palette-${kind}`}
						title="Drag onto the board, or tap to add"
						onpointerdown={(e) => liftChip(e, kind)}
						onpointermove={driftChip}
						onpointerup={dropChip}
						onpointercancel={() => (lift = null)}
						onclick={(e) => {
							if (e.detail === 0) void bear(kind, null);
						}}>{kind}</Chip
					>
				{/each}
			</div>
		</div>
	{/if}

	<ScrollArea>
		<div class="sheet">
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div
				class="board"
				data-testid="control-board"
				bind:this={board}
				style={`--columns: ${CONTROL_COLUMNS}`}
				onpointermove={move}
				onpointerup={up}
				onpointercancel={() => { drag = null; cancelVariable(); }}
				onlostpointercapture={() => { drag = null; cancelVariable(); }}
			>
				{#if elements.length === 0}
					<div class="fill">
						<EmptyState data-testid="control-empty">
							{#snippet title()}No widgets yet{/snippet}
							{#snippet hint()}{edit ? 'Drag one in from the palette, or tap it.' : 'Switch on edit mode to add some.'}{/snippet}
						</EmptyState>
					</div>
				{/if}
				{#each elements as gv (gv.name)}
					{@const c = gv.control as ControlView}
					{@const at = placed(gv)}
					{@const held = effectiveLock(gv, groupLock)}
					<!-- svelte-ignore a11y_no_static_element_interactions -->
					<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
					<div
						class="cell"
						class:picked={edit && picked === gv.name}
						data-testid={`control-${group}-${gv.element}`}
						data-node-drop={edit ? `${props.panelId}#${gv.name}` : undefined}
						style={`grid-column: ${at.x + 1} / span ${at.w}; grid-row: ${at.y + 1} / span ${at.h}`}
						tabindex={edit ? 0 : undefined}
						use:grab={(e) => down(e, gv, false)}
						onkeydown={(e) => zap(e, gv)}
						oncontextmenu={(e) => {
							e.preventDefault();
							elementMenu(e.clientX, e.clientY, gv.name);
						}}
					>
						<div
							class="widget"
							class:held={held.value || gv.expression !== undefined}
							class:broken={gv.error !== undefined}
							title={gv.expression !== undefined ? gv.error ?? `Computed by ${gv.expression}` : held.value ? 'Value-locked' : undefined}
						>
							{@render widget(c, c.kind === 'paint' ? null : variableValue(gv.name), gv.element, (v) => commitValue(gv, v), gv.name, (v) => v !== null && g.previewVariableValue(gv.name, v))}
						</div>
						<!-- svelte-ignore a11y_no_static_element_interactions -->
						<span
							class="label"
							title="Drag onto a parameter to set its expression. Double-click to rename"
							use:grab={(e) => grabVariable(e, gv)}
							onpointermove={moveVariable}
							onpointerup={dropVariable}
							onpointercancel={cancelVariable}
							onlostpointercapture={cancelVariable}
							ondblclick={(e) => startRename(gv, (e.currentTarget as HTMLElement).parentElement as HTMLElement)}
							>{gv.element}</span
						>
						{#if renaming === gv.name}
							<div class="rename">
								<TextInput
									inputmode="search"
									data-testid="control-rename"
									value={gv.element}
									autocomplete="off"
									onChange={(v) => {
										if (renaming === gv.name) void rename(gv, v);
									}}
									onkeydown={(e) => {
										if (e.key === 'Escape') renaming = null;
									}}
								/>
							</div>
						{/if}
						{#if edit}
							{#if variableForm(gv.name) === 'number'}
								<div class="learn">
									<MidiLearn label={gv.element} target={`control:${gv.name}`} testid="control-learn" onLearn={learn(gv)} />
								</div>
							{/if}
							<button
								type="button"
								class="zap"
								data-testid="control-delete"
								title="Delete {gv.element}"
								aria-label="Delete {gv.element}"
								onclick={() => void g.removeControl(group, gv.element)}><Icon name="x" /></button
							>
							<!-- svelte-ignore a11y_no_static_element_interactions -->
							<span
								class="handle"
								data-testid="control-resize"
								title="Resize"
								use:grab={(e) => down(e, gv, true)}
							></span>
						{/if}
						{#if edit && uiStore.nodeDrag !== null}
							<div
								class="node-drop-hint"
								class:active={uiStore.nodeDragOver === `${props.panelId}#${gv.name}`}
								data-testid="node-drop-hint"
							></div>
						{/if}
					</div>
				{/each}
			</div>
		</div>
	</ScrollArea>

	{#if edit && pickedView}
		{@const pv = pickedView}
		{@const pc = pv.control as ControlView}
		{@const pheld = effectiveLock(pv, groupLock)}
		{#key `${pv.name}:${placed(pv).x},${placed(pv).y}`}
			<Popover
				{anchor}
				open={anchor !== null}
				onDismiss={() => (picked = null)}
				flip
				role="dialog"
				aria-label={`${pv.element} settings`}
				style="--popover-min-width: 18rem"
				data-testid="control-props"
			>
				{#if pheld.config}
					<EmptyState>
						{#snippet hint()}This widget is config-locked.{/snippet}
					</EmptyState>
				{:else}
					<div class="props">
						<Field label="name">
							<TextInput
								inputmode="search"
								data-testid="control-props-name"
								value={pv.element}
								autocomplete="off"
								onChange={(v) => void rename(pv, v)}
							/>
						</Field>
						<Field label="widget">
							<Select
								data-testid="control-props-kind"
								value={pc.kind}
								options={CONTROL_KINDS.filter((k) => k.draws === 'any' || k.draws === variableForm(pv.name)).map((k) => k.id)}
								onChange={(v) => setControl(pv, { kind: v as Kind })}
							/>
						</Field>
						<Field label="source">
							<Segmented
								value={pv.expression !== undefined || computing ? 'expression' : 'value'}
								segments={[
									{ id: 'value', label: 'value', title: 'Set by hand, on the widget itself', testid: 'control-source-value' },
									{
										id: 'expression',
										label: 'expr',
										title: "Computed by an expression — a node's output or a MIDI controller's knob copied as it comes, or Python over them. Drop a node onto the widget, learn, or write it",
										testid: 'control-source-expression'
									}
								]}
								onChange={(id) => (id === 'expression' ? (computingFor = picked) : pv.expression !== undefined ? setExpression(pv, '') : (computingFor = null))}
								aria-label="source"
								data-testid="control-source"
							/>
						</Field>
						{#if pv.expression !== undefined || computing}
							<Field label="expression" doc="What computes the widget; a node dropped onto the widget lands here, and learn writes the controller's element">
								<ExprEditor
									value={pv.expression ?? ''}
									error={pv.error ?? null}
									onCommit={(e) => setExpression(pv, e)}
									label={`${pv.element} expression`}
									placeholder="nd('midi0').out.cc[74]"
									testid="control-props-expression"
								/>
							</Field>
						{/if}
						{#if pv.error}
							<p class="error-text" role="alert" data-testid="control-source-error">{pv.error}</p>
						{/if}
						{#if variableForm(pv.name) === 'number'}
							<Field label="range" doc="min, max and step">
								<NumberInput value={pc.min ?? 0} title="min" onChange={(v) => setControl(pv, { min: v })} />
								<NumberInput value={pc.max ?? 1} title="max" onChange={(v) => setControl(pv, { max: v })} />
								<NumberInput value={pc.step ?? 0} min={0} title="step" onChange={(v) => setControl(pv, { step: v })} />
							</Field>
						{/if}
						{#if pc.kind === 'dropdown'}
							<Field label="options" doc="Comma-separated">
								<TextInput
									inputmode="text"
									data-testid="control-props-options"
									value={(pc.options ?? []).join(', ')}
									onChange={(v) => setControl(pv, { options: v.split(',').map((o) => o.trim()).filter(Boolean) })}
								/>
							</Field>
						{/if}
						<!-- The corner buttons' door for a finger: a cell is narrower than two finger-sized targets. -->
						<div class="touch-actions">
							{#if variableForm(pv.name) === 'number'}
								<MidiLearn label={pv.element} target={`control:${pv.name}`} testid="control-learn" onLearn={learn(pv)} />
							{/if}
							<Chip tone="danger" data-testid="control-delete" onclick={() => void g.removeControl(group, pv.element)}>delete</Chip>
						</div>
					</div>
				{/if}
			</Popover>
		{/key}
	{/if}

	{#if variableGrab && uiStore.variableDrag}
		<div class="ghost variable-ghost" style={`left: ${uiStore.variableDrag.x + 12}px; top: ${uiStore.variableDrag.y + 12}px`} aria-hidden="true">variables.{uiStore.variableDrag.name}</div>
	{/if}

	{#if lift}
		<div class="ghost" class:snapped={lift.snapped} style={`left: ${lift.at.x}px; top: ${lift.at.y}px`} aria-hidden="true">
			<div class="cell born" style={`width: ${lift.w}px; height: ${lift.h}px`}>
				<div class="widget">
					{@render widget(
						{ kind: lift.kind, min: 0, max: 1, step: 0.01, x: 0, y: 0, w: 0, h: 0 },
						({ number: 0.5, any: 0, text: '', image: null } as const)[KIND[lift.kind].draws],
						lift.kind,
						() => {}
					)}
				</div>
				<span class="label">{lift.kind}</span>
			</div>
		</div>
	{/if}
</div>

<style>
	.variable-ghost {
		padding: var(--space-2);
		background: var(--surface-3);
		color: var(--text);
		border: 1px solid var(--accent);
		border-radius: var(--radius-sm);
	}

	.wrap {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	.title {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
		font-size: var(--fs-small);
	}
	.strip {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--space-2) var(--space-4);
		padding: var(--space-2) var(--space-3);
		border-bottom: 1px dashed var(--border);
	}
	.grow {
		flex: 1 1 10rem;
		min-width: 0;
	}
	.palette {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-2);
		touch-action: none;
	}
	.props {
		display: flex;
		flex-direction: column;
		gap: var(--space-4);
		/* Three numbers share one field's row, so each takes its share rather than a fixed width. */
		--number-width: 100%;
	}
	/* A bare red cross over the widget's corner, no box of its own. */
	.zap {
		position: absolute;
		top: 0;
		right: 0;
		z-index: 1;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: var(--hit);
		min-height: var(--hit);
		padding: 0;
		background: transparent;
		border: none;
		color: var(--danger);
		cursor: pointer;
	}
	.zap:focus-visible {
		outline: var(--focus-width) solid var(--focus-ink);
		outline-offset: -2px;
	}
	/* Its blue twin over the other corner; lit while it listens. */
	.learn {
		position: absolute;
		top: 0;
		left: 0;
		z-index: 1;
	}

	.touch-actions {
		display: none;
		gap: var(--space-2);
	}
	@media (hover: none) and (pointer: coarse) {
		.learn,
		.zap {
			display: none;
		}
		.touch-actions {
			display: flex;
		}
	}
	/* The board is a container, so a grid unit is a share of ITS width and follows every resize. */
	.sheet {
		container-type: inline-size;
		min-height: 100%;
	}
	.board {
		--gap: var(--space-1);
		--pad: var(--space-3);
		--col: calc((100cqw - 2 * var(--pad) - (var(--columns) - 1) * var(--gap)) / var(--columns));
		--label-h: calc(var(--fs-small) + var(--space-3));
		display: grid;
		grid-template-columns: repeat(var(--columns), minmax(0, 1fr));
		/* A row is a square of the column, floored so a two-row widget still fits a tap target. */
		grid-auto-rows: max(var(--col), calc((var(--hit) + var(--label-h) + 2 * var(--space-2)) / 2));
		gap: var(--gap);
		padding: var(--pad);
		min-height: 100%;
	}
	[data-edit='true'] .board {
		touch-action: none;
	}
	.fill {
		grid-column: 1 / -1;
	}
	.cell {
		position: relative;
		display: flex;
		flex-direction: column;
		min-width: 0;
		min-height: 0;
		padding: var(--space-2);
		border-radius: var(--radius-md);
		background: var(--surface-1);
		overflow: hidden;
	}
	[data-edit='true'] .cell {
		outline: 1px dashed var(--border-strong);
		cursor: grab;
	}
	.cell.picked {
		outline: var(--focus-width) solid var(--accent);
	}
	.cell:focus-visible {
		outline: var(--focus-width) solid var(--focus-ink);
	}
	.widget {
		flex: 1;
		min-width: 0;
		min-height: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		--number-width: 100%;
	}
	[data-edit='true'] .widget,
	.born .widget {
		pointer-events: none;
	}
	.widget.held {
		pointer-events: none;
		opacity: var(--disabled-opacity);
	}
	.widget.broken {
		outline: 1px dashed var(--danger);
	}
	.label {
		touch-action: none;
		cursor: grab;
		height: var(--label-h);
		line-height: var(--label-h);
		text-align: center;
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		color: var(--text-muted);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.rename {
		position: absolute;
		left: var(--space-2);
		right: var(--space-2);
		bottom: var(--space-2);
		z-index: 1;
	}
	/* A --hit corner to grab, painted as a small tab so it does not hide the widget. */
	.handle {
		position: absolute;
		right: 0;
		bottom: 0;
		width: var(--hit);
		height: var(--hit);
		cursor: nwse-resize;
		touch-action: none;
	}
	.handle::after {
		content: '';
		position: absolute;
		right: 0;
		bottom: 0;
		width: var(--space-6);
		height: var(--space-6);
		background: var(--accent);
		border-radius: var(--radius-sm) 0 var(--radius-md) 0;
	}
	/* The ghost IS the widget it will bear, at the size it will be born, on the cell it will land. */
	.ghost {
		position: fixed;
		z-index: var(--z-drag-ghost);
		pointer-events: none;
	}
	.ghost.snapped .born {
		outline-style: solid;
		opacity: 1;
	}
	.born {
		outline: 1px dashed var(--accent);
		opacity: 0.85;
	}
</style>
