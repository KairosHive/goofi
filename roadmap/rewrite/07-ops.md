# 07. Ops and interfaces

Everything a program can do is an op. This part defines the op framework of the goofi layer, the
interfaces that carry ops (app, CLI, console, MCP, plugins, tests), the agent harness, the
plugin interface, and the op vocabulary of goofi-pipe. Part 10.7 holds the vocabulary of
goofi-data. Appendix B maps every op of the present program to this vocabulary.

## 7.1 The op framework

### 7.1.1 Declaration

- An op is declared once: its phrase, its doc, its typed arguments, its result and its kind. The
  argument type is the schema, the validation and the documentation. An unknown argument is
  refused.
- One registry holds every op. Listings, help, completion, the MCP tool and the generated types
  of the frontend all read the registry.
- A plugin op is declared with the same types and has the same kinds as a built-in op.
- A mode does not register what it withholds (headless, demo). Listings and help shrink with it.

### 7.1.2 Kinds

- **read**: changes nothing.
- **write**: changes the patch. It is undoable, it can ride a batch, and it can be previewed
  when it names a drag key.
- **effect**: owns its consequences (a file, a process, a device, a take, a history flip). It
  runs alone.

### 7.1.3 One entry point

- Every caller reaches one dispatcher: the app, the CLI, the console prompt, MCP, a plugin, a
  script, a test.
- A call carries an envelope: the actor, the navigation context, an optional label and an
  optional group token for history.
- Order of a call: find the op; the pre-hooks of plugins; the handler; the post-hooks.
- A batch runs reads and writes as one transaction: every step is resolved before one runs; one
  settle, one document delta, one history step; a refused step rolls all back.
- A write is a transaction: it runs on a snapshot; a refusal or a panic restores the snapshot;
  a success settles once (part 03.8).

### 7.1.4 Shape of the vocabulary

- A phrase is `<noun> [<sub-noun>] <verb>`. The set of phrases is prefix-free.
- The standard verbs are `list`, `get`, `add`, `edit`, `remove` and `status`. A noun adds a verb
  of its own only for an action the standard verbs cannot express (`start`, `stop`, `save`,
  `load`, `copy`, `paste`).
- A rename is `edit --name`, for every noun.
- `remove` is idempotent for every noun and answers `{removed}`.
- A read answers data. Prose (a flowchart of a scope, a layout tree) is a rendering of that data
  by the interface, not another op.
- Argument names are the same for the same thing everywhere: `--scope` for a sub-patch,
  `--pos x,y`, `--size w,h`, `--name`. `type` names a node or panel type; `kind` names the kind
  of a value, a widget or a viewer.
- A node is addressed by its name or its identity; an output or input by `node/slot`; a param by
  `group/param` and one element by `group/param[i]`; a variable by `group.element`.
- Every positional argument is also a flag.
- A refusal teaches: it carries the usage of the op, or the listing of the group, that answers
  it.

## 7.2 Interfaces

### 7.2.1 The CLI

- The binary of a program is its CLI. `help` answers from a live server, and from the built-in
  registry when none runs.
- `<phrase> [args]` runs one op. `-` reads lines from standard input and runs them as one batch.
- The CLI holds no knowledge of the ops: the server parses a command line. Leading bare words
  fill the positional arguments; `--k v` and `--k=v` set a flag; `--x` and `--no-x` set a
  boolean; a repeated flag builds a list.
- The CLI finds its server through the session: an environment variable names the session; with
  none, exactly one live session is used; with several, the refusal lists them.
- The CLI answers in the style of its reader. On a pipe every answer is compact: one line for a
  refusal, JSON for data. On a terminal it is laid out for a person: a catalog as a table, a
  status as values and lists, a refusal as the reason and then the help, with colour that leads
  the eye. The style is detected and can be forced. `--json` prints the raw result.
- A binary result (an array snapshot) is written as raw bytes on a pipe.
- Completion scripts for the common shells call the server for candidates.
- Exit codes: 0 success, 1 refusal, 2 usage.

### 7.2.2 The console prompt

- The console panel has a prompt that runs the same command lines without the program name,
  with history and Tab completion (part 8.11).

### 7.2.3 MCP

