import { describe, it, expect } from 'vitest';
import { viewBinding } from './viewBinding';
import type { SlotView } from './inlineView';

describe('viewBinding', () => {
	it('resolves kind/settings from the stored view and writes back via write', () => {
		let state: Record<string, unknown> = { node: 'n', slot: 's' };
		const b = viewBinding(
			'ARRAY',
			() => state as SlotView,
			(s) => {
				// `edit_panel` MERGES the state it is handed, so the double does too — that is
				// the seam, and a binding writes only the key it changed.
				const settings = { ...(state.settings as object), ...s.settings };
				state = { ...state, ...s, settings };
			}
		);
		expect(b.kind).toBe('line'); // default
		b.setKind('image');
		expect(state.kind).toBe('image');
		expect(state.node, 'a kind change names the kind and nothing else').toBe('n');
		expect(b.kind).toBe('image'); // re-resolves from updated state
		b.setSetting('colormap', 'viridis');
		expect((state.settings as Record<string, unknown>).colormap).toBe('viridis');
		expect(b.settings.colormap).toBe('viridis');
	});

	it('forces the string viewer for STRING dtype regardless of stored kind', () => {
		const b = viewBinding(
			'STRING',
			() => ({ kind: 'image' }),
			() => {}
		);
		expect(b.kind).toBe('string');
	});
});
