<!-- What a track says about itself, beside its lane: its name with the actions used most, then
     its settings in groups, the group in use first. The groups that do not fit in the room the
     track has go, whole, behind one menu in the title. -->
<script lang="ts" module>
	import { GRIDS, NOTE_NAMES, SCALES } from '$lib/api/vocab';

	/** A MIDI input port the host lists, and the variable group it is read as. */
	export interface Port {
		port: string;
		group: string;
	}
	const PITCHES = Array.from({ length: 128 }, (_, p) => String(p));
	/** A pitch as a keyboard names it: C4 is 60. */
	export const pitchName = (pitch: number): string => `${NOTE_NAMES[pitch % 12]}${Math.floor(pitch / 12) - 1}`;
	const PITCH_NAMES = Object.fromEntries(PITCHES.map((p) => [p, pitchName(Number(p))]));
</script>

<script lang="ts">
	import type { Key, Note, Track } from '$lib/api/generated';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import { drag, dropTarget } from '$lib/drag';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import { Button, CurvePicker, Field, FieldGrid, Icon, IconButton, NumberInput, Select, SettingsMenu, TextInput, fit } from '$lib/ui';

	let {
		id,
		t,
		item,
		recording,
		ports,
		onRescan,
		onEdit,
		onRecord,
		onHide,
		onDelete,
		onItem
	}: {
		id: string;
		t: Track;
		/** The note or the keyframe picked in the track, the first of them. */
		item?: Note | Key;
		recording: boolean;
		/** The MIDI devices there are, for a note track's input. */
		ports: Port[];
		onRescan: () => void;
		/** Change the track's own settings: one `timeline track edit`. */
		onEdit: (patch: Record<string, unknown>) => void;
		onRecord: (e: MouseEvent) => void;
		onHide: () => void;
		onDelete: () => void;
		/** Change what is picked, or with `null` take it out. */
		onItem: (to: Partial<Note & Key> | null) => void;
	} = $props();

	const engine = drag();
	const rename = (to: string): void => {
		if (to !== t.name && isValidIdentifier(to)) onEdit({ to });
	};

	/** A track's groups of settings, in the order they stand: what is picked first, while
	 * something is, then what is set most. */
	const TITLES = { item: 'Selection', keyboard: 'Keyboard', grid: 'Grid', range: 'Range', record: 'Record' };
	type Group = keyof typeof TITLES;
	const groups = $derived<Group[]>([...(item ? (['item'] as const) : []), ...(t.kind === 'note' ? (['keyboard', 'grid', 'record'] as const) : (['range', 'record'] as const))]);
	/** The groups the track has no room for, and whether the menu that holds them is open. */
	let more = $state<string[]>([]);
	let menu = $state(false);
	// A menu that goes while it is open is closed: the next one does not come back open.
	$effect(() => {
		if (more.length === 0 && !tight) menu = false;
	});
	/** An action taken from the menu: the menu's business is done. */
	const leave = (act: () => void) => (): void => {
		menu = false;
		act();
	};
	/** The title has no room for every action beside the name, so the rarer ones go into the
	 * menu; and a row none for a label beside its control, so the label goes above it. */
	let title = $state<HTMLElement | null>(null);
	let [tight, stacked] = $state([false, false]);
	$effect(() => {
		const el = title;
		if (!el) return;
		const measure = (): void => {
			const style = getComputedStyle(el);
			const button = el.querySelector<HTMLElement>('button')!.offsetWidth + parseFloat(style.columnGap);
			tight = el.clientWidth < parseFloat(getComputedStyle(el.querySelector('.name')!).minWidth) + 4 * button;
			stacked = el.clientWidth < 13 * parseFloat(style.fontSize);
		};
		const box = new ResizeObserver(measure);
		box.observe(el);
		return () => box.disconnect();
	});
	/** The devices a note track can record from: those there are, and the one it names. */
	const devices = $derived(['', ...new Set([...ports.map((p) => p.group), ...(t.input ? [t.input] : [])])]);
	const deviceNames = $derived(Object.fromEntries(devices.map((d) => [d, d === '' ? 'auto' : (ports.find((p) => p.group === d)?.port ?? `${d} (absent)`)])));
</script>

