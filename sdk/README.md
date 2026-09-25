# Goofi plugins

A plugin is a trusted local folder in `$GOOFI_HOME/.goofi/plugins/<id>/` (the user's home when
`GOOFI_HOME` is unset). Restart goofi and reload browser clients after adding or changing a plugin. Startup prepares its
backend environment and frontend, then includes its `nodes/` directory in the normal node scan.
`plugin list` reports preparation and service failures. No plugin is loaded in demo mode.

```text
example/
    goofi-plugin.toml
    backend/plugin.py
    pyproject.toml       # optional backend dependencies
    uv.lock             # required with pyproject.toml
    frontend/
        package.json
        package-lock.json
        index.js
    nodes/              # ordinary Rust and Python node sources
    assets/
```

Only the manifest is required. It has three fields:

```toml
id = "example"
version = "0.1.0"
api = 1
```

The ID must match the folder name and contain lowercase letters, digits, or hyphens. Goofi owns
build files under its build directory and persistent plugin data under `.goofi/plugin-data/<id>/`.
Replacing source does not delete persistent data. Builds, Python code, and frontend code execute
with the user's permissions. A service process contains failures; it is not a security sandbox.

## Python backend

Goofi provides `goofi_plugin` to `backend/plugin.py`. Python 3.11 or later is required. With a
`pyproject.toml`, startup runs `uv sync --locked --no-install-project` into a separate environment.
Without one, the backend uses goofi's configured Python interpreter. Headless startup omits
frontend builds. The service SDK itself uses
only Python's standard library.

```python
from dataclasses import dataclass
from goofi_plugin import plugin

@dataclass
class Select:
    subject: str

@plugin.op("subject select", kind="effect")
async def select(ctx, args: Select) -> dict:
    """Select the subject for the next recording."""
    return {"subject": args.subject}

@plugin.on_start
async def start(ctx):
    ctx.log("Ready")

@plugin.on_stop
async def stop(ctx):
    pass
```

Operations appear as `plugin <id> <name>`. `op list`, CLI help and completion, `/control`, `/exec`,
and MCP use this same registry. Arguments are dataclasses: fields supply names, types, defaults,
and required flags. Primitive fields use normal CLI flags; structured fields use JSON. The SDK
validates arguments and declared results. Unknown argument fields are errors. Operation handlers
can be synchronous or asynchronous. Kinds are `read` and `effect`; plugin operations run alone,
outside graph batches. Graph edits made with `ctx.call` retain the host command's normal undo.

The context exposes:

- `await ctx.call(operation, arguments={})`: call the shared goofi dispatcher.
- `ctx.log(message, level="info")`: write an application log entry. Levels are `info`, `warning`,
  and `error`. Goofi adds the plugin identity. Ordinary Python prints are captured from stderr.
- `ctx.plugin_id`, `ctx.package_dir`, `ctx.data_dir`, `ctx.cache_dir`.

Use ordinary `asyncio` tasks for background work and close owned resources in `on_stop`.
Goofi cancels in-flight operation handlers before `on_stop`; plugin-created background tasks
remain the plugin's responsibility.
A process crash cannot run `on_stop`; persistent jobs must support startup reconciliation.
Each request has a 30-second limit. A timeout stops the service, so use short operations to start
long jobs and read operations to inspect progress. There is no automatic retry of effects.

## Operation hooks

```python
@plugin.pre_op("record start")
async def prepare(ctx, call):
    return call.patch(
        root=str(ctx.data_dir / "recordings"),
        name="example-Alice",
        annotations={"example": {"subject": "Alice", "tags": ["baseline"]}},
    )

@plugin.post_op("record stop")
async def stopped(ctx, outcome):
    if outcome.ok:
        ctx.log(f"Recording closed: {outcome.result['folder']}")
```

A call exposes `op`, `args`, and `actor`. Return `call.patch(...)` or `None`; use
`call.reject(message)` to refuse execution. The actor identifies a command client, not an
authenticated user or recording subject.

A pre-hook can override explicitly supplied arguments. Goofi validates the argument envelope
before and after hooks; the operation's own guards still apply. Hooks run in folder/ID order.
Two plugins that patch the same top-level argument produce an error before execution. A pre-hook
failure prevents the operation. A post-hook receives the effective arguments plus `ok`, `result`,
and `error`. A post-hook failure is logged and does not change the completed operation's outcome.

Hooks run outside graph locks. Concurrent recording starts are serialized through preparation.
Recursive operation calls are refused, including cycles through `ctx.call`. Hooks can target built-in or plugin operations. Targets resolve after package discovery. Hooks
cannot target `compound`, undo, or redo in API 1. An operation with hooks
must run alone: a graph batch refuses it before any step runs. Undo replays stored inverses without
calling plugin hooks. Hooks report operation outcomes, not guaranteed lifecycle events or durable
messages. Reconcile persistent work after a restart.

`record start` accepts `annotations`, an object keyed by namespace. Each value is JSON, at most
1 MiB, written as `annotations/<namespace>/session.json` before capture starts. Native recording
metadata remains recorder-owned. The destination and initial annotations do not change when a
plugin selects a different subject during capture. `record stop` succeeds only after finalization;
an upload failure in a post-hook leaves that local result intact.

## Frontend

