/** goofi-aware completion for the expression editor: one source that answers the goofi scope, the
 *  reference paths, a builtin's arguments and their options, and folds Python's own sources in, so
 *  every option in the one popup carries a doc for the pane. */
import { syntaxTree } from '@codemirror/language';
import { globalCompletion, localCompletionSource, pythonLanguage } from '@codemirror/lang-python';
import { snippetCompletion, type Completion, type CompletionContext, type CompletionResult } from '@codemirror/autocomplete';
import type { EditorState, Extension } from '@codemirror/state';
import type { SyntaxNode } from '@lezer/common';
import { EXPR_BUILTINS, EXPR_MATH, EXPR_MATH_CONSTANTS, STATE_FIELDS, type ExprArgInfo, type ExprBuiltinInfo } from '$lib/api/vocab';
import type { CatalogueNode, ExprCatalogue } from './catalogue';
import type { Doc } from './doc';

/** What a member chain hangs off: `me`, `nd('name')` or `sm('name')`. */
export type RefTarget = { self: true } | { name: string } | { machine: string };

/** Inside the first argument of `nd(` or `sm(`: a node's name, or a state's or a playhead's. */
export interface NameContext {
	kind: 'node' | 'machine';
	from: number;
	to: number;
	/** The quote the literal opened with — reused to close it on accept; empty before one is typed. */
	quote: string;
	terminated: boolean;
	callClosed: boolean;
}

/** The builtin call the cursor sits in, and the argument it sits at. */
export interface CallSite {
	fn: ExprBuiltinInfo;
	/** The argument the cursor is at: by keyword, else by position; undefined past the last. */
	arg?: ExprArgInfo;
}

/** An option with the doc its row and the window show. */
export type GoofiCompletion = Completion & { doc: Doc };

export type ExprContext =
	| NameContext
	| { kind: 'path'; target: RefTarget; from: number; to: number }
	| { kind: 'slot'; target: RefTarget; from: number; to: number }
	| { kind: 'group'; target: RefTarget; from: number; to: number }
	| { kind: 'param'; target: RefTarget; group: string; from: number; to: number }
	| { kind: 'variables'; from: number; to: number }
	| { kind: 'variableGroup'; group: string; from: number; to: number }
	| { kind: 'numpy'; from: number; to: number }
	/** At the start of an argument of a builtin call: its arguments not written yet, then the scope. */
	| { kind: 'argument'; call: CallSite; written: string[]; from: number; to: number }
	/** Inside the value of an argument that takes one of some strings. */
	| { kind: 'option'; call: CallSite; from: number; to: number; quote: string; terminated: boolean }
	/** A name position: the scope, and the call it sits in for the pane. */
	| { kind: 'scope'; call?: CallSite; from: number; to: number };

const text = (state: EditorState, node: SyntaxNode): string => state.doc.sliceString(node.from, node.to);

/** The builtin `call` is, or null for a call of anything else. */
function builtinOf(state: EditorState, call: SyntaxNode | null, machine: boolean): ExprBuiltinInfo | null {
	if (!call || call.name !== 'CallExpression') return null;
	const callee = call.firstChild;
	if (!callee || callee.name !== 'VariableName') return null;
	return builtin(text(state, callee), machine);
}

/** The builtins a scope reads: the shared ones and the scope's own, one entry per name. */
export function builtins(machine: boolean): ExprBuiltinInfo[] {
	return EXPR_BUILTINS.filter((b) => b.scope === 'all' || b.scope === (machine ? 'machine' : 'node'));
}

function builtin(name: string, machine: boolean): ExprBuiltinInfo | null {
	return builtins(machine).find((b) => b.name === name) ?? null;
}

/** An `ArgList`'s first ACTUAL argument, past the parens/commas and the parser's error markers. */
function firstArg(argList: SyntaxNode): SyntaxNode | null {
	for (let c = argList.firstChild; c; c = c.nextSibling) {
		if (c.name !== '(' && c.name !== ')' && c.name !== ',' && c.name !== '⚠') return c;
	}
	return null;
}

