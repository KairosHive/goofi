import { describe, it, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { python } from '@codemirror/lang-python';
import { exprContext, entriesFor, type ExprContext } from './complete';
import type { ExprCatalogue } from './catalogue';

/* The completion source, driven exactly the way the running editor drives it — a real Python parse
 * plus a cursor offset — against a FAKE catalogue. That is the whole testability seam: the component
 * cannot mount in vitest, but the decision "what does this cursor position mean" is pure and is
 * where every interesting case lives.
 *
 * The cases are chosen to be the ones a REGEX would get wrong, because using the Lezer tree instead
 * of a regex is the design decision under test: a nested call, a binary operand, a second argument,
 * a string that belongs to some other callee, a comment, an attribute one level past a slot, and
 * the argument a cursor sits at inside a builtin's call.
 */

const CAT: ExprCatalogue = {
	nodes: [
		{
			name: 'oscillator0',
			engine: 'signal',
			slots: [{ name: 'out', dtype: 'array' }],
			params: [
				{ group: 'oscillator', names: ['frequency', 'amplitude'] },
				{ group: 'common', names: ['autotrigger', 'max_frequency'] }
			]
		},
		{ name: 'buffer0', engine: 'signal', slots: [{ name: 'out', dtype: 'array' }], params: [{ group: 'buffer', names: ['size', 'axis'] }] },
		{
			name: 'spectrum0',
			engine: 'signal',
			slots: [
				{ name: 'psd', dtype: 'array' },
				{ name: 'freqs', dtype: 'array' }
			],
			params: []
		}
	],
	self: 'buffer0',
	variables: [
		{ name: 'signal.default_ufreq', group: 'signal', element: 'default_ufreq', form: 'number' },
		{ name: 'mixer.gain', group: 'mixer', element: 'gain', form: 'number' },
		{ name: 'mixer.pan', group: 'mixer', element: 'pan', form: 'number' }
	],
	playheads: [{ name: 'walker', machine: 'sweep', fields: ['state', 'progress', 'gain'] }]
};

/** The parsed state the editor would have. `python()` is the same grammar the editor loads. */
function at(doc: string, pos = doc.length, machine = false): ExprContext | null {
	return exprContext(EditorState.create({ doc, extensions: [python()] }), pos, machine);
}

/** What the popup would be offered at this cursor — `[]` when the goofi source stands down. */
function labels(doc: string, pos = doc.length): string[] {
	const c = at(doc, pos);
	return c ? entriesFor(c, CAT).map((o) => o.label) : [];
}

const byLabel = (doc: string, cat: ExprCatalogue = CAT, pos = doc.length, machine = false) => {
	const c = at(doc, pos, machine)!;
	return new Map(entriesFor(c, cat).map((o) => [o.label, o]));
};

describe('nd() node names', () => {
	it('offers every node name inside the first argument', () => {
		expect(labels("nd('")).toEqual(['oscillator0', 'buffer0', 'spectrum0']);
	});

	it('reads a double-quoted argument the same way', () => {
		expect(labels('nd("')).toEqual(['oscillator0', 'buffer0', 'spectrum0']);
	});

	// Partially typed: the source still offers the whole list and lets CodeMirror filter it, but the
	// replaced range must cover what is already typed or accepting would duplicate it.
	it('replaces the whole partially-typed name, not just the caret', () => {
		const c = at("nd('osc");
		expect(c?.kind).toBe('node');
		expect(c && [c.from, c.to], 'from = after the quote, to = end of the literal').toEqual([4, 7]);
	});

	// A terminated literal has its whole CONTENT replaced — `to` is the closing quote, not the caret,
	// so accepting `buffer0` with the caret two characters in does not leave the tail behind.
	it('replaces the whole literal when the caret sits inside a closed one', () => {
		const c = at("nd('oscillator0')", 5);
		expect(c?.kind).toBe('node');
		expect(c && [c.from, c.to], 'the literal’s content, not [caret, caret]').toEqual([4, 15]);
	});

	// The two cases a regex reads as one: `nd('` nested inside another call, and `nd('` as the right
	// operand of an expression. The tree resolves the enclosing CallExpression either way.
	it('works in a nested call and in a binary operand', () => {
		expect(labels("max(nd('a"), 'nested inside max(…)').toContain('oscillator0');
		expect(labels("nd('osc').out.data.mean() + nd('"), 'the second nd() in a sum').toContain('buffer0');
	});

	// Only the FIRST argument names a node — `nd` takes one, and a regex looking for `nd('…` would
	// happily complete the second.
	it('stands down on a later argument', () => {
		expect(labels("nd('a', '")).toEqual([]);
	});

	it('stands down inside some other callee’s string, and inside a comment', () => {
		expect(labels("open('"), 'a string argument to something else').toEqual([]);
		expect(labels("# nd('"), 'a comment').toEqual([]);
		expect(labels("'plain string'", 6), 'a bare string literal').toEqual([]);
	});

	// Before the quote is typed the names are still the answer: accepting one writes the quotes.
	it('offers the names, quoted, right after the paren', () => {
		const e = byLabel('nd(');
		expect(e.get('oscillator0')?.apply).toBe("'oscillator0')");
		expect(e.get('spectrum0')?.apply).toBe("'spectrum0').out.");
	});
});

// The completer knows each node's arity from the store, so it can say what the engine will do with
// the bare form — the graph refuses a bare multi-output reference as ambiguous — and steer to
// `.out.<slot>` instead of leaving the user to discover the refusal at bind time.
describe('output arity is surfaced, not left to the eval error', () => {
	it('a single-output node says it may be used bare', () => {
		const e = byLabel("nd('").get('oscillator0')!;
		expect(e.detail).toMatch(/bare/);
		expect(e.detail).not.toMatch(/ambiguous/);
	});

	it('a multi-output node says the bare form is ambiguous', () => {
		const e = byLabel("nd('").get('spectrum0')!;
		expect(e.detail).toMatch(/2 outputs/);
		expect(e.detail).toMatch(/ambiguous/);
	});

	// "Prefer the `.out.<slot>` form" as behaviour rather than prose: accepting finishes the call,
	// and for a multi-output node it lands past `.out.` so the slot list is the next thing to open.
	it('accepting finishes the call, and a multi-output node lands past .out.', () => {
		expect(byLabel("nd('").get('spectrum0')!.apply, 'steers to .out.<slot>').toBe("spectrum0').out.");
		expect(byLabel("nd('").get('oscillator0')!.apply, 'usable bare — just close the call').toBe("oscillator0')");
	});

	// The literal can be closed while the CALL is not (the user typed both quotes and stopped), and
	// the two are tracked separately because only the unwritten characters may be added.
	it('closes only what is not written yet', () => {
		expect(byLabel("nd('a'", CAT, 5).get('spectrum0')!.apply).toBe('spectrum0).out.');
	});

	// Both already written: inserting either would corrupt the call, and there is nowhere to put the
	// steering dot — the `)` is past the caret — so the plain name is the whole answer.
	it('inserts the bare name into an already-finished call', () => {
		expect(byLabel("nd('osc')", CAT, 5).get('spectrum0')!.apply).toBeUndefined();
	});
});

describe('the reference paths: .out, .params, me and sm', () => {
	it('offers the two namespaces after the reference', () => {
		expect(labels("nd('spectrum0').")).toEqual(['out', 'params']);
		expect(labels('me.')).toEqual(['out', 'params']);
	});

	it('offers THAT node’s slots after .out.', () => {
		expect(labels("nd('spectrum0').out.")).toEqual(['psd', 'freqs']);
		expect(labels("nd('oscillator0').out.")).toEqual(['out']);
	});

	it('carries the slot’s dtype as the detail', () => {
		expect(entriesFor(at("nd('spectrum0').out.")!, CAT)[0].detail).toBe('array');
	});

	it('replaces a partially-typed slot name', () => {
		const c = at("nd('spectrum0').out.p");
		expect(c?.kind).toBe('slot');
		expect(c && [c.from, c.to]).toEqual([20, 21]);
		expect(labels("nd('spectrum0').out.p")).toEqual(['psd', 'freqs']);
	});

	it('offers the groups after .params. and the names one level deeper', () => {
		expect(labels("nd('oscillator0').params.")).toEqual(['oscillator', 'common']);
		expect(labels("nd('oscillator0').params.oscillator.")).toEqual(['frequency', 'amplitude']);
	});

	it('me reads the editing node’s own groups', () => {
		expect(labels('me.params.')).toEqual(['buffer']);
		expect(labels('me.params.buffer.')).toEqual(['size', 'axis']);
		expect(labels('me.out.')).toEqual(['out']);
	});

	it('me offers nothing deeper without a self in the catalogue', () => {
		expect(entriesFor(at('me.params.')!, { ...CAT, self: undefined }).map((o) => o.label)).toEqual([]);
	});

	// One level further out is the value's OWN attribute (`.data`, `.mean`) — the engine's, not ours.
	it('stands down one level past the slot', () => {
		expect(labels("nd('oscillator0').out.out.")).toEqual([]);
	});

	it('stands down when the node name is not a literal it can read', () => {
		expect(labels('nd(name).')).toEqual([]);
	});

	it('stands down for an unknown node name after .out.', () => {
		expect(labels("nd('nope').out.")).toEqual([]);
	});

	// A param reads a playhead through `sm()`: the machine's own four elements, then the attributes.
	it('sm() names a playhead in a param’s expression, and its elements after the dot', () => {
		expect(labels("sm('")).toEqual(['walker']);
		expect(byLabel("sm('").get('walker')!.apply).toBe("walker').");
		expect(labels("sm('walker').")).toEqual(['state', 'progress', 'gain']);
		expect(labels("sm('nope').")).toEqual([]);
		expect(labels("sm('walker').state.")).toEqual([]);
	});
});

describe('vars. and np.', () => {
	it('offers the GROUPS after vars., and the group’s elements after one', () => {
		expect(labels('vars.')).toEqual(['signal', 'mixer']);
		expect(labels('vars.mi'), 'partially typed').toEqual(['signal', 'mixer']);
		expect(labels('vars.mixer.')).toEqual(['gain', 'pan']);
		expect(labels('vars.mixer.ga'), 'partially typed').toEqual(['gain', 'pan']);
	});

	it('carries the element’s form as the detail', () => {
		expect(entriesFor(at('vars.mixer.')!, CAT)[0].detail).toBe('number');
	});

	it('offers the curated numpy surface after np.', () => {
		const np = labels('np.');
		expect(np).toContain('mean');
		expect(np).toContain('pi');
		expect(np).toContain('linspace');
	});

	// The one that a regex on `\.$` would get wrong: a member access on something else entirely.
	it('stands down on any other object’s attribute', () => {
		expect(labels('t.')).toEqual([]);
		expect(labels("nd('a').out.data.")).toEqual([]);
	});

	it('reads vars. as the right operand of an expression', () => {
		expect(labels("nd('a').out + vars.")).toEqual(['signal', 'mixer']);
	});
});

/* A machine's expression is read against its own scope: `sm('…')` names a state, answering the
 * manager's four fields, or a playhead, answering what `me` does; `me` is the playhead with its own
 * fields and one per attribute; a node is readable as a param reads one. */
describe('the machine scope', () => {
	const MACHINE: ExprCatalogue = { ...CAT, machine: { states: ['A', 'B'], playheads: ['walker'], fields: ['name', 'state', 'arrived', 'gain'] } };
	const machine = (doc: string, pos = doc.length): string[] => {
		const c = at(doc, pos, true);
		return c ? entriesFor(c, MACHINE).map((o) => o.label) : [];
	};

	it('offers the states and the playheads inside sm(…), and lands past the dot', () => {
		expect(machine("sm('")).toEqual(['A', 'B', 'walker']);
		expect(entriesFor(at("sm('", 4, true)!, MACHINE)[0].apply).toBe("A').");
	});

	it('offers a state’s fields after sm(…)., a playhead’s for a playhead, and nothing deeper', () => {
		expect(machine("sm('A').")).toEqual(['residents', 'playheads', 'arrivals', 'departures']);
		expect(machine("sm('walker').")).toEqual(['name', 'state', 'arrived', 'gain']);
		expect(machine("sm('A').playheads.")).toEqual([]);
	});

	it('reads me as the playhead', () => {
		expect(machine('me.')).toEqual(['name', 'state', 'arrived', 'gain']);
		expect(machine('me.gain.')).toEqual([]);
	});

	it('reads a node as a param does', () => {
		expect(machine("nd('")).toEqual(['oscillator0', 'buffer0', 'spectrum0']);
		expect(machine("nd('spectrum0').out.")).toEqual(['psd', 'freqs']);
	});

	it('offers sm, me and nd at a name, and keeps the shared functions', () => {
		const got = machine('s');
		for (const name of ['sm', 'me', 'nd', 't', 'np', 'vars', 'lfo', 'nse', 'qtz', 'sin']) expect(got, name).toContain(name);
		expect(got).not.toContain('time');
	});

	it('documents sm as a state or a playhead there, and as a playhead for a param', () => {
		expect(byLabel('s', MACHINE, 1, true).get('sm')!.detail).toMatch(/state/);
		expect(byLabel('s').get('sm')!.detail).toMatch(/playhead/);
	});
});

/* The names the evaluator injects (`backend/signal/goofi-python/src/inproc/expr.rs`) are the scope,
 * and nothing else in the app tells the user they exist. They are offered at every position a name
 * may begin — the empty field, after an operator, a comma or a paren — not only once one is typed. */
describe('the injected scope', () => {
	it('offers the evaluator scope at a partially-typed name', () => {
		const got = labels('n');
		for (const name of ['nd', 'sm', 'me', 't', 'np', 'vars', 'sin', 'pi', 'lfo', 'nse', 'qtz']) {
			expect(got, `\`${name}\` is in the scope`).toContain(name);
		}
		expect(got, 'time() is gone: `t` is the clock').not.toContain('time');
	});

	it('replaces the partially-typed name', () => {
		const c = at('nd');
		expect(c?.kind).toBe('scope');
		expect(c && [c.from, c.to]).toEqual([0, 2]);
	});

	it('opens on the empty field and after an operator, a comma, a paren', () => {
		for (const doc of ['', 'a + ', 'max(1, ', 'abs(', 'x = ']) {
			const c = at(doc);
			expect(c?.kind, JSON.stringify(doc)).toBe('scope');
			expect(c && [c.from, c.to]).toEqual([doc.length, doc.length]);
			expect(labels(doc)).toContain('lfo');
		}
	});

	// A property name is not a scope position — `x.nd` must not offer the scope; nor is mid-number.
	it('stands down after a dot, inside a string and inside a number', () => {
		expect(labels('t.n')).toEqual([]);
		expect(labels("'n'", 2)).toEqual([]);
		expect(labels('x.')).toEqual([]);
		expect(labels('1.')).toEqual([]);
	});

	it('accepts a function as a call with the cursor inside, and a namespace with its dot', () => {
		const e = byLabel('l');
		expect(typeof e.get('lfo')!.apply, 'a snippet').toBe('function');
		expect(e.get('vars')!.apply).toBe('vars.');
		expect(e.get('np')!.apply).toBe('np.');
	});
});

/* Inside a builtin's call the popup is the argument list: what may still be given, each with its
 * type and default; an argument that takes one of some strings lists them; and every option carries
 * the call it sits in, so the pane shows the function with the argument under the cursor marked. */
describe('inside a builtin’s call', () => {
	it('offers the keyword arguments right after the paren, the scope after them', () => {
		const c = at('lfo(');
		expect(c?.kind).toBe('argument');
		const got = labels('lfo(');
		expect(got.slice(0, 5)).toEqual(['freq=', 'src=', 'vmin=', 'vmax=', 'fn=']);
		expect(got).toContain('nd');
		expect(byLabel('lfo(').get('freq=')!.detail).toBe('number · 1');
		expect(byLabel('lfo(').get('vmin=')!.detail).toMatch(/minimum/);
	});

	it('drops an argument already written, and reads the cursor’s slot', () => {
		const got = labels('lfo(freq=1, ');
		expect(got).not.toContain('freq=');
		expect(got.slice(0, 4)).toEqual(['src=', 'vmin=', 'vmax=', 'fn=']);
		const c = at('lfo(freq=1, vm');
		expect(c?.kind).toBe('scope');
		expect(c && 'call' in c && c.call?.fn.name).toBe('lfo');
	});

	it('names the argument the cursor is in', () => {
		const c = at('lfo(freq=n');
		expect(c?.kind).toBe('scope');
		expect(c && 'call' in c && c.call?.arg?.name).toBe('freq');
		const d = at("qtz(x, 'major', ");
		expect(d?.kind).toBe('argument');
		expect(d && 'call' in d && d.call?.arg?.name).toBe('root');
	});

	it('lists the options of an argument that takes some strings, quoted as needed', () => {
		expect(labels("lfo(fn='")).toEqual(['sin', 'cos', 'tri', 'sqr', 'saw']);
		expect(byLabel("lfo(fn='").get('tri')!.apply).toBe("tri'");
		expect(byLabel("lfo(fn='tr").get('tri')!.apply).toBe("tri'");
		expect(byLabel("lfo(fn='tri'", CAT, 8).get('saw')!.apply, 'the closing quote is there').toBe('saw');
		expect(byLabel('lfo(fn=').get('sqr')!.apply, 'no quote typed: both are written').toBe("'sqr'");
	});

	it('lists a positional option argument’s strings in place', () => {
		expect(labels("qtz(x, '")).toContain('major');
		expect(labels("qtz(x, 'major', '")).toContain('C#');
		expect(labels("qtz(x, 'major', 'C', '")).toEqual(['voct', 'hz']);
		const got = labels('qtz(x, ');
		expect(got.slice(0, 2), 'the scale options lead, in the backend’s order').toEqual(['chromatic', 'major']);
		expect(byLabel('qtz(x, ').get('major')!.apply).toBe("'major'");
	});

	it('stands down inside a number and inside an unknown call', () => {
		expect(at('lfo(freq=0.')).toBeNull();
		const c = at('other(');
		expect(c?.kind).toBe('scope');
		expect(c && 'call' in c ? c.call : undefined).toBeUndefined();
	});

	it('hands every option the call it sits in, for the pane', () => {
		const c = at('lfo(')!;
		const opts = entriesFor(c, CAT);
		for (const o of opts.slice(0, 5)) expect(typeof o.info, o.label).toBe('function');
	});
});
