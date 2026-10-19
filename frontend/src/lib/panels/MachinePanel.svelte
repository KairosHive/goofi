<!-- State machine panel: one machine on a canvas — its states as boxes, its transitions as lines,
     its playheads as dots that move as the manager moves them — with the editor's inspector pane
     beside it for the selected states; the seed rides the tab bar. A box dragged onto a param row
     maps the row to one of its attributes. Every change is a `machine` op, so the manager owns the
     model and this panel the drawing and the gesture. -->
<script lang="ts">
	import {
		Background,
		ConnectionLineType,
		ConnectionMode,
		ControlButton,
		Controls,
		SvelteFlow,
		SvelteFlowProvider,
		ViewportPortal,
		type Connection,
		type Edge,
		type FitViewOptions,
		type Node,
		type Viewport
	} from '@xyflow/svelte';
	import { asStateObject, createLongPress, portal, workspace, type MenuItem, ContextMenu, type PanelProps } from 'panelty';
	import { tick, untrack } from 'svelte';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import { on } from 'svelte/events';
	import { camera } from '$lib/editor/camera';
	import { eventPoint } from '$lib/editor/eventPoint';
	import { bindTapZoom } from '$lib/editor/doubleTapZoom';
	import FlowApi from '$lib/editor/FlowApi.svelte';
	import MachineInspector from '$lib/machine/MachineInspector.svelte';
	import PlayheadDots from '$lib/machine/PlayheadDots.svelte';
	import StateCard from '$lib/machine/StateCard.svelte';
	import type { Subject } from '$lib/machine/subject';
	import TransitionEdge from '$lib/machine/TransitionEdge.svelte';
	import { GRID } from '$lib/editor/nodeMetrics';
	import { CARD_W, TITLE_Y } from '$lib/machine/layout';
	import { summary } from '$lib/machine/triggers';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { history } from '$lib/stores/history.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { selection } from '$lib/stores/selection.svelte';
	import { ui } from '$lib/stores/ui.svelte';
	import { EmptyState, Icon, IconButton, isTextEditingTarget, NumberInput } from '$lib/ui';
	import InstanceBar from './InstanceBar.svelte';
	import SidePane from './SidePane.svelte';

	interface MachineState {
		machine?: string;
	}

	let { panelId, state: panelState, setState }: PanelProps = $props();
	const g = graph();
	const sel = selection();
	const uiStore = ui();
	const ws = workspace();

	const name = $derived((asStateObject(panelState) as MachineState).machine ?? '');
	const machine = $derived(name ? g.machines[name] : undefined);
	const tabs = $derived(Object.keys(g.machines).map((id) => ({ id, label: id })));

	function choose(machine: string): void {
		setState({ machine }, 'authored', `Show machine ${machine}`);
	}
	function call(op: MachineOp, payload: Record<string, unknown>): Promise<unknown> {
		return g.machine(op, { machine: name, ...payload }).catch((e) => notify().failure(op, e));
	}
	/** A new machine, shown here, as ONE step. */
	async function create(): Promise<void> {
		try {
			await history().transaction('Add machine', async (step) => {
				const r = await g.machine<{ name: string }>('machine add', {}, step);
				await g.setPanelState(panelId, { machine: r.name }, step);
			});
		} catch (e) {
			notify().failure('Add machine', e);
		}
	}
	function rename(machine: string, to: string): void {
		if (to !== machine && isValidIdentifier(to)) void call('machine rename', { machine, to });
	}
	/** ONE step: the last machine's successor is born first, so the removal re-aims this panel at it
	 * and one undo brings the old one back whole. The manager, never a reaction here, picks the next. */
	async function remove(machine: string): Promise<void> {
		try {
			await history().transaction(`Remove machine ${machine}`, async (step) => {
				if (tabs.length === 1) await g.machine('machine add', {}, step);
				await g.machine('machine remove', { machine }, step);
			});
		} catch (e) {
			notify().failure('Remove machine', e);
		}
	}

	// ---- the selection and the inspector: the editor's store, so a dismissed pane re-arms the same way.
	// A selected transition is read through the state it leaves (a `*` one through the state it
	// enters), its row open there: every setting of the machine is reached from a state.
	const inspectorOn = $derived(sel.inspectorVisibleFor(panelId));
	const subject = $derived.by<Subject | null>(() => {
		const m = machine;
		if (!m) return null;
		const edges = [...sel.edges(panelId)].filter((id) => id in m.transitions);
		const ids = new Set([...sel.nodes(panelId)].filter((id) => id in m.states));
		for (const id of edges) {
			const t = m.transitions[id];
			ids.add(t.from in m.states ? t.from : t.to);
		}
		return ids.size > 0 ? { ids: [...ids], open: edges } : null;
	});
	const dismiss = (): void => sel.dismissInspectorFor(panelId);

	// ---- the flow's nodes and edges, derived from the document; a drag pins a box until the doc agrees.
	let flowNodes = $state.raw<Node[]>([]);
	let flowEdges = $state.raw<Edge[]>([]);
	const pinned = new Map<string, { x: number; y: number }>();
	$effect(() => {
		const m = machine;
		if (!m) {
			flowNodes = [];
			return;
		}
		const selected = sel.nodes(panelId);
		const previous = new Map(untrack(() => flowNodes).map((n) => [n.id, n]));
		flowNodes = Object.entries(m.states).map(([id, s]) => {
			const held = previous.get(id);
			const position = pinned.get(id) ?? { x: s.pos[0], y: s.pos[1] };
			return { ...held, id, type: 'state', position, data: { machine: name }, draggable: true, selected: selected.has(id) } as Node;
		});
	});
	$effect(() => {
		const m = machine;
		if (!m) {
			flowEdges = [];
			return;
		}
		const selected = sel.edges(panelId);
		const boxes = sel.nodes(panelId);
		// A `*` transition has no box to leave from; it is listed under every state and fired there.
		const drawn = Object.entries(m.transitions).filter(([, t]) => t.from in m.states && t.to in m.states);
		// The transitions between one pair of boxes, either way, take lanes side by side.
		const pairKey = (t: { from: string; to: string }): string => [t.from, t.to].sort().join('|');
		const lanes = new Map<string, string[]>();
		for (const [id, t] of drawn) lanes.set(pairKey(t), [...(lanes.get(pairKey(t)) ?? []), id]);
		flowEdges = drawn.map(([id, t]) => {
			const siblings = lanes.get(pairKey(t))!;
			return {
				id,
				source: t.from,
				target: t.to,
				type: 'transition',
				// Above every card, the selected one included: a label over a box is still a control.
				zIndex: 1001,
				selected: selected.has(id),
				data: {
					summary: summary(m, id),
					k: siblings.indexOf(id),
					n: siblings.length,
					// The label shows while the transition or either of its boxes is selected.
					labelled: selected.has(id) || boxes.has(t.from) || boxes.has(t.to),
					onPick: (e: MouseEvent) => {
						sel.clickEdge(panelId, id, modifier(e));
						void tick().then(reassertSelection);
					}
				}
			};
		});
	});
	/** Flow's own `selected` flags, written back from the store after Flow has had its say. */
	function reassertSelection(): void {
		flowNodes = flowNodes.map((n) => ({ ...n, selected: sel.nodes(panelId).has(n.id) }));
		flowEdges = flowEdges.map((e) => ({ ...e, selected: sel.edges(panelId).has(e.id) }));
	}
	const modifier = (e: MouseEvent | TouchEvent): boolean => 'shiftKey' in e && (e.shiftKey || e.ctrlKey || e.metaKey);

	const nodeTypes = { state: StateCard };
	const edgeTypes = { transition: TransitionEdge };
	const FIT_OPTIONS = { maxZoom: 1, padding: 0.18 } satisfies FitViewOptions;
	const MIN_ZOOM = 0.05;
	const MAX_ZOOM = 4;

	// ---- the camera, kept per panel so a reshape keeps the framing.
	let rootEl = $state<HTMLDivElement | null>(null);
	let screenToFlow = $state<((p: { x: number; y: number }) => { x: number; y: number }) | undefined>(undefined);
	let getViewport = $state<(() => Viewport) | undefined>(undefined);
	let setViewport = $state<((v: Viewport) => void) | undefined>(undefined);
	let flowFit = $state<((o?: FitViewOptions) => Promise<boolean>) | undefined>(undefined);
	const cam = camera(untrack(() => panelId));
	let viewport = $state<Viewport>(cam.viewport ?? { x: 0, y: 0, zoom: 1 });
	// Frame the machine once per load, never on an interactive add.
	$effect(() => {
		const epoch = g.loadEpoch;
		if (!flowFit || epoch === cam.fittedEpoch || !g.loadSettled || !machine) return;
		cam.fittedEpoch = epoch;
		if (Object.keys(machine.states).length > 0) void flowFit(FIT_OPTIONS);
	});

	// ---- adding a state: a double-click or a held finger on the bare pane.
	const onBarePane = (target: EventTarget | null): boolean =>
		Boolean((target as HTMLElement | null)?.classList.contains('svelte-flow__pane'));
	function addState(at: { x: number; y: number }): void {
		const p = screenToFlow?.(at) ?? { x: 0, y: 0 };
		// Its name under the gesture, so the release lands there and not on a control; then on the
		// grid, as a dragged card lands.
		const pos = [p.x - CARD_W / 2, p.y - TITLE_Y].map((v) => Math.round(v / GRID) * GRID);
		void call('machine state add', { pos });
	}
	// A double click adds a box: the browser counts a mouse's clicks; a touch's taps are counted
	// here, and only a tap that follows one that left nothing selected is the second of a pair.
	const DOUBLE_TAP_MS = 350;
	let lastTap = { at: 0, x: 0, y: 0 };
	let touching = false;
	function onPaneClick({ event }: { event: MouseEvent }): void {
		const now = performance.now();
		const bare = !sel.nodes(panelId).size && !sel.edges(panelId).size;
		const doubleTap = touching && bare && now - lastTap.at < DOUBLE_TAP_MS && Math.hypot(event.clientX - lastTap.x, event.clientY - lastTap.y) < 30;
		if (event.detail >= 2 || doubleTap) {
			lastTap.at = 0;
			addState({ x: event.clientX, y: event.clientY });
			return;
		}
		lastTap = { at: now, x: event.clientX, y: event.clientY };
		sel.clickPane(panelId, event.shiftKey);
		if (sel.nodes(panelId).size || sel.edges(panelId).size) void tick().then(reassertSelection);
	}
	const panePress = createLongPress((at) => addState({ x: at.clientX, y: at.clientY }));
	function onPanePointerDown(e: PointerEvent): void {
		touching = e.pointerType === 'touch';
		if (touching && onBarePane(e.target)) panePress.start(e);
	}

	// ---- a connection drawn from one box's band onto any part of another is a transition, fired by
	// a tap until it is edited. The radius reaches a box's corners, since Flow measures from its centre.
	// The document is the one owner of edges: `false` keeps Flow from drawing a provisional one.
	function onConnect(c: Connection): false {
		if (c.source && c.target) void addTransition(c.source, c.target);
		return false;
	}
	async function addTransition(from: string, to: string): Promise<void> {
		try {
			const r = await g.machine<{ id: string }>('machine transition add', {
				machine: name, from, to, triggers: [{ kind: 'manual' }]
			});
			sel.setSelection(panelId, [], [r.id]);
		} catch (e) {
			notify().failure('Add transition', e);
		}
	}

	/** The one delete path: the selected transitions first, then the states, which take their own
	 * transitions with them. */
	function deleteSelection(): void {
		const nodes = flowNodes.filter((n) => sel.nodes(panelId).has(n.id));
		const edges = flowEdges.filter((e) => sel.edges(panelId).has(e.id));
		const gone = new Set(nodes.map((n) => n.id));
		for (const e of edges) {
			if (!gone.has(e.source) && !gone.has(e.target)) void call('machine transition remove', { id: e.id });
		}
		for (const n of nodes) void call('machine state remove', { name: n.id });
	}

	// ---- a box drag previews the state's place and lands it, on the grid, as one edit. A hold
	// that never moves leaves the box where it was. Over a param row it is a mapping, not a move:
	// the box snaps back, a chip follows the pointer, and the drop hands the row a playhead's
	// variable for one of the state's attributes, picked from a menu when there are several.
	const dragOrigin = new Map<string, { x: number; y: number }>();
	let lifted = $state<{ x: number; y: number; state: string } | null>(null);
	let picker = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function onNodeDragStart(args: { nodes: Node[] }): void {
		for (const n of args.nodes) dragOrigin.set(n.id, { x: n.position.x, y: n.position.y });
	}
	/** The param row under the pointer, where a lone box is being dragged. */
	function rowUnder(args: { nodes: Node[]; event: MouseEvent | TouchEvent }): { el: Element; x: number; y: number } | null {
		const p = args.nodes.length === 1 ? eventPoint(args.event) : null;
		const el = p && document.elementFromPoint(p.clientX, p.clientY)?.closest('[data-variable-drop]');
		return p && el ? { el, x: p.clientX, y: p.clientY } : null;
	}
	/** The box back where its drag began, the preview with it. */
	function revert(n: Node): void {
		const o = dragOrigin.get(n.id);
		if (!o) return;
		pinned.set(n.id, o);
		flowNodes = flowNodes.map((f) => (f.id === n.id ? { ...f, position: o } : f));
		g.previewState(name, n.id, { pos: [o.x, o.y] });
	}
	/** Hand `row` a variable of `state`'s: the one attribute it sets on the one playhead, or a pick. */
	function offer(row: Element, state: string, at: { x: number; y: number }): void {
		const m = machine;
		if (!m) return;
		const playheads = Object.keys(m.playheads);
		const set = Object.keys(m.states[state]?.values ?? {});
		const attrs = set.length > 0 ? set : Object.keys(m.attributes);
		if (playheads.length === 0) return notify().failure('Map state', 'no playhead travels this machine; add one in the state inspector');
		if (attrs.length === 0) return notify().failure('Map state', 'the machine has no attribute to read');
		const send = (ph: string, a: string): void => void row.dispatchEvent(new CustomEvent('variable-expression-drop', { detail: `variables.${ph}.${a}` }));
		const choices = playheads.flatMap((ph) => attrs.map((a) => ({ ph, a })));
		if (choices.length === 1) return send(choices[0].ph, choices[0].a);
		picker = { x: at.x, y: at.y, items: choices.map(({ ph, a }) => ({ label: playheads.length > 1 ? `${ph}.${a}` : a, action: () => send(ph, a) })) };
	}
	function snapped(n: Node): [number, number] {
		const o = dragOrigin.get(n.id);
		if (o && o.x === n.position.x && o.y === n.position.y) return [o.x, o.y];
		return [Math.round(n.position.x / GRID) * GRID, Math.round(n.position.y / GRID) * GRID];
	}
	function onNodeDrag(args: { nodes: Node[]; event: MouseEvent | TouchEvent }): void {
		const row = rowUnder(args);
		if (row) {
			revert(args.nodes[0]);
			lifted = { x: row.x, y: row.y, state: args.nodes[0].id };
			uiStore.variableDrag = { name: args.nodes[0].id, x: row.x, y: row.y, target: row.el };
			return;
		}
		if (lifted) {
			lifted = null;
			uiStore.variableDrag = null;
		}
		for (const n of args.nodes) {
			const [x, y] = snapped(n);
			pinned.set(n.id, { x, y });
			g.previewState(name, n.id, { pos: [x, y] });
		}
		flowNodes = flowNodes.map((n) => {
			const p = pinned.get(n.id);
			return p && args.nodes.some((d) => d.id === n.id) ? { ...n, position: p } : n;
		});
	}
	function onNodeDragStop(args: { nodes: Node[]; event: MouseEvent | TouchEvent }): void {
		const row = rowUnder(args);
		lifted = null;
		uiStore.variableDrag = null;
		if (row) {
			revert(args.nodes[0]);
			pinned.delete(args.nodes[0].id);
			return offer(row.el, args.nodes[0].id, row);
		}
		for (const n of args.nodes) {
			const pos = snapped(n);
			pinned.set(n.id, { x: pos[0], y: pos[1] });
			void call('machine state edit', { name: n.id, pos }).finally(() => pinned.delete(n.id));
		}
	}

	/** The ◇ button: the manager puts the states on a ring. The camera stays; the fit button frames. */
	function arrange(): Promise<unknown> {
		return call('machine arrange', {});
	}

	/** The keys of the active panel, outside its text fields and dialogs: Escape drops a lift or the
	 * selection, Delete takes the selection. Nothing reaches a panel that is not the active one. */
	function onKeydown(e: KeyboardEvent): void {
		const t = e.target as HTMLElement | null;
		if (e.defaultPrevented || ws.activePanelId !== panelId || isTextEditingTarget(t) || t?.closest?.('dialog[open]')) return;
		const selected = sel.nodes(panelId).size || sel.edges(panelId).size;
		if (e.key === 'Escape' && lifted) {
			lifted = null;
			uiStore.variableDrag = null;
		} else if (e.key === 'Escape' && selected) {
			sel.clear(panelId);
			e.preventDefault();
		} else if ((e.key === 'Delete' || e.key === 'Backspace') && selected) {
			e.preventDefault();
			deleteSelection();
		}
	}

	// The canvas mounts with the machine, so its gestures bind as it appears.
	$effect(() => {
		const root = rootEl;
		if (!root) return;
		const offs = [
			bindTapZoom(root, {
				allowed: onBarePane,
				onStart: panePress.cancel,
				getViewport: () => getViewport?.(),
				screenToFlow: (p) => screenToFlow?.(p),
				setViewport: (v) => setViewport?.(v),
				min: MIN_ZOOM,
				max: MAX_ZOOM
			}),
			on(root, 'pointerdown', onPanePointerDown),
			on(root, 'pointermove', panePress.move),
			on(root, 'pointerup', panePress.cancel),
			on(root, 'pointercancel', panePress.cancel),
			on(document, 'keydown', onKeydown)
		];
		return () => {
			offs.forEach((off) => off());
			panePress.cancel();
		};
	});
