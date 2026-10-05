<!-- Node-editor panel: the SvelteFlow graph and its gestures. Each instance owns its viewport, and
     its keyboard shortcuts act only while it is the active panel; app-global ones live in AppShell. -->
<script lang="ts">
	import {
		SvelteFlow,
		Controls,
		SvelteFlowProvider,
		ViewportPortal,
		type Connection,
		type Edge,
		type FitViewOptions,
		type Node,
		type Viewport
	} from '@xyflow/svelte';
	import GoofiNode from '$lib/editor/GoofiNode.svelte';
	import AddNodeMenu from '$lib/editor/AddNodeMenu.svelte';
	import PlacementPreview from '$lib/editor/PlacementPreview.svelte';
	import { GHOST_UID, ghostNode } from '$lib/editor/placementGhost';
	import { camera } from '$lib/editor/camera';
	import FlowApi from '$lib/editor/FlowApi.svelte';
	import FlowSurface from '$lib/editor/FlowSurface.svelte';
	import { provideSurface } from '$lib/viewers/plotHost';
	import type { SurfaceHandle } from '$lib/api/drawings';
	import SubpatchZoomExit from '$lib/editor/SubpatchZoomExit.svelte';
	import SnapGuides from '$lib/editor/SnapGuides.svelte';
	import ReferenceEdges from '$lib/editor/ReferenceEdges.svelte';
	import { computeSnapDelta, makeBounds, type Bounds, type Guide } from '$lib/editor/snap';
	import { graph } from '$lib/stores/graph.svelte';
	import { history } from '$lib/stores/history.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { ui, slotKey, type SlotClickSeed } from '$lib/stores/ui.svelte';
	import { seedSlot } from '$lib/editor/seedSlot';
	import { bareName } from '$lib/editor/typeId';
	import { selection } from '$lib/stores/selection.svelte';
	import { workspace } from 'panelty';
	import { getPanelType, type PanelProps } from 'panelty';
	import { portal } from 'panelty';
	import {
		linkKey,
		type LinkInfo,
		type NodeInstanceInfo,
		type NodeTypeInfo,
		type Step
	} from '$lib/api/control';
	import { ROOT_ID, childrenOfScope, drawEndpoint as sceneDrawEndpoint } from '$lib/editor/subpatchScene';
	import { nodeSurfaceSize, inputUnits } from '$lib/editor/nodeMetrics';
	import { isSlotExpanded } from '$lib/viewers/inlineView';
	import {
		inputAnchors,
		nearSlots,
		sameKeys,
		SLOT_PROXIMITY_PX,
		type SlotAnchor
	} from '$lib/editor/slotProximity';
	import { createLongPress } from 'panelty';
	import { bindTapZoom } from '$lib/editor/doubleTapZoom';
	import { eventPoint } from '$lib/editor/eventPoint';
	import { serializeClipboard, parseClipboard, fragmentCentre, centroid } from '$lib/editor/clipboard';
	import { copyText } from '$lib/clipboard';
	import { registerEditor, unregisterEditor } from './editorCommands';
	import SidePane from './SidePane.svelte';
	import Inspector from '$lib/inspector/Inspector.svelte';
	import { arrayToPath, asStateObject, pathToArray } from 'panelty';
	import { Button, IconButton, EmptyState, isTextEditingTarget } from '$lib/ui';
	import { clampToViewport, overlayViewport } from 'panelty';
	import { onMount, tick, untrack } from 'svelte';
	import { on } from 'svelte/events';

	let { panelId, state: panelState, setState }: PanelProps = $props();

	const g = graph();
	const uiStore = ui();
	const sel = selection();
	const ws = workspace();

	/** Only the active panel reacts to keyboard shortcuts and pending slot clicks. */
	const isActive = (): boolean => ws.activePanelId === panelId;

	const selectedNode = $derived(sel.selectedNode(panelId));

	// The standing ◧ preference MINUS a live ✕ dismissal, which holds only until the selection
	// next changes.
	const inspectorOn = $derived(sel.inspectorVisibleFor(panelId));

	$effect(() => {
		if (ws.activePanelId === panelId) sel.setActiveEditor(panelId);
	});

	let rootEl = $state<HTMLDivElement | null>(null);

	/** Which edge of the menu's own box the requested open point names. */
	type MenuAlign = 'start' | 'end';

	let menuOpen = $state(false);
	// The REQUESTED spawn point and the RENDERED one are separate, so the placement effect never
	// writes its own dependency.
	let menuAt = $state<{ x: number; y: number; align: MenuAlign }>({ x: 120, y: 120, align: 'start' });
	let menuPos = $state<{ x: number; y: number }>({ x: 120, y: 120 });
	let menuSeed = $state<SlotClickSeed | null>(null);
	let menuEl = $state<HTMLDivElement | null>(null);
	// A long press opens the menu with the finger still down, so the touchend arrives as a compat
	// click. At the WINDOW in capture, because which layer that click lands on is not knowable.
	let swallowMenuClick = $state(false);
	$effect(() => {
		if (!swallowMenuClick) return;
		const eat = (e: MouseEvent): void => {
			e.stopPropagation();
			swallowMenuClick = false;
		};
		// A lift that sends no click (a slide past the slop) must not leave the next tap eaten.
		let timer = 0;
		const lifted = (): void => {
			timer = window.setTimeout(() => (swallowMenuClick = false), 400);
		};
		const opts = { capture: true, once: true } as const;
		window.addEventListener('click', eat, opts);
		window.addEventListener('pointerup', lifted, opts);
		window.addEventListener('pointercancel', lifted, opts);
		return () => {
			clearTimeout(timer);
			window.removeEventListener('click', eat, opts);
			window.removeEventListener('pointerup', lifted, opts);
			window.removeEventListener('pointercancel', lifted, opts);
		};
	});

	/** Open the add-node menu at a viewport point — the one placement path for all four entry
	 * points. The point names the menu's left or right edge; the effect below clamps it. */
	function openAddMenu(
		x: number,
		y: number,
		align: MenuAlign = 'start',
		seed: SlotClickSeed | null = null,
		swallowNextDismiss = false
	): void {
		menuAt = { x, y, align };
		menuPos = { x, y }; // corrected below before paint
		menuSeed = seed;
		swallowMenuClick = swallowNextDismiss;
		menuOpen = true;
	}

	function closeMenu(): void {
		menuOpen = false;
		menuSeed = null;
	}

	/** The coarse-pointer door onto the add-node menu. Armed for `touch` alone: a held mouse button
	 * is the start of a desktop pan. */
	const canvasPress = createLongPress((at) =>
		openAddMenu(at.clientX - 8, at.clientY + 8, 'start', null, true)
	);

	/** Empty canvas only, and not while a ghost is pending: the canvas belongs to that placement. */
	function onCanvasPointerDown(e: PointerEvent): void {
		if (e.pointerType !== 'touch' || pendingPlacement) return;
		if (!onBareCanvas(e.target)) return;
		canvasPress.start(e);
	}

	const onBareCanvas = (target: EventTarget | null): boolean =>
		Boolean((target as HTMLElement | null)?.classList.contains('svelte-flow__pane'));

	/** The double-tap-and-drag zoom, beside pinch; the seam is `zoomOnDoubleClick={false}` below.
	 * Bound in `onMount`; a double tap held still stands the long-press door down. */
	function bindZoom(root: HTMLElement): () => void {
		return bindTapZoom(root, {
			allowed: (target) => !pendingPlacement && onBareCanvas(target),
			onStart: canvasPress.cancel,
			getViewport: () => getViewport?.(),
			screenToFlow: (p) => screenToFlow?.(p),
			setViewport: (v) => setViewport?.(v),
			min: MIN_ZOOM,
			max: MAX_ZOOM
		});
	}

	// Measure the mounted menu, then re-clamp: a spawn point is a degenerate anchor rect.
	$effect(() => {
		const el = menuEl;
		if (!el) return;
		const at = menuAt;
		const place = (): void => {
			const r = el.getBoundingClientRect();
			const left = at.align === 'end' ? at.x - r.width : at.x;
			// `overlayViewport()`, not `window.innerHeight`: this menu focuses its search on open, so
			// the soft keyboard is on its way up as it lands and the layout viewport does not shrink.
			const p = clampToViewport(
				{ left, top: at.y, right: left, bottom: at.y, width: 0, height: 0 },
				{ width: r.width, height: r.height },
				overlayViewport()
			);
			menuPos = { x: p.left, y: p.top };
		};
		place();
		const vv = window.visualViewport;
		vv?.addEventListener('resize', place);
		return () => vv?.removeEventListener('resize', place);
	});

	// A clicked port opens the menu beside it, seeded with the dtype filter + auto-link.
	$effect(() => {
		const seed = uiStore.pendingSlotClick;
		if (!seed) return;
		if (!isActive()) return;
		uiStore.pendingSlotClick = null;
		const source = seed.side === 'source';
		openAddMenu(
			seed.clientX + (source ? 12 : -12),
			seed.clientY - 24,
			source ? 'start' : 'end',
			seed
		);
	});

	let snapGuides = $state<Guide[]>([]);
	let pendingPlacement = $state<{
		typeInfo: NodeTypeInfo;
		seed: SlotClickSeed | null;
		initialClient: { x: number; y: number };
	} | null>(null);

	let flowNodes = $state.raw<Node[]>([]);
	let flowEdges = $state.raw<Edge[]>([]);
	// Bumped to force a flowEdges re-derive: SvelteFlow inserts a dropped connection optimistically
	// before `onConnect` runs, and a REJECTED wire produces no doc echo to rebuild from.
	let reconcileTick = $state(0);

	// The stack of instance ids this editor has descended into; empty = top level.
	const enteredPath = $derived(pathToArray(asStateObject(panelState).subpatchPath));
	const entered = $derived(enteredPath.length ? enteredPath[enteredPath.length - 1] : null);
	// The ghost's record and where the pointer holds it; the rebuild below draws it as a flow node.
	const ghost = $derived(pendingPlacement ? ghostNode(pendingPlacement.typeInfo, entered ?? ROOT_ID) : null);
	let ghostPos = $state<[number, number] | null>(null);

	/** Write the path into the panel's state as NAVIGATION: descending into a sub-patch is looking,
	 * not editing, so it must not mark the patch unsaved. */
	function setPath(path: string[]): void {
		setState({ ...asStateObject(panelState), subpatchPath: arrayToPath(path) }, 'navigation');
	}

	/** uid → the scope it is drawn in. Membership rides the record, so this is a read, not a walk. */
	const memberIndex = $derived(new Map(g.nodes.map((n) => [n.uid, n.scope])));

	/** Is this uid a sub-patch facade — the one thing this gesture can ENTER? */
	function isScope(uid: string): boolean {
		return !!g.nodeById(uid)?.subpatch;
	}

	function enterInstance(instId: string): void {
		if (!isScope(instId)) return;
		if (enteredPath[enteredPath.length - 1] === instId) return; // already inside it
		sel.clear(panelId);
		setPath([...enteredPath, instId]);
		setTimeout(fitView, 60); // frame the inside once it has rendered
	}

	/** Pop the breadcrumb back to `depth` levels (0 = top of the patch). */
	function exitToDepth(depth: number): void {
		sel.clear(panelId);
		setPath(enteredPath.slice(0, depth));
		setTimeout(fitView, 60);
	}

	// If an entered instance is dissolved/removed elsewhere, climb back out of it.
	$effect(() => {
		if (enteredPath.length === 0) return;
		let depth = enteredPath.length;
		while (depth > 0 && !isScope(enteredPath[depth - 1])) depth--;
		if (depth !== enteredPath.length) setPath(enteredPath.slice(0, depth));
	});

	/** Resolve a link endpoint to what is actually drawn in the entered scope: the facade of the
	 * nearest enclosing scope, or null when the endpoint lies outside the entered subtree. */
	function drawEndpoint(node: string, slot: string): { node: string; handle: string } | null {
		// The scene algebra always takes a real scope id; `entered` stays null at root, which is
		// what the "are we inside a sub-patch?" decisions read.
		return sceneDrawEndpoint(node, slot, entered ?? ROOT_ID, memberIndex);
	}

	/** The handles a card draws, one per slot in order — kept on the flow node, as the record it
	 * came from is mutated in place. */
	function handlesOf(n: { input_slots: Record<string, string>; output_slots: Record<string, string> }): string {
		return `${Object.keys(n.input_slots).join(' ')}|${Object.keys(n.output_slots).join(' ')}`;
	}

	// True only between a box/marquee drag's start and end, so a plain pane click's end event
	// cannot resurrect a just-cleared selection — and so a rebuild knows a marquee is live.
	let boxSelecting = false;

	// Render the direct children of the entered scope — leaves, nested facades and boundary ports
	// alike, because each is a node record that names this scope.
	$effect(() => {
		const scope = entered ?? ROOT_ID;
		// SvelteFlow hides a node it has not measured and trusts a measured one's handle bounds, so
		// the size it wrote on the previous object rides along only while the handles are the same.
		const previous = new Map(untrack(() => flowNodes).map((n) => [n.id, n]));
		const next: Node[] = [];
		for (const uid of childrenOfScope(scope, memberIndex)) {
			const n = g.nodeById(uid);
			if (!n) continue;
			const was = previous.get(uid);
			const handles = handlesOf(n);
			next.push({
				id: uid,
				type: 'goofi',
				position: pinned.get(uid) ?? { x: n.pos?.[0] ?? 0, y: n.pos?.[1] ?? 0 },
				data: { node: n, label: n.name, handles },
				// A live marquee's flags are Flow's, on these very objects: re-deriving them from
				// the store mid-drag hands the release an empty selection.
				selected: boxSelecting ? (was?.selected ?? false) : sel.nodes(panelId).has(uid),
				measured: was?.data.handles === handles ? was.measured : undefined
			});
		}
		if (ghost && ghostPos) {
			const was = previous.get(GHOST_UID);
			const handles = handlesOf(ghost);
			next.push({
				id: GHOST_UID,
				type: 'goofi',
				class: 'ghost',
				position: { x: ghostPos[0], y: ghostPos[1] },
				data: { node: ghost, label: ghost.name, handles, ghost: true },
				draggable: false,
				selectable: false,
				connectable: false,
				measured: was?.data.handles === handles ? was.measured : undefined
			});
		}
		flowNodes = next;
	});

	// Every link, rerouted to the nearest VISIBLE boundary port in the entered scope. A port's own
	// inner wire is one of them: it is a link like any other, drawn where its port is drawn.
	$effect(() => {
		reconcileTick; // re-derive on demand to drop an optimistic ghost edge after a rejected wire
		const previous = new Map(untrack(() => flowEdges).map((e) => [e.id, e]));
		const next: Edge[] = [];
		for (const l of g.links) {
			const src = drawEndpoint(l.node_out, l.slot_out);
			const dst = drawEndpoint(l.node_in, l.slot_in);
			if (!src || !dst) continue;
			if (src.node === dst.node && l.node_out !== l.node_in) continue; // internal to one collapsed child -> hidden (but keep a real self-loop)
			const id = linkKey(l);
			next.push({
				id,
				source: src.node,
				sourceHandle: src.handle,
				target: dst.node,
				targetHandle: dst.handle,
				selected: boxSelecting ? (previous.get(id)?.selected ?? false) : sel.edges(panelId).has(id),
				animated: false
			});
		}
		flowEdges = next;
	});

	/** Node ids selected in THIS editor — the store's selection plus a live marquee. */
	function selectedUids(): string[] {
		const ids = new Set<string>(sel.nodes(panelId));
		for (const n of flowNodes) if (n.selected) ids.add(n.id);
		return [...ids];
	}

	async function groupSelection(): Promise<void> {
		const names = selectedUids();
		if (names.length === 0) return;
		// Place the collapsed group node at the centroid of its members.
		const [x, y] = centroid(names.map((n) => g.nodeById(n)?.pos).filter((p): p is [number, number] => !!p));
		try {
			const instId = await g.groupNodes(names, [Math.round(x), Math.round(y)]);
			sel.selectNodes(panelId, [instId]);
		} catch (e) {
			console.warn('group failed', e);
		}
	}

	/** The link a Flow connection or edge names; null while it lacks an end. */
	function linkOf(c: { source: string; target: string; sourceHandle?: string | null; targetHandle?: string | null }): LinkInfo | null {
		if (!c.source || !c.target || !c.sourceHandle || !c.targetHandle) return null;
		return { node_out: c.source, node_in: c.target, slot_out: c.sourceHandle, slot_in: c.targetHandle };
	}

	function onConnect(c: Connection): void {
		const link = linkOf(c);
		if (!link) return;
		// Every cable is one `add_link`, the pill's included: a port is a node to the op vocabulary,
		// and a top-level wire to a collapsed facade is spliced to the inner leaf by the bridge.
		void g.addLink(link).catch((e) => {
			notify().failure('Connect', e);
			reconcileTick++;
		});
	}

	/** Drag an existing edge's endpoint to a new slot. Remove + add are one history entry, so a
	 * single undo reverts the move. */
	function onReconnect(oldEdge: Edge, c: Connection): void {
		const was = linkOf(oldEdge);
		const link = linkOf(c);
		if (!was || !link) return;
		void history()
			.transaction('Reconnect link', async (step) => {
				await g.removeLink(was, step);
				await g.addLink(link, step);
			})
			// `transaction` re-throws, so a refused move needs this catch: the rebuild puts every
			// cable back where `g.links` says it is.
			.catch((e) => {
				notify().failure('Reconnect', e);
				reconcileTick++;
			});
	}

	function onEdgeClick(args: { edge: Edge; event: MouseEvent }): void {
		const e = args.event;
		sel.clickEdge(panelId, args.edge.id, e.shiftKey || e.ctrlKey || e.metaKey);
	}

	// Input names, revealed by proximity while a cable is in flight. The anchors are snapshotted
	// ONCE per drag in FLOW space, so a canvas that pans or zooms mid-drag needs no invalidation.
	let cableAnchors: SlotAnchor[] = [];

	function publishCableNear(next: ReadonlySet<string>): void {
		if (!sameKeys(next, uiStore.cableNear)) uiStore.cableNear = next; // an equal set invalidates no node
	}

	function onCableMove(e: PointerEvent): void {
		const toFlow = screenToFlow;
		if (!toFlow || cableAnchors.length === 0) return;
		const zoom = getViewport?.().zoom ?? 1;
		// The radius is a SCREEN distance, so it is converted into flow space, not the anchors out.
		publishCableNear(
			nearSlots(cableAnchors, toFlow({ x: e.clientX, y: e.clientY }), SLOT_PROXIMITY_PX / zoom)
		);
	}

	function onCableStart(): void {
		cableAnchors = inputAnchors(
			flowNodes.map((f) => {
				const n = f.data.node as NodeInstanceInfo;
				return { uid: f.id, x: f.position.x, y: f.position.y, slots: Object.keys(n.input_slots ?? {}) };
			}),
			slotKey
		);
		publishCableNear(new Set());
		// On `window`, not the panel: a cable dragged past the panel's edge is still in flight.
		window.addEventListener('pointermove', onCableMove);
	}

	function onCableEnd(): void {
		window.removeEventListener('pointermove', onCableMove);
		cableAnchors = [];
		publishCableNear(new Set());
	}

	/** A node's snap footprint when Svelte Flow has not measured it yet. */
	function nodeFallbackSize(node: NodeInstanceInfo): { width: number; height: number } {
		return nodeSurfaceSize(
			inputUnits(Object.keys(node.input_slots ?? {})),
			Object.keys(node.output_slots ?? {}).map((s) => isSlotExpanded(node, s))
		);
	}

	function nodeBoundsFromFlow(n: Node): Bounds {
		const { width, height } = n.measured ?? {};
		if (width != null && height != null) return makeBounds(n.position.x, n.position.y, width, height);
		const fb = nodeFallbackSize(n.data.node as NodeInstanceInfo);
		return makeBounds(n.position.x, n.position.y, width ?? fb.width, height ?? fb.height);
	}

	/** Snap-target bounds for every node on screen in THIS editor, shared by the node drag and the
	 * placement preview. Only `flowNodes` — `g.nodes` would include what this scope does not draw. */
	function snapTargetBounds(exclude: Set<string>): Bounds[] {
		const targets: Bounds[] = [];
		for (const n of flowNodes) {
			if (exclude.has(n.id)) continue;
			targets.push(nodeBoundsFromFlow(n));
		}
		return targets;
	}

	function dragSnapDelta(nodes: Node[], altKey: boolean): { dx: number; dy: number; guides: Guide[] } {
		const draggedBounds = nodes.map(nodeBoundsFromFlow);
		return computeSnapDelta(draggedBounds, snapTargetBounds(new Set(nodes.map((n) => n.id))), altKey);
	}

	// Positions at drag start, so a node snaps back when the drag turns into a panel-link.
	let dragOrigin = new Map<string, { x: number; y: number }>();
	// Where the gesture has each node, from drag start until the document holds the drop: the
	// derivation reads this over the document, or a delta landing mid-drag snaps the node back.
	let pinned = new Map<string, { x: number; y: number }>();
	// The chip that follows the cursor while a drag is a reference; null = a reposition drag.
	let linkGhost = $state<{ x: number; y: number; name: string } | null>(null);

	// Every panel's and drop zone's screen rect, measured once per drag and again on a scroll:
	// nothing else moves them during a node drag, and measuring per pointer move forced a layout each.
	let panelRects: { id: string; type: string; r: DOMRect }[] = [];
	let zoneRects: { zone: string; r: DOMRect }[] = [];
	function measureTargets(): void {
		panelRects = [...document.querySelectorAll<HTMLElement>('[data-panel-id]')].map((el) => ({
			id: el.dataset.panelId ?? '',
			type: el.dataset.panelType ?? '',
			r: el.getBoundingClientRect()
		}));
		zoneRects = [...dropZones()].map((el) => ({ zone: el.dataset.nodeDrop ?? '', r: el.getBoundingClientRect() }));
	}
	const hits = (r: DOMRect, x: number, y: number): boolean => x >= r.left && x < r.right && y >= r.top && y < r.bottom;

	/** The leaf panel under a screen point. Geometric, not `elementFromPoint`: the dragged node sits
	 * under the cursor and would mask the panel beneath it. */
	function panelUnder(x: number, y: number): { id: string; type: string } | null {
		return panelRects.find((p) => hits(p.r, x, y)) ?? null;
	}

	/** Every marked drop zone — a control widget, a param row — including those a plugin panel
	 * keeps behind its shadow root, which `document.querySelectorAll` does not enter. */
	function* dropZones(): Iterable<HTMLElement> {
		yield* document.querySelectorAll<HTMLElement>('[data-node-drop]');
		for (const host of document.querySelectorAll<HTMLElement>('[data-plugin-panel]')) {
			if (host.shadowRoot) yield* host.shadowRoot.querySelectorAll<HTMLElement>('[data-node-drop]');
		}
	}

	/** The marked drop zone under a screen point. */
	function dropZoneUnder(x: number, y: number): string | null {
		return zoneRects.find((z) => hits(z.r, x, y))?.zone ?? null;
	}

	type LinkTarget = { panel: string } | { zone: string };

	/** What a dragged node would link into under the cursor: a drop zone, or a node-accepting panel
	 * other than this editor. */
	function linkTargetAt(event: MouseEvent | TouchEvent): LinkTarget | null {
		const p = eventPoint(event);
		if (!p) return null;
		if (panelRects.length === 0) measureTargets(); // a flick moves before the post-flush measure
		const zone = dropZoneUnder(p.clientX, p.clientY);
		if (zone) return { zone };
		const t = panelUnder(p.clientX, p.clientY);
		return t && t.id !== panelId && getPanelType(t.type)?.acceptsNode === true ? { panel: t.id } : null;
	}

	/** Put the dragged nodes back where the drag started. */
	function revertDragged(nodes: Node[]): void {
		const dragged = new Set(nodes.map((n) => n.id));
		flowNodes = flowNodes.map((n) => {
			if (!dragged.has(n.id)) return n;
			const o = dragOrigin.get(n.id);
			if (o) pinned.set(n.id, { x: o.x, y: o.y });
			return o ? { ...n, position: { x: o.x, y: o.y } } : n;
		});
	}

	/** Snap the dragged nodes to their neighbours, and pin and draw them where the snap puts them. */
	function snapDragged(nodes: Node[], event: MouseEvent | TouchEvent): { dx: number; dy: number; guides: Guide[] } {
		const snap = dragSnapDelta(nodes, (event as MouseEvent).altKey === true);
		const at = new Map(nodes.map((n) => [n.id, { x: n.position.x + snap.dx, y: n.position.y + snap.dy }]));
		for (const [id, p] of at) pinned.set(id, { ...p });
		if (!snap.dx && !snap.dy) return snap;
		flowNodes = flowNodes.map((n) => {
			const p = at.get(n.id);
			return p ? { ...n, position: p } : n;
		});
		return snap;
	}

	/** Release what a node drag holds: its scroll listener, the shared drag fields and the overlays. */
	function endNodeDrag(): void {
		document.removeEventListener('scroll', measureTargets, { capture: true });
		uiStore.nodeDrag = null;
		uiStore.nodeDragOver = null;
		linkGhost = null;
		snapGuides = [];
	}

	function onNodeDragStart(args: { nodes: Node[]; event: MouseEvent | TouchEvent }): void {
		dragOrigin = new Map();
		for (const n of args.nodes) {
			dragOrigin.set(n.id, { x: n.position.x, y: n.position.y });
			pinned.set(n.id, { x: n.position.x, y: n.position.y });
		}
		uiStore.nodeDrag = args.nodes[0]?.id ?? null;
		// The param rows that take this node render on `nodeDrag`, so the rects are read after that flush.
		panelRects = [];
		zoneRects = [];
		void tick().then(measureTargets);
		document.addEventListener('scroll', measureTargets, { capture: true, passive: true });
	}

	function onNodeDrag(args: { nodes: Node[]; event: MouseEvent | TouchEvent }): void {
		const target = linkTargetAt(args.event);
		if (target) {
			// A reference drag, not a coordinate move: the node snaps back and a ghost follows.
			uiStore.nodeDragOver = 'zone' in target ? target.zone : target.panel;
			// `eventPoint`, because a TouchEvent carries no `clientX` of its own.
			const p = eventPoint(args.event) ?? { clientX: 0, clientY: 0 };
			linkGhost = { x: p.clientX, y: p.clientY, name: g.nodeById(args.nodes[0]?.id ?? '')?.name ?? '' };
			snapGuides = [];
			revertDragged(args.nodes);
			return;
		}
		uiStore.nodeDragOver = null;
		linkGhost = null;
		snapGuides = snapDragged(args.nodes, args.event).guides;
	}

	function onNodeDragStop(args: {
		targetNode: Node | null;
		nodes: Node[];
		event: MouseEvent | TouchEvent;
	}): void {
		const target = linkTargetAt(args.event);
		if (target) {
			revertDragged(args.nodes);
			for (const n of args.nodes) pinned.delete(n.id);
			const uid = args.nodes[0]?.id ?? '';
			const p = eventPoint(args.event) ?? { clientX: 0, clientY: 0 };
			const where = 'zone' in target ? target.zone : target.panel;
			// The workspace's own meaning is the fallback, and only a panel has one: bind it to the node.
			if (!uiStore.dropNode(where, uid, { x: p.clientX, y: p.clientY }) && 'panel' in target) {
				ws.linkNodeToPanel(target.panel, uid);
			}
		} else {
			const { dx, dy } = snapDragged(args.nodes, args.event);
			const moves = args.nodes.map(
				(n) => [n.id, [Math.round(n.position.x + dx), Math.round(n.position.y + dy)]] as [string, [number, number]]
			);
			for (const [id, [x, y]] of moves) pinned.set(id, { x, y });
			void g.setNodePositions(moves).finally(() => {
				for (const [id] of moves) pinned.delete(id);
			});
		}
		endNodeDrag();
	}

	const DOUBLE_CLICK_MS = 350;
	type Tap = { at: number; x: number; y: number };
	const tapOf = (e: MouseEvent): Tap => ({ at: performance.now(), x: e.clientX, y: e.clientY });
	/** True when `e` repeats the click `prev` soon enough and within `slop` screen px of it. */
	function repeats(prev: Tap, e: MouseEvent, slop: number): boolean {
		return performance.now() - prev.at < DOUBLE_CLICK_MS && Math.hypot(e.clientX - prev.x, e.clientY - prev.y) < slop;
	}

	let lastPaneClick: Tap = { at: 0, x: 0, y: 0 };
	function onPaneClick(args: { event: MouseEvent }): void {
		if (repeats(lastPaneClick, args.event, 30)) {
			openAddMenu(args.event.clientX - 8, args.event.clientY + 8);
			lastPaneClick.at = 0;
			return;
		}
		lastPaneClick = tapOf(args.event);
		closeMenu();
		sel.clickPane(panelId, args.event.shiftKey);
		// SvelteFlow calls `unselectNodesAndEdges()` immediately AFTER this callback, whatever the
		// store decided, so wherever the store KEEPS the selection it must be re-derived after it.
		if (sel.nodes(panelId).size || sel.edges(panelId).size) void tick().then(reassertSelection);
	}

	/** Push the store's selection back onto the rendered `selected` flags. */
	function reassertSelection(): void {
		flowNodes = flowNodes.map((n) => ({ ...n, selected: sel.nodes(panelId).has(n.id) }));
		flowEdges = flowEdges.map((e) => ({ ...e, selected: sel.edges(panelId).has(e.id) }));
	}

	function onNodeClick(args: { node: Node; event: MouseEvent | TouchEvent }): void {
		// A click can land between a graph mutation and the flowNodes rebuild, carrying an id that
		// no longer exists.
		if (!flowNodes.some((n) => n.id === args.node.id)) return;
		const mouse = args.event as MouseEvent;
		sel.clickNode(panelId, args.node.id, mouse.shiftKey || mouse.ctrlKey || mouse.metaKey);
	}

	/** Mirror a finished marquee into the store. Keyed on start/end, never `onselectionchange`: a
	 * store-driven selection replaces every flowNodes object and Flow then emits transient echoes. */
	function onSelectionEnd(): void {
		if (!boxSelecting) return;
		boxSelecting = false;
		const nodeIds = flowNodes.filter((n) => n.selected).map((n) => n.id);
		sel.setSelection(panelId, nodeIds, flowEdges.filter((e) => e.selected).map((e) => e.id));
	}

	// Double-click to enter a sub-patch, detected here because `onnodeclick` suppresses the 2nd
	// click. In CAPTURE and CONSUMED: the inspector slides over the node the 2nd click must hit.
	const DBL_PX = 6; // a real double-click barely moves the pointer…
	const DBL_PX_TOUCH = 16; // …but a finger does, and 6px is well under any tap slop
	let lastClickInst = '';
	let lastClick: Tap = { at: 0, x: 0, y: 0 };
	/** The node a click landed on, or ''. */
	function nodeUnder(target: EventTarget | null): string {
		return (
			(target as HTMLElement | null)?.closest?.('.svelte-flow__node')?.getAttribute('data-id') ?? ''
		);
	}
	function onCanvasClick(event: MouseEvent): void {
		const hereNode = nodeUnder(event.target);
		// …of which only a sub-patch instance is something this gesture can ENTER.
		const here = isScope(hereNode) ? hereNode : '';
		// Per gesture, not per device.
		const slop = (event as PointerEvent).pointerType === 'touch' ? DBL_PX_TOUCH : DBL_PX;
		if (
			lastClickInst &&
			repeats(lastClick, event, slop) &&
			// A second click resolving to a DIFFERENT NODE is that node's first click. Asked of the
			// NODE, not the instance: '' is reserved for the inspector having slid over it.
			(hereNode === '' || hereNode === lastClickInst)
		) {
			const inst = lastClickInst;
			lastClickInst = '';
			// The gesture owns this click; `preventDefault` also covers a checkbox's activation
			// behaviour, which propagation alone does not stop.
			event.stopPropagation();
			event.preventDefault();
			enterInstance(inst);
			return;
		}
		lastClick = tapOf(event);
		lastClickInst = here;
	}

	const nodeTypes = { goofi: GoofiNode };

	/** Framing for every programmatic fit. */
	const FIT_OPTIONS = { maxZoom: 1, padding: 0.18 } satisfies FitViewOptions;

	/** How far the canvas may be zoomed; the double-tap zoom clamps itself to the same pair. */
	const MIN_ZOOM = 0.05;
	const MAX_ZOOM = 4;

	/** True when the CANVAS owns the keyboard rather than a control. Deliberately NOT a focusable
	 * DESCENDANT of a node: Tab is how a keyboard reaches the next slot pill inside one. */
	function canvasHasKeys(target: HTMLElement | null): boolean {
		if (!target) return false;
		return (
			target === document.body ||
			target.classList.contains('svelte-flow__pane') ||
			target.classList.contains('svelte-flow__node')
		);
	}

	/** Escape's rungs inside the canvas: the menu, then the selection, then the sub-patch. False
	 * when none of them was there to take it. */
	function escapeLadder(): boolean {
		if (menuOpen) closeMenu();
		else if (sel.nodes(panelId).size || sel.edges(panelId).size) sel.clear(panelId);
		else if (enteredPath.length) exitToDepth(enteredPath.length - 1); // step one level up
		else return false;
		return true;
	}

	/** Text selection and controls keep their native clipboard actions. */
	function canvasHasClipboard(target: HTMLElement | null): boolean {
		return isActive() && canvasHasKeys(target) && !window.getSelection()?.toString();
	}

	function onKeydown(e: KeyboardEvent): void {
		if (e.defaultPrevented || !isActive()) return;
		const t = e.target as HTMLElement | null;
		// The DOM says a modal owns the keyboard, NOT `ui().modalOpen` — that ref-count is also
		// raised by a merely expanded in-panel textarea.
		if (t?.closest?.('dialog[open]')) return;
		if (isTextEditingTarget(t)) return;

		const meta = e.ctrlKey || e.metaKey;
		if (meta && e.key.toLowerCase() === 'a') {
			e.preventDefault();
			selectAll();
		} else if (meta && e.key.toLowerCase() === 'c') {
			if (!canvasHasClipboard(t) || selectedUids().length === 0) return;
			// Stop the browser's own copy: it would put the (empty) DOM selection on the clipboard
			// over the payload. Ctrl+V is deliberately NOT here — the `paste` event is that door.
			e.preventDefault();
			void copySelection();
		} else if (meta && e.key.toLowerCase() === 'x') {
			if (!canvasHasClipboard(t) || selectedUids().length === 0) return;
			e.preventDefault();
			void cutSelection();
		} else if (meta && e.key.toLowerCase() === 'd') {
			e.preventDefault();
			void duplicateSelection();
		} else if (meta && e.key.toLowerCase() === 'g') {
			e.preventDefault();
			void groupSelection();
		} else if (e.key === 'Tab' && !e.shiftKey && canvasHasKeys(t)) {
			// Scoped to the bare canvas, or nothing outside it is ever Tab-reachable (WCAG 2.1.2).
			// Shift+Tab is left alone: it is the way back OUT of a canvas nothing has focused yet.
			e.preventDefault();
			openAddMenu(mouseX, mouseY);
		} else if (e.key === 'Escape' && escapeLadder()) {
			// A consumed Escape says so; unconsumed, it reaches the shell, which ends a panel maximize.
			e.preventDefault();
		} else if (e.key.toLowerCase() === 'f') {
			fitView();
		}
	}

	/** The single delete path, for SvelteFlow's `ondelete` and the header's Delete row. Nodes go as
	 * ONE batch, ports included, so undo restores them all BEFORE their links. */
	async function deleteElements({ nodes, edges }: { nodes: Node[]; edges: Edge[] }): Promise<void> {
		const nodeIds = nodes.map((n) => n.id);
		const deleted = new Set(nodeIds);
		const links = edges.filter((e) => !deleted.has(e.source) && !deleted.has(e.target));
		// One undo step for the whole selection; a link touching a deleted node went with it.
		await history()
			.transaction('Delete selection', async (step) => {
				if (nodeIds.length) await g.removeNodes(nodeIds, step);
				for (const link of links.map(linkOf)) if (link) await g.removeLink(link, step);
			})
			.catch((err) => notify().failure('Delete', err));
		sel.clear(panelId);
	}

	/** The rendered `selected` flags: the store's selection unioned with a live marquee. */
	function selectedElements(): { nodes: Node[]; edges: Edge[] } {
		return { nodes: flowNodes.filter((n) => n.selected), edges: flowEdges.filter((e) => e.selected) };
	}

	function hasSelection(): boolean {
		const { nodes, edges } = selectedElements();
		return nodes.length > 0 || edges.length > 0;
	}

	function deleteSelection(): void {
		if (hasSelection()) void deleteElements(selectedElements());
	}

	/** Select what is on screen: the entered scope's members, group nodes included. */
	function selectAll(): void {
		sel.selectNodes(panelId, childrenOfScope(entered ?? ROOT_ID, memberIndex));
	}

	/** Put the selection on the clipboard and answer what was put there. The manager reads the
	 * SUBTREE, so a sub-patch's members, ports and nested scopes come with it. */
	async function copySelection(): Promise<string[]> {
		const uids = selectedUids();
		if (uids.length === 0) return [];
		if (!(await copyText(JSON.stringify(serializeClipboard(await g.copyNodes(uids)))))) {
			notify().failure('Copy', 'the clipboard refused the payload');
			return [];
		}
		return uids;
	}

	/** Cut: the copy, then the delete as ONE history entry. The delete waits on the copy, or a
	 * failed write would take the nodes with it. */
	async function cutSelection(): Promise<void> {
		const uids = await copySelection();
		if (uids.length === 0) return;
		await history()
			.transaction('Cut nodes', (step) => g.removeNodes(uids, step))
			.catch((e) => notify().failure('Cut', e));
		sel.clear(panelId);
	}

	async function duplicateSelection(): Promise<void> {
		const rename = await history().transaction('Duplicate nodes', (step) =>
			g.cloneNodes(selectedUids(), [40, 40], entered ?? undefined, step)
		);
		const created = Object.values(rename);
		if (created.length > 0) sel.selectNodes(panelId, created);
	}


	/** The menu's paste. `navigator.clipboard.readText` needs a secure context, which plain http on
	 * a LAN is not, so a refusal points to the keyboard's `paste` event. */
	async function pasteClipboard(): Promise<void> {
		try {
			await pasteText(await navigator.clipboard.readText());
		} catch {
			notify().failure('Paste', 'this browser only pastes with the keyboard here — press Ctrl+V');
		}
	}

	async function pasteText(text: string): Promise<void> {
		const clip = parseClipboard(text);
		if (!clip) return;
		// Anchor the paste at the visible viewport centre, in FLOW space. The fragment carries the
		// positions it was copied at, so what goes to the manager is the SHIFT between the two.
		const rect = rootEl?.getBoundingClientRect();
		let at: [number, number] = [window.innerWidth / 4, window.innerHeight / 4];
		if (rect && screenToFlow) {
			const c = screenToFlow({ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 });
			at = [c.x, c.y];
		}
		const from = fragmentCentre(clip.doc);
		const rename = await history().transaction('Paste nodes', (step) =>
			g.pasteNodes(clip.doc, [Math.round(at[0] - from[0]), Math.round(at[1] - from[1])], entered ?? undefined, step)
		);
		const created = Object.values(rename);
		if (created.length > 0) sel.selectNodes(panelId, created);
	}

	async function autoLink(
		seed: SlotClickSeed,
		picked: NodeTypeInfo,
		newName: string,
		step: Step
	): Promise<void> {
		const matchedSlot = seedSlot(seed, picked);
		if (!matchedSlot) return;
		// Inputs take a single source, so an existing cable is replaced; outputs fan out.
		if (seed.side === 'target') {
			const existing = g.links.filter((l) => l.node_in === seed.node && l.slot_in === seed.slot);
			for (const l of existing) await g.removeLink(l, step).catch(() => {});
		}
		const link =
			seed.side === 'source'
				? { node_out: seed.node, slot_out: seed.slot, node_in: newName, slot_in: matchedSlot }
				: { node_out: newName, slot_out: matchedSlot, node_in: seed.node, slot_in: seed.slot };
		try {
			await g.addLink(link, step);
		} catch (e) {
			console.warn('auto-link failed', e);
		}
	}

	async function commitPlacement(pos: [number, number]): Promise<void> {
		const placement = pendingPlacement;
		if (!placement) return;
		pendingPlacement = null;
		ghostPos = null;
		// A boundary type takes this same path: `add_node` with `inst_id` is what makes a PORT of the
		// entered sub-patch, and the catalog gives it the slots `autoLink` matches against.
		const label = placement.seed
			? `Add ${bareName(placement.typeInfo.type)} + connect`
			: `Add ${bareName(placement.typeInfo.type)}`;
		await history().transaction(label, async (step) => {
			try {
				const newName = await g.addNode(placement.typeInfo.type, pos, entered ?? undefined, step);
				// Safe before `node_added` lands: flowNodes derives `selected` from this set.
				if (newName) sel.selectNodes(panelId, [newName]);
				if (placement.seed && newName) await autoLink(placement.seed, placement.typeInfo, newName, step);
			} catch (e) {
				console.warn('node add failed', e);
			}
		});
	}

	let mouseX = 0;
	let mouseY = 0;
	function trackMouse(e: MouseEvent): void {
		mouseX = e.clientX;
		mouseY = e.clientY;
	}

	// The plot surface the node cards' viewers draw on; bound from <FlowSurface> inside <SvelteFlow>.
	let plotSurface = $state.raw<SurfaceHandle | null>(null);
	provideSurface({
		get surface() {
			return plotSurface;
		}
	});

	// Bound from <FlowApi> inside <SvelteFlow>.
	let screenToFlow = $state<((p: { x: number; y: number }) => { x: number; y: number }) | undefined>(
		undefined
	);
	let getViewport = $state<(() => Viewport) | undefined>(undefined);
	let setViewport = $state<((v: Viewport) => void) | undefined>(undefined);
	let flowFit = $state<((o?: FitViewOptions) => Promise<boolean>) | undefined>(undefined);

	// A layout reshape or a page switch DESTROYS this component, so the camera outliving it is what
	// carries the framing across.
	const cam = camera(untrack(() => panelId));
	let viewport = $state<Viewport>(cam.viewport ?? { x: 0, y: 0, zoom: 0.85 });

	// Fit the whole graph after a wholesale load, never on an interactive add.
	$effect(() => {
		const epoch = g.loadEpoch;
		if (!flowFit || epoch === cam.fittedEpoch || !g.loadSettled) return;
		cam.fittedEpoch = epoch;
		// An empty load must not arm a fit that the first placed node would then satisfy.
		if (g.nodes.length > 0) void flowFit(FIT_OPTIONS);
	});

	/** The Controls button's own fit, which takes Flow's default framing. */
	function fitView(): void {
		void flowFit?.();
	}

	/** Select a node in this editor — the shared handle for focusing one from elsewhere. */
	function focusNode(uid: string): void {
		sel.selectNodes(panelId, [uid]);
	}

	/** The platform's paste, which carries its text and so needs no permission or secure context.
	 * Guarded as the key handler is: a text field or an inactive panel keeps its own paste. */
	function onPaste(e: ClipboardEvent): void {
		const t = e.target as HTMLElement | null;
		if (e.defaultPrevented || !canvasHasClipboard(t) || t?.closest?.('dialog[open]')) return;
		const text = e.clipboardData?.getData('text') ?? '';
		if (!parseClipboard(text)) return; // not ours: leave it for whatever else is listening
		e.preventDefault();
		void pasteText(text);
	}

	onMount(() => {
		registerEditor(panelId, {
			focusNode,
			selectAll,
			clearSelection: () => sel.clear(panelId),
			deleteSelection,
			groupSelection: () => void groupSelection(),
			copySelection: () => void copySelection(),
			cutSelection: () => void cutSelection(),
			pasteClipboard: () => void pasteClipboard(),
			duplicateSelection: () => void duplicateSelection(),
			hasSelection
		});
		const root = rootEl as HTMLDivElement;
		const offs = [
			bindZoom(root),
			// `document`, not `window`: the shell listens on window, so the canvas's Escape goes first.
			on(document, 'keydown', onKeydown),
			on(window, 'paste', onPaste),
			on(window, 'mousemove', trackMouse),
			on(root, 'click', onCanvasClick, { capture: true }),
			on(root, 'pointerdown', onCanvasPointerDown),
			on(root, 'pointermove', canvasPress.move),
			on(root, 'pointerup', canvasPress.cancel),
			on(root, 'pointercancel', canvasPress.cancel)
		];
		return () => {
			offs.forEach((off) => off());
			unregisterEditor(panelId);
			canvasPress.cancel(); // a press in flight must not fire into an unmounted editor
			onCableEnd(); // …nor may a cable in flight leave name tags lit on a torn-down canvas
			// Do NOT forget this panel's selection here: unmount also fires on a tab switch, and the
			// selection must survive switching away and back.
			endNodeDrag(); // a drag in flight must not leave drop outlines lit on the other panels
		};
	});
