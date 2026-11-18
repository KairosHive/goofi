<script lang="ts">
	import { onDestroy } from 'svelte';
	import { Icon, IconButton } from '$lib/ui';
	import { notify } from '$lib/stores/notify.svelte';
	import { midiLearn } from '$lib/stores/midiLearn.svelte';

	let { label, target, onLearn, testid = 'param-learn' }: {
		label: string;
		target: string;
		onLearn: (reference: string, index: number) => void;
		testid?: string;
	} = $props();
	const owner = $props.id();
	const listening = $derived(midiLearn.target === target);

	async function learn(): Promise<void> {
		if (listening) return midiLearn.stop();
		try {
			if ((await midiLearn.start(owner, target, onLearn)) === 0) notify().raise('No MIDI device is plugged in to learn from.');
		} catch (e) {
			notify().raise(`MIDI learn: ${String(e)}`);
		}
	}
	midiLearn.watch();
	onDestroy(() => { if (midiLearn.owner === owner) midiLearn.stop(); });
</script>

<!-- The press stops here, before the control board's direct drag listener sees it. -->
<span class="learn">
	<IconButton variant="ghost" size="sm" label={`MIDI learn for ${label}`} title={listening ? 'Listening for a MIDI change. Click to cancel.' : 'MIDI learn'}
		aria-pressed={listening} data-testid={testid} onpointerdown={(e) => e.stopPropagation()} onclick={() => void learn()}>
		{#if listening}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="radio" />{/if}
	</IconButton>
</span>

<style>
	/* The learn's own ink, through the button's colour hook. */
	.learn {
		display: inline-flex;
		--panelty-text: var(--info);
	}
	.spinner {
		width: 14px;
		height: 14px;
		border: 2px solid currentColor;
		border-right-color: transparent;
		border-radius: 50%;
		animation: spin 0.8s linear infinite;
	}
	@keyframes spin { to { transform: rotate(360deg); } }
</style>
