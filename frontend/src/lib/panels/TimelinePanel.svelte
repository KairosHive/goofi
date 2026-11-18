<!-- Timeline panel: every track of the patch on one time axis, each with what it says about itself
     beside its lane — a note track as a piano roll, a curve track as one line through its
     keyframes — under the clock they play on. A panel wider than tall stacks the tracks as rows
     and runs the time to the right; a panel taller than wide stands them as columns and runs the
     time down. The tracks are the patch's; what a panel hides and how large it draws a track
     are its own state; what part of the axis it looks at is no state at all.
     Every change is a `timeline` or a `clock` op or a clock variable, so the manager owns the
     model and this panel the drawing and the gesture. -->
<script lang="ts">
	import { ContextMenu, MOVE_TOLERANCE_PX, asStateObject, beginDrag, createLongPress, type MenuItem, type PanelProps } from 'panelty';
	import { flushSync, untrack } from 'svelte';
	import type { Key, Note, Track } from '$lib/api/generated';
	import { GRIDS, NOTE_NAMES, SCALES } from '$lib/api/vocab';
	import { draggable, type DragSource, type Outcome } from '$lib/drag';
	import { graph } from '$lib/stores/graph.svelte';
	import { history } from '$lib/stores/history.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { quickPick } from '$lib/stores/quickPick.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { beatSeconds, contentEnd, gridPath, hiding, lanePath, ticks } from '$lib/timeline/model';
	import RangeBar from '$lib/timeline/RangeBar.svelte';
	import TrackHead, { type Port } from '$lib/timeline/TrackHead.svelte';
	import { Bar, Button, ConfirmDialog, EmptyState, Icon, IconButton, NumberInput, Segmented } from '$lib/ui';

	interface TimelineState {
		/** The ids of the tracks this panel hides. */
		hidden?: string[];
		/** How large this panel draws a track across the axis, in pixels, by the track's id. */
		sizes?: Record<string, number>;
	}

	let { state: panelState, setState }: PanelProps = $props();
	const g = graph();
	const own = $derived(asStateObject(panelState) as TimelineState);
	const hidden = $derived(own.hidden ?? []);
	const shown = $derived(Object.keys(g.timeline).filter((id) => !hidden.includes(id)));
	// An id the timeline does not hold is a removed track's: the panel passes over it.
	const stowed = $derived(hidden.filter((id) => id in g.timeline));
	const fail = (what: string) => (e: unknown) => notify().failure(what, e);
	const fixed = (v: number): number => Number(v.toFixed(3));
	/** A time as it is stored: free, to the millionth of a beat, so a line of a fine grid is itself. */
	const exact = (v: number): number => Number(v.toFixed(6));
	const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
	const share = (f: number): string => `${100 * f}%`;
	let [wide, tall] = $state([0, 0]);
	/** A panel taller than wide runs the time down and stands the tracks beside each other. */
	const vertical = $derived(tall > wide);

	// ---- the clock: its variables read live, `clock set` to steer it, its loop a section of the axis.
	watchVariables(() => [...['time', 'playing', 'bpm', 'length'].map((n) => `clock.${n}`), ...Object.values(g.timeline).map((t) => `${t.name}.recording`)]);
	function clock(name: string): number {
		const v = variableValue(`clock.${name}`);
		return typeof v === 'number' ? v : 0;
	}
	const playing = $derived(clock('playing') > 0);
	/** The seconds a beat lasts: the axis counts beats, the clock seconds. */
	const spb = $derived(beatSeconds(clock('bpm')));
	/** The clock's position, in beats. */
	const beat = $derived(clock('time') / spb);
	/** The section the clock loops in, in seconds as the clock holds it: the clock's own, and no
	 * variable. In beats it is `loop`. */
	let looped = $state<[number, number] | null>(null);
	const loop = $derived<[number, number] | null>(looped && [looped[0] / spb, looped[1] / spb]);
	const steer = (to: { playing?: boolean; time?: number }): void => void g.op('clock set', to).catch(fail('Set clock'));
	/** Mark the section the clock loops in, from and to in beats; with no length, clear it. */
	const setLoop = (from: number, to: number): void =>
		void g
			.op<{ section: [number, number] | null }>('clock set', { section: [from * spb, to * spb] })
			.then((r) => (looped = r.section))
			.catch(fail('Set loop'));
	// What the clock loops in now: a `clock set` that sets nothing says it.
	void g
		.op<{ section: [number, number] | null }>('clock set', {})
		.then((r) => (looped = r.section))
		.catch(() => {});

	// ---- the view: the beats of the axis this panel looks at. It stays where it is put: the
	// clock runs out of it, and a double click on the ruler brings back everything there is.
	/** Where everything there is ends, in beats: the timeline's length, or without one the tracks'
	 * content with room after it. */
	const reach = (): number => (clock('length') > 0 ? clock('length') : Math.max(8, contentEnd(Object.values(g.timeline)) * 1.25));
	let view = $state<[number, number]>([0, untrack(reach)]);
	const width = $derived(view[1] - view[0]);
	const within = (seconds: number, [from, to]: [number, number]): boolean => seconds >= from && seconds <= to;
	/** Where a second falls on the axis: 0 at the start of the view, 1 at its end. */
	const along = (seconds: number): number => (seconds - view[0]) / width;
	/** A pointer over a lane: how far along the axis, and how high across it, 0 the lowest. */
	function point(p: { clientX: number; clientY: number }, el: Element): [number, number] {
		const box = el.getBoundingClientRect();
		const [x, y] = [(p.clientX - box.left) / box.width, (p.clientY - box.top) / box.height];
		return vertical ? [y, x] : [x, 1 - y];
	}
	/** Where what sits `low` up a lane, `size` high, begins across it: a row counts from its top,
	 * a column from its left. */
	const across = (low: number, size = 0): string => share(vertical ? low : 1 - low - size);
	/** What the range bar stands for: the view, and everything there is to see. */
	const total = $derived<[number, number]>([Math.min(0, view[0] - width / 10), Math.max(view[1] + width / 10, reach())]);

	/** A press on the ruler puts the clock there, and outside the loop's section clears it; a drag
	 * along it marks the section the clock loops in. A double click looks at the section it is on,
	 * or at everything there is. */
	let section = $state<[number, number] | null>(null);
	function onRuler(e: PointerEvent): void {
		const lane = e.currentTarget as HTMLElement;
		const at = (p: { clientX: number; clientY: number }): number => Math.max(0, view[0] + point(p, lane)[0] * width);
		const from = at(e);
		drag = beginDrag(lane, e.pointerId, {
			move: (p) => {
				if (section || Math.hypot(p.clientX - e.clientX, p.clientY - e.clientY) >= MOVE_TOLERANCE_PX) section = [Math.min(from, at(p)), Math.max(from, at(p))];
			},
			commit: () => {
				// A section too short to hear is a press that slipped.
				if (section && section[1] - section[0] >= 0.05) setLoop(section[0], section[1]);
				else {
					steer({ time: from * spb });
					if (loop && !within(from, loop)) setLoop(0, 0);
				}
				section = null;
			},
			cancel: () => (section = null)
		});
	}
	const band = $derived(section ?? loop);
	const onRulerTwice = (e: MouseEvent): void => {
		const at = view[0] + point(e, e.currentTarget as Element)[0] * width;
		view = loop && within(at, loop) ? [loop[0], loop[1]] : [0, reach()];
	};

	/** The beats between two lines of a note track's grid. */
	const gridOf = (t: Extract<Track, { kind: 'note' }>): number => Math.max(0.001, GRIDS[t.grid][1] ? GRIDS[t.grid][0] : GRIDS[t.grid][0] / spb);
	/** A time in beats on the nearest line of a grid, as `round` rounds. */
	const onGrid = (beats: number, grid: number, round = Math.round): number => round(beats / grid + 1e-9) * grid;

	// ---- the tracks: an edit is one op on the track; what a panel hides and how large it draws a
	// track are the panel's state.
	const edit = (track: string, patch: Record<string, unknown>): void => void g.op('timeline track edit', { track, ...patch }).catch(fail('Edit track'));
	const add = (kind: Track['kind']): void => void g.op('timeline track add', { kind }).catch(fail('Add track'));
	const hide = (ids: string[], label: string): void => setState({ hidden: ids }, 'authored', label);
	/** The track asked to be deleted, until the dialog answers or the track goes. */
	let asked = $state('');
	const doomed = $derived(g.timeline[asked]);
	$effect(() => {
		if (!doomed) asked = '';
	});

	let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function trackMenu(id: string, x: number, y: number): void {
		const entry = (which: Parameters<typeof hiding>[2], label: string): MenuItem => {
			const more = hiding(shown, id, which);
			return { label, disabled: more.length === 0, action: () => hide([...hidden, ...more], label) };
		};
		const items = [entry('this', 'Hide Track'), entry('above', 'Hide Tracks Above'), entry('below', 'Hide Tracks Below'), entry('others', 'Hide Other Tracks')];
		menu = { x, y, items: [...items, { separator: true }, { label: 'Delete Track…', icon: 'x', action: () => (asked = id) }] };
	}
	/** A menu that drops from the button that opened it. */
	function drop(e: MouseEvent, items: MenuItem[]): void {
		const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
		menu = { x: box.left, y: box.bottom, items };
	}
	const addMenu = (e: MouseEvent): void => drop(e, [{ label: 'Note Track', action: () => add('note') }, { label: 'Curve Track', action: () => add('curve') }]);
	// The touch door onto a track's menu.
	let pressed = '';
	const press = createLongPress((at) => trackMenu(pressed, at.clientX, at.clientY));

	// ---- a recording: a track takes its input in at the clock's position until it is ended. The
	// stepping thread says whether one runs, in the track's own group.
	const recording = (t: Track): boolean => variableValue(`${t.name}.recording`) === 1;
	/** The MIDI devices the host lists: read when the panel opens, and again when asked. */
	let ports = $state<Port[]>([]);
	const rescan = async (): Promise<Port[]> => (ports = (await g.op<{ ports: Port[] }>('midi list', {})).ports);
	void rescan().catch(fail('List MIDI devices'));
	/** The MIDI device a note track records from: the one it names while that is there, else the
	 * one there is, else one picked at `at` from those there are, or none where the pick is let go. */
	async function device(t: Track, at: { x: number; y: number }): Promise<string | null> {
		const found = await rescan();
		if (found.length === 0) throw new Error('no MIDI device is connected');
		if (found.some((p) => p.group === t.input)) return t.input;
		if (found.length === 1) return found[0].group;
		return quickPick().ask(at, found.map((p) => ({ id: p.group, label: p.port })), `Record ${t.name} from`);
	}
	/** Begin a recording into the track, or end the one that runs. A paused clock plays for it. */
	async function record(e: MouseEvent, id: string): Promise<void> {
		const t = g.timeline[id];
		if (recording(t)) return void (await g.op('timeline track record', { track: id, on: false }));
		const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
		const input = t.kind === 'note' ? await device(t, { x: box.left, y: box.bottom }) : t.input;
		if (input === null) return;
		if (input !== t.input) await g.op('timeline track edit', { track: id, input });
		await g.op('timeline track record', { track: id, on: true });
		if (!playing) steer({ playing: true });
	}

	// ---- a track lifted by what it says about itself: let go on another track it takes that
	// track's place, and on a param it is the expression that reads it.
	let stack = $state<HTMLElement | null>(null);
	function lift(id: string): DragSource {
		const t = g.timeline[id];
		const reads = `vars.${t.name}.${t.kind === 'curve' ? 'value' : 'notes'}`;
		return { key: id, tags: ['number'], label: reads, home: stack, expression: () => reads };
	}
	function place(id: string, outcome: Outcome | null): void {
		const onto = outcome?.inHome ? document.elementFromPoint(outcome.at.x, outcome.at.y)?.closest<HTMLElement>('[data-track]')?.dataset.track : undefined;
		if (!onto || onto === id) return;
		// Onto a track further on it goes after it, and before one that comes first.
		const before = shown.indexOf(onto) > shown.indexOf(id) ? shown[shown.indexOf(onto) + 1] : onto;
		void g.op('timeline track move', { track: id, before }).catch(fail('Move track'));
	}

	/** A track's edge dragged: how large this panel draws it, and nothing of the track. Held at an
	 * end of the track area, the area scrolls and the edge goes with it, so a track is as large as
	 * it is dragged, whatever the panel shows of it. */
	let sizing = $state<{ id: string; size: number; sent?: boolean } | null>(null);
	const [SMALLEST, EDGE, FASTEST] = [32, 32, 16];
	function resize(e: PointerEvent, id: string): void {
		e.stopPropagation();
		const grip = e.currentTarget as HTMLElement;
		const area = grip.closest<HTMLElement>('.scroll')!;
		const lane = grip.parentElement!.querySelector('.lane')!.getBoundingClientRect();
		const [client, scroll, from, to] = vertical ? (['clientX', 'scrollLeft', 'left', 'right'] as const) : (['clientY', 'scrollTop', 'top', 'bottom'] as const);
		let [size, at, frame] = [vertical ? lane.width : lane.height, e[client], 0];
		const show = (): void => void (sizing = { id, size: Math.max(SMALLEST, Math.round(size)) });
		// The pixels a frame the area scrolls by: none in its middle, more the further into an end.
		const pace = (): number => {
			const box = area.getBoundingClientRect();
			const into = Math.min(0, at - box[from] - EDGE) + Math.max(0, at - box[to] + EDGE);
			return Math.sign(into) * Math.min(FASTEST, Math.ceil(Math.abs(into) / 4));
		};
		const creep = (): void => {
			const by = pace();
			frame = 0;
			if (by === 0 || (by < 0 && size <= SMALLEST)) return;
			size += by;
			show();
			// The track is that much larger before the area scrolls by as much: the edge stays
			// under the pointer. What the area could not scroll, the track gives back.
			flushSync();
			const was = area[scroll];
			area[scroll] += by;
			size -= by - (area[scroll] - was);
			show();
			frame = requestAnimationFrame(creep);
		};
		const end = (): void => cancelAnimationFrame(frame);
		drag = beginDrag(grip, e.pointerId, {
			move: (p) => {
				size += p[client] - at;
				at = p[client];
				show();
				if (!frame) frame = requestAnimationFrame(creep);
			},
			commit: () => {
				end();
				if (!sizing) return;
				sizing.sent = true;
				setState({ sizes: { ...own.sizes, [id]: sizing.size } }, 'authored', 'Resize Track');
			},
			cancel: () => {
				end();
				sizing = null;
			}
		});
	}
	// The size dragged stands until the panel's state holds it.
	$effect(() => {
		void own.sizes;
		if (untrack(() => sizing?.sent)) sizing = null;
	});

	// ---- the notes and the keyframes.
	let picked = $state<{ track: string; index: number }[]>([]);
	/** The places in the track `id` of what is picked there. */
	const pickedIn = (id: string): number[] => picked.filter((p) => p.track === id).map((p) => p.index);
	/** Pick notes and keyframes, or none. The focus leaves first, so a field that shows what was
	 * picked before commits to that. */
	function pick(next: typeof picked): void {
		(document.activeElement as HTMLElement | null)?.blur();
		picked = next;
	}
	const held = (t: Track | undefined): (Note | Key)[] => (t?.kind === 'note' ? t.notes : (t?.keys ?? []));
	const put = (t: Track, items: unknown[]): Record<string, unknown> => (t.kind === 'note' ? { notes: items } : { keys: items });
	type NoteTrack = Extract<Track, { kind: 'note' }>;
	/** Whether a pitch is a note of the scale a note track's keyboard marks. */
	const inScale = (t: NoteTrack, pitch: number): boolean => (SCALES[t.scale] ?? []).includes((((pitch - NOTE_NAMES.indexOf(t.root as never)) % 12) + 12) % 12);
	/** The note of the scale nearest `pitch`, on the keyboard. */
	function snap(t: NoteTrack, pitch: number): number {
		const [lo, hi] = t.range;
		const notes = Array.from({ length: hi - lo + 1 }, (_, i) => lo + i).filter((p) => inScale(t, p));
		return notes.reduce((best, p) => (Math.abs(p - pitch) < Math.abs(best - pitch) ? p : best), notes[0] ?? clamp(pitch, lo, hi));
	}
	/** The note or keyframe a drag holds: the drag ends when that one leaves its lane. */
	let dragged = $state<{ track: string; index: number } | null>(null);
	/** Change every note or keyframe picked in a track, or with null remove them. */
	function change(id: string, t: Track, to: Partial<Note & Key> | null): void {
		const indices = pickedIn(id);
		if (!to) picked = picked.filter((p) => p.track !== id);
		edit(id, put(t, held(t).flatMap((o, k) => (!indices.includes(k) ? [o] : to ? [{ ...o, ...to }] : []))));
	}
	/** The cell of a note lane under a pointer: where it begins, and the note of the scale it is on. */
	function cellAt(p: { clientX: number; clientY: number }, ground: Element, t: NoteTrack): { at: number; pitch: number } {
		const [f, v] = point(p, ground);
		const [lo, hi] = t.range;
		return { at: exact(onGrid(Math.max(0, view[0] + f * width), gridOf(t), Math.floor)), pitch: snap(t, lo + Math.floor(v * (hi - lo + 1))) };
	}
	/** The cell the pointer is over, to light it. */
	let over = $state<{ track: string; at: number; pitch: number } | null>(null);
	/** A click on a lane's own ground adds a note in that cell, or a keyframe there. The note
	 * added is the one picked. */
	function addAt(e: MouseEvent, id: string, t: Track): void {
		if (e.target !== e.currentTarget) return;
		const [f, v] = point(e, e.currentTarget as Element);
		const [lo, hi] = t.range;
		const born = t.kind === 'note' ? { ...cellAt(e, e.currentTarget as Element, t), length: exact(gridOf(t)), velocity: 0.8 } : { at: exact(Math.max(0, view[0] + f * width)), value: fixed(clamp(lo + v * (hi - lo), lo, hi)), curve: 'linear' };
		if (t.kind === 'note') picked = [{ track: id, index: t.notes.length }];
		edit(id, put(t, [...held(t), born]));
	}
	/** A drag of a note or a keyframe, and with it of everything picked, by as much as the pointer
	 * moves, a note onto the grid: each move previews every lane it touches as it stands with those
	 * moved, and the release sends the last of them as the edit. `stretch` drags one note's end and
	 * not the note. A press that does not move picks a keyframe, and takes a note out. Shift lets a
	 * note go off its grid, and Ctrl off its scale. */
	let drag: (() => void) | undefined;
	function dragItem(e: PointerEvent, id: string, t: Track, index: number, stretch = false): void {
		e.stopPropagation();
		if (stretch || !pickedIn(id).includes(index)) pick([{ track: id, index }]);
		const ground = (e.currentTarget as HTMLElement).closest('.ground')!;
		const [f0, v0] = point(e, ground);
		const rows = t.range[1] - t.range[0] + 1;
		// Every track the drag moves something of, with what its lane held at the press and what
		// of it moves. The lane is taken now: a preview puts a new one in the track's record.
		const tracks = [...new Set(picked.map((p) => p.track))].map((track) => ({ track, was: g.timeline[track], all: held(g.timeline[track]), indices: pickedIn(track) }));
		// What is picked moves as one: none of it goes before 0, and a keyframe goes no further
		// than the nearest keyframe the drag leaves, so keyframes keep their order.
		let [least, most] = [-Infinity, Infinity];
		for (const { was, all, indices } of tracks) {
			for (const k of indices) {
				const [before, after] = was.kind === 'curve' ? [all[k - 1], all[k + 1]] : [];
				least = Math.max(least, (before && !indices.includes(k - 1) ? before.at + 0.001 : 0) - all[k].at);
				if (after && !indices.includes(k + 1)) most = Math.min(most, after.at - 0.001 - all[k].at);
			}
		}
		/** Each lane as it stands, with what the drag holds moved `by` beats and `dv` of the pressed
		 * lane across. A note lands on its grid in time and on its scale in pitch, unless `free`
		 * says a key is held that lets it go where the pointer is; a note the drag does not move in
		 * time or in pitch keeps the one it has, on the grid or off it. Across, only what is of the
		 * pressed lane's kind moves: a pitch is no value. */
		const moved = (by: number, dv: number, free: { time: boolean; pitch: boolean }): [string, Record<string, unknown>][] =>
			tracks.map(({ track, was, all, indices }) => [
				track,
				put(was, held(g.timeline[track]).map((it, k) => {
					const o = all[k];
					if (!indices.includes(k) || !o) return it;
					const shift = clamp(by, least, most);
					if (was.kind === 'curve') return { ...it, at: exact(o.at + shift), value: t.kind === 'curve' ? fixed(clamp((o as Key).value + dv * (was.range[1] - was.range[0]), ...was.range)) : (o as Key).value };
					const [n, grid] = [o as Note, gridOf(was)];
					// A time moved by `d` beats: where the pointer is when free, else on the grid, and
					// where it was while the move is less than half a cell.
					const on = (from: number, d: number): number => (free.time ? from + d : onGrid(d, grid) === 0 ? from : onGrid(from + d, grid));
					if (stretch) return { ...it, length: exact(on(n.at + n.length, by) === n.at + n.length ? n.length : Math.max(free.time ? 0.01 : grid, on(n.at + n.length, by) - n.at)) };
					const semitones = t.kind === 'note' ? Math.round(dv * rows) : 0;
					return { ...it, at: exact(on(n.at, shift)), pitch: semitones === 0 ? n.pitch : free.pitch ? clamp(n.pitch + semitones, ...was.range) : snap(was, n.pitch + semitones) };
				}))
			]);
		let patches: [string, Record<string, unknown>][] | null = null;
		drag = beginDrag(e.currentTarget as HTMLElement, e.pointerId, {
			move: (p) => {
				if (!patches && Math.hypot(p.clientX - e.clientX, p.clientY - e.clientY) < MOVE_TOLERANCE_PX) return;
				dragged ??= { track: id, index };
				const [f, v] = point(p, ground);
				// Shift lets a note go off the grid in time, Ctrl off the scale in pitch; either lets
				// a note's end go off the grid.
				const ctrl = p.ctrlKey || p.metaKey;
				patches = moved((f - f0) * width, v - v0, { time: p.shiftKey || (stretch && ctrl), pitch: ctrl });
				for (const [track, patch] of patches) g.previewTrack(track, patch);
			},
			commit: () => {
				dragged = null;
				const all = patches;
				if (all?.length === 1) edit(...all[0]);
				else if (all) void history().transaction('Move notes and keyframes', async (step) => {
					for (const [track, patch] of all) await g.op('timeline track edit', { track, ...patch }, step);
				}).catch(fail('Edit track'));
				else if (t.kind === 'note') {
					picked = [{ track: id, index }];
					change(id, g.timeline[id] ?? t, null);
				}
			},
			// Put back, where the track is still there.
			cancel: () => {
				dragged = null;
				if (patches) for (const { track, was, all } of tracks) if (g.timeline[track]) g.previewTrack(track, put(was, all));
			}
		});
	}
	/** A drag with Shift held draws a box: every note and keyframe it touches is picked. */
	let box = $state<{ left: number; top: number; width: number; height: number } | null>(null);
	function boxPick(e: PointerEvent): void {
		// On a lane's own ground alone: Shift on a note is a drag off the grid, and Shift anywhere
		// else is not this panel's.
		const on = e.target as Element;
		if (!e.shiftKey || !stack || !on.closest('.ground') || on.closest('[data-item]')) return;
		e.stopPropagation();
		const home = stack;
		const origin = home.getBoundingClientRect();
		const drawn = (p: { clientX: number; clientY: number }): NonNullable<typeof box> => ({ left: Math.min(e.clientX, p.clientX), top: Math.min(e.clientY, p.clientY), width: Math.abs(p.clientX - e.clientX), height: Math.abs(p.clientY - e.clientY) });
		let last = drawn(e);
		drag = beginDrag(home, e.pointerId, {
			move: (p) => {
				last = drawn(p);
				box = { ...last, left: last.left - origin.left, top: last.top - origin.top };
			},
			commit: () => {
				box = null;
				const touched = [...home.querySelectorAll<HTMLElement>('[data-item]')].filter((el) => {
					const b = el.getBoundingClientRect();
					return b.left < last.left + last.width && b.right > last.left && b.top < last.top + last.height && b.bottom > last.top;
				});
				pick(touched.map((el) => ({ track: el.closest<HTMLElement>('[data-track]')!.dataset.track!, index: Number(el.dataset.item) })));
			},
			cancel: () => (box = null)
		});
	}
	// A drag ends with the panel, and with the note or keyframe it holds.
	$effect(() => {
		if (dragged && !held(g.timeline[dragged.track])[dragged.index]) drag?.();
	});
	$effect(() => () => {
		press.cancel();
		drag?.();
	});
