<script lang="ts">
	import { onDestroy } from 'svelte';
	import { ContextMenu, type MenuItem } from 'panelty';
	import { Icon } from '$lib/ui';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { midiLearn } from '$lib/stores/midiLearn.svelte';

	let { label, target, onLearn, testid = 'param-learn' }: {
		label: string;
		target: string;
		onLearn: (reference: string, index: number) => void;
		testid?: string;
	} = $props();
	const owner = $props.id();
	const g = graph();
	const listening = $derived(midiLearn.target === target);
	let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);

	/** Stop the press before the control board's direct drag listener sees it. */
	function stopDrag(button: HTMLButtonElement): { destroy(): void } {
		const stop = (event: PointerEvent): void => event.stopPropagation();
		button.addEventListener('pointerdown', stop);
		return { destroy: () => button.removeEventListener('pointerdown', stop) };
	}

	function learn(event: MouseEvent): void {
		if (listening) return midiLearn.stop();
		midiLearn.stop();
		const groups = Object.keys(g.midiGroups);
		if (groups.length === 0) {
			notify().raise('Grab a MIDI device in the MIDI panel before MIDI learn.');
			return;
		}
		const start = (chosen: string[]): void => {
			menu = null;
			midiLearn.start(owner, target, chosen, onLearn);
		};
		if (groups.length === 1) start(groups);
		else {
			const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
			menu = { x: rect.left, y: rect.bottom, items: groups.map((group) => ({ label: group, action: () => start([group]) })) };
		}
	}
	midiLearn.watch();
	onDestroy(() => { if (midiLearn.owner === owner) midiLearn.stop(); });
</script>

<button type="button" class:listening data-testid={testid}
	aria-label={`MIDI learn for ${label}`} aria-pressed={listening}
	title={listening ? 'Listening for a MIDI change. Click to cancel.' : 'MIDI learn'}
	use:stopDrag onclick={learn}>
	{#if listening}<span class="spinner" aria-hidden="true"></span>{:else}<Icon name="radio" />{/if}
</button>
{#if menu}
	<ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => (menu = null)} />
{/if}

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