/** The arguments written in `argList`, in order: `{ keyword }` for `name=…`, null for a positional. */
function written(state: EditorState, argList: SyntaxNode): (string | null)[] {
	const out: (string | null)[] = [];
	let first: SyntaxNode | null = null;
	let keyword: string | null = null;
	const flush = () => {
		if (first) out.push(keyword);
		first = null;
		keyword = null;
	};
	for (let c = argList.firstChild; c; c = c.nextSibling) {
		if (c.name === '(' || c.name === ')' || c.name === '⚠') continue;
		if (c.name === ',') {
			flush();
			continue;
		}
		if (!first) first = c;
		else if (c.name === 'AssignOp' && first.name === 'VariableName') keyword = text(state, first);
	}
	flush();
	return out;
}

/** Which argument of `argList` holds `pos`, as an index, and whether `pos` sits at its very start
 *  (nothing of it written before the cursor); the keyword it was given, if any. */
function argAt(state: EditorState, argList: SyntaxNode, pos: number): { index: number; keyword: string | null; atStart: boolean; afterAssign: boolean } {
	let index = 0;
	let first: SyntaxNode | null = null;
	let keyword: string | null = null;
	let afterAssign = false;
	let lastEnd = argList.from + 1;
	for (let c = argList.firstChild; c; c = c.nextSibling) {
		if (c.name === '(' || c.name === ')' || c.name === '⚠') continue;
		if (c.from >= pos) break;
		if (c.name === ',') {
			index++;
			first = null;
			keyword = null;
			afterAssign = false;
			lastEnd = c.to;
			continue;
		}
		if (!first) first = c;
		else if (c.name === 'AssignOp' && first.name === 'VariableName') {
			keyword = text(state, first);
			afterAssign = c.to <= pos;
		}
		lastEnd = c.to;
	}
	const between = state.doc.sliceString(lastEnd, pos);
	const atStart = (first === null || first.from >= pos) && /^\s*$/.test(between);
	return { index, keyword, atStart, afterAssign };
}

/** The argument of `fn` at a position: the named one, else the `index`th in its signature. */
function argOf(fn: ExprBuiltinInfo, index: number, keyword: string | null): ExprArgInfo | undefined {
	if (keyword !== null) return fn.args.find((a) => a.name === keyword);
	return fn.args[index];
}

/** The builtin call whose argument list holds `node`, and the site inside it. */
function callSite(state: EditorState, node: SyntaxNode, pos: number, machine: boolean): { call: CallSite; argList: SyntaxNode; at: ReturnType<typeof argAt> } | null {
	let argList: SyntaxNode | null = node.name === 'ArgList' ? node : node.parent;
	while (argList && argList.name !== 'ArgList') argList = argList.parent;
	if (!argList) return null;
	const fn = builtinOf(state, argList.parent, machine);
	if (!fn) return null;
	const at = argAt(state, argList, pos);
	return { call: { fn, arg: argOf(fn, at.index, at.keyword) }, argList, at };
}

/** A plain string literal's quote and body, and whether it is closed; null for any other literal. */
function literal(state: EditorState, str: SyntaxNode): { quote: string; body: string; terminated: boolean } | null {
	const raw = text(state, str);
	const quote = raw[0];
	const terminated = raw.length > 1 && raw.endsWith(quote);
	return quote === "'" || quote === '"' ? { quote, terminated, body: raw.slice(1, terminated ? -1 : undefined) } : null;
}

/** The string a `nd('…')` or `sm('…')` call names with, or null when it is not a plain literal. */
function argName(state: EditorState, call: SyntaxNode): string | null {
	const argList = call.getChild('ArgList');
	const first = argList && firstArg(argList);
	return first?.name === 'String' ? (literal(state, first)?.body ?? null) : null;
}

