import { describe, it, expect } from 'vitest';
import { nodeTypeSource } from './nodeTypeSource';
import { typeInfo as ty } from '$lib/test/typeInfo';

describe('nodeTypeSource — the one word a palette row carries', () => {
	it('names the patch a node came with, so a patch-local node is distinguishable', () => {
		expect(nodeTypeSource(ty({ source: 'patch' }))).toBe('this patch');
		expect(nodeTypeSource(ty({ source: 'builtin' }))).toBe('builtin');
	});

	// The same node before and after `library save`, in the two words that tell them apart. The
	// word is the tab's, not a phrase of its own: one name for the private library everywhere.
	it('names the private library, which is where a saved node goes', () => {
		expect(nodeTypeSource(ty({ source: 'custom' }))).toBe('custom');
	});

	// A plugin belongs to no tree at all — an engine found it on its own account. The word is
	// `plugin` and not `vst3`, because the marker the backend sets names no format.
	it('names a plugin, which the vst tab and the search both read', () => {
		expect(nodeTypeSource(ty({ type: 'audio:Reverb', source: 'plugin' }))).toBe('plugin');
	});

	// Categories are gone from the menu (the user asked for a flat list), and `unavailable` was one
	// of them. The word has to survive that removal on the row itself: greyed-and-unclickable alone
	// does not say WHY, and a node that cannot load must never read as a node that goofi ignored.
	it('says unavailable before it says where the file lives', () => {
		expect(nodeTypeSource(ty({ available: false, source: 'patch' }))).toBe('unavailable');
		expect(nodeTypeSource(ty({ available: false, source: 'plugin' }))).toBe('unavailable');
	});
});
