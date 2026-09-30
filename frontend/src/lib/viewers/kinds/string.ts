/** The text of a STRING slot, which `ViewerSurface` fills its card with. */
import type { ViewerModule } from '../module';

export const string: ViewerModule = {
	id: 'string',
	settings: [
		{ key: 'markdown', label: 'Markdown', type: 'toggle', default: false },
		{ key: 'wrap', label: 'Word wrap', type: 'toggle', default: true }
	],
	ask: () => ({ dtype: 'string', ndim: [], dims: [], reduce: [] }),
	renders: () => true,
	variant: () => '',
	drawing: null
};
