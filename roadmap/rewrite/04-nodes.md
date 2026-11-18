# 04. Nodes and the node library

The nodes are a library that allows execution. A node declares what it is and implements what it
does. It does not know when it runs: that is the decision of the runtime mode (part 06).

## 4.1 What a node is

- A node type is one source file. The stem of the file names the type. The extension names the
  language and its SDK: `.rs`, `.py`, `.wgsl`. A leading underscore hides a file.
- A type runs on one engine. The engine of a file is stated in one way per language: a WGSL file
  is graphics; a Rust file names the SDK of its engine; a Python file is signal unless its first
  line is `# goofi: <engine>`.
- The type name is written the same way in every language: the name of the file stem as the
  author wrote it, in CamelCase.
- A bundle is a folder of node files. A bundle can span engines. A bundle has one small manifest
  with its name, author, description and licence.
- There is one node contract across engines and languages: the same manifest, the same params,
  the same lifecycle, the same context. An engine adds only what its plane needs.

## 4.2 The manifest

A node declares, in constants that are read once without running the node:

| Field | Meaning |
| --- | --- |
| doc | The first line is the summary a catalog shows. The rest is the body. |
| tags | From a closed vocabulary: input, output, generator, transform, analysis, control, image, text, midi, eeg, cardio, motion, music, ml, connectivity, simulation. An unknown tag is refused. |
| inputs | Per slot: name, kind, default coupling, list flag, required flag. |
| outputs | Per slot: name, kind. |
| params | See 4.3. |
| producer | The node makes data without an input; its timer is on by default. |
| feedback | The node closes a loop with one step of delay. |
| stateless | The node keeps nothing between runs; it can be replicated (part 06.3.5). |
| state | The named state the node keeps and can save (graphics state buffers, plugin state). |
| window | The node shows its input in a native window. |
| editor | The node has a native editor window. |

- The engine and the bundle are facets the palette derives. They are never tags.
- A slot name and a param name follow the name rule of nodes (part 03.2).
- A discovery refuses a manifest with an unknown tag, an illegal name, a duplicate name or an
  output of the kind of another engine. The type is then unavailable with the reason.

## 4.3 Params

- A param has: a group, a name, a value description (part 02.6.3), a doc, an optional default
  expression, a section, show rules and a role.
- Kinds: **number** (scalar or vector; float or integer; a soft range; options; a face: plain,
  colour, envelope), **boolean**, **string** (free, or with options, or with options the node
  refreshes at run time), **pulse**.
- The range of a number is the range its values can take. It guides the widgets. It is not a
  clamp.
- A colour is four numbers from 0 to 1. An envelope is attack, decay, sustain and release;
  times in seconds up to a maximum, sustain from 0 to 1.
- **Groups and sections.** The main group holds every primary option of the node. Auxiliary
  params go in other groups. A group is parted into sections by meaning.
- **Lists.** A section can repeat: a count with a minimum, a maximum (at most 64) and a default,
  and per slot the params of the section with their own defaults. An expression or a preset
  addresses one slot as any param. One list mechanism serves vectors of numbers and repeated
  sections. A list can name a widget that draws it whole; the colormap is one (stops with a
  place, a colour and a blend to the next).
- **Show rules.** A param shows only while other params of the same node hold given values. A
  rule names a controller and the values it may hold; every rule of a param must hold. A
  controller has a fixed set of values: a string or integer with options, or a boolean. A hidden
  param keeps its value and its source, and the node runs as before. Discovery refuses an
  unknown controller, a controller without a fixed set, a value that is not an option, and a
  chain that returns to its start.
- **Roles.** A param can be the feed of an input slot (it names a variable whose frames feed the
  slot), or held by the node (the saved state of the node owns the value).
- **Documentation.** Every param has a doc and a range. A node without them fails the check of
  the shipped library.
- **Common params.** Every node of an engine has one common group, declared once per engine:

  | Engine | Common params |
  | --- | --- |
  | signal, and texture sources of graphics | `autotrigger` (on for a producer); `rate` with its `unit` (updates per second, or seconds per update; 0 is no cap; the default follows `signal.default_ufreq`); `trigger` (a pulse that runs the node once). |
  | graphics | `width`, `height` (0 follows the input; the default follows the patch). |
  | audio | none. |

- A param edit never runs a node.
- A node can declare a common param itself; its declaration replaces the default and the range.
- A show rule on a section applies to every param of the section.

## 4.4 The three languages

An author writes a node in Rust. Python is for an algorithm that is too complex to port and that
an efficient Python package already implements. A shader is WGSL.

### 4.4.1 Rust

- A node is a type that implements the node interface of its engine, a manifest, and one export
  line. The node module forbids unsafe code. A node uses the SDK and a fixed list of crates.
