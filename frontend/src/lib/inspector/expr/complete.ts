/** goofi-aware completion for the expression editor, registered as Python language data so CodeMirror
 *  ranks it into the same popup as its own sources. */
import { syntaxTree } from '@codemirror/language';
import { pythonLanguage } from '@codemirror/lang-python';
import type { Completion, CompletionContext } from '@codemirror/autocomplete';
import type { EditorState, Extension } from '@codemirror/state';
import type { SyntaxNode } from '@lezer/common';
import { STATE_FIELDS } from '$lib/api/vocab';
import type { CatalogueNode, ExprCatalogue } from './catalogue';

/** What a member chain hangs off: `me`, `nd('name')` or `st('name')`. */
export type RefTarget = { self: true } | { name: string } | { state: string };

/** Inside the first argument of `nd(` or `st(`: a node's name, or a state's. */
export interface NameContext {
	kind: 'node' | 'state';
	from: number;
	to: number;
	/** The quote the literal opened with — reused to close it on accept. */
	quote: string;
	terminated: boolean;
	callClosed: boolean;
}

export type ExprContext =
	| NameContext
	| { kind: 'path'; target: RefTarget; from: number; to: number }
	| { kind: 'slot'; target: RefTarget; from: number; to: number }
	| { kind: 'group'; target: RefTarget; from: number; to: number }
	| { kind: 'param'; target: RefTarget; group: string; from: number; to: number }
	| { kind: 'variables'; from: number; to: number }
	| { kind: 'variableGroup'; group: string; from: number; to: number }
	| { kind: 'numpy'; from: number; to: number }
	| { kind: 'scope'; from: number; to: number };

const text = (state: EditorState, node: SyntaxNode): string =>
	state.doc.sliceString(node.from, node.to);

/** The reference call `call` is — `nd` or `st` — or null for any other node. */
function refCall(state: EditorState, call: SyntaxNode | null): 'nd' | 'st' | null {
	if (!call || call.name !== 'CallExpression') return null;
	const callee = call.firstChild;
	if (!callee || callee.name !== 'VariableName') return null;
	const name = text(state, callee);
	return name === 'nd' || name === 'st' ? name : null;
}

/** An `ArgList`'s first ACTUAL argument, past the parens/commas and the parser's error markers. */
function firstArg(argList: SyntaxNode | null): SyntaxNode | null {
	for (let c = argList?.firstChild ?? null; c; c = c.nextSibling) {
		if (c.name !== '(' && c.name !== ')' && c.name !== ',' && c.name !== '⚠') return c;
	}
	return null;
}

/** A plain string literal's quote and body, and whether it is closed; null for any other literal. */
function literal(state: EditorState, str: SyntaxNode): { quote: string; body: string; terminated: boolean } | null {
	const raw = text(state, str);
	const quote = raw[0];
	const terminated = raw.length > 1 && raw.endsWith(quote);
	return quote === "'" || quote === '"' ? { quote, terminated, body: raw.slice(1, terminated ? -1 : undefined) } : null;
}

/** The string a `nd('…')` or `st('…')` call names with, or null when it is not a plain literal. */
function argName(state: EditorState, call: SyntaxNode): string | null {
	const arg = firstArg(call.getChild('ArgList'));
	return arg?.name === 'String' ? (literal(state, arg)?.body ?? null) : null;
}

/** The cursor is inside a string: a name iff that string is `nd`'s or `st`'s FIRST argument. */
function stringContext(state: EditorState, str: SyntaxNode, pos: number): ExprContext | null {
	const argList = str.parent;
	if (!argList || argList.name !== 'ArgList') return null;
	const call = refCall(state, argList.parent);
	if (!call) return null;
	if (firstArg(argList)?.from !== str.from) return null;
	const lit = literal(state, str);
	if (!lit) return null;
	// The literal's CONTENT: accepting replaces all of it, tail of the old name included.
	const from = str.from + 1;
	const to = lit.terminated ? str.to - 1 : str.to;
	if (pos < from || pos > to) return null;
	return { kind: call === 'nd' ? 'node' : 'state', from, to, quote: lit.quote, terminated: lit.terminated, callClosed: !!argList.getChild(')') };
}

/** The reference a member chain hangs off — `nd('x')`, `st('x')` or `me` — and the attributes between. */
function refHead(state: EditorState, obj: SyntaxNode): { target: RefTarget; path: string[] } | null {
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
	const call = refCall(state, cur);
	if (call) {
		const name = argName(state, cur);
		return name === null ? null : { target: call === 'nd' ? { name } : { state: name }, path };
	}
	return null;
}

