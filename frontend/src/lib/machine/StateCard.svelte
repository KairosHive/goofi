<!-- A state's box on the machine canvas: its name, and one line that sums up how it decides. Its
     settings are the inspector's. Its border band is the one connection handle, so a drag from the
     edge draws a transition and a drag from the name moves the box. -->
<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { ContextMenu, createLongPress, type MenuItem } from 'panelty';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { DECISIONS, outgoing } from './model';
	import { CARD_H, CARD_W } from './layout';

	let { id, data }: NodeProps = $props();
	const g = graph();
	const machine = $derived(data.machine as string);
	const m = $derived(g.machines[machine]);
	const s = $derived(m?.states[id]);
	const out = $derived(m ? outgoing(m, id).length : 0);
	const mode = $derived(DECISIONS.find((d) => d.id === s?.mode)?.label ?? '');
	const set = $derived(Object.keys(s?.values ?? {}));
	const beat = $derived(s ? (s.interval_expression ? 'ƒ' : s.interval < 0 ? 'no beat' : `${s.interval.toFixed(2)}s`) : '');

	let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function openMenu(x: number, y: number): void {
		menu = {
			x,
			y,
			items: [
				{
					label: 'Delete state',
					icon: 'x',
					action: () => void g.machine('machine state remove', { machine, name: id }).catch((e) => notify().failure('Remove state', e))
				}
			]
		};
	}
	// The touch door onto the menu; the click its release fires must not then select the box.
	const press = createLongPress((at) => openMenu(at.clientX, at.clientY));
</script>

<div class="card" style={`width: ${CARD_W}px; height: ${CARD_H}px`} data-testid={`state-card-${id}`} data-state={id} data-mode={s?.mode}>
	<Handle type="source" position={Position.Top} class="port" />
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div
		class="body"
		oncontextmenu={(e) => {
			e.preventDefault();
			openMenu(e.clientX, e.clientY);
		}}
		onpointerdown={(e) => {
			if (e.pointerType !== 'mouse') press.start(e);
		}}
		onpointermove={press.move}
		onpointerup={press.cancel}
		onpointercancel={press.cancel}
	>
		<div class="main">
			<div class="title">
				<span class="name" data-testid="state-card-name">{id}</span>
			</div>
			<!-- What the state sets, by name, in the room under the name; the rest is an ellipsis. -->
			{#if set.length > 0}
				<div class="attrs" data-testid="state-card-attrs" title={set.join(', ')}>
					{#each set as a (a)}<span class="attr">{a}</span>{/each}
				</div>
			{/if}
		</div>
		{#if s}
			<!-- The facts, stacked at the end: how it decides and how often, what it sets, the ways out.
			     Shown under the pointer and on the selected box. -->
			<div class="facts" data-testid="state-card-summary" title="How it decides, every interval; what it sets; the transitions out">
				<span>{mode}</span>
				<span>{beat}</span>
				<span class="muted">{set.length} set</span>
				<span class="muted">{out} out</span>
			</div>
		{/if}
	</div>
</div>

{#if menu}
	<ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => (menu = null)} />
{/if}

<style>
	.card {
		position: relative;
		box-sizing: border-box;
		background: var(--surface-1);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-md);
		font-size: var(--fs-small);
	}
	/* The handle IS the card, under the body: what shows of it is the band around the body. */
	.card :global(.port) {
		position: absolute;
		inset: 0;
		width: auto;
		height: auto;
		min-width: 0;
		min-height: 0;
		transform: none;
		border: none;
		border-radius: var(--radius-md);
		background: transparent;
		cursor: crosshair;
	}
	/* ONE ring: the border turns accent and a hairline joins it outside, following the same radius,
	   so no grey shows through at the corners. It lights under the pointer, on the selected box, and
	   while a cable in flight has this box as its target. */
	:global(.svelte-flow__node.selected) .card,
	.card:hover,
	.card:has(> :global(.port.connectingto)),
	.card:has(> :global(.port.valid)) {
		border-color: var(--accent);
		box-shadow: 0 0 0 1px var(--accent);
	}
	/* Under the pointer the band reaches half again as far, outward, so the box keeps its size. */
	.card:hover :global(.port) {
		inset: calc(-1 * var(--space-3));
	}
	/* The card's footprint is the one `CARD_H` the manager places by: the band, the title, the line. */
	.body {
		position: relative;
		z-index: 1;
		display: flex;
		align-items: stretch;
		gap: var(--space-2);
		margin: var(--space-5);
		border-radius: var(--radius-sm);
		background: var(--surface-1);
	}
	.main {
		flex: 1 1 auto;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}
	.title {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		height: 34px;
		padding: 0 var(--space-2);
	}
	/* The names wrap in the room left under the title; past two lines they end in an ellipsis. */
	.attrs {
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		overflow: hidden;
		padding: 0 var(--space-2);
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		line-height: 1.2;
		color: var(--text-muted);
		overflow-wrap: anywhere;
	}
	.attr + .attr::before {
		content: ' ';
	}
	/* The facts stand at the end, centred, and show only under the pointer or on the selected box. */
	.facts {
		flex: 0 0 auto;
		display: flex;
		flex-direction: column;
		justify-content: center;
		align-items: flex-end;
		padding-inline-end: var(--space-2);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		font-variant-numeric: tabular-nums;
		line-height: 1.25;
		white-space: nowrap;
		opacity: 0;
		transition: opacity var(--dur-fast) var(--ease);
	}
	.card:hover .facts,
	:global(.svelte-flow__node.selected) .facts {
		opacity: 1;
	}
	.name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
		font-size: var(--fs-display);
		font-weight: 600;
	}
	.muted {
		color: var(--text-muted);
	}
	/* A finger needs a wider band to start a transition from. */
	@media (hover: none) and (pointer: coarse) {
		.body {
			margin: var(--space-7);
		}
	}
</style>
