<script lang="ts">
	import { onMount } from 'svelte';
	import type { PanelProps } from 'panelty';
	import type { Context, Panel } from '../../../../sdk/frontend';
	import { notify } from '$lib/stores/notify.svelte';

	let { panelId, state: panelState, setState, panel, ctx }: PanelProps & { panel: Panel; ctx: Context } = $props();
	let element: HTMLDivElement;
	let error = $state('');
	onMount(() => {
		try {
			const cleanup = panel.mount(element, {
				...ctx,
				panel_id: panelId,
				get state() { return panelState; },
				set_state: (value, intent = 'edit') => setState(value, intent === 'navigation' ? 'navigation' : undefined)
			});
			return () => {
				try { cleanup?.(); } catch (cause) { notify().failure(`Plugin panel ${panel.title}`, cause); }
			};
		} catch (cause) {
			error = String(cause);
			notify().failure(`Plugin panel ${panel.title}`, cause);
		}
	});
</script>

<div class="plugin-panel" bind:this={element}>
	{#if error}<p role="alert">{error}</p>{/if}
</div>

<style>
	.plugin-panel {
		container-type: inline-size;
		width: 100%;
		height: 100%;
		min-width: 0;
		min-height: 0;
		overflow: auto;
	}
</style>
