<!-- A state's box on the machine canvas: its name, and nothing else — its values live in the
     inspector. Its border band is the one connection handle, so a drag from the edge draws a
     transition and a drag from the middle moves the box. Deleting is the Delete key or the menu. -->
<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { ContextMenu, createLongPress, type MenuItem } from 'panelty';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { CARD_W } from './layout';

	let { id, data }: NodeProps = $props();
	const g = graph();
	const machine = $derived(data.machine as string);

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

<div class="card" style={`width: ${CARD_W}px`} data-testid={`state-card-${id}`} data-state={id}>
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
		<span class="name">{id}</span>
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
	/* The handle IS the card, under the body: what shows of it is the band around the name. */
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
	.body {
		position: relative;
		z-index: 1;
		display: flex;
		align-items: center;
		min-height: var(--hit);
		margin: var(--space-5);
		padding: 0 var(--space-2);
		border-radius: var(--radius-sm);
	}
	.name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		text-align: center;
		font-family: var(--font-mono);
		font-weight: 600;
	}
	/* A finger needs a wider band to start a transition from. */
	@media (hover: none) and (pointer: coarse) {
		.body {
			margin: var(--space-7);
		}
	}
</style>