- **Signal and texture-source interface:** setup; process (inputs, outputs, context, params);
  hooks for a changed param, refreshed options and a pulse; teardown when the node is dropped.
- **Audio interface:** the channel widths of the ports; prepare (rate); process (block);
  save and load of state. `process` does not allocate, lock or make a system call. A numeric
  param arrives as a lane of values per sample or as one value per block. Helpers cover
  per-channel state, edges, and pitch conversion.

### 4.4.2 Python

```python
import goofi
import numpy as np

class Smooth(goofi.Node):
    """Rolling mean over the last axis."""

    TAGS = ["transform"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {"smoothing": {"window": goofi.NumParam(8, 1, 512, int=True, doc="Samples in the mean.")}}

    def process(self, input):
        w = self.params.smoothing.window
        kernel = np.ones(w, dtype=np.float32) / w
        return np.apply_along_axis(lambda v: np.convolve(v, kernel, mode="same"), -1, input.data)
```

- A node declares itself in constants, never in hooks. The docstring is the doc.
- `process` receives one keyword argument per input slot. It returns nothing, a value per output
  slot, or one bare value when there is one output. A value is a frame, a string, an array with
  its meta, a dictionary (a table), an array (it takes the meta of the first input when the
  shape matches), or a texture.
- Methods: `setup`, `process`, `stop`; one refresh method and one pulse method per param that
  needs it. Members: the params, the patch time, the seeded random generator, the log, and the
  clearing of an input.
- A group that is a list of dictionaries has one section per dictionary. A show rule is a pair
  of a controller and its values.
- The package gives: frames with meta helpers; a stream with history; the geometry encoder; the
  agent harness (`goofi.agent`); the transport.
- The same file runs on either Python tier (part 05.1). The per-call cost of the binding does
  not grow with the number of params.

### 4.4.3 WGSL

- A shader file opens with a header comment that holds the manifest as JSON: doc, tags, inputs
  (texture, array, audio), params, sections, lists, show rules, feedback, window and state
  buffers. The one output is `out`.
- The body defines `shade(uv)` and one `next_<buffer>(uv)` per state buffer (part 05.3.2).
- A param is read as `p.<name>`; a list as `p.<name>[i]` with its count.

## 4.5 Lifecycle and context

- Lifecycle: construct; replay every param through the changed-param hook; setup; then runs;
  stop. Construction, replay and setup are one unit that succeeds or fails whole.
- A node can save and load its state. The state is saved in the workspace of the patch.
- The context of a run gives: the patch time (`now`), a random generator seeded per run and per
  node, the log with the node as source, the params, the count of readers per output, and the
  clearing of an input slot (it takes effect after a successful run).
- A node that reads only the context for time and randomness is deterministic in the Kahn mode.
- A panic or an exception in any hook is the error of the node and never crosses into the
  engine.

## 4.6 Discovery and build

- **Roots**, scanned in this order: the built-in bundles; the extra roots named at the start;
  the installed bundles of the library;
  the nodes of plugins; the private bundle of the user; the workspace of the open patch.
- A workspace holds its node files in `nodes/`, in one folder for all engines.
- Two types with the same name and engine in different bundles coexist (4.8). There is no "last
  root wins".
- **Rust:** a node file is built into a library against the SDK of this version. The builder
  generates the crate around the file, builds with one shared target directory, and stores the
  artifact under a content key (the version, the SDK, the allowed crates, the bytes of the
  file). A library states the version and SDK it was built for; a mismatch is refused.
- The shipped Rust nodes are built with the program: a shipped node that does not compile fails
  the build. A toolchain is needed to author a Rust node and never to run a shipped one.
- A scan never runs a build under the graph lock. A node that is not built yet says so and is
  built by a library refresh.
- **Python:** a probe imports the file in a real interpreter and reads its constants. The answer
  is cached by content, including the installed packages. A failure is never cached.
- **WGSL:** the shader is validated at the scan.
- A library refresh builds and scans again, restarts the nodes of changed types, and reports the
  types added, changed and removed.
- `library get <type> --source` returns the source of any node, to copy.
- At the start and at a load, the Rust node files of the roots and of the workspace are built
  before the scan.

## 4.7 Dependencies

- A bundle with Python nodes names its packages in `requirements.txt`, and packages that have no
  free-threaded build in a second file for the interpreter with the GIL. Bundles share the two
  environments of the runtime.
- Provisioning installs the requirements of every bundle. At the start goofi checks the
  requirements of every scanned root against both interpreters. A development build asks the
  terminal before it installs.
- A node whose dependency is missing is unavailable and names the missing module.
- A model or a data file that a node needs is downloaded on first use with a checksum, and the
  node says what it downloads and how large it is.

## 4.8 The library

### 4.8.1 Bundles and sources

