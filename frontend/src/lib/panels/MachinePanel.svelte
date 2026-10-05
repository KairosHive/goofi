<!-- State machine panel: one machine on a canvas — its states as boxes, its transitions as lines,
     its playheads as dots that move as the manager moves them — with the editor's inspector pane
     beside it. Every change is a `machine` op, so the manager owns the model and this panel owns the
     drawing and the gesture in flight. -->
<script lang="ts">
	import {
		ConnectionMode,
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
	import { asStateObject, createLongPress, type PanelProps } from 'panelty';
	import { tick, untrack } from 'svelte';
	import { on } from 'svelte/events';
	import { camera } from '$lib/editor/camera';
	import { bindTapZoom } from '$lib/editor/doubleTapZoom';
	import FlowApi from '$lib/editor/FlowApi.svelte';
	import MachineInspector from '$lib/machine/MachineInspector.svelte';
	import PlayheadDots from '$lib/machine/PlayheadDots.svelte';
	import StateCard from '$lib/machine/StateCard.svelte';
	import type { Subject } from '$lib/machine/subject';
	import TransitionEdge from '$lib/machine/TransitionEdge.svelte';
	import ConnectionPreview from '$lib/machine/ConnectionPreview.svelte';
	import { courses, type LabelSize, type RouteGeometry } from '$lib/machine/geometry';
	import { CARD_W, FALLBACK_H, cardBox } from '$lib/machine/layout';
	import { summary } from '$lib/machine/triggers';
	import { graph, type MachineOp } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { selection } from '$lib/stores/selection.svelte';
	import { Chip, EmptyState, Icon, IconButton, Select, isTextEditingTarget } from '$lib/ui';
	import PanelBar from './PanelBar.svelte';
	import SidePane from './SidePane.svelte';

	interface MachineState {
		machine?: string;
	}

	let { panelId, state: panelState, setState }: PanelProps = $props();
	const g = graph();
	const sel = selection();

	const name = $derived((asStateObject(panelState) as MachineState).machine ?? '');
	// A machine the document no longer holds leaves the panel on its chooser, as a removed one does.
	const machine = $derived(name ? g.machines[name] : undefined);

	function choose(machine: string): void {
		setState({ machine }, 'navigation');
	}
	function renamed(machine: string): void {
		const next = camera(`${panelId}/machine/${machine}`);
		next.viewport = cam.viewport;
		next.fittedEpoch = cam.fittedEpoch;
		choose(machine);
	}
	async function create(): Promise<void> {
		try {
			const r = await g.machine<{ name: string }>('machine add', {});
			choose(r.name);
		} catch (e) {
			notify().failure('Add machine', e);
		}
	}
	function call(op: MachineOp, payload: Record<string, unknown>): Promise<unknown> {
		return g.machine(op, { machine: name, ...payload }).catch((e) => notify().failure(op, e));
	}

	// ---- the selection and the inspector: the editor's store, so a dismissed pane re-arms the same way.
	const inspectorOn = $derived(sel.inspectorVisibleFor(panelId));
	const subject = $derived.by<Subject | null>(() => {
		const m = machine;
		if (!m) return null;
		const nodes = sel.nodes(panelId);
		const edges = sel.edges(panelId);
		if (nodes.size === 1 && edges.size === 0) {
			const id = [...nodes][0];
			if (id in m.states) return { kind: 'state', id };
		}
		if (edges.size === 1 && nodes.size === 0) {
			const id = [...edges][0];
			if (id in m.transitions) return { kind: 'transition', id };
		}
		return { kind: 'machine' };
	});
	const dismiss = (): void => sel.dismissInspectorFor(panelId);

	// ---- the flow's nodes and edges, derived from the document; a drag pins a box until the doc agrees.
	let flowNodes = $state.raw<Node[]>([]);
	let flowEdges = $state.raw<Edge[]>([]);
	let labelSizes = $state.raw<Record<string, LabelSize>>({});
	const geometry = $derived(machine ? courses(machine, (id) => cardBox(flowNodes, id), (key) => labelSizes[key]) : new Map());
	function measured(key: string, size: LabelSize): void {
		const previous = labelSizes[key];
		if (previous?.w !== size.w || previous?.h !== size.h) labelSizes = { ...labelSizes, [key]: size };
	}
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
		flowEdges = [...geometry.values()].map((r) => ({
			id: r.key, source: r.source, target: r.target, type: 'transition', selected: selected.has(r.id),
			data: { summary: summary(m.transitions[r.id]), geometry: r, labelSize: labelSizes[r.key],
				onMeasure: (size: LabelSize) => measured(r.key, size), onPick: (event: MouseEvent) => pickTransition(r.id, event) }
		}));
	});
	/** Flow's own `selected` flags, written back from the store after Flow has had its say. */
	function reassertSelection(): void {
		flowNodes = flowNodes.map((n) => ({ ...n, selected: sel.nodes(panelId).has(n.id) }));
		flowEdges = flowEdges.map((e) => ({ ...e, selected: sel.edges(panelId).has(transitionId(e)) }));
	}
	const modifier = (e: MouseEvent | TouchEvent): boolean => 'shiftKey' in e && (e.shiftKey || e.ctrlKey || e.metaKey);
	const transitionId = (edge: Edge): string => (edge.data?.geometry as RouteGeometry).id;
	function pickTransition(id: string, event: MouseEvent | TouchEvent): void {
		sel.clickEdge(panelId, id, modifier(event));
		void tick().then(reassertSelection);
	}

	const nodeTypes = { state: StateCard };
	const edgeTypes = { transition: TransitionEdge };
	const FIT_OPTIONS = { maxZoom: 1, padding: 0.18 } satisfies FitViewOptions;
	const MIN_ZOOM = 0.05;
	const MAX_ZOOM = 4;

	// ---- the camera, kept per panel so a reshape keeps the framing.
	let rootEl = $state<HTMLDivElement | null>(null);
	let inset = $state({ right: 0, bottom: 0 });
	$effect(() => {
		const root = rootEl;
		const open = inspectorOn && subject !== null;
		const pane = root?.querySelector<HTMLElement>('[data-testid="auto-side-panel"]');
		if (!root || !pane || !open) { inset = { right: 0, bottom: 0 }; return; }
		// Reserve the pane and its actual resize grip; its CSS remains the size and axis owner.
		const update = (): void => {
			const vertical = getComputedStyle(pane).getPropertyValue('--pane-axis').trim() === 'y';
			const grip = pane.querySelector<HTMLElement>('[data-testid="panel-resize-handle"]');
			const panel = pane.getBoundingClientRect();
			const band = grip?.getBoundingClientRect();
			const before = grip ? getComputedStyle(grip, '::before') : null;
			const extra = band ? Math.max(0, vertical ? panel.top - band.top : panel.left - band.left)
				+ Math.max(0, -(parseFloat(vertical ? before?.top ?? '' : before?.left ?? '') || 0)) : 0;
			inset = vertical ? { right: 0, bottom: pane.offsetHeight + extra } : { right: pane.offsetWidth + extra, bottom: 0 };
		};
		const observer = new ResizeObserver(update);
		observer.observe(pane);
		update();
		return () => observer.disconnect();
	});
	let screenToFlow = $state<((p: { x: number; y: number }) => { x: number; y: number }) | undefined>(undefined);
	let getViewport = $state<(() => Viewport) | undefined>(undefined);
	let setViewport = $state<((v: Viewport) => void) | undefined>(undefined);
	let flowFit = $state<((o?: FitViewOptions) => Promise<boolean>) | undefined>(undefined);
	const cam = $derived(camera(`${panelId}/machine/${name}`));
	let viewport = $state<Viewport>({ x: 0, y: 0, zoom: 1 });
	$effect(() => {
		const current = cam;
		untrack(() => { sel.clear(panelId); pinned.clear(); labelSizes = {}; viewport = current.viewport ?? { x: 0, y: 0, zoom: 1 }; });
	});
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
		const pos = [Math.round(p.x - CARD_W / 2), Math.round(p.y - FALLBACK_H / 2)];
		void call('machine state add', { pos });
	}
	function onPaneClick({ event }: { event: MouseEvent }): void {
		if (event.detail === 2) {
			addState({ x: event.clientX, y: event.clientY });
			return;
		}
		sel.clickPane(panelId, event.shiftKey);
		if (sel.nodes(panelId).size || sel.edges(panelId).size) void tick().then(reassertSelection);
	}
	const panePress = createLongPress((at) => addState({ x: at.clientX, y: at.clientY }));
	function onPanePointerDown(e: PointerEvent): void {
		if (e.pointerType === 'touch' && onBarePane(e.target)) panePress.start(e);
	}

	// ---- a connection drawn from one box's band onto any part of another is a transition, fired by
	// a tap until it is edited. The radius reaches a box's corners, since Flow measures from its centre.
	async function onConnect(c: Connection): Promise<void> {
		if (!c.source || !c.target) return;
		try {
			const r = await g.machine<{ id: string }>('machine transition add', {
				machine: name, from: c.source, to: c.target, triggers: [{ kind: 'manual' }]
			});
			sel.setSelection(panelId, [], [r.id]);
		} catch (e) {
			notify().failure('Add transition', e);
		}
	}

	/** The one delete path, for the Delete key: the transitions first, then the states, which take
	 * their own transitions with them. */
	function deleteElements({ nodes, edges }: { nodes: Node[]; edges: Edge[] }): void {
		const gone = new Set(nodes.map((n) => n.id));
		const transitions = new Set(edges.filter((e) => !gone.has(e.source) && !gone.has(e.target)).map((e) => transitionId(e)));
		for (const id of transitions) void call('machine transition remove', { id });
		for (const n of nodes) void call('machine state remove', { name: n.id });
	}

	// ---- a box drag previews the state's place and lands it as one edit.
	function onNodeDrag(args: { nodes: Node[] }): void {
		for (const n of args.nodes) {
			pinned.set(n.id, n.position);
			g.previewState(name, n.id, { pos: [Math.round(n.position.x), Math.round(n.position.y)] });
		}
	}
	function onNodeDragStop(args: { nodes: Node[] }): void {
		for (const n of args.nodes) {
			pinned.set(n.id, n.position);
			const pos = [Math.round(n.position.x), Math.round(n.position.y)];
			void call('machine state edit', { name: n.id, pos }).finally(() => pinned.delete(n.id));
		}
	}

	function onKeydown(e: KeyboardEvent): void {
		if (e.defaultPrevented || isTextEditingTarget(e.target)) return;
		if (e.key === 'Escape' && (sel.nodes(panelId).size || sel.edges(panelId).size)) {
			sel.clear(panelId);
			e.preventDefault();
		}
	}

	// The canvas mounts with the machine, after the chooser, so its gestures bind as it appears.
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
			on(root, 'keydown', onKeydown)
		];
		return () => {
			offs.forEach((off) => off());
			panePress.cancel();
		};
	});
