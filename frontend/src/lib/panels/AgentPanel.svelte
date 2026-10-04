<!-- Agent panel — a terminal on an agent harness running in this patch's workspace. The `Terminal`
     lives per instance in `$lib/stores/termSession`, so it outlives this panel. -->
<script lang="ts">
	import '@xterm/xterm/css/xterm.css';
	import type { PanelProps } from 'panelty';
	import { graph } from '$lib/stores/graph.svelte';
	import { harnesses, harnessLabel } from '$lib/stores/harness.svelte';
	import { termSession, type TerminalLike } from '$lib/stores/termSession';
	import { Bar, ChoiceGrid, EmptyState, Icon, IconButton, Select, type Choice } from '$lib/ui';
	import { untrack } from 'svelte';
	import { notify } from '$lib/stores/notify.svelte';

	let { panelId }: PanelProps = $props();
	const hs = harnesses();
	const g = graph();

	// `untrack`: claiming WRITES the map it reads, so a tracked effect would remount on every
	// roster event and re-claim the instance a Detach just let go of.
	$effect(() => {
		untrack(() => hs.mount(panelId));
		return () => hs.unmount(panelId);
	});

	const id = $derived(hs.instanceFor(panelId));

	/** The launcher's tiles: attach a harness running in this patch, or start one found on PATH. */
	const choices = $derived<Choice[]>([
		...hs.instances.map((i) => ({
			id: `attach:${i.id}`,
			label: `Attach ${harnessLabel(i)}`,
			icon: 'bot' as const,
			choose: () => hs.show(panelId, i.id)
		})),
		...hs.agents.map((a) => ({
			id: `launch:${a.name}`,
			label: a.name,
			icon: 'bot' as const,
			title: a.command,
			testid: `agent-launch-${a.name}`,
			choose: () => void hs.launch(panelId, a.name)
		}))
	]);

	let host = $state<HTMLDivElement | null>(null);

	const token = (n: string): string =>
		getComputedStyle(document.documentElement).getPropertyValue(n).trim();

	/** xterm and its fit addon as ONE object: the addon has to live with the terminal that outlives
	 * this panel, or a remount has nothing to measure with. */
	function makeTerminal(Terminal: typeof import('@xterm/xterm').Terminal, FitAddon: typeof import('@xterm/addon-fit').FitAddon): TerminalLike {
		const t = new Terminal({
			fontFamily: token('--font-mono'),
			fontSize: 12,
			scrollback: 5000,
			theme: {
				background: token('--surface-1'),
				foreground: token('--text'),
				cursor: token('--accent')
			}
		});
		const fit = new FitAddon();
		t.loadAddon(fit);
		return Object.assign(t, { proposeDimensions: () => fit.proposeDimensions() });
	}

	$effect(() => {
		const el = host;
		const at = id;
		if (!el || !at) return;
		let active = true;
		let detach = () => {};
		void Promise.all([import('@xterm/xterm'), import('@xterm/addon-fit')]).then(([{ Terminal }, { FitAddon }]) => {
			if (!active) return;
			const s = termSession(at, () => makeTerminal(Terminal, FitAddon));
			s.attach(el);
			// The container is the only size proposer; an inbound size never echoes a proposal.
			const ro = new ResizeObserver(() => s.refit());
			ro.observe(el);
			detach = () => { ro.disconnect(); s.retract(); };
		}).catch((error) => { if (active) notify().failure('Agent terminal', error); });
		return () => {
			active = false;
			// Gives up the SIZE, not the stream: the socket stays open for whoever shows this next.
			detach();
		};
	});
</script>

<div class="agent">
	{#if id}
		<Bar>
			{#snippet start()}
				<Select
					density="chrome"
					value={id}
					options={hs.instances.map((i) => i.id)}
					labels={Object.fromEntries(hs.instances.map((i) => [i.id, harnessLabel(i)]))}
					onChange={(v) => hs.show(panelId, v)}
					data-testid="agent-switcher"
				/>
			{/snippet}
			{#snippet end()}
				<IconButton
					variant="ghost"
					density="chrome"
					label="Close agent view"
					data-testid="agent-close"
					onclick={() => hs.requestClose(id)}><Icon name="x" /></IconButton
				>
			{/snippet}
		</Bar>
		<div class="host" bind:this={host} data-testid="agent-terminal"></div>
	{:else}
		<div class="launcher" data-testid="agent-launcher">
			<EmptyState>
				{#snippet title()}{g.demo ? 'Agents are off in this demo' : 'No agent running here'}{/snippet}
				{#snippet hint()}
					{g.demo
						? 'A public goofi runs no terminal and spawns no agent. Run goofi locally for those.'
						: hs.configError
							? hs.configError
							: hs.agents.length === 0
								? 'The config lists no agents — add [[agents]] entries to config.toml in your goofi home.'
								: 'It runs in this patch workspace, editing the patch with you.'}
				{/snippet}
				{#if !g.demo}<ChoiceGrid {choices} />{/if}
			</EmptyState>
		</div>
	{/if}
</div>

<style>
	.agent {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
		background: var(--surface-1);
	}
	/* The terminal's box is the PANEL's: xterm lays out against this element, so a host that grew
	   with its content would re-measure forever. `--kb-inset` is the soft keyboard's overlap. */
	.host {
		position: relative;
		flex: 1;
		min-height: 0;
		overflow: hidden;
		padding: var(--space-2);
		padding-bottom: calc(var(--space-2) + var(--kb-inset, 0px));
		touch-action: pan-y;
	}
</style>
