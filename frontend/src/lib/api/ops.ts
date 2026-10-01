// GENERATED from backend/goofi-bridge/src/ops/mod.rs — do not edit by hand.
// The manager's op registry is the only place an op name is declared: naming one that is
// not in it is a type error here and an `unknown op` refusal there. Regenerate by running
// `cargo test -p goofi-tests contracts::`, which rewrites this file when it drifts.
export type OpName =
	| `plugin ${string}`
	| 'plugin list'
	| 'session status'
	| 'session state'
	| 'session manifest'
	| 'session save'
	| 'session load'
	| 'session new'
	| 'session recoverable'
	| 'session recover'
	| 'session discard'
	| 'node state'
	| 'node snapshot'
	| 'node add'
	| 'node edit'
	| 'node param edit'
	| 'node param request'
	| 'node remove'
	| 'node baseline'
	| 'node restart'
	| 'node editor'
	| 'nodes inspect'
	| 'nodes copy'
	| 'nodes paste'
	| 'nodes group'
	| 'nodes ungroup'
	| 'link add'
	| 'link remove'
	| 'variable list'
	| 'variable entry add'
	| 'variable entry edit'
	| 'variable entry remove'
	| 'variable entry source'
	| 'variable entry lock'
	| 'variable entry rename'
	| 'variable group add'
	| 'variable group rename'
	| 'variable group lock'
	| 'control list'
	| 'control add'
	| 'control edit'
	| 'control remove'
	| 'control paint'
	| 'control source'
	| 'library list'
	| 'library get'
	| 'library save'
	| 'library refresh'
	| 'dir stat'
	| 'dir list'
	| 'log list'
	| 'log write'
	| 'op list'
	| 'op complete'
	| 'agent list'
	| 'agent start'
	| 'agent stop'
	| 'record status'
	| 'record arm'
	| 'record quality'
	| 'record disarm'
	| 'record start'
	| 'record stop'
	| 'undo'
	| 'redo'
	| 'compound'
	| 'layout inspect'
	| 'layout panel add'
	| 'layout panel edit'
	| 'layout move'
	| 'layout remove'
	| 'layout tab edit'
	| 'layout split edit'
	| 'layout viewpoint edit';

/** A write is a history step; a read touches nothing; an effect owns its consequences. */
export const OP_KINDS: Record<string, 'read' | 'write' | 'effect'> = {
	'plugin list': 'read',
	'session status': 'read',
	'session state': 'read',
	'session manifest': 'read',
	'session save': 'effect',
	'session load': 'effect',
	'session new': 'effect',
	'session recoverable': 'read',
	'session recover': 'effect',
	'session discard': 'effect',
	'node state': 'read',
	'node snapshot': 'read',
	'node add': 'write',
	'node edit': 'write',
	'node param edit': 'write',
	'node param request': 'effect',
	'node remove': 'write',
	'node baseline': 'write',
	'node restart': 'effect',
	'node editor': 'effect',
	'nodes inspect': 'read',
	'nodes copy': 'read',
	'nodes paste': 'write',
	'nodes group': 'write',
	'nodes ungroup': 'write',
	'link add': 'write',
	'link remove': 'write',
	'variable list': 'read',
	'variable entry add': 'write',
	'variable entry edit': 'write',
	'variable entry remove': 'write',
	'variable entry source': 'write',
	'variable entry lock': 'write',
	'variable entry rename': 'write',
	'variable group add': 'write',
	'variable group rename': 'write',
	'variable group lock': 'write',
	'control list': 'read',
	'control add': 'write',
	'control edit': 'write',
	'control remove': 'write',
	'control paint': 'effect',
	'control source': 'write',
	'library list': 'read',
	'library get': 'read',
	'library save': 'effect',
	'library refresh': 'effect',
	'dir stat': 'read',
	'dir list': 'read',
	'log list': 'read',
	'log write': 'effect',
	'op list': 'read',
	'op complete': 'read',
	'agent list': 'read',
	'agent start': 'effect',
	'agent stop': 'effect',
	'record status': 'read',
	'record arm': 'write',
	'record quality': 'write',
	'record disarm': 'write',
	'record start': 'effect',
	'record stop': 'effect',
	'undo': 'effect',
	'redo': 'effect',
	'compound': 'effect',
	'layout inspect': 'read',
	'layout panel add': 'write',
	'layout panel edit': 'write',
	'layout move': 'write',
	'layout remove': 'write',
	'layout tab edit': 'write',
	'layout split edit': 'write',
	'layout viewpoint edit': 'effect'
};
