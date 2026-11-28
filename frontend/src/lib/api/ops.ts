// GENERATED from backend/goofi-bridge/src/ops/mod.rs — do not edit by hand.
// The manager's op registry is the only place an op name is declared: naming one that is
// not in it is a type error here and an `unknown op` refusal there. Regenerate by running
// `cargo test -p goofi-tests contracts::`, which rewrites this file when it drifts.
export type OpName =
	| `plugin ${string}`
	| 'plugin list'
	| 'plugin state list'
	| 'plugin state edit'
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
	| 'nodes arrange'
	| 'link add'
	| 'link remove'
	| 'variable list'
	| 'variable entry add'
	| 'variable entry edit'
	| 'variable entry remove'
	| 'variable entry lock'
	| 'variable entry rename'
	| 'variable group add'
	| 'variable group rename'
	| 'variable group remove'
	| 'variable group lock'
	| 'machine list'
	| 'machine add'
	| 'machine remove'
	| 'machine edit'
	| 'machine arrange'
	| 'machine attribute add'
	| 'machine attribute edit'
	| 'machine attribute remove'
	| 'machine state add'
	| 'machine state edit'
	| 'machine state remove'
	| 'machine transition add'
	| 'machine transition edit'
	| 'machine transition remove'
	| 'machine playhead add'
	| 'machine playhead edit'
	| 'machine playhead remove'
	| 'machine fire'
	| 'machine jump'
	| 'machine reset'
	| 'clock set'
	| 'timeline list'
	| 'timeline track add'
	| 'timeline track edit'
	| 'timeline track move'
	| 'timeline track record'
	| 'timeline track remove'
	| 'midi list'
	| 'midi learn'
	| 'control list'
	| 'control add'
	| 'control edit'
	| 'control remove'
	| 'control paint'
	| 'library list'
	| 'library get'
	| 'library save'
	| 'library refresh'
	| 'dir stat'
	| 'dir list'
	| 'log list'
	| 'log write'
	| 'op list'
	| 'op vocabulary'
	| 'op complete'
	| 'guide list'
	| 'guide get'
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

/** The writes: each is a history step. */
export const WRITE_OPS: ReadonlySet<OpName> = new Set([
	'plugin state edit',
	'node add',
	'node edit',
	'node param edit',
	'node remove',
	'node baseline',
	'nodes paste',
	'nodes group',
	'nodes ungroup',
	'nodes arrange',
	'link add',
	'link remove',
	'variable entry add',
	'variable entry edit',
	'variable entry remove',
	'variable entry lock',
	'variable entry rename',
	'variable group add',
	'variable group rename',
	'variable group remove',
	'variable group lock',
	'machine add',
	'machine remove',
	'machine edit',
	'machine arrange',
	'machine attribute add',
	'machine attribute edit',
	'machine attribute remove',
	'machine state add',
	'machine state edit',
	'machine state remove',
	'machine transition add',
	'machine transition edit',
	'machine transition remove',
	'machine playhead add',
	'machine playhead edit',
	'machine playhead remove',
	'timeline track add',
	'timeline track edit',
	'timeline track move',
	'timeline track remove',
	'control add',
	'control edit',
	'control remove',
	'control paint',
	'record arm',
	'record quality',
	'record disarm',
	'layout panel add',
	'layout panel edit',
	'layout move',
	'layout remove',
	'layout tab edit',
	'layout split edit'
]);
