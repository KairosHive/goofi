<!-- Machine panel: one state machine on a canvas — its states as cards, its transitions as edges,
     its playheads as dots that move as the manager moves them. Every change is a `machine` op, so
     the manager owns the model and this panel owns the drawing and the gesture in flight. -->
<script lang="ts">
	import {
		ConnectionMode,
		Controls,
		MarkerType,
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
	import { untrack } from 'svelte';
	import { on } from 'svelte/events';
	import { camera } from '$lib/editor/camera';
	import { bindTapZoom } from '$lib/editor/doubleTapZoom';
	import FlowApi from '$lib/editor/FlowApi.svelte';
	import SnapGuides from '$lib/editor/SnapGuides.svelte';
	import { computeSnapDelta, makeBounds, type Bounds, type Guide } from '$lib/editor/snap';
	import MachinePane from '$lib/machine/MachinePane.svelte';
	import PlayheadDots from '$lib/machine/PlayheadDots.svelte';
	import StateCard from '$lib/machine/StateCard.svelte';
	import TransitionEdge from '$lib/machine/TransitionEdge.svelte';
	import { CARD_W, FALLBACK_H, cardBox } from '$lib/machine/layout';
	import { summary } from '$lib/machine/triggers';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { Chip, EmptyState, Icon, IconButton } from '$lib/ui';
	import PanelBar from './PanelBar.svelte';

	interface MachineState {
		machine?: string;
	}

	let { panelId, state: panelState, setState }: PanelProps = $props();
	const g = graph();

	const name = $derived((asStateObject(panelState) as MachineState).machine ?? '');
	// A machine the document no longer holds leaves the panel on its chooser, as a removed one does.
	const machine = $derived(name ? g.machines[name] : undefined);
	const playheads = $derived(Object.keys(machine?.playheads ?? {}));
	watchVariables(() => playheads.map((p) => `${p}.state`));

	function choose(machine: string): void {
		setState({ machine }, 'authored', `Show machine ${machine}`);
	}
	async function create(): Promise<void> {
		try {
			const r = await g.machine<{ name: string }>('machine add', {});
			choose(r.name);
		} catch (e) {
			notify().failure('Add machine', e);
		}
	}

	let picked = $state<string | null>(null);
	let paneOn = $state(false);
	// A pick outlives the replica delta that brings its transition; only a gone machine drops it.
	$effect(() => {
		if (!machine) picked = null;
	});
	function pick(id: string | null): void {
		picked = id;
		if (id !== null) paneOn = true;
	}

	/** Fire `id` for every playhead in its `from`: resting there, or moving there. */
	function fire(id: string): void {
		const t = machine?.transitions[id];
		if (!t) return;
		for (const p of playheads) {
			const at = String(variableValue(`${p}.state`) ?? machine!.playheads[p].start);
			if (t.from === '*' || at === t.from) {
				void g.machine('machine fire', { machine: name, playhead: p, transition: id }).catch((e) => notify().failure('Fire', e));
			}
		}
	}

	// ---- the flow's nodes and edges, derived from the document; a drag pins a card until the doc agrees.
	let flowNodes = $state.raw<Node[]>([]);
	let flowEdges = $state.raw<Edge[]>([]);
	const pinned = new Map<string, { x: number; y: number }>();
	$effect(() => {
		const m = machine;
		if (!m) {
			flowNodes = [];
			return;
		}
		const previous = new Map(untrack(() => flowNodes).map((n) => [n.id, n]));
		flowNodes = Object.entries(m.states).map(([id, s]) => {
			const held = previous.get(id);
			const position = pinned.get(id) ?? { x: s.pos[0], y: s.pos[1] };
			return {
				...held,
				id,
				type: 'state',
				position,
				data: { machine: name, name: id, state: s, attributes: m.attributes },
				draggable: true,
				selectable: false
			} as Node;
		});
	});
	$effect(() => {
		const m = machine;
		if (!m) {
			flowEdges = [];
			return;
		}
		// A `*` transition has no card to leave from; it is listed in the pane and fired from there.
		flowEdges = Object.entries(m.transitions)
			.filter(([, t]) => t.from in m.states && t.to in m.states)
			.map(([id, t]) => ({
				id,
				source: t.from,
				target: t.to,
				type: 'transition',
				selectable: false,
				markerEnd: { type: MarkerType.ArrowClosed },
				data: { summary: summary(t), picked: picked === id, onFire: () => fire(id), onPick: () => pick(id) }
			}));
	});

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

	// ---- adding a state: a double-click or a held finger on the bare pane, or the bar's button.
	const onBarePane = (target: EventTarget | null): boolean =>
		Boolean((target as HTMLElement | null)?.classList.contains('svelte-flow__pane'));
	function addState(client?: { x: number; y: number }): void {
		const rect = rootEl?.getBoundingClientRect();
		const at = client ?? (rect ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 } : { x: 0, y: 0 });
		const p = screenToFlow?.(at) ?? { x: 0, y: 0 };
		const pos = [Math.round(p.x - CARD_W / 2), Math.round(p.y - 20)];
		void g.machine('machine state add', { machine: name, pos }).catch((e) => notify().failure('Add state', e));
	}
	const DOUBLE_CLICK_MS = 350;
	let lastPaneClick = { at: 0, x: 0, y: 0 };
	function onPaneClick({ event }: { event: MouseEvent }): void {
		const now = performance.now();
		if (now - lastPaneClick.at < DOUBLE_CLICK_MS && Math.hypot(event.clientX - lastPaneClick.x, event.clientY - lastPaneClick.y) < 30) {
			lastPaneClick.at = 0;
			addState({ x: event.clientX, y: event.clientY });
			return;
		}
		lastPaneClick = { at: now, x: event.clientX, y: event.clientY };
		picked = null;
	}
	const panePress = createLongPress((at) => addState({ x: at.clientX, y: at.clientY }));
	function onPanePointerDown(e: PointerEvent): void {
		if (e.pointerType === 'touch' && onBarePane(e.target)) panePress.start(e);
	}

	// ---- a connection drawn between two cards is a transition, fired by a tap until it is edited.
	async function onConnect(c: Connection): Promise<void> {
		if (!c.source || !c.target) return;
		try {
			const r = await g.machine<{ id: string }>('machine transition add', {
				machine: name, from: c.source, to: c.target, triggers: [{ kind: 'manual' }]
			});
			pick(r.id);
		} catch (e) {
			notify().failure('Add transition', e);
		}
	}

	// ---- a card drag previews the state's place and lands it as one edit, snapped to its neighbours.
	let snapGuides = $state<Guide[]>([]);
	const boundsOf = (n: Node): Bounds => {
		const b = cardBox(flowNodes, n.id) ?? { x: n.position.x, y: n.position.y, w: CARD_W, h: FALLBACK_H };
		return makeBounds(n.position.x, n.position.y, b.w, b.h);
	};
	function snapped(nodes: Node[], event: MouseEvent | TouchEvent): { dx: number; dy: number; guides: Guide[] } {
		const moving = new Set(nodes.map((n) => n.id));
		const targets = flowNodes.filter((n) => !moving.has(n.id)).map(boundsOf);
		const snap = computeSnapDelta(nodes.map(boundsOf), targets, (event as MouseEvent).altKey === true);
		for (const n of nodes) pinned.set(n.id, { x: n.position.x + snap.dx, y: n.position.y + snap.dy });
		if (snap.dx || snap.dy) {
			flowNodes = flowNodes.map((n) => (moving.has(n.id) ? { ...n, position: pinned.get(n.id)! } : n));
		}
		return snap;
	}
	function onNodeDrag(args: { nodes: Node[]; event: MouseEvent | TouchEvent }): void {
		snapGuides = snapped(args.nodes, args.event).guides;
		for (const n of args.nodes) {
			const p = pinned.get(n.id)!;
			g.previewState(name, n.id, { pos: [Math.round(p.x), Math.round(p.y)] });
		}
	}
	function onNodeDragStop(args: { nodes: Node[]; event: MouseEvent | TouchEvent }): void {
		snapped(args.nodes, args.event);
		snapGuides = [];
		for (const n of args.nodes) {
			const p = pinned.get(n.id)!;
			const pos = [Math.round(p.x), Math.round(p.y)];
			void g.machine('machine state edit', { machine: name, name: n.id, pos })
				.catch((e) => notify().failure('Move state', e))
				.finally(() => pinned.delete(n.id));
		}
	}

	function onKeydown(e: KeyboardEvent): void {
		if (e.key === 'Escape' && picked !== null) {
			picked = null;
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
		<PanelBar title={name}>
			{#snippet end()}
				<IconButton variant="ghost" size="sm" label="Add a state" title="Add a state (double-click or hold the canvas)" data-testid="machine-add-state" onclick={() => addState()}><Icon name="plus" /></IconButton>
				<IconButton variant="ghost" size="sm" label="Reset the playheads" title="Put every playhead back in its start state" data-testid="machine-reset"
					onclick={() => void g.machine('machine reset', { machine: name }).catch((e) => notify().failure('Reset', e))}><Icon name="refresh-cw" /></IconButton>
				<IconButton variant={paneOn ? 'primary' : 'ghost'} size="sm" label="Toggle the pane" title="Attributes, playheads and the picked transition" aria-pressed={paneOn} data-testid="machine-pane-toggle" onclick={() => (paneOn = !paneOn)}>◧</IconButton>
			{/snippet}
		</PanelBar>
		<div class="split">
			<SvelteFlowProvider>
				<div class="canvas canvas-wrap" bind:this={rootEl}>
					<SvelteFlow
						bind:nodes={flowNodes}
						bind:edges={flowEdges}
						{nodeTypes}
						{edgeTypes}
						connectionMode={ConnectionMode.Loose}
						onconnect={onConnect}
						onnodedrag={onNodeDrag}
						onnodedragstop={onNodeDragStop}
						onpaneclick={onPaneClick}
						onedgeclick={({ edge }) => fire(edge.id)}
						fitViewOptions={FIT_OPTIONS}
						minZoom={MIN_ZOOM}
						maxZoom={MAX_ZOOM}
						bind:viewport={() => viewport, (v) => (viewport = cam.viewport = v)}
						zoomOnDoubleClick={false}
						autoPanOnNodeDrag={false}
						deleteKey={null}
					>
						<Controls showLock={false} />
						<FlowApi bind:screenToFlowPosition={screenToFlow} bind:getViewport bind:setViewport bind:fitView={flowFit} />
						<ViewportPortal target="front">
							<PlayheadDots m={machine} nodes={flowNodes} />
							{#if snapGuides.length > 0}
								<SnapGuides guides={snapGuides} testid="machine-snap-guides" />
							{/if}
						</ViewportPortal>
					</SvelteFlow>
					{#if flowNodes.length === 0}
						<div class="empty-hint">
							<EmptyState>
								{#snippet title()}No states yet{/snippet}
								{#snippet hint()}Double-click or hold the canvas to add one; drag between two cards for a transition.{/snippet}
							</EmptyState>
						</div>
					{/if}
				</div>
			</SvelteFlowProvider>
			{#if paneOn}
				<MachinePane {name} m={machine} {picked} onPick={pick} onClose={() => (paneOn = false)} />
			{/if}
		</div>
	{/if}
</div>

<style>
	/* The bar, then the canvas and pane in whatever is left. */
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
	/* The canvas and the pane share the panel: side by side, or stacked where the panel is taller than wide. */
	.split {
		display: flex;
		min-height: 0;
	}
	.canvas {
		position: relative;
		flex: 1;
		min-width: 0;
		min-height: 0;
	}
	.split > :global(.pane) {
		flex: 0 0 clamp(14rem, 38%, 24rem);
	}
	@container (orientation: portrait) {
		.split {
			flex-direction: column;
		}
		.split > :global(.pane) {
			flex: 0 0 50%;
			border-left: none;
			border-top: 1px solid var(--border);
		}
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
