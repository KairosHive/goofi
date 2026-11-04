<!-- Control panel: widgets over ONE group of variables. Every change is a variables op, so the
     manager owns the state and this panel owns only the drawing and the gesture in flight. -->
<script lang="ts">
	import { tick } from 'svelte';
	import type { PanelProps } from 'panelty';
	import { asStateObject, beginDrag, ContextMenu, type MenuItem } from 'panelty';
	import { selection } from '$lib/stores/selection.svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import { history } from '$lib/stores/history.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import type { ControlView, VariableView, LockView } from '$lib/crdt/graphDoc';
	import { effectiveLock, isValidIdentifier } from '$lib/crdt/graphDoc';
	import { drag, draggable, dropTarget, DropHint, type DragSource, type DropTargetOptions, type Outcome, type Point } from '$lib/drag';
	import MidiLearn from '$lib/inspector/MidiLearn.svelte';
	import { midiLearn } from '$lib/stores/midiLearn.svelte';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import {
		Chip,
		EmptyState,
		Field,
		Icon,
		IconButton,
		NumberInput,
		Popover,
		ScrollArea,
		Segmented,
		Select,
		TextInput,
		isTextEditingTarget
	} from '$lib/ui';
	import type { Literal } from '$lib/api/generated';
	import { CONTROL_COLUMNS, CONTROL_KINDS } from '$lib/api/vocab';
	import { variableForm, variableImage, variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { KIND, bornValue, cellAt, kindFits, movedBy, resizedBy, sameCell, type Cell, type Kind, type Units } from './controlLayout';
	import ControlWidget from './ControlWidget.svelte';
	import InstanceBar from './InstanceBar.svelte';

	interface ControlState {
		group?: string;
	}

	type Value = Literal | null;

	let props: PanelProps = $props();
	const g = graph();
	const engine = drag();

	const st = $derived(asStateObject(props.state) as ControlState);
	const group = $derived(st.group ?? '');
	const named = $derived(group !== '');
	// Every group a control panel may draw, this one's own among them even before the doc lists it.
	const tabs = $derived((g.controlGroups.includes(group) ? g.controlGroups : [...g.controlGroups, group]).map((id) => ({ id, label: id })));
	const groupLock = $derived<LockView>(g.variableGroups[group] ?? { config: false, value: false });
	const elements = $derived(g.variables.filter((gv) => gv.group === group && gv.control));
	// Values come over the data plane, one stream per widget drawn.
	watchVariables(() => elements.map((gv) => gv.name));
	// Each panel starts in edit mode and owns its mode independently.
	let editing = $state(true);
	const edit = $derived(named && editing);

	let board: HTMLDivElement | null = $state(null);
	let wrap: HTMLDivElement | null = $state(null);
	let picked = $state<string | null>(null);
	let renaming = $state<string | null>(null);
	const pickedView = $derived(elements.find((el) => el.name === picked) ?? null);
	// The form hangs off the picked widget's own cell, and follows it wherever a drag lands it.
	const anchor = $derived.by(() => {
		if (!board || !pickedView) return null;
		return board.querySelector<HTMLElement>(`[data-testid="control-${group}-${pickedView.element}"]`);
	});

	// The gestures in flight. A widget pressed anywhere is ONE drag: a move while it stays in the
	// panel, a reference carried to a target beyond it. A widget's corner resizes it; a chip lifted
	// off the palette bears a widget where it lands.
	let moving: { name: string; from: Cell; units: Units } | null = $state(null);
	let resizing: { name: string; to: Cell } | null = $state(null);
	let chip: { kind: Kind; w: number; h: number } | null = $state(null);
	const boardId = $derived(`${props.panelId}#board`);
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

	/** Where a widget is drawn: under the gesture that holds it, else where the document has it. */
	function placed(gv: VariableView): Cell {
		if (moving?.name === gv.name) {
			const to = movedTo(moving);
			if (to) return to;
		}
		if (resizing?.name === gv.name) return resizing.to;
		if (pending?.name === gv.name) return pending.to;
		return cellOf(gv);
	}

	/** The cell a move in flight has reached, or null while it is carried beyond the panel. */
	function movedTo(m: { from: Cell; units: Units }): Cell | null {
		const [s, p] = [engine.start, engine.point];
		if (!s || !p || !engine.inHome) return null;
		return movedBy(m.from, p.x - s.x, p.y - s.y, m.units, CONTROL_COLUMNS);
	}

	function unitsOf(el: HTMLElement): Units & { gap: number } {
		const cs = getComputedStyle(el);
		const gap = parseFloat(cs.rowGap) || 0;
		const inner = el.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
		const x = (inner + gap) / CONTROL_COLUMNS;
		const row = parseFloat(cs.gridAutoRows);
		return { x, y: Number.isFinite(row) ? row + gap : x, gap };
	}

	/** A client point in pixels into the board's grid. */
	function onBoard(p: Point): Point {
		const o = origin(board as HTMLElement);
		return { x: p.x - o.x, y: p.y - o.y };
	}

	/** The board's grid origin in client pixels. */
	function origin(b: HTMLElement): { x: number; y: number } {
		const r = b.getBoundingClientRect();
		const cs = getComputedStyle(b);
		return { x: r.left + parseFloat(cs.paddingLeft), y: r.top + parseFloat(cs.paddingTop) };
	}

	function bornCell(kind: Kind, point: Point, u: Units): Cell {
		return cellAt(point.x, point.y, KIND[kind].w, KIND[kind].h, u, CONTROL_COLUMNS);
	}

	/** The chip ghost's top-left for the pointer at `p`: the cell it would land on, in pixels, while
	 * over the board; else centred under the pointer. */
	function ghostAt(p: Point, kind: Kind, w: number, h: number): { at: Point; snapped: boolean } {
		if (board && engine.isOver(boardId)) {
			const u = unitsOf(board);
			const cell = bornCell(kind, onBoard(p), u);
			const o = origin(board);
			return { at: { x: o.x + cell.x * u.x, y: o.y + cell.y * u.y }, snapped: true };
		}
		return { at: { x: p.x - w / 2, y: p.y - h / 2 }, snapped: false };
	}

	function pick(group: string): void {
		props.setState({ group }, 'authored', `Show group ${group}`);
	}
	function nameGroup(from: string, raw: string): void {
		const to = raw.trim();
		if (to === from || !isValidIdentifier(to)) return;
		void g.renameVariableGroup(from, to).catch((e) => notify().failure('Rename group', e));
	}
	/** A fresh `controlN`, as a new panel is born over one, recorded and shown here as ONE step. */
	async function addGroup(): Promise<void> {
		const taken = new Set([...g.variables.map((v) => v.group), ...Object.keys(g.variableGroups), ...tabs.map((t) => t.id)]);
		let n = 0;
		while (taken.has(`control${n}`)) n++;
		try {
			await history().transaction('Add group', async (step) => {
				const group = await g.addVariableGroup(`control${n}`, step);
				await g.setPanelState(props.panelId, { group }, step);
			});
		} catch (e) {
			notify().failure('Add group', e);
		}
	}
	/** The group goes with its variables, and this panel moves to the tab beside it — or stays over
	 * the emptied one — as ONE step, so one undo puts both back. */
	async function removeGroup(target: string): Promise<void> {
		const i = tabs.findIndex((t) => t.id === target);
		const next = tabs[i - 1] ?? tabs[i + 1];
		const held = g.variables.some((v) => v.group === target) || target in g.variableGroups;
		try {
			await history().transaction(`Remove group ${target}`, async (step) => {
				if (target === group && next) await g.setPanelState(props.panelId, { group: next.id }, step);
				if (held) await g.removeVariableGroup(target, step);
			});
		} catch (e) {
			notify().failure('Remove group', e);
		}
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

	let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function elementMenu(x: number, y: number, name: string): void {
		menu = { x, y, items: selection().referenceItems({ variable: name }, `vars.${name}`) };
	}

	/** The form a widget's expression takes and gives: text for a text widget, a number otherwise. */
	const tagOf = (c: ControlView): 'text' | 'number' => (KIND[c.kind].draws === 'text' ? 'text' : 'number');

	/** What a press on a widget lifts: the variable, carried as `vars.<name>`; in edit mode the
	 * panel is its home, where the drag is a move. */
	function lift(gv: VariableView): DragSource | null {
		if (edit && board) moving = { name: gv.name, from: cellOf(gv), units: unitsOf(board) };
		return {
			key: gv.name,
			tags: [tagOf(gv.control as ControlView)],
			label: `vars.${gv.name}`,
			home: edit ? wrap : null,
			expression: () => `vars.${gv.name}`
		};
	}

	/** Which presses on a cell count: in edit mode all but its tools, else the label alone. */
	function allowed(t: Element): boolean {
		return edit ? !t.closest('.zap, .learn, .rename, .handle') : t.closest('.label') !== null;
	}

	function press(e: PointerEvent, gv: VariableView): void {
		if (!edit) return;
		picked = gv.name;
		(e.currentTarget as HTMLElement).focus();
		e.preventDefault();
	}

	// ONE op per gesture, so a drag is one undo step, the way every other frozen drag is.
	function landMove(outcome: Outcome | null): void {
		const m = moving;
		moving = null;
		if (!m || !outcome || !outcome.inHome) return;
		const to = movedBy(m.from, outcome.at.x - outcome.start.x, outcome.at.y - outcome.start.y, m.units, CONTROL_COLUMNS);
		land(m.name, m.from, to);
	}

	function land(name: string, from: Cell, to: Cell): void {
		const gv = elements.find((el) => el.name === name);
		if (!gv || sameCell(to, from)) return;
		pending = { name, to };
		void g.editControl(group, gv.element, to).catch(() => (pending = null));
	}

	// Listened to DIRECTLY, not delegated: a finger's touch is snapped to the nearest element that
	// listens, and a delegated listener is on the root.
	function corner(el: HTMLElement, on: (e: PointerEvent) => void): { update(on: (e: PointerEvent) => void): void; destroy(): void } {
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

	/** The corner: a resize moves the far edge alone, and lands as one op. */
	function resize(e: PointerEvent, gv: VariableView): void {
		if (!edit || !board || e.button !== 0) return;
		e.stopPropagation();
		e.preventDefault();
		const [from, units, start] = [cellOf(gv), unitsOf(board), { x: e.clientX, y: e.clientY }];
		resizing = { name: gv.name, to: from };
		beginDrag(e.currentTarget as HTMLElement, e.pointerId, {
			move: (m) => (resizing = { name: gv.name, to: resizedBy(from, m.clientX - start.x, m.clientY - start.y, units, CONTROL_COLUMNS) }),
			commit: () => {
				const r = resizing;
				resizing = null;
				if (r) land(gv.name, from, r.to);
			},
			cancel: () => (resizing = null)
		});
	}

	/** What a widget takes in edit mode: an expression of its form, from anything but itself. */
	function takesExpression(gv: VariableView): DropTargetOptions {
		return {
			id: `${props.panelId}#${gv.name}`,
			tags: [tagOf(gv.control as ControlView)],
			accept: (s) => s.key !== gv.name,
			onDrop: async ({ source, tag, at }) => {
				const expression = await source.expression?.(tag, at);
				if (expression) void g.computeControl(group, gv.element, expression).catch(() => {});
			}
		};
	}

	function zap(e: KeyboardEvent, gv: VariableView): void {
		if (!edit || isTextEditingTarget(e.target) || (e.key !== 'Delete' && e.key !== 'Backspace')) return;
		e.preventDefault();
		void g.removeControl(group, gv.element);
	}

	/** A chip off the palette: the kind it bears, at the size it would be born. */
	function liftChip(kind: Kind): DragSource | null {
		if (!board) return null;
		const u = unitsOf(board);
		const born = KIND[kind];
		chip = { kind, w: born.w * u.x - u.gap, h: born.h * u.y - u.gap };
		return { key: kind, tags: ['widget'] };
	}

	// A tap bears the widget where the manager places it; a drop bears it where it was let go.
	async function bear(kind: Kind, point: Point | null): Promise<void> {
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

{#if menu}
	<ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => (menu = null)} />
{/if}

<div class="wrap" data-testid="control-panel" data-group={group} data-edit={edit} bind:this={wrap}>
	<InstanceBar testid="control-tabs" items={tabs} active={group} onSelect={pick} onAdd={() => void addGroup()} onRename={nameGroup} onClose={(t) => void removeGroup(t)}>
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
	</InstanceBar>

	{#if edit}
		<div class="strip">
			<div class="palette" data-testid="control-palette">
				{#each CONTROL_KINDS as { id: kind } (kind)}
					<!-- The grip around the chip carries the press: an action needs an element of its own. -->
					<span class="grip" use:draggable={{ source: () => liftChip(kind), onTap: () => void bear(kind, null), onEnd: () => (chip = null) }}>
						<Chip
							tone={chip?.kind === kind ? 'accent' : 'neutral'}
							data-testid={`control-palette-${kind}`}
							title="Drag onto the board, or tap to add"
							onclick={(e) => {
								if (e.detail === 0) void bear(kind, null);
							}}>{kind}</Chip
						>
					</span>
				{/each}
			</div>
		</div>
	{/if}

	<ScrollArea>
		<!-- The sheet fills the scroll area, so a chip let go under the widgets still lands on the board. -->
		<div
			class="sheet"
			use:dropTarget={edit ? { id: boardId, tags: ['widget'], onDrop: ({ source, at }) => void bear(source.key as Kind, onBoard(at)) } : null}
		>
			<div class="board" data-testid="control-board" bind:this={board} style={`--columns: ${CONTROL_COLUMNS}`}>
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
						style={`grid-column: ${at.x + 1} / span ${at.w}; grid-row: ${at.y + 1} / span ${at.h}`}
						tabindex={edit ? 0 : undefined}
						use:draggable={{
							source: () => lift(gv),
							allow: allowed,
							onPress: (e) => press(e, gv),
							onHold: (at) => elementMenu(at.x, at.y, gv.name),
							onEnd: landMove
						}}
						use:dropTarget={edit ? takesExpression(gv) : null}
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
							<ControlWidget
								control={c}
								value={c.kind === 'paint' ? null : variableValue(gv.name)}
								label={gv.element}
								onChange={(v) => commitValue(gv, v)}
								onInput={(v) => g.previewVariableValue(gv.name, v)}
								image={c.kind === 'paint' ? variableImage(gv.name) : null}
								onStroke={(ops) => g.paintControl(group, gv.element, ops)}
							/>
						</div>
						<!-- svelte-ignore a11y_no_static_element_interactions -->
						<span
							class="label"
							title="Drag onto a parameter to set its expression. Double-click to rename"
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
							<span class="handle" data-testid="control-resize" title="Resize" use:corner={(e) => resize(e, gv)}></span>
							<DropHint id={`${props.panelId}#${gv.name}`} />
						{/if}
					</div>
				{/each}
			</div>
		</div>
	</ScrollArea>

	<!-- The settings stand down while a widget is moved or resized: they sit outside the panel, so
	     a pointer over them would read as carried beyond it, and they hide the board it lands on. -->
	{#if edit && pickedView && !moving && !resizing}
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
								options={CONTROL_KINDS.filter((k) => kindFits(k, variableForm(pv.name), variableValue(pv.name))).map((k) => k.id)}
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
						{#if variableForm(pv.name) === 'number' || pc.kind === 'vector'}
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

	{#if chip && engine.point}
		{@const ghost = ghostAt(engine.point, chip.kind, chip.w, chip.h)}
		<div class="ghost" class:snapped={ghost.snapped} style={`left: ${ghost.at.x}px; top: ${ghost.at.y}px`} aria-hidden="true">
			<div class="cell born" style={`width: ${chip.w}px; height: ${chip.h}px`}>
				<div class="widget">
					<ControlWidget
						control={{ kind: chip.kind, min: 0, max: 1, step: 0.01, x: 0, y: 0, w: 0, h: 0 }}
						value={KIND[chip.kind].draws === 'number' ? 0.5 : bornValue(KIND[chip.kind])}
						label={chip.kind}
						onChange={() => {}}
					/>
				</div>
				<span class="label">{chip.kind}</span>
			</div>
		</div>
	{/if}
</div>

<style>

	.wrap {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	.strip {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--space-2) var(--space-4);
		padding: var(--space-2) var(--space-3);
		border-bottom: 1px dashed var(--border);
	}
	.palette {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-2);
		touch-action: none;
	}
	.grip {
		display: inline-flex;
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
