<script lang="ts">
	import { onMount } from 'svelte';
	import type { PanelProps } from 'panelty';
	import type { Context, NodeDrag, NodeDrop, Panel } from '../../../../sdk/frontend';
	import { notify } from '$lib/stores/notify.svelte';
	import { drag, dropTarget, DropHint, type DropTargetOptions, type Point } from '$lib/drag';

	let { panelId, state: panelState, setState, panel, ctx }: PanelProps & { panel: Panel; ctx: Context } = $props();
	let element: HTMLDivElement;
	let error = $state('');
	const engine = drag();
	const accepts = $derived(panel.accepts_node === true);
	const drops = new Set<(drop: NodeDrop) => void>();
	const drags = new Set<(drag: NodeDrag | null) => void>();

	/** The zone under `at`, as the plugin spelled it after `<panelId>#` on the element's
	 * `data-node-drop`, found in the plugin's own tree, through a shadow root when it keeps one. */
	function zoneAt(at: Point): string | null {
		const root = element.shadowRoot ?? document;
		const key = root.elementFromPoint(at.x, at.y)?.closest<HTMLElement>('[data-node-drop]')?.dataset.nodeDrop ?? '';
		return key.startsWith(`${panelId}#`) ? key.slice(panelId.length + 1) : null;
	}
	const over = $derived(accepts && engine.isOver(panelId));
	const zone = $derived(over && engine.point ? zoneAt(engine.point) : null);
	const live = $derived.by((): NodeDrag | null => {
		const s = engine.source;
		return accepts && s?.tags.includes('node') ? { node: s.key, over, zone } : null;
	});
	const target = $derived<DropTargetOptions | null>(
		accepts
			? {
					id: panelId,
					tags: ['node'],
					onDrop: ({ source, at }) => {
						const zone = zoneAt(at) ?? '';
						for (const handler of drops) fenced(() => handler({ node: source.key, zone, x: at.x, y: at.y }));
					}
				}
			: null
	);

	// A plugin failing in its handler must not break the editor's drag, so each is fenced.
	function fenced(run: () => void): void {
		try { run(); } catch (cause) { notify().failure(`Plugin panel ${panel.title}`, cause); }
	}
	$effect(() => {
		const current = live;
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
				fenced(() => cleanup?.());
			};
		} catch (cause) {
			error = String(cause);
			notify().failure(`Plugin panel ${panel.title}`, cause);
		}
	});
</script>

<div class="plugin-panel" use:dropTarget={target}>
	<div class="plugin-root" data-plugin-panel bind:this={element}></div>
	{#if error}<p role="alert">{error}</p>{/if}
	{#if accepts}<DropHint id={panelId} />{/if}
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
