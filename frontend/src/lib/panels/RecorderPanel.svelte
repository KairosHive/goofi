<!-- Recorder panel — the name, the root and the start/stop of one recording, over the streams that
     are armed. What is armed lives in the document; the session lives in `record status`. -->
<script lang="ts">
	import { ContextMenu, type MenuItem, type PanelProps } from 'panelty';
	import { VIDEO_QUALITIES, type VideoQuality } from '$lib/api/types';
	import type { DropPoint } from '$lib/stores/ui.svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import { history } from '$lib/stores/history.svelte';
	import { ui } from '$lib/stores/ui.svelte';
	import { Bar, Button, Field, Icon, IconButton, ScrollArea, Select, StatusDot, TextInput } from '$lib/ui';

	const { panelId }: PanelProps = $props();

	const g = graph();
	const uiStore = ui();

	// The next start's arguments, not a copy of backend state: empty is what makes the backend
	// fall back to the `record.*` globals.
	let name = $state('');
	let root = $state('');
	let failure = $state<string | null>(null);

	const dragActive = $derived(uiStore.nodeDrag !== null);
	const over = $derived(uiStore.nodeDragTarget === panelId);
	const rec = $derived(g.record);

	let nodeMenu = $state<DropPoint | null>(null);
	let slotMenu = $state<{ uid: string; at: DropPoint; slots: string[]; selected: string[] } | null>(null);
	const qualityLabels = { small: 'Smaller file', high: 'High', very_high: 'Very high' };

	function armSlots(uid: string, slots: string[]): void {
		const node = g.nodeById(uid);
		if (!node || slots.length === 0) return;
		failure = null;
		void history()
			.transaction(`Arm ${node.name}`, async () => {
				for (const slot of slots) await g.armSlot(uid, slot);
			})
			.catch((e: unknown) => (failure = String(e)));
	}

	function chooseSlots(uid: string, at: DropPoint): void {
		const slots = Object.keys(g.nodeById(uid)?.output_slots ?? {});
		if (slots.length === 1) armSlots(uid, slots);
		else if (slots.length > 1) slotMenu = { uid, at, slots, selected: [...slots] };
	}

	function applySlots(): void {
		const chosen = slotMenu;
		slotMenu = null;
		if (chosen) armSlots(chosen.uid, chosen.slots.filter((slot) => chosen.selected.includes(slot)));
	}

	const nodeItems = $derived(g.nodes
		.filter((node) => Object.keys(node.output_slots ?? {}).length > 0)
		.toSorted((a, b) => a.name.localeCompare(b.name))
		.map((node): MenuItem => ({
			label: node.name,
			action: () => { if (nodeMenu) chooseSlots(node.uid, nodeMenu); }
		})));

	const slotItems = $derived.by((): MenuItem[] => {
		const menu = slotMenu;
		if (!menu) return [];
		const all = menu.selected.length === menu.slots.length;
		return [
			{
				label: all ? 'Deselect all' : 'Select all',
				action: () => { menu.selected = all ? [] : [...menu.slots]; }
			},
			{ separator: true },
			...menu.slots.map((slot): MenuItem => ({
				label: slot,
				checked: menu.selected.includes(slot),
				action: () => {
					menu.selected = menu.selected.includes(slot)
						? menu.selected.filter((name) => name !== slot)
						: [...menu.selected, slot];
				}
			}))
		];
	});

	$effect(() => uiStore.onNodeDrop(panelId, (uid, _zone, at) => chooseSlots(uid, at)));

	type Row = {
		uid: string;
		slot: string;
		label: string;
		quality: VideoQuality;
		video: boolean;
		frames: number;
		dropped: number;
		/** Dropping NOW: the count moved on the last report, which the cumulative one cannot say. */
		losing: boolean;
		fill: number;
	};

	const rows = $derived.by((): Row[] =>
		g.armed.map((a) => {
			const node = g.nodeById(a.uid);
			const label = node?.name ?? a.uid;
			const st = rec.streams.find(
				(s) => (s.node === label || s.node === a.uid) && s.slot === a.slot
			);
			return {
				uid: a.uid,
				slot: a.slot,
				label,
				quality: a.quality,
				video: node?.output_slots?.[a.slot] === 'TEXTURE',
				frames: st?.frames ?? 0,
				dropped: st?.dropped ?? 0,
				losing: g.dropping.has(`${st?.node ?? label}/${a.slot}`),
				fill: st?.fill ?? 0
			};
		})
	);

	const dropping = $derived(rows.some((r) => r.losing));

	function clock(seconds: number | null): string {
		const t = Math.max(0, Math.floor(seconds ?? 0));
		const m = Math.floor(t / 60);
		return `${String(m).padStart(2, '0')}:${String(t % 60).padStart(2, '0')}`;
	}

	function toggle(): void {
		failure = null;
		const act = rec.running ? g.stopRecording() : g.startRecording(name, root);
		void act.catch((e: unknown) => (failure = String(e)));
	}
</script>

