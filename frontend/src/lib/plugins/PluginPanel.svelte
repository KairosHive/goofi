<script lang="ts">
	import { onMount } from 'svelte';
	import type { PanelProps } from 'panelty';
	import type { Context, NodeDrag, NodeDrop, Panel } from '../../../../sdk/frontend';
	import { notify } from '$lib/stores/notify.svelte';
	import { ui } from '$lib/stores/ui.svelte';

	let { panelId, state: panelState, setState, panel, ctx }: PanelProps & { panel: Panel; ctx: Context } = $props();
	let element: HTMLDivElement;
	let error = $state('');
	const uiStore = ui();
	const accepts = $derived(panel.accepts_node === true);
	const dragActive = $derived(accepts && uiStore.nodeDrag !== null);
	const over = $derived(uiStore.nodeDragTarget === panelId);
	// The zone under the pointer, as the plugin spelled it: what follows `<panelId>#`.
	const zone = $derived.by(() => {
		const z = uiStore.nodeDragZone;
		return z !== null && z.startsWith(`${panelId}#`) ? z.slice(panelId.length + 1) : null;
	});
	const drag = $derived<NodeDrag | null>(
		dragActive ? { node: uiStore.nodeDrag as string, over: over || zone !== null, zone } : null
	);
	const drops = new Set<(drop: NodeDrop) => void>();
	const drags = new Set<(drag: NodeDrag | null) => void>();

	// A plugin failing in its handler must not break the editor's drag, so each is fenced.
	function fenced(run: () => void): void {
		try { run(); } catch (cause) { notify().failure(`Plugin panel ${panel.title}`, cause); }
	}
	$effect(() => {
		if (!accepts) return;
		return uiStore.onNodeDrop(panelId, (node, zone, at) => {
			for (const handler of drops) fenced(() => handler({ node, zone, x: at.x, y: at.y }));
		});
	});
	$effect(() => {
		const current = drag;
		for (const handler of drags) fenced(() => handler(current));
	});
	onMount(() => {
		try {
			const cleanup = panel.mount(element, {
				...ctx,
				panel_id: panelId,
				get state() { return panelState; },
				set_state: (value, intent = 'edit') => setState(value, intent === 'navigation' ? 'navigation' : undefined),
				on_node_drop(handler) {
					drops.add(handler);
					return () => { drops.delete(handler); };
				},
				on_node_drag(handler) {
					drags.add(handler);
					return () => { drags.delete(handler); };
				}
			});
			return () => {
				drops.clear();
				drags.clear();
				try { cleanup?.(); } catch (cause) { notify().failure(`Plugin panel ${panel.title}`, cause); }
			};
		} catch (cause) {
			error = String(cause);
			notify().failure(`Plugin panel ${panel.title}`, cause);
		}
	});
</script>

<div class="plugin-panel">
	<div class="plugin-root" data-plugin-panel bind:this={element}></div>
	{#if error}<p role="alert">{error}</p>{/if}
	{#if dragActive}
		<div class="node-drop-hint" class:active={over} data-testid="node-drop-hint"></div>
	{/if}
</div>

<style>
	.plugin-panel {
		position: relative;
		container-type: inline-size;
		width: 100%;
		height: 100%;
		min-width: 0;
		min-height: 0;
		overflow: auto;
	}
	.plugin-root {
		width: 100%;
		height: 100%;
		min-width: 0;
		min-height: 0;
	}
</style>