</script>

<div class="timeline" class:vertical bind:clientWidth={wide} bind:clientHeight={tall} data-testid="timeline-panel">
	<!-- One group that wraps: a narrow panel puts the controls on as many lines as they take. -->
	<Bar>
		{#snippet start()}
			<div class="tools">
			<IconButton variant="ghost" label={playing ? 'Pause' : 'Play'} data-testid="clock-play" onclick={() => steer({ playing: !playing })}><Icon name={playing ? 'pause' : 'play'} /></IconButton>
			<IconButton variant="ghost" label="Stop" data-testid="clock-stop" onclick={() => steer({ playing: false, time: 0 })}><Icon name="square" /></IconButton>
			<NumberInput class="position" aria-label="Position" title="The clock's position, in beats" data-testid="clock-time" value={beat} digits={1} min={0} onChange={(v) => steer({ time: v * spb })} />
			<label class="labeled">bpm <NumberInput title="The tempo, in beats a minute" data-testid="clock-bpm" value={clock('bpm')} min={0} max={999} onChange={(v) => void g.setVariableValue('clock.bpm', v).catch(fail('Set tempo'))} /></label>
			<label class="labeled">dur <NumberInput title="The timeline's length in beats: the clock restarts when it reaches it; 0 is endless" data-testid="clock-length" value={clock('length')} min={0} onChange={(v) => void g.setVariableValue('clock.length', v).catch(fail('Set length'))} /></label>
			{#if loop}
				<Segmented value="loop" segments={[{ id: 'loop', label: 'loop', title: 'The clock loops in the section marked on the ruler: press to clear it', testid: 'clock-loop' }]} onChange={() => setLoop(0, 0)} />
			{/if}
			{#if stowed.length > 0}
				<Button size="sm" data-testid="timeline-hidden" title="Hidden tracks: pick one to show it" onclick={(e) => drop(e, stowed.map((id) => ({ label: g.timeline[id].name, action: () => hide(hidden.filter((h) => h !== id), 'Show Track') })))}>
					{stowed.length} hidden <Icon name="chevron-down" />
				</Button>
			{/if}
			<IconButton variant="ghost" label="Add track" data-testid="timeline-add" onclick={addMenu}><Icon name="plus" /></IconButton>
			</div>
		{/snippet}
	</Bar>
	<div class="body">
		<div class="range"><RangeBar {view} {total} {vertical} onChange={(next) => (view = next)} /></div>
		<div class="scroll thin-scrollbar">
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div class="stack" bind:this={stack} onpointerdowncapture={boxPick}>
				<div class="row ruler" data-testid="timeline-ruler">
					<span class="info"></span>
					<!-- svelte-ignore a11y_no_static_element_interactions -->
					<div class="lane" onpointerdown={onRuler} ondblclick={onRulerTwice} title="Press to put the clock here; drag to mark the section it loops in; double click to look at that section, or at everything">
						{#each ticks(view[0], view[1]) as tick}<span class="tick" style:--a={share(along(tick))}>{tick}</span>{/each}
					</div>
				</div>
				{#each shown as id (id)}
					{@const t = g.timeline[id]}
					{@const chosen = pickedIn(id)}
					{@const mine = held(t)[chosen[0]]}
					{@const [lo, hi] = t.range}
					{@const board = t.kind === 'note' ? Array.from({ length: hi - lo + 1 }, (_, i) => lo + i) : []}
					{@const size = sizing?.id === id ? sizing.size : own.sizes?.[id]}
					<!-- svelte-ignore a11y_no_static_element_interactions -->
					<section
						class="row track"
						class:notes={t.kind === 'note'}
						data-testid={`track-${t.name}`}
						data-track={id}
						style:--size={size ? `${size}px` : null}
						style:--rows={t.kind === 'note' ? hi - lo + 1 : null}
						oncontextmenu={(e) => {
							e.preventDefault();
							trackMenu(id, e.clientX, e.clientY);
						}}
						onpointerdown={(e) => {
							pressed = id;
							if (e.pointerType !== 'mouse') press.start(e);
						}}
						onpointermove={press.move}
						onpointerup={press.cancel}
						onpointercancel={press.cancel}
					>
						<div class="info" use:draggable={{ source: () => lift(id), allow: (el) => !el.closest('input, select, button, .input'), onEnd: (outcome) => place(id, outcome) }}>
							<TrackHead
								{id}
								{t}
								item={mine}
								recording={recording(t)}
								{ports}
								onRescan={() => void rescan().catch(fail('List MIDI devices'))}
								onRecord={(e) => void record(e, id).catch(fail('Record'))}
								onHide={() => hide([...hidden, id], 'Hide Track')}
								onDelete={() => (asked = id)}
								onItem={(to) => change(id, t, to)}
							/>
						</div>
						{#if t.kind === 'note'}
							<!-- The keyboard: a key per pitch, the notes of the scale lit, each root named. -->
							<div class="keys" data-testid="track-keyboard" aria-hidden="true">
								{#each board as p (p)}
									<span class:black={[1, 3, 6, 8, 10].includes(p % 12)} class:out={!inScale(t, p)} data-pitch={p}>{#if NOTE_NAMES[p % 12] === t.root}<b>{t.root}{Math.floor(p / 12) - 1}</b>{/if}</span>
								{/each}
							</div>
						{/if}
						<div class="lane">
							<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
							<div
								class="ground"
								data-testid="track-lane"
								onclick={(e) => addAt(e, id, t)}
								onpointerdown={() => pick([])}
								onpointermove={(e) => (over = t.kind === 'note' && e.target === e.currentTarget && e.pointerType === 'mouse' ? { track: id, ...cellAt(e, e.currentTarget, t) } : null)}
								onpointerleave={() => (over = null)}
							>
								{#if t.kind === 'note'}
									<!-- The cells: a row a pitch, darker off the scale and on a black key; a line a cell. -->
									<div class="rows" aria-hidden="true">
										{#each board as p (p)}<span class:black={[1, 3, 6, 8, 10].includes(p % 12)} class:out={!inScale(t, p)}></span>{/each}
									</div>
									<svg class="grid" viewBox={vertical ? '0 0 100 1000' : '0 0 1000 100'} preserveAspectRatio="none" aria-hidden="true">
										<path d={gridPath(view[0], view[1], gridOf(t), vertical)} vector-effect="non-scaling-stroke" />
									</svg>
									{#if over?.track === id}
										<div class="cell" data-testid="track-cell" style:--a={share(along(over.at))} style:--l={share(gridOf(t) / width)} style:--c={across((over.pitch - lo) / (hi - lo + 1), 1 / (hi - lo + 1))} style:--s={share(1 / (hi - lo + 1))}></div>
									{/if}
									{#each t.notes as n, i}
										<div
											class="note"
											class:picked={chosen.includes(i)}
											data-item={i}
											style:--a={share(along(n.at))}
											style:--l={share((n.length) / width)}
											style:--c={across((n.pitch - lo) / (hi - lo + 1), 1 / (hi - lo + 1))}
											style:--s={share(1 / (hi - lo + 1))}
											style:opacity={0.4 + 0.6 * n.velocity}
											onpointerdown={(e) => dragItem(e, id, t, i)}
										>
											<span class="end" onpointerdown={(e) => dragItem(e, id, t, i, true)}></span>
										</div>
									{/each}
								{:else}
									<svg viewBox={vertical ? '0 0 100 1000' : '0 0 1000 100'} preserveAspectRatio="none" aria-hidden="true">
										<path d={lanePath(t.keys, t.range, (at) => along(at), vertical)} vector-effect="non-scaling-stroke" />
									</svg>
									{#each t.keys as k, i}
										<span class="key" class:picked={chosen.includes(i)} data-item={i} style:--a={share(along(k.at))} style:--c={across((k.value - lo) / (hi - lo || 1))} onpointerdown={(e) => dragItem(e, id, t, i)}></span>
									{/each}
								{/if}
							</div>
						</div>
						<span class="grip" data-testid="track-grip" title="Drag to resize the track" onpointerdown={(e) => resize(e, id)}></span>
					</section>
				{/each}
				<!-- The loop's section and the clock's position, across the ruler and every track. -->
				{#if band && band[1] > view[0] && band[0] < view[1]}
					{@const from = clamp(along(band[0]), 0, 1)}
					<div class="band" data-testid="timeline-loop" style:--a={from} style:--l={clamp(along(band[1]), 0, 1) - from}></div>
				{/if}
				{#if box}
					<div class="box" data-testid="timeline-box" style:left={`${box.left}px`} style:top={`${box.top}px`} style:width={`${box.width}px`} style:height={`${box.height}px`}></div>
				{/if}
				{#if within(beat, view)}
					<div class="playhead" data-testid="timeline-playhead" style:--a={along(beat)}></div>
				{/if}
			</div>
			{#if shown.length === 0}
				<EmptyState>
					{#snippet title()}{stowed.length > 0 ? 'Every track is hidden' : 'No tracks'}{/snippet}
					{#snippet hint()}{stowed.length > 0 ? 'Show one from the hidden tracks in the bar.' : 'Add a note track or a curve track.'}{/snippet}
					{#if stowed.length === 0}
						<IconButton variant="ghost" label="Add track" data-testid="timeline-add-first" onclick={addMenu}><Icon name="plus" /></IconButton>
					{/if}
				</EmptyState>
			{/if}
		</div>
	</div>
</div>

{#if menu}
	<ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => (menu = null)} />
{/if}
<ConfirmDialog
	open={doomed !== undefined}
	question={`Delete track ${doomed?.name}?`}
	detail="Its notes or keyframes and its variable group go with it. Undo brings the track back."
	onClose={() => (asked = '')}
	data-testid="track-delete-dialog"
>
	<Button
		variant="danger"
		data-testid="track-delete-confirm"
		onclick={() => {
			void g.op('timeline track remove', { track: asked }).catch(fail('Delete track'));
			asked = '';
		}}>Delete</Button
	>
	<Button variant="ghost" onclick={() => (asked = '')}>Cancel</Button>
</ConfirmDialog>

<style>
	/* Along the axis a thing is placed by `--a` and is `--l` long; across it, by `--c` and `--s`.
	   A row reads them as left, width, top and height; a column the other way round. */
	.timeline {
		/* What a track says about itself, beside its lane. A pitch's row in a note lane. */
		--side: calc(var(--space-8) * 10);
		--keys: calc(var(--space-8) * 1.5);
		--row: var(--space-3);
		/* The room before and after a lane's ground: a keyframe at the end of its range shows whole. */
		--pad: var(--space-3);
		--number-width: calc(var(--space-8) * 2.5);
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	/* Room for some thousands of beats, to the tenth. */
	.timeline :global(.position) {
		--number-width: calc(var(--space-8) * 4);
	}
	.tools {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--space-3);
		min-width: 0;
	}
	.labeled {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		font-size: var(--fs-small);
		color: var(--text-muted);
	}
	.body {
		flex: 1 1 auto;
		display: grid;
		grid-template-rows: auto minmax(0, 1fr);
		min-height: 0;
	}
	/* A narrow panel gives the lanes more of its width. */
	@container (max-width: 32rem) {
		.body {
			--side: calc(var(--space-8) * 8);
		}
	}
	.scroll {
		min-width: 0;
		min-height: 0;
		overflow: auto;
	}
	.range {
		padding-left: var(--side);
	}
	/* Room after the last track, so its edge is never the panel's own. */
	.stack {
		position: relative;
		margin-bottom: var(--space-8);
	}
	.row {
		display: grid;
		grid-template-columns: var(--side) minmax(0, 1fr);
	}
	/* One track a lighter grey, the next a darker. */
	.track {
		position: relative;
		border-top: 1px solid var(--border);
		background: var(--surface-2);
	}
	.track:nth-of-type(even) {
		background: var(--surface-3);
	}
	/* It takes the room its lane gives it: its title, then the settings that fit there. */
	.info {
		contain: size;
		display: flex;
		flex-direction: column;
		--number-width: max(calc(var(--space-8) * 1.75), var(--hit));
		padding: var(--space-2);
		overflow: hidden;
		font-size: var(--fs-small);
		color: var(--text-muted);
		cursor: grab;
	}
	/* A note track's keyboard stands between what it says and its lane, a key per row. */
	.notes .info {
		padding-right: calc(var(--keys) + var(--space-2));
	}
	.keys {
		position: absolute;
		inset: var(--pad) auto var(--pad) calc(var(--side) - var(--keys));
		display: flex;
		flex-direction: column-reverse;
		width: var(--keys);
	}
	.keys span {
		flex: 1 1 0;
		display: flex;
		align-items: center;
		min-width: 0;
		min-height: 0;
		border-top: 1px solid var(--bg);
		background: var(--text-dim);
		color: var(--bg);
	}
	/* A root's name, in type that reads: it is larger than its key, and lies over the keys beside it. */
	.keys b {
		position: relative;
		z-index: 1;
		padding: 0 var(--space-1);
		border-radius: var(--radius-sm);
		background: inherit;
		font-size: var(--fs-small);
		line-height: 1;
		white-space: nowrap;
	}
	.keys .black {
		background: var(--surface-4);
		color: var(--text);
	}
	.keys .out {
		opacity: 0.3;
	}
	.lane {
		position: relative;
		display: flex;
		min-height: var(--size, calc(var(--rows, 26) * var(--row)));
		padding: var(--pad) 0;
		overflow: hidden;
		touch-action: none;
		user-select: none;
	}
	.ground {
		position: relative;
		flex: 1 1 auto;
	}
	.ruler {
		position: sticky;
		top: 0;
		z-index: 1;
		background: var(--surface-1);
	}
	.ruler .lane {
		min-height: var(--space-8);
		padding: 0;
		cursor: pointer;
	}
	.tick {
		position: absolute;
		inset: 0 auto 0 var(--a);
		padding-left: var(--space-1);
		border-left: 1px solid var(--border-strong);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		color: var(--text-muted);
		pointer-events: none;
	}
	.rows {
		position: absolute;
		inset: 0;
		display: flex;
		flex-direction: column-reverse;
		pointer-events: none;
	}
	.rows span {
		flex: 1 1 0;
		border-top: 1px solid var(--bg);
	}
	.rows .black {
		background: var(--surface-1);
	}
	.rows .out {
		background: var(--bg);
	}
	.grid path {
		stroke: var(--bg);
		stroke-width: 1;
	}
	.note,
	.cell {
		position: absolute;
		left: var(--a);
		top: var(--c);
		width: var(--l);
		height: var(--s);
	}
	.cell {
		background: var(--hover-fill);
		pointer-events: none;
	}
	.end {
		position: absolute;
		inset: 0 0 0 auto;
		/* Never more of a short note than its last third: the rest is the note's own. */
		width: min(var(--space-3), 34%);
		cursor: ew-resize;
	}
	.note {
		min-width: var(--space-2);
		border-radius: var(--radius-sm);
		background: var(--accent);
		cursor: grab;
	}
	svg {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		overflow: visible;
		pointer-events: none;
	}
	path {
		fill: none;
		stroke: var(--accent);
		stroke-width: 1.5;
	}
	/* Placed by its centre with no transform: a transform would keep its hit area above the other dots. */
	.key {
		position: absolute;
		left: calc(var(--a) - var(--space-5) / 2);
		top: calc(var(--c) - var(--space-5) / 2);
		width: var(--space-5);
		height: var(--space-5);
		border-radius: 50%;
		background: var(--accent-strong);
		cursor: grab;
	}
	.box {
		position: absolute;
		z-index: 2;
		border: 1px solid var(--accent);
		background: var(--accent-fill);
		pointer-events: none;
	}
	.picked {
		outline: var(--focus-width) solid var(--focus-ink);
	}
	.grip {
		position: absolute;
		inset: auto 0 0 0;
		height: var(--space-2);
		cursor: ns-resize;
		touch-action: none;
	}
	.band,
	.playhead {
		position: absolute;
		inset: 0 auto 0 calc(var(--side) + (100% - var(--side)) * var(--a));
		z-index: 1;
		pointer-events: none;
	}
	.band {
		width: calc((100% - var(--side)) * var(--l));
		background: var(--accent-fill);
	}
	.playhead {
		width: max(2px, var(--space-1));
		background: var(--danger);
	}

	/* ---- a panel taller than wide: the time runs down, the tracks stand as columns. */
	.vertical .body {
		grid-template-rows: none;
		grid-template-columns: auto minmax(0, 1fr);
	}
	.vertical .range {
		display: flex;
		padding: var(--side) 0 0;
	}
	.vertical .stack {
		display: flex;
		width: max-content;
		min-width: 100%;
		height: 100%;
		margin: 0 var(--space-8) 0 0;
	}
	.vertical .row {
		grid-template-columns: none;
		grid-template-rows: var(--side) minmax(0, 1fr);
	}
	.vertical .track {
		flex: none;
		width: var(--size, calc(var(--space-8) * 8));
		border-top: 0;
		border-left: 1px solid var(--border);
	}
	.vertical .notes .info {
		padding: var(--space-2) var(--space-2) calc(var(--keys) + var(--space-2));
	}
	.vertical .keys {
		inset: calc(var(--side) - var(--keys)) var(--pad) auto var(--pad);
		flex-direction: row;
		width: auto;
		height: var(--keys);
	}
	.vertical .keys span {
		border-top: 0;
		border-left: 1px solid var(--bg);
	}
	.vertical .lane {
		min-height: 0;
		padding: 0 var(--pad);
	}
	.vertical .ruler {
		top: auto;
		left: 0;
		flex: none;
		width: calc(var(--space-8) * 2);
	}
	.vertical .ruler .lane {
		padding: 0;
	}
	.vertical .tick {
		inset: var(--a) 0 auto 0;
		border-left: 0;
		border-top: 1px solid var(--border-strong);
	}
	.vertical .rows {
		flex-direction: row;
	}
	.vertical .rows span {
		border-top: 0;
		border-left: 1px solid var(--bg);
	}
	.vertical .note,
	.vertical .cell {
		left: var(--c);
		top: var(--a);
		width: var(--s);
		height: var(--l);
	}
	.vertical .note {
		min-width: 0;
		min-height: var(--space-2);
	}
	.vertical .end {
		inset: auto 0 0 0;
		width: auto;
		height: min(var(--space-3), 34%);
		cursor: ns-resize;
	}
	.vertical .key {
		left: calc(var(--c) - var(--space-5) / 2);
		top: calc(var(--a) - var(--space-5) / 2);
	}
	.vertical .grip {
		inset: 0 0 0 auto;
		width: var(--space-2);
		height: auto;
		cursor: ew-resize;
	}
	.vertical .band,
	.vertical .playhead {
		inset: calc(var(--side) + (100% - var(--side)) * var(--a)) 0 auto 0;
	}
	.vertical .band {
		width: auto;
		height: calc((100% - var(--side)) * var(--l));
	}
	.vertical .playhead {
		width: auto;
		height: max(2px, var(--space-1));
	}

	/* Under a finger a row is taller, a track's edge is thicker, and a keyframe takes a press a
	   hit-size wide, behind every keyframe's own dot. */
	@media (hover: none) and (pointer: coarse) {
		.timeline {
			--row: var(--space-5);
			--pad: var(--space-6);
		}
		.ground {
			isolation: isolate;
		}
		.key::after {
			content: '';
			position: absolute;
			inset: calc((100% - var(--hit)) / 2);
			z-index: -1;
		}
		.grip {
			height: var(--space-5);
		}
		.end {
			width: min(var(--space-6), 34%);
		}
		.vertical .end {
			width: auto;
			height: min(var(--space-6), 34%);
		}
		.vertical .grip {
			width: var(--space-5);
			height: auto;
		}
	}
</style>