<div class="recorder" data-testid="recorder-panel">
	<Bar>
		{#snippet start()}
			<StatusDot
				tone={rec.running ? (dropping ? 'warn' : 'ok') : 'error'}
				size="sm"
				pulse={rec.running}
				title={rec.running ? 'A recording runs' : 'No recording runs'}
			/>
			<span class="elapsed" data-testid="recorder-elapsed">{clock(rec.elapsed)}</span>
		{/snippet}
		{#snippet end()}
			<Button
				variant={rec.running ? 'danger' : 'primary'}
				size="sm"
				data-testid="recorder-toggle"
				disabled={!rec.running && rows.length === 0}
				onclick={toggle}
				><Icon name={rec.running ? 'square-dashed' : 'circle-dot'} />{rec.running
					? 'Stop'
					: 'Start'}</Button
			>
		{/snippet}
	</Bar>

	<div class="add-node">
		<Button size="sm" disabled={nodeItems.length === 0} data-testid="recorder-add-node"
			onclick={(event) => {
				const box = event.currentTarget.getBoundingClientRect();
				nodeMenu = { x: box.left, y: box.bottom };
			}}
		><Icon name="plus" />Add node<Icon name="chevron-down" /></Button>
	</div>

	<div class="fields">
		<Field label="Name" doc="The folder name for the next recording. Empty uses record.name.">
			<TextInput
				value={name}
				onChange={(v) => (name = v)}
				placeholder="auto"
				data-testid="recorder-name"
			/>
		</Field>
		<Field label="Root" doc="Where the folder is made. Empty uses record.root.">
			<TextInput
				value={root}
				onChange={(v) => (root = v)}
				placeholder="~/.goofi/recordings"
				data-testid="recorder-root"
			/>
		</Field>
	</div>

	{#if rec.folder}
		<div class="folder" data-testid="recorder-folder" title={rec.folder}>{rec.folder}</div>
	{/if}
	{#if failure ?? rec.error}
		<div class="failure" data-testid="recorder-error">{failure ?? rec.error}</div>
	{/if}

	<ScrollArea>
		{#if rows.length === 0}
			<p class="hint">No slot is armed. Add a node above or drag one here to record its output slots.</p>
		{:else}
			<ul class="streams">
				{#each rows as r (r.uid + '/' + r.slot)}
					<li class="row" data-testid="recorder-stream" data-output={`${r.uid}/${r.slot}`}>
						<div class="stream-status">
							<StatusDot
								tone={r.losing ? 'warn' : 'ok'}
								size="sm"
								title={r.losing
									? `Dropping frames — ${r.dropped} lost`
									: `${r.dropped} frames lost`}
							/>
							<span class="who">{r.label}/{r.slot}</span>
							<span class="num" title="Frames written">{r.frames}</span>
							<span class="num drops" class:bad={r.losing} title="Frames dropped"
								>{r.dropped}</span
							>
							<span class="fill" title="Buffer fill">
								<span class="fill-bar" style={`width: ${Math.round(r.fill * 100)}%`}></span>
							</span>
							<IconButton
								variant="ghost"
								label={`Disarm ${r.label}/${r.slot}`}
								onclick={() => void g.disarmSlot(r.uid, r.slot).catch(() => {})}
								><Icon name="x" /></IconButton
							>
						</div>
						<div class="quality">
							{#if r.video}
								<Field label="Quality" doc="Higher quality produces larger files. A change during recording starts a new video file.">
									<Select value={r.quality} options={[...VIDEO_QUALITIES]} labels={qualityLabels}
										data-testid="recorder-quality"
										onChange={(value) => void g.setRecordQuality(r.uid, r.slot, value as VideoQuality)
											.catch((error: unknown) => (failure = String(error)))} />
								</Field>
							{:else}
								<span class="native-quality">Native · full precision</span>
							{/if}
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</ScrollArea>

	{#if dragActive}
		<div class="node-drop-hint" class:active={over} data-testid="node-drop-hint"></div>
	{/if}
</div>

{#if nodeMenu}
	<ContextMenu x={nodeMenu.x} y={nodeMenu.y} items={nodeItems} onClose={() => (nodeMenu = null)} />
{/if}
{#if slotMenu}
	<ContextMenu x={slotMenu.at.x} y={slotMenu.at.y} items={slotItems} closeOnSelect={false} onClose={applySlots} />
{/if}

<style>
	.recorder {
		position: relative;
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	.elapsed {
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		color: var(--text-muted);
		font-variant-numeric: tabular-nums;
	}
	.fields {
		display: grid;
		gap: var(--space-2);
		padding: var(--space-3);
	}
	/* One field per line until the panel is wide enough for two — @container, since a panel's
	   width and the viewport's are separate questions. */
	@container (min-width: 30rem) {
		.fields {
			grid-template-columns: 1fr 1fr;
		}
	}
	.folder,
	.failure {
		padding: 0 var(--space-3) var(--space-2);
		font-size: var(--fs-small);
		overflow-wrap: anywhere;
	}
	.folder {
		color: var(--text-muted);
		font-family: var(--font-mono);
	}
	.failure {
		color: var(--danger);
	}
	.hint {
		margin: 0;
		padding: var(--space-6) var(--space-3);
		text-align: center;
		color: var(--text-muted);
	}
	.streams {
		list-style: none;
		margin: 0;
		padding: 0 var(--space-2) var(--space-2);
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
	}
	.row {
		min-width: 0;
		border-radius: var(--radius-sm);
		background: var(--surface-2);
	}
	.add-node {
		padding: var(--space-3) var(--space-3) 0;
	}
	.quality {
		padding: 0 var(--space-2) var(--space-2);
	}
	.native-quality {
		color: var(--text-muted);
		font-size: var(--fs-small);
	}
	.stream-status {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--hit);
		padding-left: var(--space-2);
		border-radius: var(--radius-sm);
		background: var(--surface-2);
	}
	.who {
		flex: 1 1 auto;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
		font-size: var(--fs-small);
	}
	.num {
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		font-variant-numeric: tabular-nums;
		color: var(--text-muted);
	}
	.drops.bad {
		color: var(--warning);
	}
	.fill {
		flex: 0 0 3rem;
		height: var(--space-2);
		border-radius: var(--radius-sm);
		background: var(--surface-3);
		overflow: hidden;
	}
	.fill-bar {
		display: block;
		height: 100%;
		background: var(--accent);
	}
</style>
