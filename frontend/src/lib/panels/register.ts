import type { Component } from 'svelte';
import { registerPanel, type PanelProps } from 'panelty';
import { PANEL_TYPES, type PanelTypeId } from '$lib/api/vocab';
import { harnesses } from '$lib/stores/harness.svelte';
import EmptyPanel from './EmptyPanel.svelte';
import NodeEditorPanel from './NodeEditorPanel.svelte';
import InspectorPanel from './InspectorPanel.svelte';
import ViewerPanel from './ViewerPanel.svelte';
import ConsolePanel from './ConsolePanel.svelte';
import VariablesPanel from './VariablesPanel.svelte';
import ControlPanel from './ControlPanel.svelte';
import AgentPanel from './AgentPanel.svelte';
import RecorderPanel from './RecorderPanel.svelte';

const components: Record<PanelTypeId, Component<PanelProps>> = {
	empty: EmptyPanel,
	'node-editor': NodeEditorPanel,
	inspector: InspectorPanel,
	viewer: ViewerPanel,
	console: ConsolePanel,
	variables: VariablesPanel,
	control: ControlPanel,
	agent: AgentPanel,
	recorder: RecorderPanel
};

/** Panel types that answer their own ✕: closing an agent view must not silently kill the agent. */
const confirmClose: Partial<Record<PanelTypeId, (panelId: string) => boolean>> = {
	agent: (panelId) => {
		const id = harnesses().instanceFor(panelId);
		if (!id) return false;
		harnesses().requestClose(id, panelId);
		return true;
	}
};

let done = false;

export function registerAppPanels(): void {
	if (done) return;
	done = true;

	for (const t of PANEL_TYPES) {
		registerPanel({
			id: t.id,
			title: t.title,
			icon: t.icon,
			component: components[t.id],
			acceptsNode: t.acceptsNode,
			confirmClose: confirmClose[t.id]
		});
	}
}
