<!-- What a track says about itself, beside its lane: its name and the button to its menu, then its
     settings in named sections that fold. -->
<script lang="ts" module>
	import { GRIDS, NOTE_NAMES, SCALES } from '$lib/api/vocab';

	/** A MIDI input port the host lists, and the variable group it is read as. */
	export interface Port {
		port: string;
		group: string;
	}
	/** A pitch as a keyboard names it: C4 is 60. */
	export const pitchName = (pitch: number): string => `${NOTE_NAMES[pitch % 12]}${Math.floor(pitch / 12) - 1}`;
</script>

<script lang="ts">
	import type { Track } from '$lib/api/generated';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import { numberDescriptor } from '$lib/machine/attributes';
	import { Disclosure, Field, FieldGrid, Icon, IconButton, NumberInput, Select, TextInput } from '$lib/ui';

	let {
		id,
		t,
		recording,
		ports,
		onRescan,
		onEdit,
		onRecord,
		onMenu
	}: {
		id: string;
		t: Track;
		recording: boolean;
		/** The MIDI devices there are, for a note track's input. */
		ports: Port[];
		onRescan: () => void;
		/** Change the track's own settings: one `timeline track edit`. */
		onEdit: (patch: Record<string, unknown>) => void;
		onRecord: (e: MouseEvent) => void;
		/** Open the track's menu from its button. */
		onMenu: (e: MouseEvent) => void;
	} = $props();

	const rename = (to: string): void => {
		if (to !== t.name && isValidIdentifier(to)) onEdit({ to });
	};

	const TITLES = { record: 'Record', keyboard: 'Keyboard' };
	type Group = keyof typeof TITLES;
	const groups = $derived<Group[]>(['record', ...(t.kind === 'note' ? (['keyboard'] as const) : [])]);
	/** The recording's settings are set once, so its section begins folded. */
	const open = $state<Record<Group, boolean>>({ record: false, keyboard: true });
	/** A curve's input that is no expression: none, or a number alone. */
	const plain = $derived(t.input.trim() === '' || Number.isFinite(Number(t.input)));
	/** The devices a note track can record from: those there are, and the one it names. */
	const devices = $derived(['', ...new Set([...ports.map((p) => p.group), ...(t.input ? [t.input] : [])])]);
	const deviceNames = $derived(Object.fromEntries(devices.map((d) => [d, d === '' ? 'auto' : (ports.find((p) => p.group === d)?.port ?? `${d} (absent)`)])));
</script>

{#snippet group(name: Group)}
	{#if name === 'keyboard' && t.kind === 'note'}
		<Field label="root" row doc="The note the keyboard's scale starts on">
			<Select aria-label="Root" value={t.root} options={[...NOTE_NAMES]} onChange={(root) => onEdit({ root })} />
		</Field>
		<Field label="scale" row doc="The scale whose notes the keyboard marks and a note lands on">
			<Select aria-label="Scale" value={t.scale} options={Object.keys(SCALES)} onChange={(scale) => onEdit({ scale })} />
		</Field>
		<Field label="grid" row doc="The lines a note of this track snaps to when it is made or dragged; Shift or Ctrl lets it go free">
			<Select aria-label="Grid" value={t.grid} options={Object.keys(GRIDS)} onChange={(grid) => onEdit({ grid })} />
		</Field>
	{:else if name === 'record' && t.kind === 'note'}
		<Field label="input" row doc="The MIDI device a recording takes its notes from; auto is the one there is, or the one picked when it begins">
			<Select aria-label="Input" data-testid="track-device" value={t.input} options={devices} labels={deviceNames} onRefresh={onRescan} onChange={(input) => onEdit({ input })} />
		</Field>
	{:else if name === 'record' && t.kind === 'curve'}
		<!-- A param's source: an expression typed, learned from a MIDI control, or a node, a variable
		     or a state dropped on the row. A number alone is the row's constant, and none is no input. -->
		<div class="whole"><ParamField
			paramName="input"
			descriptor={numberDescriptor(plain ? Number(t.input) || 0 : 0, t.range[0], t.range[1], 0, plain ? '' : t.input)}
			viewKey={`track/${id}/input`}
			accept={(source) => source.key !== id}
			data-testid="track-input"
			onCommit={(v) => onEdit({ input: String(v) })}
			onSetSource={(source) => onEdit({ input: source.expression ?? (source.mode === 'constant' ? '' : t.input) })}
		/></div>
		<Field label="rate" row doc="The most samples a second a recording takes of the input; a node slower than that gives its own">
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

<div class="title">
	<TextInput class="name" aria-label="Track name" value={t.name} onChange={rename} />
	<IconButton variant="ghost" size="sm" label="Track menu" data-testid="track-more" onclick={onMenu}><Icon name="ellipsis" /></IconButton>
</div>
<div class="groups thin-scrollbar">
	{#each groups as name (name)}
		<Disclosure class="group" data-testid={`track-section-${name}`} stretched bind:open={open[name]} aside={name === 'record' ? recorder : undefined}>
			{#snippet summary()}{TITLES[name]}{/snippet}
			<FieldGrid>{@render group(name)}</FieldGrid>
		</Disclosure>
	{/each}
</div>

<style>
	.title {
		flex: none;
		display: flex;
		align-items: center;
		gap: var(--space-1);
		min-width: 0;
	}
	/* The name keeps a width a finger meets, and takes what the button leaves. */
	.title :global(.name) {
		flex: 1 1 var(--number-width);
		min-width: var(--number-width);
	}
	/* A param's own row is not a label and a control of the grid: it takes the grid's width. */
	.whole {
		grid-column: 1 / -1;
		min-width: 0;
	}
	.rec {
		display: inline-flex;
		margin-left: auto;
	}
	.groups :global(.live) {
		color: var(--danger);
		background: var(--danger-fill);
	}
	/* The sections, one under the other; a track too short for them scrolls them. */
	.groups {
		flex: 1 1 0;
		display: flex;
		flex-direction: column;
		min-height: 0;
		overflow: hidden auto;
	}
	/* A section reaches from one edge of what the track says to the other, so the ground its name's
	   hover lights goes edge to edge, as a param's own row does. */
	.groups > :global(.group) {
		flex: none;
		margin-inline: calc(0px - var(--head-pad, 0px));
		padding-inline: var(--head-pad, 0px);
		--disclosure-body-pad: 0 0 var(--space-2);
	}
</style>
