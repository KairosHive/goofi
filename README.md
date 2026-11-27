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
patch without either holding the other up. Several people can work inside it at once, on one
history: undo takes back the last write, whoever made it.

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
`--debug` the same way. `GOOFI_BOOT_ONLY=1` boots the node library and exits without serving,
which is what a CI job runs to prove the build; it has no flag.

| Flag | Default | Effect |
| --- | --- | --- |
| `--port N` | `8000` | The port to serve on. |
| `--bind HOST` | `127.0.0.1` | The address to serve on. Anything beyond this machine warns: there is no auth, and `/term` is a real shell. |
| `--extra-nodes ROOT` | — | A folder of node files, scanned after the shipped bundles. Repeatable; a later root wins a type name it shares with an earlier one. The plugins' nodes, the private library `~/.goofi/custom/` and the open patch's own workspace are scanned after it, and each beats the roots before it. |
| `--load PATCH` | — | Open this `.gfi` at start. `GOOFI_LOAD` is the same. |
| `--headless` | — | Serve the API alone — `/control`, `/data`, `/term`, and `/mcp` under `--mcp`. The app's routes are never mounted and the `layout` ops leave the op table. |
| `--mcp` | — | Serve the MCP endpoint at `/mcp`: one tool, `goofi_exec`, running the CLI's command lines. Off by default; `GOOFI_MCP=1` is the same switch. |
| `--demo` | — | Withhold what a public instance cannot offer. [`demo.toml`](demo.toml) is the one list of it: the ops that leave the op table, the modules of the server that are not mounted or not opened (`/exec`, `/mcp`, `/term`, `/patch.gfi`, plugins, VST3, native windows, every sound card but `browser`), and the bundles and node types that leave the library. `GOOFI_DEMO=1` is the same switch, and `cargo run -p goofi-init -- --demo` installs no package of a withheld bundle. Not a sandbox. |
| `--debug` | — | Open `/dev/*`: the UI primitive gallery at `/dev/ui`, and the other development surfaces. Shut otherwise. |

The Save and Open dialogs each carry a second door — *Download a copy* and *Upload…* — which pass
the `.gfi` through the browser rather than the backend. This is a copy out and a copy in: it leaves the patch's remembered file alone, so
Ctrl+S never silently retargets to a download.

## Three engines, and your plugins

A node runs on an engine, and the half of its type name before the colon says which — `signal:Psd`,
`audio:Osc`, `graphics:Blur`. They share one graph and one node interface, so a cable crosses
between them: a band power off the signal engine can set a filter cutoff on the audio one.

| Engine | Frames of | Runs on |
| --- | --- | --- |
| `signal` | Array data, at whatever rate the source runs. LSL and OSC in and out, MIDI out, spectra and band powers, complexity measures, and the arithmetic to route what comes out of them. | A thread per node, each scheduling itself. |
| `audio` | The same node interface at audio rate. Live in and out, oscillators, filters, envelopes and feedback. | One topologically ordered thread. |
| `graphics` | Pixels from WGSL shaders, with shared GPU textures. Noise and shapes generate images; blur, displacement, and feedback process them. A Python node can run inside the tick (Camera, VideoFile, NdiIn, Depth); a signal node (a model in Python) makes or reads a texture on its own pace. | The GPU. |

An `AudioOut` plays on a sound card of the machine goofi runs on, or on the `browser` device:
the mix is sent to every browser on the app as Opus, and each tab plays it unless the sound
button in its top bar is off. A goofi on another machine, or a public one, is heard that way.
The `default` device is the machine's default sound card, and `browser` where it has none.

VST3 is a **format**, not an engine — a plugin runs on the audio engine. Its nodes come off the
machine the patch is open on, so goofi supports VST3 and ships none.

The nodes goofi does ship live in `node-bundles/`, one directory per bundle: `signal` and `audio`
(the Rust built-ins), `graphics` (the shaders), plus `eeg` (playback, preprocessing, band power, specparam, connectivity),
`complexity` (the antropy measures), `biotuner` (harmonicity, microtonal keys, rhythm),
`simulation` (attractors, Kuramoto, Hopfield, neural mass), `ai` (language, voice and coding-agent
nodes), `ml`, `image`, `image-generation`, `computer-vision`, `harmonic-geometry` and `inception`. Every bundle is built at
goofi's build time and embedded, so `cargo run` carries them all; `--extra-nodes` adds a root
outside the repo the same way. A bundle with Python nodes names the packages they import in a
`requirements.txt`; `goofi-init` installs every bundle's, and at startup goofi checks every scanned
root's against both interpreters. A development build asks the terminal before it installs, and
stops when there is no terminal or the answer is no.

