/** The Cables panel: a few settings for the next cable, then the list, each row a drop target. */
const STYLE = `
:host { display: block; height: 100%; color: inherit; font: inherit; }
.cables { position: relative; display: flex; flex-direction: column; height: 100%; min-height: 0; }
.bar { display: flex; align-items: center; gap: var(--space-2); min-height: var(--hit); padding: 0 var(--space-3); }
.dot { width: 0.5rem; height: 0.5rem; border-radius: 50%; background: var(--danger); flex: none; }
.dot.ok { background: var(--success, var(--accent)); }
.status { flex: 1 1 auto; min-width: 0; font-size: var(--fs-small); color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.fields { display: grid; gap: var(--space-2); padding: var(--space-3); }
@container (min-width: 30rem) { .fields { grid-template-columns: 1fr auto auto; align-items: end; } }
label { display: flex; flex-direction: column; gap: var(--space-1); font-size: var(--fs-small); color: var(--text-muted); min-width: 0; }
input, select, button { font: inherit; color: inherit; min-height: var(--hit); min-width: 0; border-radius: var(--radius-sm); border: 1px solid var(--border, var(--surface-3)); background: var(--surface-2); padding: 0 var(--space-2); }
button { cursor: pointer; }
button.primary { background: var(--accent); color: var(--on-accent, var(--surface-1)); border-color: transparent; }
button:disabled { opacity: 0.5; cursor: default; }
.failure { padding: 0 var(--space-3) var(--space-2); font-size: var(--fs-small); color: var(--danger); overflow-wrap: anywhere; }
.list { flex: 1 1 auto; min-height: 0; overflow: auto; }
.hint { margin: 0; padding: var(--space-6) var(--space-3); text-align: center; color: var(--text-muted); }
ul { list-style: none; margin: 0; padding: 0 var(--space-2) var(--space-2); display: flex; flex-direction: column; gap: var(--space-1); }
.row { display: flex; align-items: center; gap: var(--space-2); min-height: var(--hit); padding-left: var(--space-2); border-radius: var(--radius-sm); background: var(--surface-2); border: 2px solid transparent; }
.row.armed { border-color: color-mix(in srgb, var(--accent) 55%, transparent); border-style: dashed; }
.row.target { border-style: solid; background: color-mix(in srgb, var(--accent) 16%, var(--surface-2)); }
.name { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.device { font-family: var(--font-mono); font-size: var(--fs-small); color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.num { font-family: var(--font-mono); font-size: var(--fs-small); color: var(--text-muted); font-variant-numeric: tabular-nums; }
.remove { border: none; background: transparent; width: var(--hit); padding: 0; }
`;

const CHANNELS = [1, 2, 4, 6, 8, 16];

export default function (plugin) {
	plugin.register_panel({
		id: 'cables',
		title: 'Cables',
		accepts_node: true,
		mount(element, panel) {
			const root = element.attachShadow({ mode: 'open' });
			root.innerHTML = `<style>${STYLE}</style>
<div class="cables" data-testid="cables-panel">
	<div class="bar"><span class="dot"></span><span class="status" data-testid="cables-status">Checking PipeWire…</span></div>
	<form class="fields">
		<label>Name <input name="name" aria-label="Cable name" placeholder="goofi to DAW" required></label>
		<label>Channels <select name="channels" aria-label="Channels">${CHANNELS.map((c) => `<option value="${c}"${c === 2 ? ' selected' : ''}>${c}</option>`).join('')}</select></label>
		<button class="primary" data-testid="cables-create">+ Create</button>
	</form>
	<div class="failure" role="alert" hidden></div>
	<div class="list"><p class="hint">No cable yet. Create one above, then drag an AudioOut or AudioIn node onto it. Desktop sound settings hide virtual devices until you turn them on.</p></div>
</div>`;
			const status = root.querySelector('.status');
			const dot = root.querySelector('.dot');
			const form = root.querySelector('form');
			const create = root.querySelector('[data-testid=cables-create]');
			const failure = root.querySelector('.failure');
			const list = root.querySelector('.list');
			let cables = [];
			let drag = null;
			let alive = true;
			let shown = '';

			const fail = (error) => {
				failure.textContent = String(error && error.message ? error.message : error);
				failure.hidden = false;
			};
			const clear = () => { failure.hidden = true; failure.textContent = ''; };

			const render = () => {
				if (cables.length === 0) {
					list.innerHTML = '<p class="hint">No cable yet. Create one above, then drag an AudioOut or AudioIn node onto it. Desktop sound settings hide virtual devices until you turn them on.</p>';
					return;
				}
				const rows = cables.map((cable) => {
					const zone = `${panel.panel_id}#${cable.name}`;
					const cls = ['row', drag ? 'armed' : '', drag && drag.zone === cable.name ? 'target' : ''].filter(Boolean).join(' ');
					return `<li class="${cls}" data-node-drop="${escape(zone)}" data-testid="cable-row" data-cable="${escape(cable.name)}">
	<span class="name" title="${escape(cable.device)}">${escape(cable.name)}</span>
	<span class="device">${escape(cable.device)}</span>
	<span class="num" title="Channels">${cable.channels}ch</span>
	<button type="button" class="remove" aria-label="Remove ${escape(cable.name)}" title="Remove">✕</button>
</li>`;
				});
				list.innerHTML = `<ul>${rows.join('')}</ul>`;
				for (const row of list.querySelectorAll('li')) {
					row.querySelector('.remove').onclick = async () => {
						clear();
						try {
							await panel.call('plugin virtual-cables remove', { name: row.dataset.cable });
							await refresh();
						} catch (error) { fail(error); }
					};
				}
			};

			const refresh = async () => {
				const reply = await panel.call('plugin virtual-cables list');
				if (!alive) return;
				// The poll answers the same list almost every time; an unchanged reply redraws nothing.
				const text = JSON.stringify(reply);
				if (text === shown) return;
				shown = text;
				cables = reply.cables;
				const supported = !reply.unsupported;
				dot.classList.toggle('ok', supported);
				status.textContent = supported
					? `PipeWire · ${cables.length} cable${cables.length === 1 ? '' : 's'}`
					: reply.unsupported;
				status.title = status.textContent;
				create.disabled = !supported;
				render();
			};

			form.onsubmit = async (event) => {
				event.preventDefault();
				clear();
				const name = form.elements.name.value.trim();
				const channels = Number(form.elements.channels.value);
				try {
					await panel.call('plugin virtual-cables create', { name, channels });
					form.elements.name.value = '';
					await refresh();
				} catch (error) { fail(error); }
			};

			const route = async (node, cable) => {
				clear();
				try {
					const reply = await panel.call('plugin virtual-cables route', { node, cable });
					panel.log(`${reply.nodes.length} node${reply.nodes.length === 1 ? '' : 's'} now on ${reply.device}`);
				} catch (error) { fail(error); }
			};
			const stopDrop = panel.on_node_drop((drop) => {
				if (drop.zone) void route(drop.node, drop.zone);
				else if (cables.length === 1) void route(drop.node, cables[0].name);
				else fail(cables.length === 0 ? 'Create a cable first, then drop the node on it' : 'Drop the node on one cable row');
			});
			const stopDrag = panel.on_node_drag((next) => {
				drag = next;
				render();
			});

			const timer = setInterval(() => void refresh().catch(fail), 2000);
			void refresh().catch(fail);
			return () => {
				alive = false;
				clearInterval(timer);
				stopDrop();
				stopDrag();
				form.onsubmit = null;
			};
		}
	});
}

function escape(text) {
	return String(text).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}