The frontend build writes an ES module named `index.js` and its assets to
`process.env.GOOFI_PLUGIN_OUT_DIR`. Goofi runs `npm ci` and `npm run build` in a cached source copy when package source
changes. Completed assets are immutable for the lifetime of a running server. Runtime dependencies must be bundled or resolved as relative assets; the host does not
provide a Svelte runtime. Authors may use Svelte, other UI libraries, or plain DOM code.

The module exports an activation function. Goofi supplies `@goofi/plugin` as a types-only SDK
to the build; use `import type { Activate, Plugin, Context } from '@goofi/plugin'` in TypeScript.
`sdk/frontend/index.ts` is its source. Activation has a 10-second limit. Register panels during
activation; they become available only after activation succeeds.

```javascript
export default function (plugin, ctx) {
    const subject = plugin.register_header({
        id: 'subject',
        label: 'No subject selected'
    });

    plugin.register_panel({
        id: 'corpus',
        title: 'Corpus',
        mount(element, panel) {
            // Mount UI in element; use panel.call(...) for backend operations.
            // Return a function that removes listeners and closes UI resources.
        }
    });

    subject.update({ label: 'Subject: Alice' });
    return () => subject.dispose();
}
```

Panel IDs become `plugin:<plugin-id>:<panel-id>`. Users add and focus panels through goofi's
normal panel UI. The framework owns layout and gestures. Panel registration lasts for the browser
page; there is no panel-open or panel-removal API. The mount context has `call`, `log`, `panel_id`,
a getter for `state`, `set_state(value, intent)`, `on_node_drop`, and `on_node_drag`. Intent is
`edit` by default or `navigation`.

A panel registered with `accepts_node: true` is a drop target for a node dragged from the editor,
and goofi draws the same drop hint it draws on its own panels. `on_node_drop(handler)` receives
`{node, zone, x, y}`: `node` is the node's uid, and `zone` is the part after `#` of the
`data-node-drop="<panel_id>#<zone>"` attribute of the element the drop landed on, or empty for the
panel itself. Goofi finds those elements inside a shadow root. `on_node_drag(handler)` receives
`{node, over, zone}` while a drag runs and `null` when it ends, so a row can highlight itself.
Both return a function that removes the handler.
Do not store credentials or database contents in panel state: authored panel state is part of the
patch. Use CSS custom properties for theme values and container queries for panel sizing. A shadow
root can contain plugin styles.

Header handles support `update(patch)` and `dispose()`. Updates keep the entry ID fixed. Entries
may have `label`, `title`, `disabled`, and `onActivate`. Goofi places entries in its existing header
overflow menu when space is limited. A disposed handle cannot be updated. Registration IDs must
be unique within their contribution kind.

Frontend contexts expose the same `call(operation, arguments)` and `log(message, level)` actions.
Backend state remains authoritative. Refresh after actions and poll read operations when needed;
API 1 has no public state subscription or event bus. Each browser has its own frontend activation.
Return a cleanup function from activation to release timers and other resources.

## Nodes

`nodes/` is scanned as an ordinary bundle. Rust files use the existing engine SDK and build cache;
Python files use the existing node execution tiers. No manifest list duplicates the directory.
Put Python **node** dependencies in the bundle's existing requirements files under `nodes/`.
Backend `pyproject.toml` dependencies belong to its private service environment and do not install
packages into node interpreters.

A param can show only while one other param of the same node has one of a list of values. That
param must have a fixed set of values: a string or int param with options, or a bool. Name it
`name` in the same group or `group.name`. An int compares as decimal text and a bool as `true` or
`false`. A param that a hidden param controls is also hidden. The inspector omits hidden params
and a group tab with no shown param. A hidden param keeps its value and source, and the node runs
as before. A group can have sections, and the inspector draws a line between two shown sections.
Discovery refuses an unknown controller, a controller without a fixed set of values, a value that
is not an option, a chain that returns to its param, and a name used twice in one group.

```python
PARAMS = {"filter": [
    {"mode": goofi.StringParam("fir", options=["fir", "iir"])},
    {"order": goofi.IntParam(4, 2, 8, options=[2, 4, 8], show=("mode", ["iir"])),
     "ripple": goofi.FloatParam(0.5, 0.0, 1.0, show=("order", [4, 8]))},
]}
```

A group that is a list of dicts has one section for each dict. In Rust, a `ParamDecl` sets
`section: 1` and `show: Some(Show { param: "mode", any_of: &["iir"] })`, or `section: 0` and
`show: None`. A WGSL header param adds `"section": 1` and
`"show": {"param": "mode", "any_of": ["iir"]}`.

A plugin can provide its own playback node and configure it through `ctx.call`. Downloads and
database access must stay outside audio callbacks and other time-critical processing. Prefer stable
recording IDs or portable paths in saved node parameters. No resource-provider API is required.

## Shipped plugins

`plugins/` at the repository root holds plugins goofi ships as source. Install one by copying or
linking its folder into `.goofi/plugins/`. `plugins/virtual-cables` creates PipeWire virtual audio
devices on Linux and points AudioIn and AudioOut nodes at them by drag and drop.

## Validation

`backend/goofi-tests/fixtures/plugins/example` is a complete small package. It registers a subject
operation, a corpus panel, an editable header item, recording hooks, and Rust and Python nodes.
It is a fixture and authoring example, not a remote database adapter.

```sh
cargo test -p goofi-tests --test plugins
cd tests/e2e
npx playwright test --config playwright.plugins.config.ts
```

The browser host uses test clocks and does not open audio devices or native windows.
