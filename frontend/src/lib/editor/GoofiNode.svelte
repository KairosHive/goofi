<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { dtypeColor } from './categoryColor';
	import { engineColor } from './typeId';
	import SlotViewer from '$lib/viewers/SlotViewer.svelte';
	import { isSlotExpanded } from '$lib/viewers/inlineView';
	import { ui } from '$lib/stores/ui.svelte';
	import { flash } from '$lib/stores/flash.svelte';
	import { inputPorts, inputUnits, outputTops } from './nodeMetrics';
	import { ContextMenu, createLongPress, type MenuItem } from 'panelty';
	import { selection } from '$lib/stores/selection.svelte';
	import { graph, slotReference } from '$lib/stores/graph.svelte';
	import { nodeHealth } from './nodeHealth';
	import { StatusDot } from '$lib/ui';
	import { formatUpdateRate } from './nodeStats';
	import type { NodeInstanceInfo } from '$lib/api/control';
	import { provideAnchor } from '$lib/viewers/plotHost';

	let { data, selected, positionAbsoluteX, positionAbsoluteY, zIndex }: NodeProps = $props();
	// Where this card sits in flow units, for the viewers that draw on the editor's plot surface.
	let card = $state<HTMLElement | null>(null);
	provideAnchor({
		get x() {
			return positionAbsoluteX;
		},
		get y() {
			return positionAbsoluteY;
		},
		get z() {
			return zIndex;
		},
		get el() {
			return card;
		}
	});
	const node = $derived(data.node as NodeInstanceInfo);
	const label = $derived((data.label as string | undefined) ?? node?.name);
	const inputs = $derived(Object.keys(node?.input_slots ?? {}));
	const outputs = $derived(Object.keys(node?.output_slots ?? {}));
	const uiStore = ui();
	// Reached HERE, never inside a derived: a lazy singleton first called in a tracking scope
	// creates its `$state` there, where an outside `pulse()` can never re-run it.
	const flashStore = flash();

	function onInputClick(e: MouseEvent, slot: string, dtype: string): void {
		e.stopPropagation();
		uiStore.pendingSlotClick = { node: node.uid, slot, dtype, side: 'target', clientX: e.clientX, clientY: e.clientY };
	}

	let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function slotMenu(x: number, y: number, slot: string): void {
		menu = { x, y, items: selection().referenceItems({ node: node.uid, slot }, slotReference(node, slot)) };
	}
	// The touch door onto the right-click menu; the click its release fires is the menu's, not the add menu's.
	let pressSlot = '';
	let held = false;
	const press = createLongPress((at) => {
		held = true;
		slotMenu(at.clientX, at.clientY, pressSlot);
	});

	function onOutputClick(e: MouseEvent, slot: string, dtype: string): void {
		e.stopPropagation();
		if (held) {
			held = false;
			return;
		}
		uiStore.pendingSlotClick = { node: node.uid, slot, dtype, side: 'source', clientX: e.clientX, clientY: e.clientY };
	}

	const health = $derived(nodeHealth(node));
	const isError = $derived(health.kind === 'error' || health.kind === 'dead');
	const isBooting = $derived(health.kind === 'booting');
	const flashing = $derived(flashStore.active(node?.uid));
	const rateLabel = $derived(formatUpdateRate(node?.stats));

	const multiInputs = $derived(new Set(node?.input_multi ?? []));
	const inPorts = $derived(
		inputPorts(inputs).map((p) => ({ ...p, dtype: node.input_slots[p.slot], multi: multiInputs.has(p.slot) }))
	);
	const minBody = $derived(inputUnits(inputs));

	// The overlay is unclipped, so it walks the slot stack itself to place each output pill.
	const outPorts = $derived(
		outputTops(outputs, (slot) => isSlotExpanded(node, slot)).map((p) => ({ ...p, dtype: node.output_slots[p.slot] }))
	);

	// The control elements this node's params read: light and always shown, as a cable would be.
	const g = graph();
	const driving = $derived.by(() => {
		const names = new Set<string>();
		for (const group of Object.values(node?.params ?? {}))
			for (const d of Object.values(group))
				for (const m of (d.mode === 'expression' && d.expression?.matchAll(/variables\.(\w+\.\w+)/g)) || [])
					if (g.variables.some((v) => v.name === m[1] && v.control)) names.add(m[1]);
		return [...names];
	});
</script>

<div
	class="goofi-node"
	bind:this={card}
	class:selected
	class:has-error={isError}
	class:booting={isBooting}
	class:undo-flash={flashing}
	class:ghost={data.ghost}
	style="min-height: calc(var(--node-header) + {minBody} * var(--node-u)); --engine: {engineColor(node?.type ?? '')};"
	data-testid={data.ghost ? 'placement-ghost' : node?.subpatch ? 'subpatch-node' : undefined}