</script>

{#if lifted}
	<div class="link-ghost" use:portal style="left: {lifted.x}px; top: {lifted.y}px" aria-hidden="true">variables.⟨playhead⟩.{lifted.state}…</div>
{/if}
{#if picker}
	<ContextMenu x={picker.x} y={picker.y} items={picker.items} onClose={() => (picker = null)} />
{/if}

<div class="wrap" data-testid="machine-panel" data-machine={name}>
	<InstanceBar testid="machine-tabs" items={tabs} active={name} onSelect={choose} onAdd={() => void create()} onRename={rename}
		onClose={(m) => void remove(m)}>
		{#snippet end()}
			{#if machine}
				<span class="seed" title="Starts the random draws, so a run repeats">
					<NumberInput value={machine.seed ?? 0} min={0} step={1} aria-label="seed" data-testid="machine-seed"
						onChange={(v) => void call('machine edit', { seed: Math.max(0, Math.round(v)) })} />
				</span>
			{/if}
			<IconButton variant="ghost" size="sm" label="Reset the playheads" title="Put every playhead back in its start state" data-testid="machine-reset"
				onclick={() => void call('machine reset', {})}><Icon name="refresh-cw" /></IconButton>
		{/snippet}
	</InstanceBar>
	{#if machine}
		<SvelteFlowProvider>
			<div class="canvas canvas-wrap" bind:this={rootEl}>
				<SvelteFlow
					bind:nodes={flowNodes}
					bind:edges={flowEdges}
					{nodeTypes}
					{edgeTypes}
					connectionMode={ConnectionMode.Loose}
					connectionLineType={ConnectionLineType.Straight}
					connectionRadius={CARD_W / 2 + 20}
					onbeforeconnect={onConnect}
					onnodedragstart={onNodeDragStart}
					onnodedrag={onNodeDrag}
					onnodedragstop={onNodeDragStop}
					onnodeclick={({ node, event }) => {
						// A plain click is the whole selection, the transition included, as Flow itself has it.
						if (modifier(event) || sel.multiSelect) sel.clickNode(panelId, node.id, true);
						else sel.setSelection(panelId, [node.id], []);
						void tick().then(reassertSelection);
					}}
					onedgeclick={({ edge, event }) => {
						sel.clickEdge(panelId, edge.id, modifier(event));
						void tick().then(reassertSelection);
					}}
					onselectionend={() => sel.setSelection(panelId, flowNodes.filter((n) => n.selected).map((n) => n.id), flowEdges.filter((e) => e.selected).map((e) => e.id))}
					onpaneclick={onPaneClick}
					fitViewOptions={FIT_OPTIONS}
					minZoom={MIN_ZOOM}
					maxZoom={MAX_ZOOM}
					bind:viewport={() => viewport, (v) => (viewport = cam.viewport = v)}
					zoomOnDoubleClick={false}
					autoPanOnNodeDrag={false}
				>
					<Background gap={GRID} size={1} patternColor="var(--border)" />
					<Controls showLock={false}>
						<ControlButton title="Arrange the states on a ring" aria-label="Arrange" data-testid="arrange" onclick={() => void arrange()}>
							<Icon name="workflow" />
						</ControlButton>
					</Controls>
					<FlowApi bind:screenToFlowPosition={screenToFlow} bind:getViewport bind:setViewport bind:fitView={flowFit} />
					<ViewportPortal target="front">
						<PlayheadDots m={machine} nodes={flowNodes} />
					</ViewportPortal>
				</SvelteFlow>
				{#if flowNodes.length === 0}
					<div class="empty-hint">
						<EmptyState>
							{#snippet title()}No states yet{/snippet}
							{#snippet hint()}Double-click or hold the canvas to add one; drag from a box's edge to another box for a transition.{/snippet}
						</EmptyState>
					</div>
				{/if}
				<!-- Its ✕ DISMISSES, holding only until the selection changes; its ◧ is the switch. -->
				<SidePane {subject} enabled={inspectorOn} onClose={dismiss} onToggle={() => sel.setInspector(panelId, !inspectorOn)}>
					{#snippet children(shown)}
						<MachineInspector {name} m={machine} subject={shown} onClose={dismiss} />
					{/snippet}
				</SidePane>
			</div>
		</SvelteFlowProvider>
	{/if}
</div>

<style>
	/* The bar, then the canvas in whatever is left. */
	.wrap {
		display: grid;
		grid-template-rows: auto minmax(0, 1fr);
		height: 100%;
	}
	.seed {
		display: inline-flex;
		width: 4.5em;
		--number-width: 100%;
	}
	/* The pane's host: the canvas and the pane side by side, so the pane never covers the cluster. */
	.canvas {
		position: relative;
		display: flex;
		min-width: 0;
		min-height: 0;
	}
	.canvas > :global(.svelte-flow) {
		flex: 1 1 0;
		min-width: 0;
		min-height: 0;
	}
	@media all {
		@container (orientation: portrait) {
			.canvas {
				flex-direction: column;
			}
		}
	}
	/* Shown while the pointer is in the panel; a touch screen, with no pointer, shows it always. */
	.canvas :global(.svelte-flow__controls) {
		margin: 0;
		bottom: var(--space-6);
		left: var(--space-6);
		opacity: 0;
		transition: opacity var(--dur-fast) var(--ease);
	}
	.canvas:hover :global(.svelte-flow__controls),
	.canvas :global(.svelte-flow__controls:focus-within) {
		opacity: 1;
	}
	@media (hover: none) and (pointer: coarse) {
		.canvas :global(.svelte-flow__controls) {
			opacity: 1;
		}
		/* A portrait sheet's grab band leans --hit over the canvas's bottom edge; the cluster sits above it. */
		@container (orientation: portrait) {
			.canvas:has(> :global(.side-panel.open)) :global(.svelte-flow__controls) {
				bottom: calc(var(--space-6) + var(--hit));
			}
		}
	}
	/* The chip that follows a box lifted over a param row: what the drop maps. */
	.link-ghost {
		position: fixed;
		z-index: var(--z-drag-ghost);
		transform: translate(12px, 12px);
		pointer-events: none;
		padding: var(--space-2);
		background: var(--surface-3);
		color: var(--text);
		border: 1px solid var(--accent);
		border-radius: var(--radius-sm);
		font-family: var(--font-mono);
		font-size: var(--fs-small);
	}
	.empty-hint {
		position: absolute;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		pointer-events: none;
	}
</style>