- Every node belongs to a bundle. The bundles `signal`, `audio`, `graphics` and `eeg` are built
  in. The other bundles of appendix A live in a separate repository of bundles and are installed
  from it.
- A source is a Git URL, or `<owner>/<repo>` with an optional `@<ref>`. It works through the Git
  of the user, with the authentication of the user. One source per repository; a new ref
  replaces the old.
- A checkout lives at `~/.goofi/library/<owner>/<repo>/`. The private bundle of the user is
  `~/.goofi/library/_local/`.
- A fresh home has no source and contacts no remote.

### 4.8.2 Entries and selection

- An entry has: name, engine, bundle, author, and source (built in, local, patch, plugin, or Git
  with URL and ref).
- A node is selected by its name plus optional selectors: bundle, engine, author, source, ref.
  Exactly one match succeeds. No match and several matches are refusals that list the
  candidates. There is no implicit preference between sources. Every caller shares the rule.
- A resolved reference is stored in the patch, the clipboard and the history. A later collision
  or an update of a repository does not change a stored reference.

### 4.8.3 Behaviour

- The library registers and removes sources; browses, inspects and searches; installs and
  removes whole bundles; checks for and pulls updates; refreshes; and reports the state of
  preparation.
- To register a source lists its bundles without running node code.
- To install a bundle prepares its dependencies and its nodes, shows progress and failures, and
  keeps a failed bundle visible for a new attempt.
- The files of a checkout are the truth: an edit applies on refresh or at the start.
- A pull is an action of the user and is fast-forward only. To uninstall keeps the checkout.
- Library management works without a running server, is allowed while a patch runs, never
  dirties the patch and never enters the history.
- A library view in the app shows sources, bundles, entries and their state. The add menu
  offers only loaded nodes.
- A bundle can ship example patches.

### 4.8.4 Loading a patch with missing nodes

- A patch carries, per node, its structured reference with the source and the revision that was
  observed.
- When nodes are unavailable, the load offers: install and load; load anyway (skip the nodes,
  report the skipped nodes and cables, keep the rest); cancel. The CLI, MCP and scripts get the
  same choices as an argument of the load.
- A node of the same name from another source is never substituted silently.

### 4.8.5 The online library

- The online library is the hosted side of the same model, in the spirit of VCV Rack and
  ComfyUI. Under the goofi account a person publishes a bundle, and another person searches,
  browses, installs and updates it from the app.
- A published bundle is a source like any other; the account adds identity, search and sharing.

## 4.9 The shipped library

- Appendix A lists every node of the present program: 215 node files in 15 bundles and the
  built-in nodes of the engines. The rewrite keeps every one of them.
- Structural nodes of every plane: a crossing node per pair of planes, named after the source
  plane and living on the receiving plane (`signal:AudioIn`, `signal:GraphicsIn`,
  `audio:SignalIn`, `audio:GraphicsIn`, `graphics:SignalIn`, `graphics:AudioIn`); one `Variable`
  node per plane; pass-through anchors for expressions (one per kind on the signal plane, one on
  the audio plane); one `Feedback`
  node per plane; playback nodes for recorded arrays, strings, tables and audio.
- **Loader and writer nodes** connect a graph to a dataset (part 10.4): a loader emits the
  frames of a sample between markers; a writer writes blobs and samples.
- Conventions every shipped node follows:

  | Matter | Convention |
  | --- | --- |
  | Primary slots | `input` and `out`. |
  | Names | Slots and params in snake case. Types in CamelCase, acronyms as words (`Lfo`, `Llm`, `Fft`). |
  | The primary selector | `mode`. |
  | Frequency bounds | `low` and `high`, in Hz. |
  | Pitch | Hz on the signal plane, volts per octave on the audio plane; a param in the other unit says so in its name. |
  | Angles | Degrees. |
  | Spelling | `color`. |
  | Seeds | An integer; −1 is random. |
  | Lists | A vector or a list param, not a string to parse. |
  | Run control | `running` (boolean) and `reset` (pulse). |
  | Files | `file` or `folder`; a bare name resolves in the home; a path is absolute or relative to the workspace. |
  | Images | A texture on every plane that takes images. |
  | Axis | `axis`, default −1 (time). |
  | Generators | `mode` of value or block, with `sfreq`. |

- The example patches of the repository use only the built-in bundles. Every other bundle ships
  its own examples. A check loads each example against its bundles.

## 4.10 Open decisions

1. The graphics policies shared by all shipped shaders: alpha, colour space, borders and units.
2. Authentication for private repositories; preparation of dependencies without build tools of
   the host.
3. Node content that is listed as later work in the present roadmap (further graphics, audio,
   simulation and timbre nodes) is outside the rewrite; appendix A names the candidates.
4. Whether the hosted library needs review, signing or sandboxing of published bundles.