/** The cursor is at `<something>.` — whose attribute do we know? */
function memberContext(state: EditorState, node: SyntaxNode, pos: number): ExprContext | null {
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
		if (name === 'variables') return { kind: 'variables', from, to };
		if (name === 'np') return { kind: 'numpy', from, to };
	}
	// `variables.<group>.` — a variable's name has two halves, so the head here is itself a member.
	if (obj.name === 'MemberExpression') {
		const head = obj.firstChild;
		const group = head?.nextSibling?.nextSibling ?? null;
		if (head && head.name === 'VariableName' && text(state, head) === 'variables' && group) {
			return { kind: 'variableGroup', group: text(state, group), from, to };
		}
	}
	const head = refHead(state, obj);
	if (!head) return null;
	const { target, path } = head;
	if (path.length === 0) return { kind: 'path', target, from, to };
	// A state's record and a playhead's are one level deep: past that is Python's own.
	if ('state' in target) return null;
	if (path[0] === 'out' && path.length === 1) return { kind: 'slot', target, from, to };
	if (path[0] === 'params' && path.length === 1) return { kind: 'group', target, from, to };
	if (path[0] === 'params' && path.length === 2)
		return { kind: 'param', target, group: path[1], from, to };
	// Deeper is the value's own attribute, and none of our business.
	return null;
}

/** What a cursor position means, and the range a completion there replaces; `null` leaves the stock
 *  Python sources to answer alone. */
export function exprContext(state: EditorState, pos: number): ExprContext | null {
	const node = syntaxTree(state).resolveInner(pos, -1);
	if (node.name === 'Comment') return null;
	if (node.name === 'String') return stringContext(state, node, pos);
	const member = memberContext(state, node, pos);
	if (member) return member;
	// A real `VariableName` only — after a dot the name is a PropertyName and must not see the scope.
	if (node.name === 'VariableName') return { kind: 'scope', from: node.from, to: node.to };
	return null;
}

/** The evaluator's injected scope (`expr.rs`'s `eval` variables) that every expression shares:
 *  goofi's own functions, `time()`, and the common slice of `from math import *`, since only eval
 *  is authoritative. */
const SHARED_SCOPE: Completion[] = [
	{ label: 'lfo', type: 'function', detail: 'sine wave — lfo(freq=1, src=t, vmin=slider min, vmax=slider max)', boost: 1 },
	{ label: 'noi', type: 'function', detail: 'smooth noise — noi(freq=1, src=t, vmin=slider min, vmax=slider max)', boost: 1 },
	{ label: 'qtz', type: 'function', detail: "volts per octave onto a scale — qtz(v, scale='major', root='C')", boost: 1 },
	{ label: 't', type: 'variable', detail: 'seconds since start', boost: 1 },
	{ label: 'np', type: 'namespace', detail: 'numpy', boost: 1 },
	{ label: 'variables', type: 'namespace', detail: 'patch variables — variables.group.element', boost: 1 },
	{ label: 'time', type: 'function', detail: 'wall-clock seconds — time()', boost: 1 },
	...'sin cos tan asin acos atan atan2 sinh cosh tanh exp expm1 log log2 log10 sqrt hypot floor ceil fabs fmod copysign degrees radians'
		.split(' ')
		.map((label) => ({ label, type: 'function', detail: 'math' })),
	...'pi e tau inf nan'.split(' ').map((label) => ({ label, type: 'constant', detail: 'math' }))
];

/** What a node's expression reads besides: the nodes, and itself. */
const NODE_SCOPE: Completion[] = [
	{ label: 'nd', type: 'function', detail: "node reference — nd('name')", boost: 1 },
	{ label: 'me', type: 'variable', detail: 'this node — me.out / me.params', boost: 1 }
];

/** What a machine's expression reads besides: its states, and the playhead it is read for. */
const MACHINE_SCOPE: Completion[] = [
	{ label: 'st', type: 'function', detail: "a state — st('name').residents / .playheads", boost: 1 },
	{ label: 'me', type: 'variable', detail: 'this playhead — me.name / me.state / me.<attribute>', boost: 1 }
];

/** What `st('…')` answers, in the order the manager declares the fields. */
const STATE_FIELD_DETAIL: Record<(typeof STATE_FIELDS)[number], string> = {
	residents: 'how many playheads rest in it',
	playheads: 'their names, longest resident first',
	arrivals: 'how many arrived at this instant',
	departures: 'how many left at this instant'
};

