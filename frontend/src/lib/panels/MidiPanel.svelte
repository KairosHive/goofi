<!-- MIDI panel — the devices the host lists, and nothing else: grab one onto the variable bus as a
     group, or release it. Which device a group reads is the patch's; the list is the host's,
     re-asked while the panel is open, so a device plugged in or pulled shows up. -->
<script lang="ts">
	import { onMount } from 'svelte';
	import type { PanelProps } from 'panelty';
	import { graph, type MidiPort } from '$lib/stores/graph.svelte';
	import { Bar, Button, EmptyState, Icon, IconButton, ScrollArea, Select, StatusDot } from '$lib/ui';

	let {}: PanelProps = $props();
	const g = graph();
	const POLL_MS = 2000;
	const CHANNELS = ['all', ...Array.from({ length: 16 }, (_, i) => String(i + 1))];
	let ports = $state<MidiPort[]>([]);
	let reason = $state<string | undefined>();
	let channel = $state<Record<string, string>>({});
	let busy = $state(false);
	let error = $state('');
	let asked = $state(false);

	async function refresh(): Promise<void> {
		try {
			const listed = await g.listMidi();
			ports = listed.ports;
			reason = listed.reason;
		} catch (e) {
			error = String(e);
		}
		asked = true;
	}

	onMount(() => {
		void refresh();
		const timer = setInterval(() => void refresh(), POLL_MS);
		return () => clearInterval(timer);
	});
	// A grab or a release lands in the document first: the list follows it at once.
	$effect(() => {
		void g.midiGroups;
		void refresh();
	});

	async function guarded(op: () => Promise<unknown>): Promise<void> {
		busy = true;
		error = '';
		try {
			await op();
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}

	const grab = (port: MidiPort) =>
		guarded(() => g.grabMidi(port.port, channel[port.port] && channel[port.port] !== 'all' ? Number(channel[port.port]) : undefined));
</script>

<div class="panel-wrap" data-testid="midi-panel">
	<Bar>
		{#snippet start()}<span class="title">MIDI devices</span>{/snippet}
		{#snippet end()}
			<IconButton variant="ghost" label="Refresh" title="Refresh the list" data-testid="midi-refresh" onclick={() => void refresh()}><Icon name="refresh-cw" /></IconButton>
		{/snippet}
	</Bar>
	<ScrollArea>
		<div class="body">
			{#if asked && ports.length === 0}
				<EmptyState data-testid="midi-empty">
					{#snippet title()}No MIDI devices{/snippet}
					{#snippet hint()}{reason ?? 'Plug in a controller; it shows up here.'}{/snippet}
				</EmptyState>
			{/if}
			{#each ports as port (port.port + (port.group ?? ''))}
				<div class="port" data-testid="midi-port" data-port={port.port} data-state={port.state} data-group={port.group}>
					<StatusDot tone={port.state === 'present' ? 'success' : 'danger'} size="sm"
						aria-label={port.state === 'present' ? 'Present' : 'Gone'} />
					<div class="names">
						<span class="port-name">{port.port}</span>
						{#if port.group}
							<span class="group-name" data-testid="midi-group">variables.{port.group}{#if port.channel} · channel {port.channel}{/if}</span>
						{/if}
					</div>
					{#if port.group}
						<Button variant="ghost" size="sm" data-testid="midi-release" disabled={busy}
							onclick={() => void guarded(() => g.releaseMidi(port.group!))}>release</Button>
					{:else}
						<Select aria-label="Channel" data-testid="midi-channel" value={channel[port.port] ?? 'all'} options={CHANNELS}
							onChange={(value) => (channel[port.port] = value)} />
						<Button variant="primary" size="sm" data-testid="midi-grab" disabled={busy} onclick={() => void grab(port)}>grab</Button>
					{/if}
				</div>
			{/each}
			{#if error}<p class="error-text" role="alert">{error}</p>{/if}
		</div>
	</ScrollArea>
</div>

<style>
	.title {
		font-size: var(--fs-small);
		color: var(--text-muted);
	}
	.body {
		padding: var(--space-3) var(--space-5) var(--space-6);
	}
	.port {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		min-height: var(--hit);
		padding: var(--space-2) 0;
		font-size: var(--fs-small);
	}
	.port + .port {
		border-top: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
	}
	.names {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-width: 0;
	}
	.port-name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.group-name {
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		color: var(--text-muted);
	}
	@container (max-width: 280px) {
		.port { flex-wrap: wrap; }
		.names { flex-basis: 100%; }
	}
</style>
