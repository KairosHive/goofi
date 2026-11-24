/** The rows of a TABLE slot, which `ViewerFeed` fills its card with. */
import type { ViewerModule } from '../module';

export const table: ViewerModule = {
	id: 'table',
	settings: [{ key: 'decimals', label: 'Decimals', type: 'number', default: 3, min: 0, max: 10, step: 1 }],
	ask: () => ({ dtype: 'table', ndim: [], reduce: [] }),
	drawing: null
};
