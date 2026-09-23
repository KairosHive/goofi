# The library: plugin distribution

The local package contract is now `sdk/README.md`; `plugins.md` tracks the first corpus plugin.
Installed packages live in `.goofi/plugins/<id>/`, with an optional `nodes/` bundle.

One place to find, install, publish and update what goofi does not ship: **bundles** of nodes, and
**panel add-ons**. This item replaces `node-marketplace.md` and `panel-plugins.md`: distribution
and trust want one answer for both kinds of add-on, and the UI that browses them is one panel.

The model is the VCV Rack library. An author keeps the code in a git repo they host; the library
registers the repo, detects what is in it, and lets the author group the result into bundles that
anyone can install.

## What is already true and works in its favour

- Node discovery is a probe over a directory, and `--extra-nodes DIR` already adds directories to
  the scan. **An installed bundle is a directory.**
  - **But a later one does NOT win a shared type name, and this is load-bearing here.** Measured
    2026-09-06: `--extra-nodes node-bundles/biotuner`, whose files are also embedded, warns `two
    node files claim the type name ...; the later one wins` and then keeps the SHIPPED entry. While
    both files agreed that read as working. Edit the later one and the type turns `probe failed` —
    two files sharing a basename are one Python module name, so the second import answers with the
    first — and the node leaves the palette wearing a message that names no file. An installed
    bundle shadowing a shipped node is the whole point of the middle slot below, so this must be a
    real precedence over WHOLE bundles before any of it is built.
- A node that cannot load is registered UNAVAILABLE with its missing dependency named, so a bundle
  with unmet requirements degrades legibly instead of vanishing.
- `$GOOFI_HOME/.goofi/` exists (`goofi_core::home`), which is where an installed bundle lands.
  The **private library** already lives there — `.goofi/custom/`, a flat root with `library save`
  behind it — so the middle slot's precedence, the palette facet and the `.gfi`'s carrying of a
  non-shipped node are all worked examples rather than open questions.
- The `library` op group exists — `list`, `get`, `refresh` — and every op is on every transport, so
  the CLI, an agent, the panel and a test reach the same door.
- `registerPanel` exists, and the layout stores `panel_type` as a STRING: a panel add-on's type
  already rides the document, undo, a second tab and the `.gfi` with no backend change.
- The agent panel is a worked example of a panel with its own backend process.

## Decisions

**The installation unit is a plugin package.** `goofi-plugin.toml` defines its ID, version, and
interface version. Optional `backend/`, `frontend/`, and `nodes/` directories supply its content.
The `nodes/` directory is an ordinary flat node bundle, including its existing Python requirements
files. The local loader and SDK are implemented; see `sdk/README.md`. Distribution installs,
publishes, and updates a complete package. There is no per-node install.

**Installed plugins live in `$GOOFI_HOME/.goofi/plugins/<id>/`.** Their `nodes/` directories are
scanned after shipped and extra roots, before `.goofi/custom/` and the patch workspace. The plugin
ID supplies the node bundle's provenance. There is no separate installed-bundle directory.

**The repo's own bundles live in `node-bundles/<name>/`** for now, and they publish through the
same door a third party's do. `nodes_<engine>/` names the PATCH's own folder alone. Until the
install half exists, `--extra-nodes node-bundles/<name>` is how one is loaded.

**The bundles leave this repo** (decided 2026-09-23). goofi keeps its builtin nodes; every other
bundle moves to a node repo registered through the library like anyone else's, and the tests that
exercise a bundle's nodes move with it (`goofi-tests` situations use signal, audio and graphics
nodes only; today `all/graphics.rs` and `e2e/tests/harmonic-geometry.spec.ts` still reach into
bundles). The CI cost this removes, measured on the last green Linux run: the bundles' Python
packages installed at provisioning (60 s, CUDA and cuDNN wheels among them), the node-library
indexing at every boot (70 s, the biotuner probe alone 38 s), and the bundle build in the bridge's
build script.

**A source is a git repo, and its folders are its bundles.** An author registers a repo URL.
goofi parses the repo as folders: each folder that holds a goofi-parsable node is one bundle, and a
folder that holds none is ignored. By default every folder with a node is registered. The author's
one control is to deselect folders; ignoring is the only pattern, there is no include list and no
manifest that names bundles. A published bundle names the repo, the commit and the folder; an
install fetches exactly that; an update is a new pin.