>
	<!-- Clipped to the rounded node shape, so nothing inside it needs to round itself. -->
	<div class="surface">
		<div class="header">
			<!-- The card is sized in fixed px, so the dot is told its diameter. -->
			<StatusDot
				tone={health.tone}
				pulse={health.kind === 'dead'}
				title={health.title}
				style="--status-dot-size: 8px"
				data-testid="node-health"
			/>
			<span class="name">{label}</span>
			{#if isBooting}
				<span class="boot-label" data-testid="boot-label">{health.label}</span>
			{:else if rateLabel}
				<span class="rate" title="update rate">{rateLabel}</span>
			{/if}
		</div>

		{#if outputs.length > 0}
			<div class="viewers">
				{#each outputs as slot (slot)}
					<SlotViewer {node} {slot} dtype={node.output_slots[slot]} label={node.slot_labels?.[slot]} />
				{/each}
			</div>
		{/if}
	</div>

	{#if driving.length}
		<div class="driving" data-testid="node-control-chips">
			{#each driving as name (name)}<span class="chip" title="A param reads variables.{name}">{name}</span>{/each}
		</div>
	{/if}

	<!-- Connector overlay: outside the clip, so the pills can overhang the edges. -->
	<div class="ports">
		{#each inPorts as port (port.slot)}
			<!-- svelte-ignore a11y_click_events_have_key_events -->
			<div
				class="conn in"
				class:multi={port.multi}
				class:cable-near={uiStore.isCableNear(node.uid, port.slot)}
				style="top: {port.top}px; --dtype: {dtypeColor(port.dtype)};"
				onclick={(e) => onInputClick(e, port.slot, port.dtype)}
				role="button"
				tabindex="0"
				data-testid="slot-input"
				data-multi={port.multi ? 'true' : undefined}
				title={port.multi ? `${port.dtype.toLowerCase()} · list (multi-input)` : port.dtype.toLowerCase()}
			>
				<Handle id={port.slot} type="target" position={Position.Left} />
				<span class="conn-label">{node.slot_labels?.[port.slot] ?? port.slot}</span>
			</div>
		{/each}

		{#each outPorts as port (port.slot)}
			<!-- svelte-ignore a11y_click_events_have_key_events -->
			<div
				class="conn out"
				style="top: {port.top}px; --dtype: {dtypeColor(port.dtype)};"
				onclick={(e) => onOutputClick(e, port.slot, port.dtype)}
				oncontextmenu={(e) => {
					e.preventDefault();
					e.stopPropagation();
					slotMenu(e.clientX, e.clientY, port.slot);
				}}
				onpointerdown={(e) => {
					held = false;
					if (e.pointerType === 'mouse') return;
					pressSlot = port.slot;
					press.start(e);
				}}
				onpointermove={press.move}
				onpointerup={press.cancel}
				onpointercancel={press.cancel}
				role="button"
				tabindex="0"
				data-testid="slot-output-pin"
				title={port.dtype.toLowerCase()}
			>
				<span class="out-label" data-testid="slot-output">{node.slot_labels?.[port.slot] ?? port.slot}</span>
				<Handle id={port.slot} type="source" position={Position.Right} />
			</div>
		{/each}
	</div>
</div>

{#if menu}
	<ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => (menu = null)} />
{/if}

<style>
	.driving {
		position: absolute;
		bottom: 100%;
		left: 6px;
		display: flex;
		gap: 3px;
		margin-bottom: 3px;
		pointer-events: none;
	}
	.chip {
		font-size: 9px;
		line-height: 1;
		padding: 2px 5px;
		border-radius: 3px;
		border: 1px dashed var(--border);
		color: var(--text-muted);
		background: var(--surface-1);
	}
	.goofi-node {
		position: relative;
		display: flex;
		flex-direction: column;
		width: var(--node-w);
		color: var(--text);
		font-family: var(--font-mono);
	}
	.surface {
		flex: 1 1 auto;
		overflow: hidden;
		display: flex;
		flex-direction: column;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		transition:
			border-color var(--dur-fast) var(--ease),
			box-shadow var(--dur-fast) var(--ease);
	}
	/* Header and slots paint their own backgrounds, and a plot body stays transparent for the
	   surface beneath; only the room below the last slot needs a fill. */
	.surface::after {
		content: '';
		flex: 1 1 auto;
		background: var(--surface-1);
	}
	.goofi-node.selected .surface {
		border-color: var(--accent);
		box-shadow: var(--shadow-2);
	}
	.goofi-node.has-error .surface {
		border-color: var(--danger);
	}
	/* A placement's ghost: the same card, dashed and translucent until it is born. */
	.goofi-node.ghost {
		opacity: 0.9;
	}
	.goofi-node.ghost .surface {
		border: 1.5px dashed var(--accent);
		box-shadow: var(--shadow-1);
	}
	/* A finger has no cursor to say "not placed yet", so a coarse pointer states it in the drawing. */
	@media (hover: none) and (pointer: coarse) {
		.goofi-node.ghost {
			opacity: 0.6;
		}
		.goofi-node.ghost .surface {
			border-width: 2px;
			box-shadow: none;
		}
	}
	.goofi-node.booting .surface {
		opacity: 0.75;
	}
	.boot-label {
		flex: 0 0 auto;
		font-size: 9px;
		color: var(--text-muted);
		font-style: italic;
	}
	.goofi-node.undo-flash .surface {
		animation: undo-flash 0.7s ease-out;
	}
	@keyframes undo-flash {
		0% {
			box-shadow: 0 0 0 0 color-mix(in srgb, var(--accent) 80%, transparent);
		}
		100% {
			box-shadow: 0 0 0 10px color-mix(in srgb, var(--accent) 0%, transparent);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.goofi-node.undo-flash .surface {
			animation: none;
		}
	}
	.header {
		flex: 0 0 auto;
		height: var(--node-header);
		box-sizing: border-box;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 0 10px;
		background: color-mix(in srgb, var(--engine) 18%, var(--surface-2));
		border-bottom: 1px solid var(--border);
		cursor: pointer;
		user-select: none;
	}
	.name {
		font-weight: 600;
		font-size: 12px;
		color: var(--text);
		line-height: normal;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		flex: 1 1 auto;
		min-width: 0;
	}
	.rate {
		flex: 0 0 auto;
		font-size: 9px;
		color: var(--text-muted);
		font-variant-numeric: tabular-nums;
		opacity: 0.3;
		transition: opacity var(--dur-slow) var(--ease);
	}
	.goofi-node:hover .rate,
	.goofi-node.selected .rate {
		opacity: 0.85;
	}
	/* No hover to resolve the faint resting state, so the rate rests where hover would leave it. */
	@media (hover: none) and (pointer: coarse) {
		.rate {
			opacity: 0.85;
		}
	}
	.viewers {
		display: flex;
		flex-direction: column;
	}

	/* These connectors sit BELOW `--hit` deliberately: they tile the left edge at `--node-u`, so a
	   44px target would cover its neighbours and make the WRONG slot answer a tap. */
	.ports {
		position: absolute;
		inset: 0;
		pointer-events: none;
	}
	.conn {
		position: absolute;
		display: grid;
		place-items: center;
		height: var(--node-u);
		pointer-events: auto;
		cursor: pointer;
	}
	/* The box is the hit area, and the handle is the box. Svelte Flow anchors a cable on the
	   handle's outer EDGE, so each box ends 4px past the node border and the pill straddles that
	   border from there: the cable lands under the pill. One slot tall, so neighbours never overlap. */
	.conn :global(.svelte-flow__handle) {
		inset: 0;
		width: 100%;
		height: 100%;
		min-width: 0;
		min-height: 0;
		transform: none;
		background: transparent;
		border: 0;
		border-radius: 0;
	}
	.conn.in {
		left: -4px;
		width: 22px;
		transform: translateY(-50%);
	}
	.conn.in :global(.svelte-flow__handle)::before {
		left: 4px;
		transform: translate(-50%, -50%);
	}
	.conn.in.multi :global(.svelte-flow__handle)::before {
		background: transparent;
		border: 2px solid var(--dtype, var(--border-strong));
	}
	/* An output's box runs from its label to 4px past the border, the pill's zone 22px wide. */
	.conn.out {
		right: -4px;
		padding: 0 22px 0 4px;
		transform: translateY(-50%);
		font-family: var(--font-mono);
		font-size: 10px;
	}
	.conn.out :global(.svelte-flow__handle)::before {
		right: 4px;
		transform: translate(50%, -50%);
	}
	.out-label {
		color: var(--dtype, var(--text-dim));
		border-radius: 3px;
		padding: 0 2px;
		pointer-events: none;
		transition: background var(--dur-fast) var(--ease);
	}
	@media (hover: hover) {
		.conn.out:hover .out-label {
			background: color-mix(in srgb, var(--dtype, var(--accent)) 22%, transparent);
		}
	}
	.conn-label {
		position: absolute;
		right: calc(100% + 6px);
		top: 50%;
		transform: translateY(-50%);
		font-size: 9px;
		line-height: 1;
		color: var(--text-dim);
		background: var(--surface-2);
		border: 1px solid var(--border);
		border-radius: 3px;
		padding: 2px 5px;
		white-space: nowrap;
		pointer-events: none;
		opacity: 0;
		transition: opacity var(--dur-fast) var(--ease);
	}
	.conn.in:hover .conn-label,
	.conn.in:focus-visible .conn-label,
	/* The touch door: while a cable is in flight, the inputs it nears name themselves. */
	.conn.in.cable-near .conn-label {
		opacity: 1;
	}
</style>
