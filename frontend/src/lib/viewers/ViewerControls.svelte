<!-- Shared viewer header controls: the ARRAY viewer-type dropdown plus the settings cog. -->
<script lang="ts">
	import ViewerSettingsMenu from './ViewerSettingsMenu.svelte';
	import { ARRAY_KINDS, pinnedKind, type ViewerKind } from './registry';
	import type { ViewBinding } from './viewBinding';
	import { Select } from '$lib/ui';

	let { dtype, binding }: { dtype: string; binding: ViewBinding } = $props();
</script>

<!-- No wrapper: the host strip owns the gap. Every unpinned dtype, texture and audio too, gets the
     array kinds; stopPropagation keeps a pick on a node header from toggling the slot's collapse. -->
{#if !pinnedKind(dtype)}
	<Select
		density="chrome"
		value={binding.kind}
		options={[...ARRAY_KINDS]}
		onChange={(k) => binding.setKind(k as ViewerKind)}
		onclick={(e) => e.stopPropagation()}
		title="viewer type"
		data-testid="viewer-kind"
	/>
{/if}
<ViewerSettingsMenu {binding} />