**Detection is ONE static scan, and the real probe stays on the user's machine.** The service
never imports a stranger's code. It parses a repo for `goofi.Node` subclasses and their declared
`INPUTS`, `OUTPUTS`, `PARAMS` and docstring — a scanner in `goofi-node`, beside `scan_nd_calls`,
that goofi runs on a local folder and the service runs on a linked repo, so the panel shows the
list the service will. Availability is decided at install by the probe every node already
passes: an import that fails names its missing package in the palette, as today.

**Everything is `library` ops, and the service speaks the remote half of them.** The vocabulary
has a local half and a remote half:

- local: `plugin list` already reports installed packages. Add `plugin install | update | remove`
  against a path and a pinned git URL, with no remote service required.
- remote, goofi as a CLIENT of the service: `library login`, `library source add | list | remove`,
  `library search`, `library publish`. The service's HTTP API IS these phrases, so the panel's
  code calls `library source add` and does not know whether goofi forwards it or the website
  sends it directly.

An agent authors and publishes a bundle with exactly the CLI a human uses: log in, link a public
repo, define the bundle, publish.

**The library panel is one component, in goofi and on the website.** A user logs in inside their
local goofi and browses one library: their own custom nodes, the installed nodes, the bundles
available to install, and a search over all of it. An author who has registered a repo gets one
more tab that manages it: the repo's folders, with the ones goofi detected nodes in selected and
deselectable. The panel is built inside goofi first, driven by the `library` ops over the socket,
registered through `registerPanel` like every panel, and the exact same code renders in
`../goofi-website/`, where it drives the same phrases over HTTP.

**Login is OAuth against the forge that hosts the repo** (GitHub, GitLab). The library holds no
passwords: the login exists to prove the author owns the repo they link, and the forge already
knows that.

**A patch names the bundles it uses.** The manifest records `name@version` for every bundle a
node on the canvas came from. A load on a machine without one offers the install and otherwise
leaves the node UNAVAILABLE with the bundle named — never a silent absence. A patch's own
`workspace/nodes_*/` is untouched by this: it still travels inside the `.gfi`.

**Trust is provenance, not a sandbox.** A bundle is arbitrary code on the user's machine, and a
panel add-on is arbitrary code in the app's origin with the control socket; neither has a sandbox
and adding one is a project of its own. What the library guarantees instead: a bundle publishes
only from a public repo, at a pinned commit, under an account the forge vouches for, and the
panel shows all three before an install. Whether anything more is needed is decided by use.

## Plugin runtime

The folder loader, Python service SDK, operation hooks, frontend panel registration, editable
header entries, and bundled Rust/Python nodes are implemented. `sdk/README.md` owns that contract;
`plugins.md` tracks the session/corpus plugin. Distribution must use that interface without a
second panel loader or service protocol.

## Order of work

1. **Done: `node-bundles/complexity` and `node-bundles/eeg`**, loaded with `--extra-nodes`, each
   naming its packages in a `requirements.txt` that provisioning installs and startup checks, and
   one scenario per bundle in `goofi-tests`.
2. **Package distribution**: `plugin install <path | git url>`, `update`, and `remove` around the
   implemented folder loader. Add package version requirements to `.gfi` manifests.
3. **The static scanner** in `goofi-node`, and `library source` against a local folder, so an
   author previews the detection before any service exists.
4. **The panel**, inside goofi, against the local half.
5. **The service and the remote half**: accounts, sources, bundles, publish, search. Then the
   panel exports to the website.
6. **The bundles move out**: each `node-bundles/<name>` that is not builtin becomes a folder of a
   registered node repo, its situations and e2e specs go with it, and provisioning, the boot
   scan and CI keep only the builtin nodes.

## Open

- What the service is written in and where it is hosted, and what it stores in. It shares the
  scanner with goofi, so Rust is the default; nothing else is decided.
- Published Rust nodes use the existing engine SDKs and build allowlists through the package
  `nodes/` directory. Verify that the published package contains every required source.
- Which bundles are builtin and stay in this repo, and whether a builtin node is anything but a
  bundle that ships inside the binary.
- Whether a first-party bundle is ever privileged over a third party's — pinned, unremovable, or
  exempt from naming itself in a patch's manifest. Nothing is today, and the door is only proved
  while nothing is.
- A private repo as a source: the login could reach it, but "publishes only from a public repo"
  is what trust rests on above.
