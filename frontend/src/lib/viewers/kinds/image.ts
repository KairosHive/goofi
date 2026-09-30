/** A bitmap: (H, W), or (H, W, C) for 1–4 channels. */
import type { ViewerModule } from '../module';
import { boxAsk, manual } from '../module';
import { ImageDrawing } from '../drawing';
import { COLORMAPS } from '../colormaps';

export const image: ViewerModule = {
	id: 'image',
	settings: [
		{ key: 'colormap', label: 'Colormap', type: 'select', default: 'gray', options: COLORMAPS },
		{ key: 'auto', label: 'Auto range', type: 'toggle', default: true },
		{ key: 'vmin', label: 'Min', type: 'number', default: 0, step: 0.1, showWhen: manual('auto') },
		{ key: 'vmax', label: 'Max', type: 'number', default: 1, step: 0.1, showWhen: manual('auto') },
		{ key: 'stretch', label: 'Stretch to fill', type: 'toggle', default: false }
	],
	// 8-bit: an image is drawn through a 256-level LUT or as colour bytes either way, so the
	// other three quarters of the bandwidth buy nothing. The channel axis is drawn whole.
	ask: (w, h) => {
		const spec = boxAsk('image', w, h);
		spec.reduce.push({ dim: 2, max: 'whole' });
		return spec;
	},
	renders: (shape) => shape.length === 2 || [1, 2, 3, 4].includes(shape[2]),
	variant: () => '',
	drawing: (surface) => new ImageDrawing(surface)
};
