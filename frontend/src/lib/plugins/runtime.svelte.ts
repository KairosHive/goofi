/** Load plugin frontends while the workspace is available. */
import { registerPanel } from 'panelty';
import { getControl } from '$lib/api/control';
import { notify } from '$lib/stores/notify.svelte';
import PluginPanel from './PluginPanel.svelte';
import type { Component } from 'svelte';
import type { PanelProps } from 'panelty';
import type { Activate, Context, Header, Panel, PanelContext, Plugin } from '../../../../sdk/frontend';

export type { Panel, PanelContext };
export interface HeaderEntry extends Header { key: string }
export const pluginHeaders = $state<{ entries: HeaderEntry[] }>({ entries: [] });

function identifier(id: string): void {
	if (!/^[a-z0-9-]{1,80}$/.test(id)) throw new Error('Plugin IDs must use lowercase letters, digits, or hyphens');
}

function context(id: string): Context {
	return {
		call: (op, args = {}) => getControl().call(op as Parameters<ReturnType<typeof getControl>['call']>[0], args),
		log: (message, level = 'info') => {
			void getControl().call('log write', { component: `plugin:${id}`, text: message, level }).catch((error) => notify().failure('Plugin log', error));
		}
	};
}

export function activatePlugin(id: string): { plugin: Plugin; ctx: Context; commit: () => void; dispose: () => void } {
	identifier(id);
	const ctx = context(id);
	const registered = new Set<string>();
	const panels: Parameters<typeof registerPanel>[0][] = [];
	let committed = false;
	const headers = new Set<string>();
	let disposed = false;
	const plugin: Plugin = {
		register_panel(panel) {
			identifier(panel.id);
			if (typeof panel.title !== 'string' || typeof panel.mount !== 'function') throw new Error('A panel needs a title and mount function');
			if (panel.accepts_node !== undefined && typeof panel.accepts_node !== 'boolean') throw new Error('accepts_node must be a boolean');
			if (disposed) throw new Error('Plugin frontend is disposed');
			if (committed) throw new Error('Panels must register during activation');
			const key = `plugin:${id}:${panel.id}`;
			if (registered.has(key)) throw new Error(`Duplicate plugin panel: ${panel.id}`);
			registered.add(key);
			// The adapter supplies the framework's existing panel props to arbitrary DOM content.
			const component: Component<PanelProps> = (internals, props) => PluginPanel(internals, { get panelId() { return props.panelId; }, get state() { return props.state; }, get setState() { return props.setState; }, panel, ctx });
			panels.push({ id: key, title: panel.title, icon: 'square-dashed', component, acceptsNode: panel.accepts_node === true });
		},
		register_header(entry) {
			identifier(entry.id);
			validateHeader(entry);
			if (disposed) throw new Error('Plugin frontend is disposed');
			const key = `plugin:${id}:${entry.id}`;
			if (headers.has(key)) throw new Error(`Duplicate plugin header: ${entry.id}`);
			headers.add(key);
			let active = true;
			pluginHeaders.entries = [...pluginHeaders.entries, { ...entry, key }];
			return {
				update(patch) {
					if (!active || disposed) throw new Error('Header entry is disposed');
					pluginHeaders.entries = pluginHeaders.entries.map((value) => {
						if (value.key !== key) return value;
						const next = { ...value, ...patch, id: entry.id, key };
						validateHeader(next);
						return next;
					});
				},
				dispose() {
					if (!active) return;
					active = false;
					headers.delete(key);
					pluginHeaders.entries = pluginHeaders.entries.filter((value) => value.key !== key);
				}
			};
		}
	};
	return {
		plugin, ctx,
		commit() {
			if (disposed || committed) throw new Error('Plugin activation is already closed');
			for (const panel of panels) registerPanel(panel);
			committed = true;
		},
		dispose() {
			disposed = true;
			pluginHeaders.entries = pluginHeaders.entries.filter((entry) => !headers.has(entry.key));
		}
	};
}

export function activateHeader(entry: HeaderEntry): void {
	if (entry.disabled || !entry.onActivate) return;
	try {
		void Promise.resolve(entry.onActivate()).catch((error) => notify().failure('Plugin header', error));
	} catch (error) {
		notify().failure('Plugin header', error);
	}
}

export async function loadPlugins(): Promise<() => void> {
	const disposers: (() => void)[] = [];
	const { plugins } = await getControl().call<{ plugins: { id: string; frontend?: string; error?: string }[] }>('plugin list');
	for (const item of plugins) {
		if (item.error) {
			notify().failure(`Plugin ${item.id}`, item.error);
			continue;
		}
		if (!item.frontend) continue;
		const runtime = activatePlugin(item.id);
		try {
			const cleanup = await activateWithinDeadline(item.frontend, runtime.plugin, runtime.ctx);
			runtime.commit();
			disposers.push(() => { try { cleanup?.(); } finally { runtime.dispose(); } });
		} catch (error) {
			runtime.dispose();
			notify().failure(`Plugin ${item.id}`, error);
		}
	}
	return () => { for (const dispose of disposers.reverse()) dispose(); };
}

async function activateWithinDeadline(url: string, plugin: Plugin, ctx: Context): Promise<void | (() => void)> {
	let timer: ReturnType<typeof setTimeout>;
	let expired = false;
	const activation = (async () => {
		const module = await import(/* @vite-ignore */ url) as { default: Activate };
		const cleanup = await module.default(plugin, ctx);
		if (expired) cleanup?.();
		else return cleanup;
	})();
	try {
		return await Promise.race([
			activation,
			new Promise<never>((_, reject) => {
				timer = setTimeout(() => { expired = true; reject(new Error('Plugin frontend activation timed out')); }, 10_000);
			})
		]);
	} finally {
		clearTimeout(timer!);
	}
}

function validateHeader(entry: Header): void {
	if (typeof entry.label !== 'string'
		|| (entry.title !== undefined && typeof entry.title !== 'string')
		|| (entry.disabled !== undefined && typeof entry.disabled !== 'boolean')
		|| (entry.onActivate !== undefined && typeof entry.onActivate !== 'function')) {
		throw new Error('Invalid plugin header entry');
	}
}
