/** Viewer-kind BEHAVIOUR; the vocabulary itself is the manager's, in `$lib/api/vocab`. */
import type { ArrayData } from '$lib/codec/decode';
import { DEFAULT_KIND, VIEWER_KINDS, type SlotDtype, type ViewerKind } from '$lib/api/vocab';
import type { SettingsMap } from './settingsSchema';

export type { ViewerKind };

/** ARRAY viewer kinds the viewer-type dropdown offers, in order. */
export const ARRAY_KINDS: readonly ViewerKind[] = VIEWER_KINDS.filter(
	(k) => k.dtype === 'ARRAY'
).map((k) => k.id);

/** The kind a dtype PINS: a STRING slot is always drawn by the string viewer, whatever was
 * stored, and a slot with no pin is one whose kind a viewer may choose. */
export function pinnedKind(dtype: string | null): ViewerKind | null {
	return VIEWER_KINDS.find((k) => k.dtype !== 'ARRAY' && k.dtype === dtype)?.id ?? null;
}

/** The viewer kind to actually use: a dtype-pinned kind wins over the stored one, and a slot
 * nobody has chosen for — or one stored under a word the vocabulary no longer has — opens with
 * what draws its dtype. */
export function resolveKind(dtype: string | null, stored: ViewerKind | undefined): ViewerKind {
	const known = stored && ARRAY_KINDS.includes(stored) ? stored : undefined;
	return pinnedKind(dtype) ?? known ?? DEFAULT_KIND[(dtype ?? 'ARRAY') as SlotDtype] ?? 'line';
}

/** Whether the line viewer draws a trajectory rather than series, by its settings. */
export function isTrajectory(kind: ViewerKind, settings: SettingsMap): boolean {
	return kind === 'line' && settings.mode === 'trajectory';
}

export type BrainMode = 'topomap' | 'ring' | '3d';

/** What the brain viewer draws for a frame of `ndim` axes: its setting, or under `auto` a
 * topomap of one value per channel and a ring of a channel-by-channel matrix. */
export function brainMode(settings: SettingsMap, ndim: number): BrainMode {
	const mode = settings.mode;
	if (mode === 'topomap' || mode === 'ring' || mode === '3d') return mode;
	return ndim <= 1 ? 'topomap' : 'ring';
}

/** The kinds the panel's plot surface draws; their card body is a transparent frame over it. */
export function drawsOnSurface(kind: ViewerKind, settings: SettingsMap): boolean {
	return (kind === 'line' && !isTrajectory(kind, settings)) || kind === 'image';
}

/** Whether an array of the given shape can be drawn by `kind` under `settings`; a non-array
 * frame always can. */
export function isRenderable(kind: ViewerKind, spec: ArrayData | null, settings: SettingsMap): boolean {
	if (!spec) return true;
	const s = spec.shape;
	const draws = VIEWER_KINDS.find((k) => k.id === kind)?.draws;
	if (!draws) return true;
	if (s.length < draws[0] || s.length > draws[1]) return false;
	if (kind === 'image') return s.length === 2 || [1, 2, 3, 4].includes(s[2]);
	if (isTrajectory(kind, settings)) return s.length === 2 && s[0] >= 2;
	if (kind === 'brain') {
		const mode = brainMode(settings, s.length);
		return s.length === 1 ? mode === 'topomap' : mode !== 'topomap' && s[0] === s[1];
	}
	return true;
}
