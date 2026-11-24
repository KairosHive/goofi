<!-- The settings of the notes or the keyframes picked in a track, in a popover on one of them: a
     note's pitch, length and velocity, a keyframe's value and curve. It shows the first one picked,
     and a change is every picked one's. Here too are the way to copy what is picked, and the way
     to take a keyframe out: a click takes a note out, and picks a keyframe. -->
<script lang="ts">
	import type { Key, Note, Track } from '$lib/api/generated';
	import { Button, CurvePicker, Field, FieldGrid, Icon, NumberInput, Popover, Select } from '$lib/ui';
	import { shapeOf } from './model';
	import { pitchName } from './TrackHead.svelte';

	let {
		t,
		range,
		item,
		count,
		anchor,
		onChange,
		onCopy,
		onDelete,
		onClose
	}: {
		t: Track;
		/** What the lane shows across its axis: the keyboard's pitches, or the curve's range. */
		range: [number, number];
		/** The first note or keyframe picked. */
		item: Note | Key;
		/** How many are picked. */
		count: number;
		/** The note or the keyframe as the lane draws it. */
		anchor: HTMLElement;
		onChange: (to: Partial<Note & Key>) => void;
		onCopy: () => void;
		onDelete: () => void;
		onClose: () => void;
	} = $props();

	const what = $derived(t.kind === 'note' ? 'note' : 'keyframe');
	/** Every key of a note track's keyboard, the highest first as the lane has them, and the
	 * note's own pitch where an op put it off the keyboard. */
	const pitches = $derived('pitch' in item ? [...new Set([...Array.from({ length: range[1] - range[0] + 1 }, (_, i) => range[1] - i), item.pitch])].sort((a, b) => b - a) : []);
</script>

<Popover {anchor} open onDismiss={onClose} role="group" aria-label={count > 1 ? `${count} ${what}s` : what} style="--popover-radius: var(--radius-sm)" data-testid="picked-menu">
	<div class="body"><FieldGrid>
		{#if 'pitch' in item}
			<Field label="pitch" row doc="The key the note plays">
				<Select aria-label="Pitch" value={String(item.pitch)} options={pitches.map(String)} labels={Object.fromEntries(pitches.map((p) => [String(p), pitchName(p)]))} onChange={(p) => onChange({ pitch: Number(p) })} />
			</Field>
			<Field label="length" row doc="How long the note lasts, in beats">
				<NumberInput value={item.length} min={0.01} step={0.25} onChange={(length) => onChange({ length })} />
			</Field>
			<Field label="velocity" row doc="The note's velocity, 0 to 1">
				<NumberInput value={item.velocity} min={0} max={1} step={0.05} onChange={(velocity) => onChange({ velocity })} />
			</Field>
		{:else}
			<Field label="value" row doc="The keyframe's value">
				<NumberInput value={item.value} min={range[0]} max={range[1]} step={0.01} onChange={(value) => onChange({ value })} />
			</Field>
			<Field label="curve" row doc="How the value eases from this keyframe to the next">
				<CurvePicker value={item.curve} smooth={t.kind === 'curve' ? shapeOf(t.keys, Math.max(0, t.keys.indexOf(item))) : undefined} onChange={(curve) => onChange({ curve })} />
			</Field>
		{/if}
		<div class="acts">
			<Button size="sm" data-testid="picked-copy" onclick={onCopy}><Icon name="copy" /> Copy</Button>
			{#if !('pitch' in item)}
				<Button size="sm" data-testid="picked-delete" onclick={onDelete}><Icon name="x" /> {count > 1 ? `Delete ${count} keyframes` : 'Delete'}</Button>
			{/if}
		</div>
	</FieldGrid></div>
</Popover>

<style>
	.body {
		width: 14rem;
	}
	.acts {
		grid-column: 1 / -1;
		display: flex;
		gap: var(--space-2);
	}
</style>
