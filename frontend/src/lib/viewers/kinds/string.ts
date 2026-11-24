/** The text of a STRING slot, which `ViewerFeed` fills its card with. */
import type { ViewerModule } from '../module';

export const string: ViewerModule = {
	id: 'string',
	settings: [
		{ key: 'markdown', label: 'Markdown', type: 'toggle', default: false },
		{ key: 'wrap', label: 'Word wrap', type: 'toggle', default: true }
	],
	ask: () => ({ dtype: 'string', ndim: [], reduce: [] }),
	drawing: null
};
