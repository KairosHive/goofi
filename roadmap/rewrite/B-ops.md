# Appendix B. Present ops and their place in the rewrite

The present program registers 101 built-in ops (25 reads, 53 writes, 23 effects; a count from a
survey of the op registry on 2026-11-18). This table maps each to the vocabulary of part 07.6.
A check asserts that every present op has a row (part 12.5).

## B.1 Mapping

| Present op | Rewrite op |
| --- | --- |
| `session status` | `session status` (instance, engines, resources) and `patch status` (save path, unsaved, errors) |
| `session state` | `patch get` |
| `session manifest` | `patch get --manifest` |
| `session save` | `patch save` |
| `session load` | `patch load` |
| `session new` | `patch new` |
| `session recoverable` | `patch recovery list` |
| `session recover` | `patch recovery load` |
| `session discard` | `patch recovery remove` |
| `session list` (CLI) | `session list` |
| `node state` | `node get` |
| `node snapshot` | `node output get` |
| `node add` | `node add` (`--scope` replaces `--inst_id`) |
| `node edit` | `node edit` (name, position); `node view edit` (viewers) |
| `node param edit` | `node param edit` |
| `node param request --request pulse` | `node param pulse` |
| `node param request --request refresh` | `node param refresh` |
| `node remove` | `node remove` |
| `node baseline` | `node param reset` |
| `node restart` | `node restart` |
| `node editor` | `node editor open`, `node editor close` |
| `nodes inspect` | `node list` (a flowchart is a rendering) |
| `nodes copy` | `node copy` |
| `nodes paste` | `node paste` |
| `nodes group` | `node group` |
| `nodes ungroup` | `node ungroup` |
| `nodes arrange` | `node arrange` |
| `link add` | `link add` |
| `link remove` | `link remove` |
| `variable list` | `variable list` |
| `variable entry add` | `variable add` |
| `variable entry edit` | `variable edit` |
| `variable entry remove` | `variable remove` |
| `variable entry lock` | `variable edit --lock` |
| `variable entry rename` | `variable edit --name` |
| `variable group add` | `variable group add` |
| `variable group rename` | `variable group edit --name` |
| `variable group remove` | `variable group remove` |
| `variable group lock` | `variable group edit --lock` |
| `control list` | `variable list --group` |
| `control add` | `variable add --widget` |
| `control edit` | `variable edit` |
| `control remove` | `variable remove` |
| `control paint` | `variable paint` |
| `machine list` | `machine list`, `machine get` |
| `machine add` | `machine add` |
| `machine remove` | `machine remove` |
| `machine rename` | `machine edit --name` |
| `machine edit` | `machine edit` |
| `machine arrange` | `machine arrange` |
| `machine attribute add` | `machine attribute add` |
| `machine attribute edit` | `machine attribute edit` |
| `machine attribute remove` | `machine attribute remove` |
| `machine attribute rename` | `machine attribute edit --name` |
| `machine state add` | `machine state add` |
| `machine state edit` | `machine state edit` |
| `machine state remove` | `machine state remove` |
| `machine state rename` | `machine state edit --name` |
| `machine transition add` | `machine transition add` |
| `machine transition edit` | `machine transition edit` |
| `machine transition remove` | `machine transition remove` |
| `machine playhead add` | `machine playhead add` |
| `machine playhead edit` | `machine playhead edit` |
| `machine playhead remove` | `machine playhead remove` |
| `machine playhead rename` | `machine playhead edit --name` |
| `machine fire` | `machine fire` |
| `machine jump` | `machine jump` |
| `machine reset` | `machine reset` |
| `midi list` | `midi list` |
| `midi learn` | `midi learn` |
| `library list` | `library list` |
| `library get` | `library get` |
| `library save` | `library save` |
| `library refresh` | `library refresh` |
| `dir stat` | `dir get` |
| `dir list` | `dir list` |
| `log list` | `log list` |
| `log write` | `log add` |
| `op list` | `op list`, `op get` |
| `op complete` | `op complete` |
| `guide list` | `guide list` |
| `guide get` | `guide get` |
| `agent list` | `agent list` |
| `agent start` | `agent start` |
| `agent stop` | `agent stop` |
| `record status` | `record status` |
| `record arm` | `record arm` |
| `record quality` | `record edit --quality` |
| `record disarm` | `record disarm` |
| `record start` | `record start` |
| `record stop` | `record stop` |
| `plugin list` | `plugin list` |
| `undo` | `history undo` |
| `redo` | `history redo` |
| `compound` | `op batch` |
| `layout inspect` | `layout get` |
| `layout panel add` | `layout add` |
| `layout panel edit` | `layout edit` (a panel) |
| `layout move` | `layout move` |
| `layout remove` | `layout remove` |
| `layout tab edit` | `layout edit` (a tab) |
| `layout split edit` | `layout edit` (a split) |
| `layout viewpoint edit` | `layout viewpoint edit` |
| `help`, `completions` (CLI) | `help`, `completions` (CLI) |
| `host describe`, `host serve`, `vst3-scan` (internal) | Internal child commands of the node library and the VST3 scan |

## B.2 Ops the rewrite adds

`node slot edit`, `link list`, `history get`, `agent send`, `account status`, `account login`,
`account logout`, `machine event`, `variable fire`, `library source list|add|remove`,
`library bundle list|add|remove|update`, `library publish`, `library search`.

## B.3 Inconsistencies of the present vocabulary that the shape of part 07.1.4 removes

- Eleven read verbs; prose results beside data results; no `node list` and no `link list`.
- A rename that is an op of its own for six nouns and a flag of `edit` for three.
- `remove` that is idempotent for two nouns and refuses for the others.
- Two groups for one noun (`node`, `nodes`); two noun families for one thing (`variable entry`
  and `control`); one view stored through two ops.
- Three names for a sub-patch argument (`inst_id`, `scope`, `subpatch`); two position forms;
  `vars.` and `variables.` for one namespace.
- `session` for the process, the document, the file and recovery at once.
- `undo`, `redo` and `compound` without a group.
- Three wire envelopes for one dispatcher; live values and node errors on several paths.
