<script lang="ts">
	import { harnesses, harnessLabel } from '$lib/stores/harness.svelte';
	import { detachTermSession } from '$lib/stores/termSession';
	import { workspace } from 'panelty';
	import { Button, ConfirmDialog } from '$lib/ui';

	const hs = harnesses();
	const ws = workspace();

	const inst = $derived(hs.instances.find((i) => i.id === hs.closing?.id) ?? null);
	const name = $derived(inst ? harnessLabel(inst) : 'the harness');

	function answer(kill: boolean): void {
		const at = hs.closing;
		if (!at) return;
		if (kill) {
			hs.kill(at.id);
		} else {
			detachTermSession(at.id);
			const panel = hs.panelShowing(at.id);
			if (panel) hs.release(panel);
			else hs.cancelClose();
		}
		if (at.closePanel) ws.close(at.closePanel);
	}
</script>

<ConfirmDialog
	open={!!hs.closing}
	question="Close this agent view?"
	detail={`Detaching leaves ${name} running in the patch workspace — re-attach it from any agent panel. Killing stops it.`}
	onClose={() => hs.cancelClose()}
	data-testid="agent-close-dialog"
>
	<Button data-testid="agent-detach" onclick={() => answer(false)}>Detach</Button>
	<Button variant="danger" data-testid="agent-kill" onclick={() => answer(true)}>Kill</Button>
	<Button variant="ghost" onclick={() => hs.cancelClose()}>Cancel</Button>
</ConfirmDialog>
