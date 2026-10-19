<!-- A state's box on the machine canvas: its name, and one line that sums up how it decides. Its
     settings are the inspector's. Its border band is the one connection handle, so a drag from the
     edge draws a transition and a drag from the name moves the box. -->
<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { ContextMenu, createLongPress, type MenuItem } from 'panelty';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { DECISIONS, outgoing } from './decision';
	import { CARD_H, CARD_W } from './layout';

	let { id, data }: NodeProps = $props();
	const g = graph();
	const machine = $derived(data.machine as string);
	const m = $derived(g.machines[machine]);
	const s = $derived(m?.states[id]);
	const out = $derived(m ? outgoing(m, id).length : 0);
	const mode = $derived(DECISIONS.find((d) => d.id === s?.mode)?.label ?? '');
	const set = $derived(Object.keys(s?.values ?? {}).length);

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
		<div class="title">
			<span class="name" data-testid="state-card-name">{id}</span>
		</div>
		{#if s}
			<div class="summary" data-testid="state-card-summary" title="How it decides, every interval; what it sets; the transitions out">
				<span>{mode} · {s.interval < 0 ? 'no beat' : `${s.interval.toFixed(2)}s`}</span>
				<span class="muted">{set} set · {out} out</span>
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
	:global(.svelte-flow__node.selected) .card {
		outline: var(--focus-width) solid var(--accent);
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
	/* The outline lights and thickens fourfold under the pointer — inward, as a shadow, so nothing
	   moves — and while a cable in flight has this box as its target. */
	.card:hover,
	.card:has(> :global(.port.connectingto)),
	.card:has(> :global(.port.valid)) {
		border-color: var(--accent);
		box-shadow: inset 0 0 0 3px var(--accent);
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
		flex-direction: column;
		margin: var(--space-5);
		border-radius: var(--radius-sm);
		background: var(--surface-1);
	}
	.title {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		height: 34px;
		padding: 0 var(--space-2);
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
	.summary {
		display: flex;
		justify-content: space-between;
		gap: var(--space-2);
		padding: 0 var(--space-2);
		font-family: var(--font-mono);
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
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