A MIDI controller comes in as variables: `goofi midi learn` turns it into a variable group, and
the `Variable` node of each engine puts a variable on a cable. Notes for playing stay a node: the
audio engine's `MidiIn` opens a port and deals its notes out as voices, gate, pitch and velocity.

## Nodes

Drop a file in the patch workspace's `nodes_signal/`, `nodes_audio/` or `nodes_graphics/`, or
straight into a root: `smooth.py`, `Smooth.rs`, `Blur.wgsl`. The stem names the type and a leading
`_` hides it. The node declares its engine: a Python class by its base (`goofi.SignalNode`,
`goofi.GraphicsNode`), a Rust file by the SDK whose `export!` it spells (`goofi_signal_sdk`,
`goofi_audio_sdk`, `goofi_graphics_sdk`), and a `.wgsl` file is a graphics node. The nodes goofi ships are the same kind of file under `node-bundles/`, so a toolchain is
needed to author a Rust node and never to run one; `goofi library get <type> --source` hands back
any node's source to copy.

```python
import goofi
import numpy as np


class Smooth(goofi.SignalNode):
    """Rolling mean over the last axis."""

    INPUTS = {"data": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {"smoothing": {"window": goofi.NumParam(8, 1, 512, int=True, doc="Samples in the mean.")}}

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
See [textures](sdk/graphics.md) for shaders and for Rust and Python nodes that make or read a texture.

The same Python file runs on either tier, and the file does not choose: a discovery probe imports
it in a real interpreter and routes it in-process when its imports keep the GIL disabled, else to a
subprocess. The palette shows the tags a node declares, never the tier it runs on. The two
interpreters (free-threaded 3.14t and a GIL Python) live in goofi's runtime directory, both made
by `goofi-init`, and goofi uses no others. Re-run it after a version bump; it is idempotent.

Nothing fails silently. A node whose dependencies are missing everywhere is listed as
`unavailable`, greyed out and naming the missing module; an exception inside `process()` surfaces
on the node's error channel instead of taking anything down.

## Console

The Console panel shows application logs and captured stdout/stderr. Exact messages with the
same source, level, and stream share one row and a count. Each repeat moves its row to the bottom.
Use the severity buttons, node selector, and text filter to inspect the retained history.

The prompt runs the same commands as the CLI, without the leading `goofi`. Press Up or Down for
command history, and Tab for completion. Commands use the browser tab's undo history.

The CLI answers in the reader's style. On a pipe every answer is compact: one line for a refusal,
JSON for data, what an agent or a script parses. On a terminal it is laid out for a person — a
catalog as a table, a status as its values and then its lists, a refusal as why and then the help
that answers it — and painted so attention lands in order: the alert, then a title, then the
names under it, then their arguments, prose last. `GOOFI_STYLE=human|compact` decides the layout,
`GOOFI_COLOR=always|never` the color, which `NO_COLOR` and `TERM=dumb` also turn off; `--json`
prints the raw result either way.

`goofi log list` reads the groups, `goofi log write "message" --level warning --component my-tool`
adds a message. Log history cannot be cleared from the app or command interface. Host Rust components can
write through `goofi_supervisor::log::record`. Python node text keeps its node identity; native writes
without an identity appear under `goofi`.

The backend retains up to 10,000 groups and 16 MiB of message text. A message is limited to 64 KiB.
Stream lines end at a newline or EOF. Reconnects restore the retained groups; live repeats send
only their identity, count, sequence, and last timestamp. Agent PTYs keep their own terminals.

## Agents

goofi launches the coding harness you already use — `claude`, `codex` and `opencode` out of the
box, any command you name in `config.toml`, with the flags its `model` and `effort` options become.
It comes up on a PTY with the patch workspace as its working directory, and its terminal is a
panel beside the canvas. The workspace's `AGENTS.md` says what the directory is and points at
`goofi guide get orientation` — the same orientation an agent outside goofi reads, from any shell
with `goofi` on its PATH. `goofi guide` names it and the skills beside it: goofi's own, which
ride into every workspace that has none (patching a biosignal chain, designing biofeedback,
designing an emergent shader), and the patch's.

```bash
goofi agent start --name claude --model opus --effort high
goofi node add signal:Psd
goofi link add lslin/out psd/input
```

The harness plane is `goofi_agent`, a crate every goofi process of a session shares: an instance
has a record in the session directory and a loopback socket on the process that owns its PTY, so
the panel, the CLI and a node all list, attach to, type into and stop the same instances. The `ai`
bundle builds on it: `Agent` is a node that starts an instance, or picks a running one, and types
every text frame it receives into it as a steering prompt; `LLM` answers each prompt through
pydantic-ai, from any provider or a local server, in a running conversation or one request at a
time; `VoiceAgent` is a live call with a speech-to-speech model whose one tool hands commands to an
`Agent` node; `Decision` picks one of the answers on its options input for each text, with the
model's probability for each; `TextToSpeech` and `SpeechToText` cross between text and audio frames.

Under `--mcp`, an agent connected over `/mcp` reaches the same ops the editor does, so there is nothing you can do that it
cannot. Every edit it makes arrives as the delta every client gets, so you watch the patch build
itself and take the mouse back whenever you like — its work lands on the same undo stack as yours.
Everything goofi can do, it does through that one op vocabulary: `/control`, `/mcp`, the CLI and a
test are transports over one entry point, never four surfaces with four sets of behaviour.

A node added without a position lands on the canvas grid right of the one added before it, and
`goofi nodes arrange` lays a patch out by its dataflow in one undo step, so an agent's patch reads
as well as a hand-built one.

## State machines

A patch can carry state machines beside its graph: boxes on a second canvas, transitions between
them, and playheads that step through them and set the patch's variables as they go. A state's
value may play an ADSR envelope on a playhead's arrival, and over a transition's duration each
attribute eases by a curve of its own — a preset, or a curve of anchor points drawn in a curve editor. A state
decides which transition out to take every interval, in turn, by a weighted draw or by an
expression; a transition's own `condition` takes it the instant the condition turns true. Both are
the Python a param's expression is, over `vars.*`, `nd()`, `t` and the same functions, plus `me`,
the playhead the expression is read for, and `sm('name')`: a state's residents and the moves of
the instant, or another playhead's record. A param reads a playhead the same way, `sm('name').x`.
`goofi machine add`, `machine state add` and `machine arrange` build one from the CLI or an agent;
the same ops drive the panel. Beside the canvas, the score shows the whole machine as columns and
rows: a column per state, in loop order and as wide as its interval, a row per attribute, every
cell a slider a drag paints across — so a melody is drawn over the states and read off them — the
transitions as arcs above, the playheads sweeping the columns they rest in, and a cog on each
column, row and cell for the settings behind it. A selection on the canvas lights its columns and
is edited as one; a line dragged out of a selected group fans out, into one fans in, and with the
chain tool wires every box it crosses.

## The clock and the timeline

A patch plays on one clock from the moment it loads, and by default it never ends. `t` in an
expression is its position in seconds, and so is `vars.clock.time`. `goofi clock set --no-playing`
pauses the position and `--time 12` puts it at a second; the engines run on through a pause, and
a recording never stops. `vars.clock.bpm` is the tempo. `vars.clock.length` is the timeline's
length in beats: with one, the position restarts from 0 when it reaches it. A length of 0 is
endless.

The timeline holds tracks on that position, their times in beats, so a change of the tempo moves
them all together. A `note` track holds notes; it writes the held velocities to
`vars.<track>.notes`, and the audio `MidiIn` node lists it as a port (`timeline: <track>`), so
its notes play an instrument as a keyboard does. Its lane has a piano keyboard beside it, the
panel's own: from a lowest to a highest pitch, marking the notes of a scale and a root, the ones a
`Quantize` node offers, and a grid; a note made or moved on the lane lands on that scale and that
grid. Until its bar is dragged, a keyboard shows two octaves from C3 and every note the track
holds; then it shows what was set, and a note outside that lies off the lane. A `curve` track is one line
through keyframes: the value eases from a keyframe to the next by that keyframe's curve, holds
outside them, and is `vars.<track>.value`. A keyframe's curve is `smooth` unless it is given
another: the value goes through the keyframe at the slope the keyframes beside it give it, so a
run that falls keeps falling, and it is level at a peak. The others are `step`, `linear`,
`easing`, which is slow at both ends, and a custom curve of anchor points. `goofi timeline track add note` and
`timeline track edit` build one from the CLI or an agent; the same ops drive the timeline panel.

A track records its `input` into itself while the clock plays: the record button beside its
name, or `timeline track record`, begins a recording, and ends it. The input is an expression, as
a param's. A note track reads the keys a MIDI device holds, `vars.<device>.notes`: a track with
no input is given the device there is, or asked which where there are several. The notes played
are added to the notes the track holds. A curve track's input is typed or dropped on it: a node,
a variable, a state. The track takes the input's value at most `rate` times a second (30 unless
set), and a node slower than that gives a sample for each of its
frames; an input that reads no node holds its value between two changes, a step. The end of the
recording puts keyframes with fitted bezier curves that follow what came, in place of the
keyframes of the stretch it covered. A loop or a seek begins another pass, so what a seek jumps
over stays as it was. `vars.<track>.recording` is 1 while a
recording runs, and `vars.<track>.take` holds what it has taken so far, which the panel draws in
the lane, in the recording's colour, until the take is the track's.

The panel puts every track on one time axis, each beside what it says about itself, with a line
across them where the clock is. A click on a lane makes a note or a keyframe, and a drag along it one in
each cell or stretch it goes through; a drag along a curve's lane draws over the keyframes in
its path. A note or a keyframe may stand before 0, as it may past the timeline's length: the
clock does not come there, and a curve from before 0 is the curve at 0. A click on a note takes it out, and a click on a keyframe picks it. A click with Ctrl or Shift
held picks one more, or one less, and a drag with Shift held on a lane draws a box that picks
what it touches. A drag of a note or a keyframe moves it, and with it everything picked. The
Delete key takes out what is picked, and Escape picks none. Ctrl+C copies what is picked, as the
Copy button of its menu does, and Ctrl+V, or Paste in a track's menu, takes the copy in hand: it
is drawn over the lanes where it would land, and a press on a lane places it where the press is
let go, as one edit. Notes begin in the cell under the pointer, the lowest on the pointer's key;
keyframes begin at the pointer's beat, keep their values, and take the place of the keyframes in
the stretch they cover. A copy from several tracks lands in tracks as far apart as those were,
each part in a track of its kind. Escape, or the chip in the bar, lets the copy go. A right click on a note or a keyframe, or a held finger, opens a
menu on it with the settings of what is picked: a note's pitch, length and velocity, a
keyframe's value and curve. A note
snaps to its keyboard's grid, and a new keyframe eases as the keyframe before it does. In the corner beside the ruler
are play, stop and the clock's position, a number that a click makes a field. A press on the
ruler places the clock, and a drag along it marks a section the clock loops in, in the length's place: that section is
the clock's own, no variable holds it and no patch carries it. The bar above the ruler is the
view, as long as a lane: a view of the whole timeline fills it, a drag of it moves along the
axis, and a drag of either end of it zooms, as far out as you like: past an end of the timeline
that end of the bar folds and takes a muted accent. A wheel turned along the axis over a lane
or the ruler moves the view too: sideways, or with Shift held on a wheel that turns one way,
and down in a panel taller than wide. With Ctrl held a wheel zooms, about the point of the axis
under the pointer. A double click on the ruler looks at everything
again. A line across the tracks marks where the timeline begins, and one where it ends when it
has a length. A track's edge resizes it, as far as it is dragged, and the track area scrolls. Between what a track says and its lane
is a bar of what the lane shows: the keys of a keyboard, or the values of a curve with its two
limits at the bar's ends, each a number a click makes a field. A drag of the bar moves that
range, a drag of an end changes it, and a double click fits it to what the track holds; a
double click on the bar above the ruler looks at everything too. Beside its lane a track says its
name, with the button to its menu, and its settings in named sections that fold; a track too
short for them scrolls them. A note track is as tall as its keyboard has keys, until its edge is
dragged. The recording's section has the record
button on its line and begins folded; a curve's input in it is a param's own row, so it takes
an expression, a MIDI learn, or a node, a variable or a state dropped on it. A panel hides the tracks it is told to, so two panels show one timeline in different ways. In a panel
taller than wide the time runs down and the tracks stand as columns.

## Testing

```bash
cargo test --workspace --no-fail-fast         # backend
cargo clippy --workspace --all-targets        # prints nothing
cd frontend && npm run check && npm run test  # svelte-check, then vitest
cd tests/e2e && npm install && npm run e2e    # Playwright against the real binary
```

## License

goofi is free software under the [GNU Affero General Public License v3.0](LICENSE).

Closed-source projects that wish to build with or on top of goofi are welcome to do so under a
commercial license, available on request at [team@kairos-hive.org](mailto:team@kairos-hive.org).