/** The cursor is inside a string: a name iff that string is `nd`'s or `sm`'s FIRST argument, an
 *  option iff it is an argument that takes one of some strings. */
function stringContext(state: EditorState, str: SyntaxNode, pos: number, machine: boolean): ExprContext | null {
	const argList = str.parent;
	if (!argList || argList.name !== 'ArgList') return null;
	const fn = builtinOf(state, argList.parent, machine);
	if (!fn) return null;
	const lit = literal(state, str);
	if (!lit) return null;
	// The literal's CONTENT: accepting replaces all of it, tail of the old name included.
	const from = str.from + 1;
	const to = lit.terminated ? str.to - 1 : str.to;
	if (pos < from || pos > to) return null;
	const at = argAt(state, argList, str.from + 1);
	const base = { from, to, quote: lit.quote, terminated: lit.terminated };
	if (fn.pick && at.index === 0 && at.keyword === null) {
		return { kind: fn.pick, ...base, callClosed: !!argList.getChild(')') };
	}
	const arg = argOf(fn, at.index, at.keyword);
	if (arg && arg.options.length > 0) return { kind: 'option', call: { fn, arg }, ...base };
	return null;
}

/** The reference a member chain hangs off — `nd('x')`, `sm('x')` or `me` — and the attributes between. */
function refHead(state: EditorState, obj: SyntaxNode, machine: boolean): { target: RefTarget; path: string[] } | null {
	const path: string[] = [];
	let cur: SyntaxNode | null = obj;
	while (cur && cur.name === 'MemberExpression') {
		const prop = cur.getChild('PropertyName');
		if (!prop) return null;
		path.unshift(text(state, prop));
		cur = cur.firstChild;
	}
	if (!cur) return null;
	if (cur.name === 'VariableName' && text(state, cur) === 'me') return { target: { self: true }, path };
	const fn = builtinOf(state, cur, machine);
	if (fn?.pick) {
		const name = argName(state, cur);
		return name === null ? null : { target: fn.pick === 'node' ? { name } : { machine: name }, path };
	}
	return null;
}

/** The cursor is at `<something>.` — whose attribute do we know? */
function memberContext(state: EditorState, node: SyntaxNode, pos: number, machine: boolean): ExprContext | null {
	let from = pos;
	let to = pos;
	if (node.name === 'PropertyName') {
		from = node.from;
		to = node.to;
	} else if (node.name !== '.') return null;
	const member = node.parent;
	if (!member || member.name !== 'MemberExpression') return null;
	const obj = member.firstChild;
	if (!obj) return null;
	if (obj.name === 'VariableName') {
		const name = text(state, obj);
		if (name === 'vars') return { kind: 'variables', from, to };
		if (name === 'np') return { kind: 'numpy', from, to };
	}
	// `vars.<group>.` — a variable's name has two halves, so the head here is itself a member.
	if (obj.name === 'MemberExpression') {
		const head = obj.firstChild;
		const group = head?.nextSibling?.nextSibling ?? null;
		if (head && head.name === 'VariableName' && text(state, head) === 'vars' && group) {
			return { kind: 'variableGroup', group: text(state, group), from, to };
		}
	}
	const head = refHead(state, obj, machine);
	if (!head) return null;
	const { target, path } = head;
	if (path.length === 0) return { kind: 'path', target, from, to };
	// A machine's record and a playhead's are one level deep: past that is Python's own.
	if ('machine' in target || (machine && 'self' in target)) return null;
	if (path[0] === 'out' && path.length === 1) return { kind: 'slot', target, from, to };
	if (path[0] === 'params' && path.length === 1) return { kind: 'group', target, from, to };
	if (path[0] === 'params' && path.length === 2) return { kind: 'param', target, group: path[1], from, to };
	// Deeper is the value's own attribute, and none of our business.
	return null;
}

/** Whether a name may begin at `pos`: the document's start, or after whitespace, a bracket, a
 *  comma, an assignment or an operator — never mid-word, after a dot or inside a literal. */
