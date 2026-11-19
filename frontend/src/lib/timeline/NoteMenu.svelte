<!-- The settings of the notes picked in a track, in a menu that hangs from one of them: pitch,
     length and velocity, and the way to take the notes out. A right click on a note opens it, or
     a held finger. It shows the first note picked, and a change is every picked note's. -->
<script lang="ts">
	import type { Note, Track } from '$lib/api/generated';
	import { Button, Field, FieldGrid, Icon, NumberInput, Select, SettingsMenu } from '$lib/ui';
	import { pitchName } from './TrackHead.svelte';

	let {
		t,
		note,
		count,
		anchor,
		onChange,
		onDelete,
		onClose
	}: {
		t: Extract<Track, { kind: 'note' }>;
		/** The first note picked. */
		note: Note;
		/** How many notes are picked. */
		count: number;
		/** The note as the lane draws it. */
		anchor: HTMLElement;
		onChange: (to: Partial<Note>) => void;
		onDelete: () => void;
		onClose: () => void;
	} = $props();

	/** Every key of the track's keyboard, the highest first as the lane has them, and the note's
	 * own pitch where an op put it off the keyboard. */
	const pitches = $derived([...new Set([...Array.from({ length: t.range[1] - t.range[0] + 1 }, (_, i) => t.range[1] - i), note.pitch])].sort((a, b) => b - a));
</script>

<SettingsMenu label={count > 1 ? `${count} notes` : 'Note'} testid="note" {anchor} bind:open={() => true, (to) => void (to || onClose())}>
	<FieldGrid>
		<Field label="pitch" row doc="The key the note plays" data-menu-row>
			<Select aria-label="Pitch" value={String(note.pitch)} options={pitches.map(String)} labels={Object.fromEntries(pitches.map((p) => [String(p), pitchName(p)]))} onChange={(p) => onChange({ pitch: Number(p) })} />
		</Field>
		<Field label="length" row doc="How long the note lasts, in beats" data-menu-row>
			<NumberInput value={note.length} min={0.01} step={0.25} onChange={(length) => onChange({ length })} />
		</Field>
		<Field label="velocity" row doc="The note's velocity, 0 to 1" data-menu-row>
			<NumberInput value={note.velocity} min={0} max={1} step={0.05} onChange={(velocity) => onChange({ velocity })} />
		</Field>
		<div class="acts" data-menu-row>
			<Button size="sm" data-testid="note-delete" onclick={onDelete}><Icon name="x" /> {count > 1 ? `Delete ${count} notes` : 'Delete'}</Button>
		</div>
	</FieldGrid>
</SettingsMenu>

<style>
	.acts {
		grid-column: 1 / -1;
		display: flex;
	}
</style>