{#snippet group(name: Group)}
	{#if name === 'item' && item}
		{#if 'pitch' in item}
			<Field label="length" row doc="How long the note lasts, in beats" data-menu-row>
				<NumberInput value={item.length} min={0.01} step={0.25} onChange={(length) => onItem({ length })} />
				{#snippet adornment()}{@render remove('Delete note')}{/snippet}
			</Field>
			<Field label="velocity" row doc="The note's velocity, 0 to 1" data-menu-row>
				<NumberInput value={item.velocity} min={0} max={1} step={0.05} onChange={(velocity) => onItem({ velocity })} />
			</Field>
		{:else}
			<Field label="value" row doc="The keyframe's value" data-menu-row>
				<NumberInput value={item.value} min={t.range[0]} max={t.range[1]} step={0.01} onChange={(value) => onItem({ value })} />
				{#snippet adornment()}{@render remove('Delete keyframe')}{/snippet}
			</Field>
			<Field label="curve" row doc="How the value eases from this keyframe to the next" data-menu-row>
				<CurvePicker value={item.curve} onChange={(curve) => onItem({ curve })} />
			</Field>
		{/if}
	{:else if name === 'keyboard' && t.kind === 'note'}
		<Field label="keys" row doc="The lowest and the highest pitch the keyboard shows" data-menu-row>
			<Select aria-label="Lowest" value={String(t.range[0])} options={PITCHES.slice(0, t.range[1])} labels={PITCH_NAMES} onChange={(p) => onEdit({ range: [Number(p), t.range[1]] })} />
			<Select aria-label="Highest" value={String(t.range[1])} options={PITCHES.slice(t.range[0] + 1)} labels={PITCH_NAMES} onChange={(p) => onEdit({ range: [t.range[0], Number(p)] })} />
		</Field>
		<Field label="scale" row doc="The scale whose notes the keyboard marks and a note lands on, and the note it starts on" data-menu-row>
			<Select aria-label="Root" class="root" value={t.root} options={[...NOTE_NAMES]} onChange={(root) => onEdit({ root })} />
			<Select aria-label="Scale" value={t.scale} options={Object.keys(SCALES)} onChange={(scale) => onEdit({ scale })} />
		</Field>
	{:else if name === 'grid' && t.kind === 'note'}
		<Field label="grid" row doc="The lines a note of this track snaps to when it is made or dragged; Shift or Ctrl lets it go free" data-menu-row>
			<Select aria-label="Grid" value={t.grid} options={Object.keys(GRIDS)} onChange={(grid) => onEdit({ grid })} />
		</Field>
	{:else if name === 'range' && t.kind === 'curve'}
		<Field label="range" row doc="The lowest and the highest value the lane shows" data-menu-row>
			{#each [0, 1] as end (end)}
				<NumberInput aria-label={end ? 'Highest' : 'Lowest'} value={t.range[end]} step={0.1} onChange={(v) => onEdit({ range: t.range.with(end, v) })} />
			{/each}
		</Field>
	{:else if name === 'record' && t.kind === 'note'}
		<Field label="input" row doc="The MIDI device a recording takes its notes from; auto is the one there is, or the one picked when it begins" data-menu-row>
			<Select aria-label="Input" data-testid="track-device" value={t.input} options={devices} labels={deviceNames} onRefresh={onRescan} onChange={(input) => onEdit({ input })} />
		</Field>
	{:else if name === 'record' && t.kind === 'curve'}
		<Field label="input" row doc="What a recording takes: an expression, as a param's. Drop a node, a variable or a state on it" data-menu-row>
			<div
				class="input"
				class:armed={engine.source?.tags.includes('number') && engine.source.key !== id}
				use:dropTarget={{
					tags: ['number'],
					accept: (source) => source.key !== id,
					onDrop: async ({ source, tag, at }) => {
						const input = await source.expression?.(tag, at);
						if (input) onEdit({ input });
					}
				}}
			>
				<ExprEditor label="Input" value={t.input} placeholder="nd('lfo0')" testid="track-input" onCommit={(input) => onEdit({ input })} />
			</div>
		</Field>
		<Field label="rate" row doc="The most samples a second a recording takes of the input; a node slower than that gives its own" data-menu-row>
			<NumberInput aria-label="Rate" data-testid="track-rate" value={t.rate} min={1} max={120} step={1} onChange={(rate) => onEdit({ rate })} />
			{#snippet adornment()}Hz{/snippet}
		</Field>
	{/if}
{/snippet}

{#snippet remove(label: string)}
	<IconButton variant="ghost" size="sm" {label} data-testid="track-delete-picked" onclick={() => onItem(null)}><Icon name="x" /></IconButton>
{/snippet}

<div class="title" bind:this={title}>
	<TextInput class="name" aria-label="Track name" value={t.name} onChange={rename} />
	<IconButton
		variant="ghost"
		size="sm"
		class={recording ? 'live' : ''}
		label="Record"
		title={recording ? 'End the recording: what it took goes into the track' : `Record ${t.kind === 'note' ? 'the keys of a MIDI device' : 'the input'} into the track while the clock plays`}
		aria-pressed={recording}
		data-testid="track-record"
		onclick={onRecord}><Icon name="circle-dot" /></IconButton
	>
	{#if more.length > 0 || tight}
		<SettingsMenu label="More" icon="ellipsis" testid="track-more" bind:open={menu}>
			{#each groups.filter((name) => more.includes(name)) as name (name)}
				<section aria-label={TITLES[name]}>
					<h4>{TITLES[name]}</h4>
					<FieldGrid>{@render group(name)}</FieldGrid>
				</section>
			{/each}
			{#if tight}
				<section aria-label="Track">
					<h4>Track</h4>
					<div class="acts" data-menu-row>
						<Button size="sm" data-testid="track-hide" onclick={leave(onHide)}><Icon name="eye-off" /> Hide</Button>
						<Button size="sm" data-testid="track-delete" onclick={leave(onDelete)}><Icon name="x" /> Delete…</Button>
					</div>
				</section>
			{/if}
		</SettingsMenu>
	{/if}
	{#if !tight}
		<IconButton variant="ghost" size="sm" label="Hide track" data-testid="track-hide" onclick={onHide}><Icon name="eye-off" /></IconButton>
		<IconButton variant="ghost" size="sm" label="Delete track" data-testid="track-delete" onclick={onDelete}><Icon name="x" /></IconButton>
	{/if}
</div>
<div class="groups" use:fit={(hidden) => (more = hidden)}>
	{#each groups as name (name)}
		<div class="group" data-fit={name} role="group" aria-label={TITLES[name]}><FieldGrid {stacked}>{@render group(name)}</FieldGrid></div>
	{/each}
</div>

<style>
	/* The track's name, then what is done to a track most, each one press. */
	/* A track narrower than its name and two buttons wraps them: nothing of the title is cut. */
	.title {
		flex: none;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--space-1);
		min-width: 0;
	}
	/* The name keeps a width a finger meets, and takes what the buttons leave. */
	.title :global(.name) {
		flex: 1 1 var(--number-width);
		min-width: var(--number-width);
	}
	/* The record button while a recording runs. */
	.title :global(.live) {
		color: var(--danger);
		background: var(--danger-fill);
	}
	/* The room under the title: the groups that fit, one under the other. */
	.groups {
		flex: 1 1 0;
		display: flex;
		flex-direction: column;
		min-height: 0;
		overflow: hidden;
	}
	/* A group, parted by a line from the one before; a label in it is as narrow as its word. */
	.group {
		flex: none;
		padding: var(--space-1) 0;
		--field-label-min: 0px;
	}
	.group + .group {
		border-top: 1px solid var(--border);
	}
	/* The root is as wide as a note's name, and the scale takes the rest. */
	.group :global(.root),
	section :global(.root) {
		flex: 0 0 auto;
	}
	/* In the menu a group says its name, and has the menu's own margins. */
	section {
		padding: var(--space-3) var(--space-5);
		--field-label-min: 0px;
	}
	section + section {
		border-top: 1px solid var(--border);
	}
	h4 {
		margin: 0 0 var(--space-3);
		font-size: var(--fs-micro);
		font-weight: 600;
		letter-spacing: 0.04em;
		text-transform: uppercase;
		color: var(--text-dim);
	}
	.acts {
		display: flex;
		gap: var(--space-3);
	}
	/* A curve track's input: a dragged value it takes outlines it. */
	.input {
		flex: 1 1 auto;
		min-width: 0;
		cursor: text;
	}
	.input.armed {
		outline: 1px dashed var(--border-strong);
		outline-offset: -1px;
	}
</style>
