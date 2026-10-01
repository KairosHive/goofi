/** A scalp map of one scalar per channel, or the connectivity between channels as a ring or in 3-D. */
import type { ViewerModule } from '../module';
import { manual, wholeAsk } from '../module';
import { BrainDrawing, brainMode } from '../brainDrawing';
import { COLORMAPS } from '../colormaps';

/** The brain viewer's drawings: `auto` is a topomap of a 1-D frame and a ring of a (C, C) one. */
const BRAIN_MODES = ['auto', 'topomap', 'ring', '3d'] as const;

export const brain: ViewerModule = {
	id: 'brain',
	settings: [
		{ key: 'mode', label: 'Mode', type: 'select', default: 'auto', options: [...BRAIN_MODES] },
		{ key: 'colormap', label: 'Colormap', type: 'select', default: 'magma', options: COLORMAPS },
		{ key: 'auto', label: 'Auto range', type: 'toggle', default: true },
		{ key: 'vmin', label: 'Min', type: 'number', default: -1, step: 0.1, showWhen: manual('auto') },
		{ key: 'vmax', label: 'Max', type: 'number', default: 1, step: 0.1, showWhen: manual('auto') },
		{ key: 'contours', label: 'Contour lines', type: 'toggle', default: false, showWhen: { key: 'mode', anyOf: ['topomap'] } },
		{ key: 'top', label: 'Edges %', type: 'number', default: 15, min: 1, max: 100, step: 1, showWhen: { key: 'mode', anyOf: ['ring', '3d'] } },
		{ key: 'curve', label: 'Curve', type: 'number', default: 0.1, min: 0, max: 1, step: 0.1, showWhen: { key: 'mode', anyOf: ['3d'] } }
	],
	// Every channel is a place on the head: nothing here is subsampled.
	ask: () => wholeAsk('brain', 2),
	renders(shape, s) {
		const mode = brainMode(s, shape.length);
		return shape.length === 1 ? mode === 'topomap' : mode !== 'topomap' && shape[0] === shape[1];
	},
	drawing: (surface) => new BrainDrawing(surface)
};
