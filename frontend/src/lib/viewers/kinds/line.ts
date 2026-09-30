/** A time plot, or as a setting the trajectory over pairs of rows. */
import type { ViewerModule } from '../module';
import { drawnNdim, manual } from '../module';
import { LineDrawing, TrajectoryDrawing } from '../drawing';

/** The line viewer's two drawings of a (C, N) frame. */
export const LINE_MODES = ['series', 'trajectory'] as const;
/** The traces a line plot can tell apart, whatever its height: past this, channels are subsampled. */
const MAX_ROWS = 32;
const MAX_POINTS = 4096;

const series = { key: 'mode', anyOf: ['series'] };
const trajectory = { key: 'mode', anyOf: ['trajectory'] };

export const line: ViewerModule = {
	id: 'line',
	settings: [
		{ key: 'mode', label: 'Mode', type: 'select', default: 'series', options: [...LINE_MODES] },
		{ key: 'logX', label: 'Log X', type: 'toggle', default: false, showWhen: series },
		{ key: 'logY', label: 'Log Y', type: 'toggle', default: false, showWhen: series },
		{ key: 'yAuto', label: 'Auto range', type: 'toggle', default: true },
		{ key: 'yMin', label: 'Min', type: 'number', default: -1, step: 0.1, showWhen: manual('yAuto') },
		{ key: 'yMax', label: 'Max', type: 'number', default: 1, step: 0.1, showWhen: manual('yAuto') },
		{ key: 'points', label: 'Show points', type: 'toggle', default: false, showWhen: series },
		{ key: 'pointSize', label: 'Point size', type: 'number', default: 2, min: 0, max: 12, step: 1, showWhen: trajectory }
	],
	ask(w, _h, s) {
		if (s.mode === 'trajectory') {
			// (dims × points): every row is a coordinate, and the path is the LAST axis.
			return {
				dtype: 'array',
				ndim: [['eq', 2]],
				dims: [],
				reduce: [
					{ dim: 0, max: 'whole' },
					{ dim: -1, max: Math.min(w, MAX_POINTS) }
				]
			};
		}
		// For 1-D, dim 0 and -1 collide on the bridge, which takes the larger cap.
		// Half floats: 11 significant bits are more than a device pixel resolves, for half the bytes.
		return {
			dtype: 'array',
			ndim: drawnNdim('line'),
			dims: [],
			reduce: [
				{ dim: 0, max: MAX_ROWS },
				{ dim: -1, max: w }
			],
			depth: 'f16'
		};
	},
	renders: (shape, s) => s.mode !== 'trajectory' || (shape.length === 2 && shape[0] >= 2),
	variant: (s) => (s.mode === 'trajectory' ? 'trajectory' : 'series'),
	drawing: (surface, variant) => (variant === 'trajectory' ? new TrajectoryDrawing(surface) : new LineDrawing(surface))
};
