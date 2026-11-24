/** The Latency panel: two drop rows, a source and a target, and the run between them. */
const STYLE = `
:host { display: block; height: 100%; color: inherit; font: inherit; }
.latency { position: relative; display: flex; flex-direction: column; height: 100%; min-height: 0; }
.bar { display: flex; align-items: center; gap: var(--space-2); min-height: var(--hit); padding: 0 var(--space-3); }
.dot { width: 0.5rem; height: 0.5rem; border-radius: 50%; background: var(--text-muted); flex: none; }
.dot.on { background: var(--danger); }
.status { flex: 1 1 auto; min-width: 0; font-size: var(--fs-small); color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
button { font: inherit; color: inherit; min-height: var(--hit); min-width: 0; border-radius: var(--radius-sm); border: 1px solid var(--border, var(--surface-3)); background: var(--surface-2); padding: 0 var(--space-3); cursor: pointer; }
button.primary { background: var(--accent); color: var(--on-accent, var(--surface-1)); border-color: transparent; }
button:disabled { opacity: 0.5; cursor: default; }
.ends { display: grid; gap: var(--space-1); padding: var(--space-2); }
.row { display: flex; align-items: center; gap: var(--space-2); min-height: var(--hit); padding: 0 var(--space-2); border-radius: var(--radius-sm); background: var(--surface-2); border: 2px dashed color-mix(in srgb, var(--text-muted) 40%, transparent); }
.row.armed { border-color: color-mix(in srgb, var(--accent) 55%, transparent); }
.row.target { border-style: solid; background: color-mix(in srgb, var(--accent) 16%, var(--surface-2)); }
.role { flex: none; width: 3.5rem; font-size: var(--fs-small); color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.04em; }
.name { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.name.empty { color: var(--text-muted); }
.slot { font-family: var(--font-mono); font-size: var(--fs-small); color: var(--text-muted); }
.stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(5.5rem, 1fr)); gap: var(--space-2); padding: var(--space-2) var(--space-3); }
.stat { display: flex; flex-direction: column; gap: 2px; }
.stat .k { font-size: var(--fs-small); color: var(--text-muted); }
.stat .v { font-family: var(--font-mono); font-variant-numeric: tabular-nums; }
.path { padding: 0 var(--space-3) var(--space-2); font-family: var(--font-mono); font-size: var(--fs-small); color: var(--text-muted); overflow-wrap: anywhere; }
.failure { padding: 0 var(--space-3) var(--space-2); font-size: var(--fs-small); color: var(--danger); overflow-wrap: anywhere; }
.hint { margin: 0; padding: var(--space-4) var(--space-3); text-align: center; color: var(--text-muted); }
`;

const ROLES = ['source', 'target'];

