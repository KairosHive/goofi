<!-- The one quick pick, drawn where it was asked for. Mounted once, in the shell. -->
<script lang="ts">
	import { QuickPick } from '$lib/ui';
	import { quickPick } from '$lib/stores/quickPick.svelte';

	const picks = quickPick();
</script>

{#if picks.pending}
	{@const p = picks.pending}
	<QuickPick x={p.at.x} y={p.at.y} options={p.options} label={p.label} onPick={(id) => picks.settle(id)} onClose={() => picks.settle(null)} data-testid="quick-pick" />
{/if}