- With the MCP flag the server offers one tool that runs command lines of the CLI. One command
  runs directly; several run as one batch.
- The description of the tool teaches the grammar and names the two first calls: the orientation
  guide and the op list.
- An agent over MCP reaches every op. Its edits arrive at every client as deltas and land on the
  one history.

### 7.2.4 Guides

- A guide is a text an agent reads. `guide list` names the orientation first, then every skill
  of the workspace of the patch. `guide get` returns one.
- The orientation says: what the program is; how to drive it (one op per line, the op list,
  help, batches, the shared undo); how to see (the reads); how to build (add, link, param,
  expression); how to write a node; what to handle with care.
- Shipped skills are embedded in the binary and seeded into a workspace that has none:
  - **patching-biosignals**: the fixed chain from a source through causal preprocessing and a
    buffer to analysis; the filter on the raw stream; one buffer for all analyses; checks of the
    rate and the labels.
  - **designing-biofeedback**: a feature that is clear, prompt and steady; a stimulus that
    matches the goal; only the feature moves saliently; a hand-over checklist.
  - **designing-emergent-shaders**: evolution from state and not from the clock; the five moves;
    craft rules; a cost model; a verification list.
- A new workspace also gets an `AGENTS.md` that says what the directory is and points at the
  orientation.

## 7.3 The agent harness

- goofi launches the coding harness a person already uses. An entry of `config.toml` names a
  harness: its name, its command, and flag templates for options such as model and effort.
  Default entries exist for the common harnesses.
- An instance runs on a PTY in the workspace of the patch, under a login shell, with the
  session in its environment and the binary of this server first on its PATH.
- The harness plane belongs to the session, not to one process: the record of an instance is in
  the session directory, and its views are connections to the process that owns its PTY. Every
  process of the session (the server for its panel and ops, the CLI, a node on either Python
  tier) spawns, lists, attaches to, types into and stops the same instances.
- An option the entry has no template for is refused. An option value is passed as one argument
  and is never pasted into a shell line.
- A view replays the recent output on attach. The terminal has one size: the last view to speak
  sets it. A slow view loses output; the harness never stalls.
- To send text types it as one entry and presses Enter. Stop asks, waits a grace time, then
  kills. An exited instance is removed from the roster by the plane, not by a client.
- All instances are stopped at shutdown.
- Demo mode has no agents.

## 7.4 Plugins

A plugin extends a program without a change to its source. It is a trusted local folder in the
home, loaded at the start. No plugin loads in demo mode.

### 7.4.1 Package

- `~/.goofi/plugins/<id>/` with a manifest (`id`, `version`, `api`), and optionally a Python
  backend, its locked dependencies, a frontend, a `nodes/` folder and assets.
- goofi prepares the backend environment and builds the frontend when the source changes.
  Persistent data of a plugin lives in `plugin-data/<id>/` and survives a replaced source.
- `plugin list` reports each plugin with its preparation or service error.

### 7.4.2 Backend

- A backend declares ops, which appear as `plugin <id> <name>`. Arguments are typed records.
  Kinds are read and effect. A plugin op runs alone, outside a batch.
- A context gives: `call` (the dispatcher; graph edits keep the normal undo), `log`, the plugin
  id and its package, data and cache directories.
- Start and stop hooks. A request has a time limit; a timeout stops the service.
- **Op hooks:** a pre-hook on any op can change its arguments or refuse it; the changed
  arguments are validated against the op. A post-hook receives the outcome. Hooks run outside
  the graph lock, in a fixed order. Two plugins that change the same argument are an error. A
  hooked op runs alone. Recursion through hooks is refused. Undo does not call hooks.
- `record start` takes annotations by namespace, so a plugin can label a take.
- The backend of a plugin uses the same Python package and the same transport as a node.

### 7.4.3 Frontend

- The frontend is an ES module with an activation function. It has a time limit. A types-only
  SDK is supplied to the build.
- During activation a plugin registers **panels** (a mount function into an element, with the
  op client, the log, the panel state with an edit or navigation intent, and node drop and drag
  handlers with named drop zones) and **header entries** (a label, a title, an action; they can
  be updated and disposed).
