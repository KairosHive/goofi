import { describe, expect, it, vi } from 'vitest';
import { listPanelTypes } from 'panelty';
import { activatePlugin, pluginHeaders } from './runtime.svelte';

vi.mock('$lib/api/control', () => ({ getControl: () => ({ call: vi.fn() }) }));

describe('plugin frontend lifetime', () => {
	it('updates entries and does not let an old handle remove a replacement', () => {
		const runtime = activatePlugin('header-test');
		const first = runtime.plugin.register_header({ id: 'subject', label: 'Alice' });
		first.update({ label: 'Bob' });
		expect(pluginHeaders.entries.find((entry) => entry.key === 'plugin:header-test:subject')?.label).toBe('Bob');
		first.dispose();
		const replacement = runtime.plugin.register_header({ id: 'subject', label: 'Carol' });
		first.dispose();
		expect(pluginHeaders.entries.find((entry) => entry.key === 'plugin:header-test:subject')?.label).toBe('Carol');
		expect(() => first.update({ label: 'stale' })).toThrow('disposed');
		runtime.dispose();
		expect(pluginHeaders.entries.some((entry) => entry.key === 'plugin:header-test:subject')).toBe(false);
		expect(() => replacement.update({ label: 'stale' })).toThrow('disposed');
	});

	it('publishes panels only after successful activation', () => {
		const failed = activatePlugin('failed-test');
		failed.plugin.register_panel({ id: 'panel', title: 'Failed', mount() {} });
		failed.dispose();
		expect(listPanelTypes().some((panel) => panel.id === 'plugin:failed-test:panel')).toBe(false);
		const ready = activatePlugin('ready-test');
		ready.plugin.register_panel({ id: 'panel', title: 'Ready', mount() {} });
		ready.commit();
		expect(listPanelTypes().some((panel) => panel.id === 'plugin:ready-test:panel')).toBe(true);
		expect(() => ready.plugin.register_panel({ id: 'late', title: 'Late', mount() {} })).toThrow('during activation');
		ready.dispose();
	});
});
