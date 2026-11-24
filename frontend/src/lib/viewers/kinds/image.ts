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
		{ key: 'log', label: 'Log scale', type: 'toggle', default: false },
		{ key: 'stretch', label: 'Stretch to fill', type: 'toggle', default: true }
	],
	// 8-bit: an image is drawn through a 256-level LUT or as colour bytes either way, so the
	// other three quarters of the bandwidth buy nothing. The channel axis is drawn whole.
	// Under a log scale the texels would lose the low end to quantization, so the f32 is asked.
	ask: (w, h, settings) => {
		const spec = boxAsk('image', w, h);
		spec.reduce.push({ dim: 2, max: 'whole' });
		if (settings.log === true) spec.depth = 'f32';
		return spec;
	},
	renders: (shape) => shape.length === 2 || [1, 2, 3, 4].includes(shape[2]),
	drawing: (surface) => new ImageDrawing(surface)
};