- A plugin panel is a panel type like a built-in one and is added through the normal panel
  interface. A plugin gives its panel an icon.
- A plugin frontend can subscribe to the events of the program; it does not poll.
- Authored panel state is part of the patch; credentials and data never go there.
- The names of the SDK use one case convention.
- An error in a handler of a plugin is caught and reported as a notification. A header entry can
  be disabled.

### 7.4.4 Nodes

- `nodes/` of a plugin is scanned as an ordinary bundle (part 04).

### 7.4.5 Shipped plugins

- **latency**: two nodes dropped on its panel; it records every node on the path between them
  and writes, per target tick, the total latency and the wait and run time per hop, from the
  `source` stamps of the frames. Its panel has start, stop, statistics and the path of the
  result; stop disarms what the run armed.
- **virtual-cables** (Linux): creates virtual audio devices and points `AudioIn` and `AudioOut`
  nodes at them by drag and drop. Its panel has a form with a name and a channel count; each
  cable row takes a dropped node and has a remove control.
- **session and corpus** (planned): people, corpora, acquisition sessions, tags and a catalogue,
  with an upload queue and a playback node. Its data side becomes goofi-data.

## 7.5 The shared nouns

These nouns exist in every program of the ecosystem with the same meaning.

| Noun | Verbs | Meaning |
| --- | --- | --- |
| `session` | `status`, `list` | The running program: instance, engines, resources. `list` shows the live sessions. |
| `op` | `list`, `get`, `complete`, `batch` | The registry; help for one op; completion of a line; several ops as one transaction. |
| `history` | `get`, `undo`, `redo` | The labels of the one history; the flips. |
| `log` | `list`, `add` | The log (part 02.5). |
| `guide` | `list`, `get` | Guides and skills. |
| `agent` | `list`, `start`, `stop`, `send` | The agent harness. |
| `plugin` | `list`; `plugin <id> <name>` | Plugins and their ops. |
| `account` | `status`, `login`, `logout` | The goofi account. |
| `library` | see 7.6 | The node library. |
| `record` | see 7.6 | The recorder. |
| `dir` | `list`, `get` | Directory browsing for the file dialogs. |

## 7.6 The op vocabulary of goofi-pipe

W is a write, E an effect, R a read. P marks a write that can be previewed.

### patch — the open patch
| Op | Kind | Meaning |
| --- | --- | --- |
| `patch status` | R | Save path, unsaved flag, errors of nodes with their age. |
| `patch get` | R | The whole document; `--manifest` gives the text of the file. |
| `patch new` | E | An empty patch. Clears the history. |
| `patch load [path]` | E | Replace the patch from a file, from `--content`, or from `--example`. `--missing install\|skip\|cancel` decides about unavailable nodes (part 04.8). |
| `patch save [path]` | E | Pack the patch and its workspace. A given path becomes the remembered file. |
| `patch recovery list` | R | The recovery copies. |
| `patch recovery load <id>` | E | Load one as unsaved work. |
| `patch recovery remove <id>` | E | Discard one, or all. |

### node
| Op | Kind | Meaning |
| --- | --- | --- |
| `node list [--scope]` | R | The nodes and cables of a scope. |
| `node get <node>` | R | Params, outputs with rates, slots, error. |
| `node add <type>` | W | `--name --pos --scope --param`. Returns the name, identity, slots and params. |
| `node edit <node>` | W P | `--name --pos`. |
| `node remove <node>…` | W | A leaf, a port, or a whole sub-patch. |
| `node restart <node>` | E | Rebuild in place. |
| `node copy <node>…` | R | A fragment. |
| `node paste` | W | `--doc --pos --scope`. Fresh identities and names. |
| `node group <node>…` | W | A new sub-patch. |
| `node ungroup <node>` | W | Dissolve a sub-patch. |
| `node arrange` | W | `--scope`. Lay out by dataflow. |
| `node param edit <node> <param>` | W P | `--value --expression --mode`. One param or one element. |
| `node param reset <node>` | W | The present state becomes the zero point. |
| `node param pulse <node> <param>` | E | Fire a pulse. |
| `node param refresh <node> <param>` | E | List the options again. |
| `node slot edit <node> <slot>` | W | `--coupling each\|zip\|hold --group`. |
| `node view edit <output>` | W P | The inline viewer: `--kind --settings --collapsed`. |
| `node output get <output>` | R | The newest frame once: shape and range, or the raw array. |
| `node editor open <node>`, `node editor close <node>` | E | The native editor window of a plugin node. |

