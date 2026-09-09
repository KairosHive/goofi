<p align="center">
<img src="frontend/static/favicon.png" width="120" alt="">
</p>

<h1 align="center">goofi</h1>
<h3 align="center">Generative Organic Oscillation Feedback Isomorphism Pipeline</h3>

<p align="center">
  <a href="https://goofi.kairos-hive.org"><b>goofi.kairos-hive.org</b></a> — the install guide, the
  op vocabulary, and every shipped node with its parameters and its source
</p>

<p align="center">
  <a href="LICENSE"><img alt="GitHub License" src="https://img.shields.io/github/license/KairosHive/goofi"></a>
</p>

A real-time, node-based data-processing platform for biosignals — EEG, ECG, audio, video. You
build **patches** in a browser node-graph: each node ingests, transforms or emits `Data`, and
edges carry frames between output and input slots. It targets live, high-rate streams (kHz EEG,
HD video) with many simultaneous viewers.

The graph you edit is the graph that is running: add a node, move a cable, turn a parameter, and
the next frame is the one you changed. There is no global tick — nodes schedule themselves and
frames travel node to node over shared memory, so a kHz EEG stream and an audio chain sit in one
patch without either holding the other up. Several people can work inside it at once, each with
their own undo.

## Running it

Requires [rustup](https://rustup.rs), [Node.js](https://nodejs.org) and
[`uv`](https://docs.astral.sh/uv/). The Rust toolchain itself is pinned in `rust-toolchain.toml`,
so rustup fetches the right one on the first build; nothing to choose.

```bash
cargo run -p goofi-init   # once per clone: provisions everything cargo cannot
cargo run                 # builds the SPA if needed, starts the server, prints the URL
```

Two commands, and there is never a third. `goofi-init` builds the two Python interpreters, installs
the `goofi` package into each, installs the frontend's dependencies, and writes the cargo config
that points pyo3 at the free-threaded interpreter — every precondition `cargo` has and cannot
provide for itself. That last one must happen *before cargo starts*, because cargo reads
`.cargo/config.toml` only at startup; until it has, the build stops with one line saying so. It is
a workspace crate rather than a shell script, so that first line is the same command in PowerShell,
cmd, bash, zsh and fish.

Python is part of goofi, not an add-on: a node can be written in it, and params are expressions
it evaluates.

The SPA is compiled into the binary, so `cargo run` builds it whenever a frontend source is newer
than the last bundle — and **fails** if it cannot. It will not fall back to the previous bundle:
one that does not match the binary around it is the wrong app, and on a fresh clone there is no
previous bundle at all. A binary that ended up with no app refuses to start rather than serving
nothing at every route.

`GOOFI_HEADLESS=1` is `--headless` spelled as an environment variable, and it applies to the build
as well: set for `cargo build`, it leaves the app out of the binary entirely — no Node.js needed,
and the result is headless for life, with no flag to remember at every run. `GOOFI_DEBUG=1` is
`--debug` the same way.

| Flag | Default | Effect |
| --- | --- | --- |
| `--port N` | `8000` | The port to serve on. |
| `--bind HOST` | `127.0.0.1` | The address to serve on. Anything beyond this machine warns: there is no auth, and `/term` is a real shell. |
| `--extra-nodes ROOT` | — | A folder of node files, scanned after the shipped bundles and before the open patch's own workspace. Repeatable; a later root wins a type name it shares with an earlier one. |
| `--list-nodes` | — | Print the registered node types and exit. |
| `--headless` | — | Serve the API alone — `/control`, `/data`, `/term`, `/mcp`. The app's routes are never mounted. |
| `--demo` | — | Withhold the doors a public instance cannot offer: `dir`, `agent`, session save/load and `library save` leave the op table, and `/exec`, `/mcp`, `/term` and `/patch.gfi` are never mounted. `GOOFI_DEMO=1` is the same switch. Not a sandbox. |
| `--debug` | — | Open `/dev/*`: the UI primitive gallery at `/dev/ui`, and the other development surfaces. Shut otherwise. |

**When the backend is not on your machine,** the Save and Open dialogs each carry a second door —
*Download a copy* and *Open from this computer…* — which pass the `.gfi` through the browser rather
than the backend. This is a copy out and a copy in: it leaves the patch's remembered file alone, so
Ctrl+S never silently retargets to a download.

## Three engines, and your plugins

A node runs on an engine, and the half of its type name before the colon says which — `signal:Psd`,
`audio:Osc`, `graphics:Blur`. They share one graph and one node interface, so a cable crosses
between them: a band power off the signal engine can set a filter cutoff on the audio one.

| Engine | Frames of | Runs on |
| --- | --- | --- |
| `signal` | Array data, at whatever rate the source runs. LSL, MIDI and OSC in and out, spectra and band powers, complexity measures, and the arithmetic to route what comes out of them. | A thread per node, each scheduling itself. |
| `audio` | The same node interface at audio rate. Live in and out, MIDI, oscillators, filters, envelopes and feedback. | One topologically ordered thread. |
| `graphics` | Pixels. A node is a WGSL fragment shader: noise and shapes to make them, blur and displacement to work them, feedback to read back what the last tick drew. | The GPU. |

VST3 is a **format**, not an engine — a plugin runs on the audio engine. Its nodes come off the
machine the patch is open on, so goofi supports VST3 and ships none.

The nodes goofi does ship live in `node-bundles/`, one directory per bundle: `signal` and `audio`
(the Rust built-ins), `graphics` (the shaders), plus `eeg` (playback, LSL, band power, FOOOF),
`complexity` (the antropy measures), `biotuner` (harmonicity, microtonal keys, rhythm),
`simulation` (attractors, Kuramoto, Hopfield, neural mass) and `ml`. Every bundle is built at
goofi's build time and embedded, so `cargo run` carries them all; `--extra-nodes` adds a root
outside the repo the same way. A bundle with Python nodes names the packages they import in a
`requirements.txt`; `goofi-init` installs every bundle's, and at startup goofi checks every scanned
root's against both interpreters — a terminal is asked before anything is installed, and without
one the nodes are simply unavailable.

## Nodes

Drop a file in the patch workspace's `nodes_signal/`, `nodes_audio/` or `nodes_graphics/`, or
straight into a root: `smooth.py`, `Smooth.rs`, `Blur.wgsl`. The stem names the type, a leading `_`
hides it, and the extension names the SDK it is written against, which is what routes it to its
engine. The nodes goofi ships are the same kind of file under `node-bundles/`, so a toolchain is
needed to author a Rust node and never to run one; `goofi library get <type> --source` hands back
any node's source to copy.

```python
import goofi
import numpy as np


class Smooth(goofi.Node):
    """Rolling mean over the last axis."""

    INPUTS = {"data": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {"smoothing": {"window": goofi.IntParam(8, 1, 512, doc="Samples in the mean.")}}

    def process(self, data):
        w = self.params.smoothing.window
        kernel = np.ones(w, dtype=np.float32) / w
        return np.apply_along_axis(lambda v: np.convolve(v, kernel, mode="same"), -1, data.data)
```

A node declares itself in constants, read once by the import — never in hooks — and the class
docstring is its doc, whose FIRST LINE is the nutshell a catalog shows. `process` receives one
keyword argument per declared input slot and returns `{slot: value}`, or a bare value when there is
exactly one output. The full contract — every constant, every param type, the Rust and WGSL SDKs —
is at **[goofi.kairos-hive.org/docs/authoring](https://goofi.kairos-hive.org/docs/authoring/)**.

The same Python file runs on either tier, and the file does not choose: a discovery probe imports
it in a real interpreter and routes it in-process when its imports keep the GIL disabled, else to a
subprocess. The palette shows the tags a node declares, never the tier it runs on. The two
interpreters are `.gfivenv-ft` (free-threaded 3.14t) and `.gfivenv` (a GIL Python), both made by
`goofi-init`, and goofi uses no others. Re-run it after a version bump; it is idempotent.

Nothing fails silently. A node whose dependencies are missing everywhere is listed as
`unavailable`, greyed out and naming the missing module; an exception inside `process()` surfaces
on the node's error channel instead of taking anything down.

## Console

The Console panel shows application logs and captured stdout/stderr. Exact messages with the
same source, level, and stream share one row and a count. Each repeat moves its row to the bottom.
Use the severity buttons, node selector, and text filter to inspect the retained history.

The prompt runs the same commands as the CLI, without the leading `goofi`. Press Up or Down for
command history, and Tab for completion. Commands use the browser tab's undo history.

`goofi log list` reads the groups, `goofi log write "message" --level warning --component my-tool`
adds a message. Log history cannot be cleared from the app or command interface. Host Rust components can
write through `goofi_core::log::record`. Python node text keeps its node identity; native writes
without an identity appear under `goofi`.

The backend retains up to 10,000 groups and 16 MiB of message text. A message is limited to 64 KiB.
Stream lines end at a newline or EOF. Reconnects restore the retained groups; live repeats send
only their identity, count, sequence, and last timestamp. Agent PTYs keep their own terminals.

## Agents

goofi launches the coding harness you already use — `claude`, `codex` and `opencode` out of the
box, any command you name in `config.toml`. It comes up on a PTY with the patch workspace as its
working directory, reads that workspace's `AGENTS.md` to orient itself, and its terminal is a panel
beside the canvas.

```bash
goofi agent start --name claude
goofi node add signal:Psd
goofi link add lslin.out psd.data
```

Over `/mcp` an agent reaches the same ops the editor does, so there is nothing you can do that it
cannot. Every edit it makes arrives as the delta every client gets, so you watch the patch build
itself and take the mouse back whenever you like — its work lands on the same undo stack as yours.
Everything goofi can do, it does through that one op vocabulary: `/control`, `/mcp`, the CLI and a
test are transports over one entry point, never four surfaces with four sets of behaviour.

## Testing

```bash
cargo test --workspace --no-fail-fast         # backend
cargo test -p goofi-tests --features embed    # …plus the in-process Python tier
cargo clippy --workspace --all-targets        # prints nothing
cd frontend && npm run check && npm run test  # svelte-check, then vitest
cd tests/e2e && npm install && npm run e2e    # Playwright against the real binary
```

## License

MIT — see [LICENSE](LICENSE).
