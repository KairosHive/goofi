# The library: plugin distribution

One place to find, install, publish and update what goofi does not ship: **bundles** of nodes, and
**panel add-ons**. Distribution and trust want one answer for both kinds of add-on, and the UI that
browses them is one panel.

The model is the VCV Rack library. An author keeps the code in a git repo they host; the library
registers the repo, detects what is in it, and lets the author group the result into bundles that
anyone can install.

## A precondition, measured 2026-09-06

A later root does NOT win a shared type name, and this is load-bearing: `--extra-nodes` on a
bundle whose files are also shipped warns `two node files claim the type name ...; the later one
wins` (`goofi-cli/src/main.rs:772-775`) and then keeps the SHIPPED entry; edit the later one and
the type turns `probe failed`, because two files sharing a basename are one Python module name in
the probe (`goofi-pymod/src/loader.rs:58`). Precedence is still per type name, last root wins
(`goofi-bridge/src/lib.rs:1034-1038`). An installed bundle shadowing a shipped node is the whole
point of the plugin slot, so this must be a real precedence over WHOLE bundles before any of it is
built.

## Decisions

**The installation unit is a plugin package** (`sdk/README.md` is the contract; `plugins.md`
tracks the first plugin). `goofi-plugin.toml` defines its ID, version, and interface version;
optional `backend/`, `frontend/`, and `nodes/` directories supply its content, `nodes/` being an
ordinary flat node bundle with its requirements files. Distribution installs, publishes, and
updates a complete package. There is no per-node install, and no second installed-bundle format.

**Installed plugins live in `$GOOFI_HOME/.goofi/plugins/<id>/`.** Their `nodes/` are scanned after
shipped and extra roots, before `.goofi/custom/` and the patch workspace. The plugin ID supplies
the node bundle's provenance.

**The bundles leave this repo** (decided 2026-09-23). goofi keeps its builtin nodes; every other
`node-bundles/<name>` moves to a node repo registered through the library like anyone else's, and
the tests that exercise a bundle's nodes move with it (`goofi-tests` situations use signal, audio
and graphics nodes only; the graphics situation still adds the graphics bundle's shaders by type
until it brings its own). Until then, `node-bundles/<name>` publishes through the same door a third
party's does, and `nodes_<engine>/` names the PATCH's own folder alone.

**A source is a git repo, and its folders are its bundles.** Each folder that holds a
goofi-parsable node is one bundle; a folder that holds none is ignored. The author's one control is
to deselect folders; ignoring is the only pattern, there is no include list and no manifest that
names bundles. A published bundle names the repo, the commit and the folder; an install fetches
exactly that; an update is a new pin.

**Detection is ONE static scan, and the real probe stays on the user's machine.** The service never
imports a stranger's code. It parses a repo for `goofi.Node` subclasses and their declared
`INPUTS`, `OUTPUTS`, `PARAMS` and docstring — a scanner in `goofi-node`, beside `scan_nd_calls` —
that goofi runs on a local folder and the service runs on a linked repo, so the panel shows the
list the service will. Availability is decided at install by the probe every node already passes.

**Everything is `library` ops, and the service speaks the remote half of them.**

- local: `plugin list` exists. Add `plugin install | update | remove` against a path and a pinned
  git URL, with no remote service required.
- remote, goofi as a CLIENT of the service: `library login`, `library source add | list | remove`,
  `library search`, `library publish`. The service's HTTP API IS these phrases, so the panel's code
  does not know whether goofi forwards it or the website sends it directly.

An agent authors and publishes a bundle with exactly the CLI a human uses.

**The library panel is one component, in goofi and on the website.** Built inside goofi first,
driven by the `library` ops over the socket, registered through `registerPanel`, and the same code
renders in `../goofi-website/` over HTTP. An author who has registered a repo gets one more tab
that manages its folders.

**Login is OAuth against the forge that hosts the repo** (GitHub, GitLab). The library holds no
passwords: the login proves the author owns the repo they link.

**A patch names the bundles it uses.** The manifest records `name@version` for every bundle a node
on the canvas came from. A load on a machine without one offers the install and otherwise leaves
the node UNAVAILABLE with the bundle named — never a silent absence. A patch's own
`workspace/nodes_*/` still travels inside the `.gfi`.

**Trust is provenance, not a sandbox.** A bundle is arbitrary code on the user's machine and a panel
add-on is arbitrary code in the app's origin; adding a sandbox is a project of its own. The library
guarantees instead: a bundle publishes only from a public repo, at a pinned commit, under an
account the forge vouches for, and the panel shows all three before an install.

## Order of work

1. **Package distribution**: `plugin install <path | git url>`, `update`, and `remove` around the
   folder loader. Add package version requirements to `.gfi` manifests.
2. **The static scanner** in `goofi-node`, and `library source` against a local folder.
3. **The panel**, inside goofi, against the local half.
4. **The service and the remote half**: accounts, sources, bundles, publish, search. Then the panel
   exports to the website.
5. **The bundles move out**: each `node-bundles/<name>` that is not builtin becomes a folder of a
   registered node repo, its situations and e2e specs go with it, and the bridge's `build.rs`, the
   boot scan and CI keep only the builtin nodes.

## Open

- What the service is written in and where it is hosted. It shares the scanner with goofi, so Rust
  is the default; nothing else is decided.
- Published Rust nodes use the existing engine SDKs and build allowlists through the package
  `nodes/` directory. Verify that the published package contains every required source.
- Which bundles are builtin and stay in this repo, and whether a builtin node is anything but a
  bundle that ships inside the binary. Today every `node-bundles/<name>` is embedded and shipped
  (`goofi-bridge/build.rs:21-27`).
- Whether a first-party bundle is ever privileged over a third party's. Nothing is today.
- A private repo as a source: the login could reach it, but "publishes only from a public repo" is
  what trust rests on.