function startsName(state: EditorState, pos: number): boolean {
	if (pos === 0) return true;
	const before = state.doc.sliceString(pos - 1, pos);
	return /[\s(\[{,=+\-*/%<>!&|^~:]/.test(before);
}

/** What a cursor position means, and the range a completion there replaces; `null` leaves nothing
 *  to offer. `machine` says whose scope the expression is read in. */
export function exprContext(state: EditorState, pos: number, machine = false): ExprContext | null {
	const node = syntaxTree(state).resolveInner(pos, -1);
	if (node.name === 'Comment') return null;
	if (node.name === 'String') return stringContext(state, node, pos, machine);
	const member = memberContext(state, node, pos, machine);
	if (member) return member;
	// A real `VariableName` only — after a dot the name is a PropertyName and must not see the scope.
	if (node.name === 'VariableName') {
		const site = node.parent?.name === 'ArgList' || node.parent?.name === 'CallExpression' ? callSite(state, node, pos, machine) : null;
		const call = site && node.parent?.name === 'ArgList' ? site.call : undefined;
		return { kind: 'scope', ...(call ? { call } : {}), from: node.from, to: node.to };
	}
	if (node.name === 'PropertyName' || node.name === 'Number') return null;
	if (!startsName(state, pos)) return null;
	const site = callSite(state, node, pos, machine);
	if (site) {
		const { call, argList, at } = site;
		if (at.afterAssign) {
			return call.arg && call.arg.options.length > 0
				? { kind: 'option', call, from: pos, to: pos, quote: '', terminated: false }
				: { kind: 'scope', call, from: pos, to: pos };
		}
		if (at.atStart) return { kind: 'argument', call, written: written(state, argList).filter((w): w is string => w !== null), from: pos, to: pos };
		return { kind: 'scope', call, from: pos, to: pos };
	}
	return { kind: 'scope', from: pos, to: pos };
}

/** The builtin the cursor documents: the one whose name it is on, else the call it sits in with
 *  the argument it is at — off the tree as it stands, so `nse(` reads as `nse()` does. */
export function docSite(state: EditorState, pos: number, machine: boolean): CallSite | null {
	const node = syntaxTree(state).resolveInner(pos, -1);
	if (node.name === 'VariableName') {
		const fn = builtin(text(state, node), machine);
		if (fn) return { fn };
	}
	for (let n: SyntaxNode | null = node; n; n = n.parent) {
		if (n.name !== 'ArgList') continue;
		const fn = builtinOf(state, n.parent, machine);
		if (!fn) continue;
		const at = argAt(state, n, pos);
		return { fn, arg: argOf(fn, at.index, at.keyword) };
	}
	return null;
}

/** What `sm('…')` answers, in the order the manager declares the fields. */
const STATE_FIELD_DOC: Record<(typeof STATE_FIELDS)[number], string> = {
	residents: 'how many playheads rest in it',
	playheads: 'their names, longest resident first',
	arrivals: 'how many arrived at this instant',
	departures: 'how many left at this instant'
};

const PLAYHEAD_FIELD_DOC: Record<string, string> = {
	name: 'the playhead’s name',
	state: 'the state it rests in, or heads for',
	prev: 'the state it left, while in flight; empty at rest',
	progress: 'how far along its flight it is, 0 to 1',
	arrived: 'the patch second it arrived in its state'
};

/** The numpy names offered after `np.`: hand-picked, never the real namespace. */
const NUMPY_NAMES =
	'abs angle arange arccos arcsin arctan arctan2 argmax argmin argsort array asarray blackman ceil clip ' +
	'concatenate cos cosh cumsum degrees diff dot exp eye flip floor full hamming hanning hstack imag interp ' +
	'isfinite isnan linspace log log2 log10 max maximum mean median min minimum nanmean nanstd ones percentile ' +
	'power prod radians ravel real reshape roll round sign sin sinh sort sqrt square std sum tan tanh transpose ' +
	'unwrap var vstack where zeros';

/** One option with its doc attached for the window. */
function entry(c: Completion, doc: Doc): GoofiCompletion {
	return { ...c, doc };
}

/** What finishes a name typed inside `nd('` or `sm('`: only what is not written yet — an
 *  already-closed call's paren sits past the caret, so a dot inserted here would land inside it. */
function finish(ctx: NameContext, steer: string): string {
	return `${ctx.terminated ? '' : ctx.quote || "'"}${ctx.callClosed ? '' : `)${steer}`}`;
}

/** One node-name entry; a multi-output node steers to a `.slot`, since bare use of one raises. */
function nodeEntry(node: CatalogueNode, ctx: NameContext): Completion {
	const n = node.slots.length;
	const multi = n > 1;
	const detail = multi ? `${n} outputs — bare use is ambiguous, pick .out.<slot>` : n === 1 ? '1 output — usable bare' : 'no outputs';
	const tail = finish(ctx, multi ? '.out.' : '');
	const open = ctx.quote ? '' : "'";
	const apply = open || tail ? `${open}${node.name}${tail}` : undefined;
	const slots = node.slots.map((s) => `${s.name}: ${s.dtype}`).join(', ');
	return entry(
		{ label: node.name, detail, type: 'variable', apply },
		{ title: `nd('${node.name}')`, text: `A ${node.engine ?? 'port'} node. ${n === 0 ? 'No outputs.' : `Outputs: ${slots}.`} ${multi ? 'Read one as `.out.<slot>`.' : n === 1 ? 'Bare `nd()` is its one output.' : ''}`.trim() }
	);
}

/** The catalogue entry a reference target names — the editing node itself for `me`. */
function nodeOf(cat: ExprCatalogue, target: RefTarget): CatalogueNode | undefined {
	const name = 'self' in target ? cat.self : 'name' in target ? target.name : undefined;
	return name == null ? undefined : cat.nodes.find((n) => n.name === name);
}

/** The scope's names as options: the builtins first, then math, each with its doc. */
function scopeEntries(cat: ExprCatalogue): Completion[] {
	const own = builtins(!!cat.machine).map((b) => {
		const type = b.kind === 'function' ? 'function' : b.kind === 'namespace' ? 'namespace' : 'variable';
		const base: Completion = { label: b.name, type, detail: b.signature, boost: 90 };
		const doc: Doc = { builtin: b };
		if (b.kind === 'function') return entry(snippetCompletion(`${b.name}(#{})`, base), doc);
		if (b.kind === 'namespace') return entry({ ...base, apply: `${b.name}.` }, doc);
		return entry(base, doc);
	});
	const math = [
		...EXPR_MATH.map((label) => entry({ label, type: 'function', detail: 'math', boost: 50 }, { title: `${label}()`, text: `Python’s math.${label}, in scope as itself.` })),
		...EXPR_MATH_CONSTANTS.map((label) => entry({ label, type: 'constant', detail: 'math', boost: 50 }, { title: label, text: `Python’s math.${label}.` }))
	];
	return [...own, ...math];
}

/** A playhead's fields, under `sm('name').` in a param's expression, or a machine's playhead's. */
function playheadFields(fields: string[]): Completion[] {
	return fields.map((label) =>
		entry({ label, type: 'property', detail: PLAYHEAD_FIELD_DOC[label] ? 'the machine’s own' : 'attribute' }, { title: `.${label}`, text: PLAYHEAD_FIELD_DOC[label] ?? 'An attribute the states set and the transitions ease.' })
	);
}

/** The options of an argument that takes one of some strings, quoted as the site needs. */
function optionEntries(call: CallSite, quote: string, terminated: boolean): Completion[] {
	const arg = call.arg;
	if (!arg) return [];
	const open = quote || "'";
	return arg.options.map((o) =>
		entry(
			{ label: o, type: 'constant', detail: arg.name, boost: 100, apply: quote ? `${o}${terminated ? '' : quote}` : `${open}${o}${open}` },
			{ title: `${arg.name}=${open}${o}${open}`, text: `One of \`${arg.name}\`’s ${arg.options.length} options for ${call.fn.name}().`, arg }
		)
	);
}

/** What the popup is offered at a classified position. */
export function entriesFor(ctx: ExprContext, cat: ExprCatalogue): Completion[] {
	switch (ctx.kind) {
		case 'node':
			return cat.nodes.map((n) => nodeEntry(n, ctx));
		case 'machine': {
			const open = ctx.quote ? '' : "'";
			const tail = finish(ctx, '.');
			const apply = (label: string) => (open || tail ? `${open}${label}${tail}` : undefined);
			if (cat.machine) {
				return [
					...cat.machine.states.map((label) => entry({ label, detail: 'state', type: 'variable', apply: apply(label) }, { title: `sm('${label}')`, text: 'A state: its census, `.residents`, `.playheads`, `.arrivals` and `.departures`.' })),
					...cat.machine.playheads.map((label) => entry({ label, detail: 'playhead', type: 'variable', apply: apply(label) }, { title: `sm('${label}')`, text: 'A playhead: its `.name`, `.state`, `.arrived` and one field per attribute.' }))
				];
			}
			return cat.playheads.map((p) => entry({ label: p.name, detail: `playhead of ${p.machine}`, type: 'variable', apply: apply(p.name) }, { title: `sm('${p.name}')`, text: `A playhead of ${p.machine}: \`.state\`, \`.prev\`, \`.progress\`, \`.arrived\` or an attribute.` }));
		}
		case 'path':
			if ('machine' in ctx.target) {
				const name = ctx.target.machine;
				if (cat.machine) {
					if (cat.machine.states.includes(name)) {
						return STATE_FIELDS.map((label) => entry({ label, detail: STATE_FIELD_DOC[label], type: 'property' }, { title: `.${label}`, text: STATE_FIELD_DOC[label] }));
					}
					return cat.machine.playheads.includes(name) ? playheadFields(cat.machine.fields) : [];
				}
				const p = cat.playheads.find((p) => p.name === name);
				return p ? playheadFields(p.fields) : [];
			}
			if (cat.machine) {
				return 'self' in ctx.target ? playheadFields(cat.machine.fields) : [];
			}
			return [
				entry({ label: 'out', type: 'namespace', detail: 'output slots — .out.<slot>', apply: 'out.' }, { title: '.out.<slot>', text: 'One output slot by name. A node with one output needs no slot.' }),
				entry({ label: 'params', type: 'namespace', detail: 'params — .params.<group>.<param>', apply: 'params.' }, { title: '.params.<group>.<param>', text: 'A param’s current value, as the node holds it.' })
			];
		case 'slot':
			return (nodeOf(cat, ctx.target)?.slots ?? []).map((s) => entry({ label: s.name, detail: s.dtype, type: 'property' }, { title: `.out.${s.name}`, text: `An output of dtype ${s.dtype}.` }));
		case 'group':
			return (nodeOf(cat, ctx.target)?.params ?? []).map((g) =>
				entry({ label: g.group, detail: `${g.names.length} param${g.names.length === 1 ? '' : 's'}`, type: 'namespace', apply: `${g.group}.` }, { title: `.params.${g.group}`, text: `Params: ${g.names.join(', ')}.` })
			);
		case 'param':
			return (nodeOf(cat, ctx.target)?.params ?? [])
				.filter((g) => g.group === ctx.group)
				.flatMap((g) => g.names.map((n) => entry({ label: n, type: 'property' }, { title: `.params.${g.group}.${n}`, text: 'The param’s current value: a number, a list, a bool or a text.' })));
		case 'variables': {
			const counts = new Map<string, number>();
			for (const v of cat.variables) counts.set(v.group, (counts.get(v.group) ?? 0) + 1);
			return [...counts].map(([label, n]) => entry({ label, detail: `${n} variable${n === 1 ? '' : 's'}`, type: 'namespace', apply: `${label}.` }, { title: `vars.${label}`, text: `A group of ${n} variable${n === 1 ? '' : 's'}.` }));
		}
		case 'variableGroup':
			return cat.variables.filter((g) => g.group === ctx.group).map((g) => entry({ label: g.element, detail: g.form ?? 'variable', type: 'variable' }, { title: `vars.${g.name}`, text: g.form ? `Holds a ${g.form} right now.` : 'A variable; its value arrives as a frame.' }));
		case 'numpy':
			return [
				...NUMPY_NAMES.split(' ').map((label) => entry({ label, type: 'function' }, { title: `np.${label}()`, text: `numpy.${label}.` })),
				...'e inf nan newaxis pi'.split(' ').map((label) => entry({ label, type: 'constant' }, { title: `np.${label}`, text: `numpy.${label}.` })),
				...'fft linalg random'.split(' ').map((label) => entry({ label, type: 'namespace', apply: `${label}.` }, { title: `np.${label}`, text: `The numpy.${label} module.` }))
			];
		case 'argument': {
			const { call, written } = ctx;
			const pending = call.fn.args.filter((a) => !written.includes(a.name));
			const out: Completion[] = [];
			// The positional argument at this index takes one of some strings: those first.
			if (call.arg && !written.includes(call.arg.name) && call.arg.options.length > 0 && !call.arg.keyword) {
				out.push(...optionEntries(call, '', false));
			}
			// A node's or a machine's name goes straight in, quotes and all.
			if (call.fn.pick && !written.length) {
				const site: NameContext = { kind: call.fn.pick, from: ctx.from, to: ctx.to, quote: '', terminated: false, callClosed: false };
				out.push(...entriesFor(site, cat).map((c) => ({ ...c, boost: 95 })));
			}
			out.push(
				...pending
					.filter((a) => a.keyword || call.fn.pick === null)
					.map((a) =>
						entry(
							{ label: `${a.name}=`, displayLabel: a.name, type: 'property', detail: `${a.kind}${a.default ? ` · ${a.default}` : ''}`, boost: a === call.arg ? 99 : 98, apply: `${a.name}=` },
							{ title: `${a.name}=`, text: a.doc, arg: a }
						)
					)
			);
			out.push(...scopeEntries(cat));
			return out;
		}
		case 'option':
			return optionEntries(ctx.call, ctx.quote, ctx.terminated);
		case 'scope':
			return scopeEntries(cat);
	}
}

/** Python's own sources, folded in with a doc each so the pane never goes blank on them. */
function pythonEntries(ctx: CompletionContext, from: number): Completion[] {
	const out: Completion[] = [];
	for (const source of [localCompletionSource, globalCompletion]) {
		const r = source(ctx);
		if (!r || r instanceof Promise || r.from !== from) continue;
		for (const c of r.options) {
			const kind = c.type === 'keyword' ? 'Python keyword' : source === localCompletionSource ? 'a name in this expression' : `Python ${c.type ?? 'builtin'}`;
			out.push(entry({ ...c, boost: -1 }, { title: c.label, text: `${kind}.` }));
		}
	}
	return out;
}

export function goofiLanguageData(catalogue: () => ExprCatalogue): Extension {
	return pythonLanguage.data.of({
		autocomplete: (ctx: CompletionContext): CompletionResult | null => {
			const cat = catalogue();
			const where = exprContext(ctx.state, ctx.pos, !!cat.machine);
			if (!where) return null;
			const own = entriesFor(where, cat);
			const options = where.kind === 'scope' || where.kind === 'argument' ? [...own, ...pythonEntries(ctx, where.from)] : own;
			return options.length ? { from: where.from, to: where.to, options, validFor: /^\w*$/ } : null;
		}
	});
}