export default function (plugin) {
	plugin.register_panel({
		id: 'latency',
		title: 'Latency',
		accepts_node: true,
		mount(element, panel) {
			const root = element.attachShadow({ mode: 'open' });
			root.innerHTML = `<style>${STYLE}</style>
<div class="latency" data-testid="latency-panel">
	<div class="bar"><span class="dot"></span><span class="status" data-testid="latency-status">Idle</span><button class="primary" data-testid="latency-toggle">Start</button></div>
	<div class="ends">${ROLES.map((role) => `<div class="row" data-node-drop="${escape(`${panel.panel_id}#${role}`)}" data-testid="latency-${role}"><span class="role">${role}</span><span class="name empty">Drop a node here</span><span class="slot"></span></div>`).join('')}</div>
	<div class="failure" role="alert" hidden></div>
	<div class="stats" hidden></div>
	<div class="path" data-testid="latency-csv" hidden></div>
	<p class="hint">Drag the node the tick starts at onto the source row and the node it ends at onto the target row, then start. Each tick of the target writes one line: the patch time it ran, the latest source tick before it, and the milliseconds between.</p>
</div>`;
			const dot = root.querySelector('.dot');
			const status = root.querySelector('.status');
			const toggle = root.querySelector('[data-testid=latency-toggle]');
			const failure = root.querySelector('.failure');
			const stats = root.querySelector('.stats');
			const path = root.querySelector('.path');
			const hint = root.querySelector('.hint');
			const rows = Object.fromEntries(ROLES.map((role) => [role, root.querySelector(`[data-testid=latency-${role}]`)]));
			let names = {};
			let drag = null;
			let running = false;
			let alive = true;
			let shown = '';

			const fail = (error) => {
				failure.textContent = String(error && error.message ? error.message : error);
				failure.hidden = false;
			};
			const clear = () => { failure.hidden = true; failure.textContent = ''; };
			const picked = () => {
				const state = panel.state && typeof panel.state === 'object' ? panel.state : {};
				return { source: typeof state.source === 'string' ? state.source : null, target: typeof state.target === 'string' ? state.target : null };
			};

			const ms = (v) => (v === null || v === undefined ? '–' : `${v.toFixed(2)} ms`);
			const renderRows = (reply) => {
				const ends = picked();
				for (const role of ROLES) {
					const row = rows[role];
					const end = reply && reply.running ? reply[role] : null;
					const uid = end ? end.uid : ends[role];
					const name = row.querySelector('.name');
					name.textContent = uid ? (end ? end.name : names[uid] || uid) : 'Drop a node here';
					name.classList.toggle('empty', !uid);
					row.querySelector('.slot').textContent = end ? `/${end.slot}` : '';
					row.classList.toggle('armed', !!drag);
					row.classList.toggle('target', !!drag && drag.zone === role);
				}
			};
			const render = (reply) => {
				renderRows(reply);
				running = !!(reply && reply.running);
				dot.classList.toggle('on', running);
				toggle.textContent = running ? 'Stop' : 'Start';
				const ends = picked();
				toggle.disabled = !running && !(ends.source && ends.target);
				if (!reply || !reply.folder) {
					status.textContent = 'Idle';
					stats.hidden = true;
					path.hidden = true;
					hint.hidden = false;
					return;
				}
				hint.hidden = true;
				status.textContent = running ? `Recording · ${reply.ticks} ticks` : `Done · ${reply.ticks} ticks`;
				stats.hidden = false;
				stats.innerHTML = [
					['ticks', String(reply.ticks)],
					['last', ms(reply.last_ms)],
					['mean', ms(reply.mean_ms)],
					['min', ms(reply.min_ms)],
					['max', ms(reply.max_ms)],
					['unpaired', String(reply.unpaired)]
				].map(([k, v]) => `<div class="stat"><span class="k">${k}</span><span class="v" data-stat="${k}">${escape(v)}</span></div>`).join('');
				path.hidden = false;
				path.textContent = reply.csv;
				if (reply.error) fail(reply.error);
			};

			const refresh = async () => {
				const reply = await panel.call('plugin latency status');
				if (!alive) return;
				const text = JSON.stringify([reply, picked(), names, drag && drag.zone]);
				if (text === shown) return;
				shown = text;
				render(reply);
			};
			const named = async () => {
				const ends = picked();
				const wanted = ROLES.map((role) => ends[role]).filter((uid) => uid && !names[uid]);
				if (wanted.length === 0) return;
				const nodes = (await panel.call('session state')).nodes;
				for (const uid of wanted) if (nodes[uid]) names[uid] = nodes[uid].name;
			};

			toggle.onclick = async () => {
				clear();
				toggle.disabled = true;
				try {
					if (running) await panel.call('plugin latency stop');
					else {
						const ends = picked();
						await panel.call('plugin latency start', { source: ends.source, target: ends.target });
					}
				} catch (error) { fail(error); }
				shown = '';
				await refresh().catch(fail);
			};
			const stopDrop = panel.on_node_drop((drop) => {
				clear();
				const role = ROLES.includes(drop.zone) ? drop.zone : null;
				if (!role) return fail('Drop the node on the source row or the target row');
				if (running) return fail('Stop the run before changing its ends');
				panel.set_state({ ...picked(), [role]: drop.node });
				void named().then(refresh).catch(fail);
			});
			const stopDrag = panel.on_node_drag((next) => {
				drag = next;
				void refresh().catch(fail);
			});

			const timer = setInterval(() => void refresh().catch(fail), 500);
			void named().then(refresh).catch(fail);
			return () => {
				alive = false;
				clearInterval(timer);
				stopDrop();
				stopDrag();
				toggle.onclick = null;
			};
		}
	});
}

function escape(text) {
	return String(text).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}