/** The numpy names offered after `np.`: hand-picked, never the real namespace. */
const NUMPY: Completion[] = [
	...('abs angle arange arccos arcsin arctan arctan2 argmax argmin argsort array asarray blackman ceil clip ' +
		'concatenate cos cosh cumsum degrees diff dot exp eye flip floor full hamming hanning hstack imag interp ' +
		'isfinite isnan linspace log log2 log10 max maximum mean median min minimum nanmean nanstd ones percentile ' +
		'power prod radians ravel real reshape roll round sign sin sinh sort sqrt square std sum tan tanh transpose ' +
		'unwrap var vstack where zeros').split(' ').map((label) => ({ label, type: 'function' })),
	...'e inf nan newaxis pi'.split(' ').map((label) => ({ label, type: 'constant' })),
	...'fft linalg random'.split(' ').map((label) => ({ label, type: 'namespace' }))
];

/** What finishes a name typed inside `nd('` or `st('`: only what is not written yet — an
 *  already-closed call's paren sits past the caret, so a dot inserted here would land inside it. */
function finish(ctx: NameContext, steer: string): string {
	return `${ctx.terminated ? '' : ctx.quote}${ctx.callClosed ? '' : `)${steer}`}`;
}

/** One node-name entry; a multi-output node steers to a `.slot`, since bare use of one raises. */
function nodeEntry(node: CatalogueNode, ctx: NameContext): Completion {
	const n = node.slots.length;
	const multi = n > 1;
	const detail = multi
		? `${n} outputs — bare use is ambiguous, pick .out.<slot>`
		: n === 1
			? '1 output — usable bare'
			: 'no outputs';
	const tail = finish(ctx, multi ? '.out.' : '');
	return {
		label: node.name,
		detail,
		type: 'variable',
		apply: tail ? `${node.name}${tail}` : undefined
	};
}

/** The catalogue entry a reference target names — the editing node itself for `me`. */
function nodeOf(cat: ExprCatalogue, target: RefTarget): CatalogueNode | undefined {
	const name = 'self' in target ? cat.self : 'name' in target ? target.name : undefined;
	return name == null ? undefined : cat.nodes.find((n) => n.name === name);
}

/** What the popup is offered at a classified position. */
export function entriesFor(ctx: ExprContext, cat: ExprCatalogue): Completion[] {
	switch (ctx.kind) {
		case 'node':
			return cat.machine ? [] : cat.nodes.map((n) => nodeEntry(n, ctx));
		// A state is read by field, so accepting lands past the dot.
		case 'state': {
			const tail = finish(ctx, '.');
			return (cat.machine?.states ?? []).map((label) => ({ label, detail: 'state', type: 'variable', apply: tail ? `${label}${tail}` : undefined }));
		}
		case 'path':
			if ('state' in ctx.target) {
				return cat.machine ? STATE_FIELDS.map((label) => ({ label, detail: STATE_FIELD_DETAIL[label], type: 'property' })) : [];
			}
			if (cat.machine) {
				return 'self' in ctx.target ? cat.machine.fields.map((label) => ({ label, type: 'property' })) : [];
			}
			return [
				{ label: 'out', type: 'namespace', detail: 'output slots — .out.<slot>' },
				{ label: 'params', type: 'namespace', detail: 'params — .params.<group>.<param>' }
			];
		case 'slot':
			return (nodeOf(cat, ctx.target)?.slots ?? []).map((s) => ({
				label: s.name,
				detail: s.dtype,
				type: 'property'
			}));
		case 'group':
			return (nodeOf(cat, ctx.target)?.params ?? []).map((g) => ({
				label: g.group,
				detail: `${g.names.length} param${g.names.length === 1 ? '' : 's'}`,
				type: 'namespace'
			}));
		case 'param':
			return (nodeOf(cat, ctx.target)?.params ?? [])
				.filter((g) => g.group === ctx.group)
				.flatMap((g) => g.names.map((n) => ({ label: n, type: 'property' })));
		case 'variables': {
			const counts = new Map<string, number>();
			for (const v of cat.variables) counts.set(v.group, (counts.get(v.group) ?? 0) + 1);
			return [...counts].map(([label, n]) => ({ label, detail: `${n} variable${n === 1 ? '' : 's'}`, type: 'namespace' }));
		}
		case 'variableGroup':
			return cat.variables
				.filter((g) => g.group === ctx.group)
				.map((g) => ({ label: g.element, detail: g.form ?? 'variable', type: 'variable' }));
		case 'numpy':
			return NUMPY;
		case 'scope':
			return [...(cat.machine ? MACHINE_SCOPE : NODE_SCOPE), ...SHARED_SCOPE];
	}
}

export function goofiLanguageData(catalogue: () => ExprCatalogue): Extension {
	return pythonLanguage.data.of({
		autocomplete: (ctx: CompletionContext) => {
			const where = exprContext(ctx.state, ctx.pos);
			if (!where) return null;
			const options = entriesFor(where, catalogue());
			return options.length ? { from: where.from, to: where.to, options } : null;
		}
	});
}
