/** Per-viewer settings schema: what each viewer kind exposes in its cog menu, one flat list. */
import type { ViewerKind } from './kind';
import { COLORMAPS } from './colormaps';

export type SettingValue = boolean | number | string;
export type SettingType = 'toggle' | 'select' | 'number';

export interface SettingDescriptor {
	key: string;
	label: string;
	type: SettingType;
	default: SettingValue;
	options?: string[];
	min?: number;
	max?: number;
	step?: number;
	/** Only show this control while another setting holds one of these values. */
	showWhen?: { key: string; anyOf: SettingValue[] };
}

/** The line viewer's two drawings of a (C, N) frame. */
export const LINE_MODES = ['series', 'trajectory'] as const;
/** The brain viewer's drawings: `auto` is a topomap of a 1-D frame and a ring of a (C, C) one. */
export const BRAIN_MODES = ['auto', 'topomap', 'ring', '3d'] as const;

const series = { key: 'mode', anyOf: ['series'] };
const trajectory = { key: 'mode', anyOf: ['trajectory'] };
const manual = (key: string) => ({ key, anyOf: [false] });

const SCHEMA: Record<ViewerKind, SettingDescriptor[]> = {
	line: [
		{ key: 'mode', label: 'Mode', type: 'select', default: 'series', options: [...LINE_MODES] },
		{ key: 'logX', label: 'Log X', type: 'toggle', default: false, showWhen: series },
		{ key: 'logY', label: 'Log Y', type: 'toggle', default: false, showWhen: series },
		{ key: 'yAuto', label: 'Auto range', type: 'toggle', default: true },
		{ key: 'yMin', label: 'Min', type: 'number', default: -1, step: 0.1, showWhen: manual('yAuto') },
		{ key: 'yMax', label: 'Max', type: 'number', default: 1, step: 0.1, showWhen: manual('yAuto') },
		{ key: 'points', label: 'Show points', type: 'toggle', default: false, showWhen: series },
		{ key: 'pointSize', label: 'Point size', type: 'number', default: 2, min: 0, max: 12, step: 1, showWhen: trajectory }
	],
	image: [
		{ key: 'colormap', label: 'Colormap', type: 'select', default: 'gray', options: COLORMAPS },
		{ key: 'auto', label: 'Auto range', type: 'toggle', default: true },
		{ key: 'vmin', label: 'Min', type: 'number', default: 0, step: 0.1, showWhen: manual('auto') },
		{ key: 'vmax', label: 'Max', type: 'number', default: 1, step: 0.1, showWhen: manual('auto') },
		{ key: 'stretch', label: 'Stretch to fill', type: 'toggle', default: false }
	],
	brain: [
		{ key: 'mode', label: 'Mode', type: 'select', default: 'auto', options: [...BRAIN_MODES] },
		{ key: 'colormap', label: 'Colormap', type: 'select', default: 'coolwarm', options: COLORMAPS },
		{ key: 'auto', label: 'Auto range', type: 'toggle', default: true },
		{ key: 'vmin', label: 'Min', type: 'number', default: -1, step: 0.1, showWhen: manual('auto') },
		{ key: 'vmax', label: 'Max', type: 'number', default: 1, step: 0.1, showWhen: manual('auto') },
		{ key: 'contours', label: 'Contour lines', type: 'toggle', default: false, showWhen: { key: 'mode', anyOf: ['auto', 'topomap'] } },
		{ key: 'top', label: 'Top edges %', type: 'number', default: 100, min: 1, max: 100, step: 1, showWhen: { key: 'mode', anyOf: ['auto', 'ring', '3d'] } },
		{ key: 'curve', label: 'Curve', type: 'number', default: 0.5, min: 0, max: 1, step: 0.1, showWhen: { key: 'mode', anyOf: ['3d'] } }
	],
	table: [{ key: 'decimals', label: 'Decimals', type: 'number', default: 3, min: 0, max: 10, step: 1 }],
	string: [
		{ key: 'markdown', label: 'Markdown', type: 'toggle', default: false },
		{ key: 'wrap', label: 'Word wrap', type: 'toggle', default: true }
	]
};

export function settingsSchemaFor(kind: ViewerKind): SettingDescriptor[] {
	return SCHEMA[kind] ?? [];
}

/** A bag of explicitly-set setting overrides (or resolved values). */
export type SettingsMap = Record<string, SettingValue>;

function defaultSettings(kind: ViewerKind): SettingsMap {
	const out: SettingsMap = {};
	for (const s of settingsSchemaFor(kind)) out[s.key] = s.default;
	return out;
}

/** A kind's declared defaults with the explicit overrides applied on top. */
export function resolveSettings(kind: ViewerKind, overrides: SettingsMap | undefined): SettingsMap {
	return { ...defaultSettings(kind), ...(overrides ?? {}) };
}
