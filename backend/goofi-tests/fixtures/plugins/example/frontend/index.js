export default function (plugin, ctx) {
	const entry = plugin.register_header({
		id: 'subject',
		label: 'Subject: none',
		onActivate: async () => {
			await ctx.call('plugin example subject select', { subject: 'Header' });
			await refresh();
		}
	});
	const refresh = async () => {
		const status = await ctx.call('plugin example status');
		entry.update({ label: `Subject: ${status.subject}` });
	};
	plugin.register_panel({
		id: 'corpus',
		title: 'Corpus',
		mount(element, panel) {
			const root = element.attachShadow({ mode: 'open' });
			root.innerHTML = `<style>:host { display: block; } form { display: grid; gap: .5rem; padding: 1rem; color: inherit; font: inherit; } input, button { font: inherit; min-height: 2.5rem; min-width: 0; }</style><form><label>Subject <input name="subject" aria-label="Subject" value="Alice"></label><button>Select subject</button><output role="status"></output><button type="button" id="remove">Remove header entry</button></form>`;
			const form = root.querySelector('form');
			const output = root.querySelector('output');
			form.onsubmit = async (event) => {
				event.preventDefault();
				try {
					const subject = root.querySelector('input').value;
					await panel.call('plugin example subject select', { subject });
					await refresh();
					output.textContent = `Selected ${subject}`;
				} catch (error) { output.textContent = String(error); }
			};
			root.querySelector('#remove').onclick = () => entry.dispose();
			return () => { form.onsubmit = null; root.querySelector('#remove').onclick = null; };
		}
	});
	void refresh().catch((error) => ctx.log(String(error), 'error'));
	return () => entry.dispose();
}
