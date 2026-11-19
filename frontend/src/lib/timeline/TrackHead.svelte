<!-- What a track says about itself, beside its lane: its name with hide and delete, then its
     settings in named sections that fold: the keyframe picked, the recording with its button on
     the section's own line, and the lane's own settings. The sections that do not fit in the room
     the track has go, whole, behind one menu in the title. A picked note's settings are not here:
     they are in a menu on the note. -->
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
	import type { Key, Track } from '$lib/api/generated';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import { numberDescriptor } from '$lib/machine/attributes';
	import { Button, CurvePicker, Disclosure, Field, FieldGrid, Icon, IconButton, NumberInput, Select, SettingsMenu, TextInput, fit } from '$lib/ui';

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
		onItem,
		onGrow
	}: {
		id: string;
		t: Track;
		/** The keyframe picked in the track, the first of them. */
		item?: Key;
		recording: boolean;
		/** The MIDI devices there are, for a note track's input. */
		ports: Port[];
		onRescan: () => void;
		/** Change the track's own settings: one `timeline track edit`. */
		onEdit: (patch: Record<string, unknown>) => void;
		onRecord: (e: MouseEvent) => void;
		onHide: () => void;
		onDelete: () => void;
		/** Change the keyframes picked, or with `null` take them out. */
		onItem: (to: Partial<Key> | null) => void;
		/** Ask for this much more room across the axis, in pixels; answers whether the track grows. */
		onGrow: (by: number) => boolean;
	} = $props();

	const rename = (to: string): void => {
		if (to !== t.name && isValidIdentifier(to)) onEdit({ to });
	};

	/** A track's sections of settings, in the order they stand: the keyframe picked first, while
	 * one is, then the recording, then the lane's own. */
	const TITLES = { item: 'Selection', record: 'Record', keyboard: 'Keyboard', range: 'Range' };
	type Group = keyof typeof TITLES;
	const groups = $derived<Group[]>([...(item ? (['item'] as const) : []), 'record', t.kind === 'note' ? 'keyboard' : 'range']);
	/** The sections that are unfolded: the recording's settings are set once, so it begins folded. */
	const open = $state<Record<Group, boolean>>({ item: true, record: false, keyboard: true, range: true });
	/** A section was unfolded by hand and its room is not yet found: until the page has laid the
	 * section out, which a measure of the room follows at once. */
	let unfolded = false;
	function unfold(name: Group, to: boolean): void {
		open[name] = to;
		unfolded = to;
		if (to) requestAnimationFrame(() => (unfolded = false));
	}
	let room = $state<HTMLElement | null>(null);
	/** The sections with no room beside the lane. One that was just unfolded by hand asks the
	 * track to grow for it first: a section does not leave for the menu because it was opened. */
	function fitted(hidden: string[]): void {
		const parts = unfolded && room ? [...room.querySelectorAll<HTMLElement>(':scope > [data-fit]')].filter((part) => part.style.display === 'none') : [];
		unfolded = false;
		for (const part of parts) part.style.display = '';
		const short = room ? room.scrollHeight - room.clientHeight : 0;
		for (const part of parts) part.style.display = 'none';
		if (!(short > 0 && onGrow(short))) more = hidden;
	}
	/** A curve's input that is no expression: none, or a number alone. */
	const plain = $derived(t.input.trim() === '' || Number.isFinite(Number(t.input)));
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
			tight = el.clientWidth < parseFloat(getComputedStyle(el.querySelector('.name')!).minWidth) + 3 * button;
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
		<Field label="value" row doc="The keyframe's value" data-menu-row>
			<NumberInput value={item.value} min={t.range[0]} max={t.range[1]} step={0.01} onChange={(value) => onItem({ value })} />
			{#snippet adornment()}
				<IconButton variant="ghost" size="sm" label="Delete keyframe" data-testid="track-delete-picked" onclick={() => onItem(null)}><Icon name="x" /></IconButton>
			{/snippet}
		</Field>
		<Field label="curve" row doc="How the value eases from this keyframe to the next" data-menu-row>
			<CurvePicker value={item.curve} onChange={(curve) => onItem({ curve })} />
		</Field>
	{:else if name === 'keyboard' && t.kind === 'note'}
		<Field label="keys" row doc="The lowest and the highest pitch the keyboard shows" data-menu-row>
			<Select aria-label="Lowest" value={String(t.range[0])} options={PITCHES.slice(0, t.range[1])} labels={PITCH_NAMES} onChange={(p) => onEdit({ range: [Number(p), t.range[1]] })} />
			<Select aria-label="Highest" value={String(t.range[1])} options={PITCHES.slice(t.range[0] + 1)} labels={PITCH_NAMES} onChange={(p) => onEdit({ range: [t.range[0], Number(p)] })} />
		</Field>
		<Field label="scale" row doc="The scale whose notes the keyboard marks and a note lands on, and the note it starts on" data-menu-row>
			<Select aria-label="Root" class="root" value={t.root} options={[...NOTE_NAMES]} onChange={(root) => onEdit({ root })} />
			<Select aria-label="Scale" value={t.scale} options={Object.keys(SCALES)} onChange={(scale) => onEdit({ scale })} />
		</Field>
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
		<!-- What a recording takes is a param's source: an expression typed, learned from a MIDI
		     control, or a node, a variable or a state dropped on the row. A number alone is the
		     row's constant, and none is no input. -->
		<div class="whole"><ParamField
			paramName="input"
			descriptor={numberDescriptor(plain ? Number(t.input) || 0 : 0, t.range[0], t.range[1], 0, plain ? '' : t.input)}
			viewKey={`track/${id}/input`}
			accept={(source) => source.key !== id}
			data-testid="track-input"
			data-menu-row
			onCommit={(v) => onEdit({ input: String(v) })}
			onSetSource={(source) => onEdit({ input: source.expression ?? (source.mode === 'constant' ? '' : t.input) })}
		/></div>
		<Field label="rate" row doc="The most samples a second a recording takes of the input; a node slower than that gives its own" data-menu-row>
			<NumberInput aria-label="Rate" data-testid="track-rate" value={t.rate} min={1} max={120} step={1} onChange={(rate) => onEdit({ rate })} />
			{#snippet adornment()}Hz{/snippet}
		</Field>
	{/if}
{/snippet}

<!-- The recording's own button: on its section's line, folded or not. -->
{#snippet recorder()}
	<span class="rec"><IconButton
		variant="ghost"
		size="sm"
		class={recording ? 'live' : ''}
		label="Record"
		title={recording ? 'End the recording: what it took goes into the track' : `Record ${t.kind === 'note' ? 'the keys of a MIDI device' : 'the input'} into the track while the clock plays`}
		aria-pressed={recording}
		data-testid="track-record"
		onclick={onRecord}><Icon name="circle-dot" /></IconButton
	></span>
{/snippet}

<div class="title" bind:this={title}>
	<TextInput class="name" aria-label="Track name" value={t.name} onChange={rename} />
	{#if more.length > 0 || tight}
		<SettingsMenu label="More" icon="ellipsis" testid="track-more" bind:open={menu}>
			{#each groups.filter((name) => more.includes(name)) as name (name)}
				<section aria-label={TITLES[name]}>
					<h4>{TITLES[name]}{#if name === 'record'}{@render recorder()}{/if}</h4>
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
<div class="groups" bind:this={room} use:fit={fitted}>
	{#each groups as name (name)}
		<Disclosure class="group" data-fit={name} data-testid={`track-section-${name}`} stretched bind:open={() => open[name], (to) => unfold(name, to)} aside={name === 'record' ? recorder : undefined}>
			{#snippet summary()}{TITLES[name]}{/snippet}
			<FieldGrid {stacked}>{@render group(name)}</FieldGrid>
		</Disclosure>
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
	/* A param's own row is not a label and a control of the grid: it takes the grid's width. */
	.whole {
		grid-column: 1 / -1;
		min-width: 0;
	}
	/* The record button stands at the end of its section's line. */
	.rec {
		display: inline-flex;
		margin-left: auto;
	}
	/* The record button while a recording runs. */
	.groups :global(.live),
	h4 :global(.live) {
		color: var(--danger);
		background: var(--danger-fill);
	}
	/* The room under the title: the groups that fit, one under the other. */
	/* It cuts what does not fit below, and not the focus ring of a control at its side. */
	.groups {
		flex: 1 1 0;
		display: flex;
		flex-direction: column;
		min-height: 0;
		overflow-x: visible;
		overflow-y: clip;
	}
	/* A section under its own name; a label in it is as narrow as its word, and its rows stand
	   under the name with no margin of their own. The section is as wide as what the track says
	   about itself, so the ground its name's hover lights goes from edge to edge, as a param's
	   own row does. */
	.groups > :global(.group) {
		flex: none;
		margin-inline: calc(0px - var(--head-pad, 0px));
		padding-inline: var(--head-pad, 0px);
		--field-label-min: 0px;
		--disclosure-body-pad: 0 0 var(--space-2);
	}
	/* The root is as wide as a note's name, and the scale takes the rest. */
	.groups :global(.root),
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
		display: flex;
		align-items: center;
		justify-content: space-between;
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
</style>