</script>

<SvelteFlowProvider>
	<!-- `canvas-wrap` is the marker PlacementPreview uses to tell a commit click from a cancel. -->
	<div class="editor-panel canvas-wrap" bind:this={rootEl}>
		{#if enteredPath.length > 0}
			<nav class="breadcrumb" data-testid="subpatch-breadcrumb" aria-label="Sub-patch path">
				<Button variant="ghost" size="sm" onclick={() => exitToDepth(0)} title="Back to the top-level patch"
					>Patch</Button
				>
				{#each enteredPath as inst, i (inst)}
					{@const label = g.nodeById(inst)?.name ?? inst}
					<span class="sep">›</span>
					<Button
						variant="ghost"
						size="sm"
						class={i === enteredPath.length - 1 ? 'crumb-current' : ''}
						onclick={() => exitToDepth(i + 1)}
						title="Go to {label}">{label}</Button
					>
				{/each}
			</nav>
		{/if}
		<SvelteFlow
			bind:nodes={flowNodes}
			bind:edges={flowEdges}
			{nodeTypes}
			deleteKey={['Delete', 'Backspace']}
			onconnect={onConnect}
			onreconnect={onReconnect}
			onconnectstart={onCableStart}
			onconnectend={onCableEnd}
			onreconnectstart={onCableStart}
			onreconnectend={onCableEnd}
			onnodedragstart={onNodeDragStart}
			onnodedrag={onNodeDrag}
			onnodedragstop={onNodeDragStop}
			onpaneclick={onPaneClick}
			onnodeclick={onNodeClick}
			onselectionstart={() => (boxSelecting = true)}
			onselectionend={onSelectionEnd}
			onedgeclick={onEdgeClick}
			ondelete={deleteElements}
			fitViewOptions={FIT_OPTIONS}
			minZoom={MIN_ZOOM}
			maxZoom={MAX_ZOOM}
			bind:viewport={() => viewport, (v) => (viewport = cam.viewport = v)}
			zoomOnDoubleClick={false}
			autoPanOnNodeDrag={false}
		>
			<!-- `showLock` off: goofi has no read-only mode, so Flow's lock reads as breakage. -->
			<Controls showLock={false} />
			<FlowApi bind:screenToFlowPosition={screenToFlow} bind:getViewport bind:setViewport bind:fitView={flowFit} />
			<FlowSurface bind:surface={plotSurface} />
			<SubpatchZoomExit {entered} options={FIT_OPTIONS} minZoom={MIN_ZOOM} onExit={() => exitToDepth(enteredPath.length - 1)} />
			{#if pendingPlacement && ghost}
				<PlacementPreview
					initialClient={pendingPlacement.initialClient}
					size={nodeFallbackSize(ghost)}
					targets={snapTargetBounds(new Set([GHOST_UID]))}
					onMove={(pos) => (ghostPos = pos)}
					onCommit={(pos) => void commitPlacement(pos)}
					onCancel={() => {
						pendingPlacement = null;
						ghostPos = null;
					}}
				/>
			{/if}
			<ViewportPortal target="back">
				<ReferenceEdges nodes={flowNodes} selected={selectedNode?.uid ?? null} />
			</ViewportPortal>
			{#if snapGuides.length > 0}
				<ViewportPortal target="front">
					<SnapGuides guides={snapGuides} testid="snap-guides" />
				</ViewportPortal>
			{/if}
		</SvelteFlow>

		{#if flowNodes.length === 0 && !pendingPlacement && !menuOpen}
			<div class="empty-hint" data-testid="empty-hint">
				<EmptyState>
					{#snippet title()}{entered ? 'This sub-patch is empty' : 'Empty patch'}{/snippet}
					{#snippet hint()}Double-click the canvas or press <kbd>Tab</kbd> to add a node.{/snippet}
				</EmptyState>
			</div>
		{/if}

		<!-- Fixed and positioned in VIEWPORT coordinates, so both portal to <body>: `.panel-body`
		     is a query container and must never become their containing block. -->
		{#if menuOpen}
			<div class="menu-overlay" use:portal onclick={closeMenu} role="presentation"></div>
			<div
				class="menu-anchor"
				bind:this={menuEl}
				use:portal
				data-testid="add-node-menu-anchor"
				style="left: {menuPos.x}px; top: {menuPos.y}px"
			>
				<AddNodeMenu
					seed={menuSeed}
					boundary={entered !== null}
					onPick={(typeInfo) => {
						ghostPos = null;
						pendingPlacement = { typeInfo, seed: menuSeed, initialClient: { x: mouseX, y: mouseY } };
						closeMenu();
					}}
					onClose={closeMenu}
				/>
			</div>
		{/if}

		<!-- Its ✕ DISMISSES, holding only until the selection changes; its ◧ is the switch. -->
		<SidePane
			subject={selectedNode}
			enabled={inspectorOn}
			onClose={() => sel.dismissInspectorFor(panelId)}
			onToggle={() => sel.setInspector(panelId, !inspectorOn)}
		>
			{#snippet children(node)}
				<Inspector {node} onClose={() => sel.dismissInspectorFor(panelId)} />
			{/snippet}
		</SidePane>
	</div>
</SvelteFlowProvider>

{#if linkGhost}
	<div class="link-ghost" use:portal style="left: {linkGhost.x}px; top: {linkGhost.y}px">
		<span class="lg-icon">🔗</span>{linkGhost.name}
	</div>
{/if}

<style>
	.editor-panel {
		position: relative;
		width: 100%;
		height: 100%;
		min-width: 0;
		min-height: 0;
	}
	/* The inset keeps the 16px corner grips reachable. `margin: 0` is load-bearing: Flow's own
	   `.svelte-flow__panel` sets `margin: 15px`, which STACKS on these offsets. */
	.editor-panel :global(.svelte-flow__controls) {
		margin: 0;
		bottom: var(--space-8);
		left: var(--space-8);
	}
	/* On touch each button is floored to --hit in both axes, so the cluster is a slab and needs a
	   smaller inset — still clear of the grip, whose 16px box is clipped to a triangle. */
	@media (hover: none) and (pointer: coarse) {
		.editor-panel :global(.svelte-flow__controls) {
			bottom: var(--space-6);
			left: var(--space-6);
		}
	}
	/* Non-interactive, so it never eats the double-click that opens the add-node menu under it. */
	.empty-hint {
		position: absolute;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		pointer-events: none;
		z-index: 1;
	}
	.empty-hint kbd {
		font-size: var(--fs-small);
		padding: 1px var(--space-3);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--surface-1);
	}
	.link-ghost {
		position: fixed;
		/* Offset off the cursor so it reads as carried, not pinned. */
		transform: translate(14px, 12px);
		z-index: var(--z-tab-drag);
		pointer-events: none;
		display: flex;
		align-items: center;
		gap: var(--space-3);
		padding: var(--space-2) var(--space-5);
		max-width: 220px;
		background: var(--surface-2);
		border: 1px solid var(--accent);
		border-radius: var(--radius-sm);
		box-shadow: var(--shadow-2);
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		color: var(--text);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.lg-icon {
		font-size: var(--fs-small);
	}
	.menu-overlay {
		position: fixed;
		inset: 0;
		z-index: calc(var(--z-addmenu) - 1);
	}
	.menu-anchor {
		position: fixed;
		z-index: var(--z-addmenu);
		/* The clamp can only SHIFT a surface that fits, so on a narrow phone the width gives first. */
		width: min(440px, calc(100vw - var(--space-8)));
	}
	.breadcrumb {
		position: absolute;
		top: 10px;
		left: 10px;
		z-index: 6;
		display: flex;
		align-items: center;
		gap: var(--space-2);
		padding: var(--space-2) var(--space-5);
		max-width: calc(100% - 90px);
		overflow: hidden;
		background: color-mix(in srgb, var(--surface-1) 88%, transparent);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		box-shadow: var(--shadow-1);
		font-family: var(--font-mono);
		font-size: var(--fs-small);
	}
	.breadcrumb :global(.crumb-current) {
		font-weight: 600;
		color: var(--text);
	}
	.breadcrumb .sep {
		color: var(--text-muted);
	}
	/* The placement ghost is drawn, never touched: the pointer under it is the placement's. */
	:global(.svelte-flow__node.ghost) {
		pointer-events: none;
	}
</style>