</script>

<div class="wrap" data-testid="machine-panel" data-machine={name}>
	{#if !machine}
		<div class="chooser" data-testid="machine-chooser">
			<EmptyState>
				{#snippet title()}{name ? `No machine ${name}` : 'No machine shown'}{/snippet}
				{#snippet hint()}Pick one, or start a new one.{/snippet}
			</EmptyState>
			<div class="choices">
				{#each Object.keys(g.machines) as m (m)}
					<Chip data-testid="machine-pick" onclick={() => choose(m)}>{m}</Chip>
				{/each}
				<Chip tone="accent" data-testid="machine-new" onclick={() => void create()}><Icon name="plus" /> new machine</Chip>
			</div>
		</div>
	{:else}
		<PanelBar>
			{#snippet start()}
				<Select aria-label="Machine" data-testid="machine-switch" value={name} options={Object.keys(g.machines)} onChange={choose} />
			{/snippet}
			{#snippet end()}
				<IconButton variant="ghost" size="sm" label="New machine" title="Start a new machine here" data-testid="machine-new" onclick={() => void create()}><Icon name="plus" /></IconButton>
				<IconButton variant="ghost" size="sm" label="Reset the playheads" title="Put every playhead back in its start state" data-testid="machine-reset"
					onclick={() => void call('machine reset', {})}><Icon name="refresh-cw" /></IconButton>
			{/snippet}
		</PanelBar>
		{#key name}
		<SvelteFlowProvider>
			<div class="canvas canvas-wrap" bind:this={rootEl} style:padding-right={`${inset.right}px`} style:padding-bottom={`${inset.bottom}px`}>
				<SvelteFlow
					bind:nodes={flowNodes}
					bind:edges={flowEdges}
					{nodeTypes}
					{edgeTypes}
					connectionMode={ConnectionMode.Loose}
					connectionLineComponent={ConnectionPreview}
					connectionRadius={CARD_W / 2 + 20}
					deleteKey={['Delete', 'Backspace']}
					ondelete={deleteElements}
					onconnect={onConnect}
					onnodedrag={onNodeDrag}
					onnodedragstop={onNodeDragStop}
					onnodeclick={({ node, event }) => {
						// A plain click is the whole selection, the transition included, as Flow itself has it.
						sel.clickNode(panelId, node.id, modifier(event));
						void tick().then(reassertSelection);
					}}
					onedgeclick={({ edge, event }) => pickTransition(transitionId(edge), event)}
					onselectionend={() => sel.setSelection(panelId, flowNodes.filter((n) => n.selected).map((n) => n.id), flowEdges.filter((e) => e.selected).map((e) => transitionId(e)))}
					onpaneclick={onPaneClick}
					fitViewOptions={FIT_OPTIONS}
					minZoom={MIN_ZOOM}
					maxZoom={MAX_ZOOM}
					bind:viewport={() => viewport, (v) => (viewport = cam.viewport = v)}
					zoomOnDoubleClick={false}
					autoPanOnNodeDrag={false}
				>
					<Controls showLock={false} />
					<FlowApi bind:screenToFlowPosition={screenToFlow} bind:getViewport bind:setViewport bind:fitView={flowFit} />
					<ViewportPortal target="front">
						<PlayheadDots m={machine} nodes={flowNodes} {geometry} />
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
						<MachineInspector {name} m={machine} subject={shown} onClose={dismiss} onRename={renamed}
							onSelectTransition={(id) => { sel.setSelection(panelId, [], [id]); }} />
					{/snippet}
				</SidePane>
			</div>
		</SvelteFlowProvider>
		{/key}
	{/if}
</div>

<style>
	/* The bar, then the canvas in whatever is left. */
	.wrap {
		display: grid;
		grid-template-rows: auto minmax(0, 1fr);
		height: 100%;
	}
	.chooser {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: var(--space-4);
		height: 100%;
		padding: var(--space-4);
	}
	.choices {
		display: flex;
		flex-wrap: wrap;
		justify-content: center;
		gap: var(--space-2);
	}
	/* The pane's host: `.canvas` is what it slides in over. */
	.canvas {
		position: relative;
		min-width: 0;
		min-height: 0;
	}
	.canvas :global(.svelte-flow__controls) {
		margin: 0;
		bottom: var(--space-6);
		left: var(--space-6);
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