### link
| Op | Kind | Meaning |
| --- | --- | --- |
| `link list [--scope]` | R | The cables of a scope. |
| `link add <from> <to>` | W | Refuses a kind mismatch. |
| `link remove <from> <to>` | W | |

### variable
| Op | Kind | Meaning |
| --- | --- | --- |
| `variable list` | R | Variables with values, locks, widgets, expressions, errors; `--group` narrows. |
| `variable add [name]` | W | `--group --value --widget --range --step --options --cell`. A widget makes it a control. |
| `variable edit <name>` | W P | `--name --value --expression --widget --range --step --options --cell --lock`. |
| `variable remove <name>` | W | |
| `variable paint <name>` | W P | Draw on an image variable with the drawing script (strokes, fills, clear). |
| `variable group add [group]`, `variable group edit <group>`, `variable group remove <group>` | W | `edit` takes `--name --lock`. |

### machine
| Op | Kind | Meaning |
| --- | --- | --- |
| `machine list`, `machine get <machine>` | R | |
| `machine add [name]`, `machine edit <machine>`, `machine remove <machine>` | W P | `edit` takes `--name --seed`. |
| `machine arrange <machine>` | W P | States on a ring. |
| `machine attribute add\|edit\|remove <machine> <name>` | W P | `--name --value --kind --curve`. |
| `machine state add\|edit\|remove <machine> [name]` | W P | `--name --pos --values --exit-values --envelopes --interval --mode --order --weights --expression`. |
| `machine transition add\|edit\|remove <machine> [id]` | W P | `--from --to --condition --edge --event --duration --values --internal`. |
| `machine playhead add\|edit\|remove <machine> [name]` | W P | `--name --color --start`. |
| `machine fire`, `machine jump`, `machine event`, `machine reset` | E | Take a transition; put a playhead in a state; send a named event; reset. |

### layout — absent when headless
| Op | Kind | Meaning |
| --- | --- | --- |
| `layout get [tab]` | R | The tree of tabs, splits and panels. |
| `layout add` | W | A new empty panel beside a panel (`--beside --side --ratio`) or on a new tab (`--tab --index`). |
| `layout edit <entry>` | W P | A tab: `--name`. A split: `--fractions`. A panel: `--type --state`. |
| `layout move <entry>` | W | A panel, a subtree or a tab. |
| `layout remove <entry>` | W | |
| `layout viewpoint edit` | E | The viewpoint of this client. Never dirties. |

### record, library, midi
| Op | Kind | Meaning |
| --- | --- | --- |
| `record status` | R | |
| `record arm <output>`, `record edit <output>`, `record disarm <output>` | W | `--quality small\|high\|very_high` for a texture output. |
| `record start [name]` | E | `--root --annotations --type files\|dataset`. |
| `record stop` | E | |
| `library list` | R | The catalog; `--full` for the palette; selectors of part 04.8. |
| `library get <type>` | R | One entry with its language, tier, origin and path; `--source` for the file text. |
| `library save <type>` | E | Move a node file of the patch into the private bundle. |
| `library refresh` | E | Build and scan again; restart the nodes of changed types. |
| `library source list\|add\|remove`, `library bundle list\|add\|remove\|update` | R / E | Sources and bundles (part 04.8). |
| `library publish <bundle>`, `library search <text>` | E / R | The online library (part 04.8). |
| `midi list` | R | Ports, their groups, the learning state. |
| `midi learn` | E | Open every port for a set time; `--stop` ends it. |

## 7.7 Open decisions

1. The vocabulary of 7.5 and 7.6 is a proposal made from the present 101 ops and their
   inconsistencies (appendix B). It is not confirmed.
2. Whether the CLI can attach a terminal to an agent instance (`agent attach`).
3. Whether the graph ops are shared word for word by goofi-data (part 10.8, item 9).
