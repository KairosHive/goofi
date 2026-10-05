<script lang="ts">
	import { onDestroy } from 'svelte';
	import { Icon } from '$lib/ui';
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

	/** Stop the press before the control board's direct drag listener sees it. */
	function stopDrag(button: HTMLButtonElement): { destroy(): void } {
		const stop = (event: PointerEvent): void => event.stopPropagation();
		button.addEventListener('pointerdown', stop);
		return { destroy: () => button.removeEventListener('pointerdown', stop) };
	}

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

<button type="button" class:listening data-testid={testid}
	aria-label={`MIDI learn for ${label}`} aria-pressed={listening}
	title={listening ? 'Listening for a MIDI change. Click to cancel.' : 'MIDI learn'}
	use:stopDrag onclick={() => void learn()}>
	{#if listening}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="radio" />{/if}
</button>

<style>
	button {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: var(--hit);
		min-height: var(--hit);
		padding: 0;
		background: transparent;
		border: none;
		border-radius: var(--radius-md);
		color: var(--info);
		cursor: pointer;
	}
	button:focus-visible { outline: var(--focus-width) solid var(--focus-ink); }
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
