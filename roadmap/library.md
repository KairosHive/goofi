# The library: plugin distribution

One place to find, install, publish and update bundles of nodes and panel add-ons. The model is the
VCV Rack library: an author hosts a git repo; the library registers it, detects what is in it, and
lets the author group the result into bundles anyone can install.

## Order of work

1. **Package distribution**: `plugin install <path | git url>`, `update`, and `remove` around the
   folder loader (`plugin list` exists). Add package version requirements to `.gfi` manifests: a
   load on a machine without a bundle offers the install and otherwise leaves the node UNAVAILABLE
   with the bundle named, never a silent absence.
2. **The static scanner** in `goofi-node`, beside `scan_nd_calls`: parse a repo for `goofi.Node`
   subclasses and their declared `INPUTS`, `OUTPUTS`, `PARAMS` and docstring without importing.
   Then `library source` against a local folder.
3. **The panel**, inside goofi, against the local half, registered through `registerPanel`.
4. **The service and the remote half**: accounts, sources, bundles, publish, search. Then the panel
   exports to `../goofi-website/`.
5. **The bundles move out**: each `node-bundles/<name>` that is not builtin becomes a folder of a
   registered node repo, its situations and e2e specs go with it, and the bridge's `build.rs`, the
   boot scan and CI keep only the builtin nodes.

## Decisions the steps depend on

- The installation unit is a plugin package (`sdk/README.md`); `nodes/` is an ordinary flat bundle.
  No per-node install, no second installed-bundle format.
- A source is a git repo; each folder holding a goofi-parsable node is one bundle. The author's
  one control is to deselect folders. A published bundle names repo, commit and folder; an install
  fetches exactly that; an update is a new pin.
- The service never imports a stranger's code: one static scan on the service, the real probe at
  install on the user's machine.
- Everything is `library` ops; the service's HTTP API is the same phrases, so the panel does not
  know whether goofi forwards or the website sends directly. An agent publishes with the CLI a
  human uses.
- Login is OAuth against the forge that hosts the repo; the library holds no passwords.
- Trust is provenance, not a sandbox: public repo, pinned commit, forge-vouched account, all three
  shown before an install.

## Open

- Precedence is per type name, last root wins. Decide whether an installed bundle shadows a shipped
  one as a WHOLE bundle before step 1 ships.
- What the service is written in and where it is hosted. It shares the scanner, so Rust is the
  default; nothing else is decided.
- Published Rust nodes use the engine SDKs and build allowlists through `nodes/`. Verify that a
  published package contains every required source.
- Which bundles are builtin and stay in this repo, and whether a builtin node is anything but a
  bundle that ships inside the binary. Today every `node-bundles/<name>` is embedded.
- Whether a first-party bundle is ever privileged over a third party's. Nothing is today.
- A private repo as a source: the login could reach it, but trust rests on "public repo".
