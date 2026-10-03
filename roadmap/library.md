# The node library

Find, inspect, filter, curate and install node bundles in goofi. The library includes builtin
nodes, installed external nodes, saved local nodes and nodes available from registered sources.
Updated 2026-10-03. This entry owns node distribution. The plugin interface remains in
`sdk/README.md`; a node bundle does not require a plugin manifest, backend or frontend.

## Decisions

- Every node belongs to a bundle. A bundle is a folder of node sources and the files they need.
  A bundle can contain nodes for different engines; its name is not an engine selection.
- Core bundles stay builtin. All other shipped bundles move to the separate `goofi-nodes` repo,
  whose checkout is `../goofi-nodes` and whose remote is `KairosHive/goofi-nodes` on GitHub.
- Installed external bundles live in `~/.goofi/nodes/<bundle>/`. goofi manages this tree.
  `goofi_supervisor::layout` remains the one owner of these paths and their home override.
  Compiled artifacts and tools remain in the runtime, outside the node source tree.
- `~/.goofi/nodes/_local/` is a reserved bundle managed by goofi. Nodes authored in a patch and
  saved to the library go here. This replaces the flat `~/.goofi/custom/` location. `_local` is
  always available without a repo and cannot be replaced by a repo bundle.
- A bundle source is a GitHub repo with one folder per bundle. Node files at the repo root are
  invalid. Repo-level files such as a README and licence can remain at the root.
- Source registration indexes a repo and offers its bundles and nodes for installation. It does
  not clone the repo, install its nodes or register executable node types in an engine.
  Indexing must not import or execute source code.
- An installed repo bundle records its source repo, commit and folder. Installation uses that
  commit; an update selects a new pin. Keep this rule from the earlier roadmap.
- A plus button on a node or bundle starts installation. goofi fetches the source, installs
  required dependencies, compiles where required and registers the installed nodes automatically.
  The caller must not run a separate clone, build or refresh command.
- Library management works as a pure CLI without an active goofi server. The CLI, frontend,
  MCP and scripts use the same operations and implementation. A running app uses the installed
  result; a later app start discovers nodes installed while no server was active.
- Private GitHub repos are supported. A development build uses the user's installed Git and
  existing authentication. An installed distribution supplies Git and standard Git authentication
  paths. The library must not require a public repo or a library service account.
- Add Git to the development prerequisites with uv, npm and rustup. Provision the distribution's
  Git through the pinned tool manifest and resolve it through `layout::Runtime`, like other tools.
- A hosted catalogue, publishing service and website integration are not prerequisites for this
  work. Registering a repo must be sufficient to use it as a source.

These decisions replace the earlier plugin-package installation unit, the ban on per-node
installation, the public-repo trust rule and the service-first distribution plan. The exact
effect of a node-level install is still open below.

## Current implementation

- There are 13 folders under `node-bundles/`. `backend/goofi-bridge/build.rs` embeds every folder
  and prebuilds every Rust node. `materialise_shipped` in the bridge writes them into the runtime.
  There is no builtin/external split today.
- `AppState::node_roots` scans shipped and extra roots, plugin node folders, then the flat custom
  library. `rescan` adds the patch's `nodes_<engine>` folders last. A later file wins the same
  `engine:Name` type. Root folder names supply bundle labels; they do not provide repo identity.
- `goofi_node::describe` discovers files in a single folder, using `.py`, `.rs` and `.wgsl`.
  `goofi-build` compiles Rust against the embedded engine SDKs and their dependency allowlists.
  Its cache key includes the node file and SDK inputs, but not separate bundle helper files.
  Python discovery loads code. WGSL already has a data header. There is no repo catalogue scanner
  that can describe all three source formats without executing code.
- `library list`, `get`, `save` and `refresh` are bridge operations tied to `AppState` and its
  graph. CLI library commands currently forward to a running server. There are no source,
  install, update, remove or Git authentication operations.
- `layout::custom_nodes`, `AppState::custom`, `library save` and patch archive handling use
  `.goofi/custom`. Saved local nodes travel with a `.gfi`; an equal local copy can become the
  source again on load. Preserve this behavior when the location becomes `_local`.
- Provenance currently uses patch, custom, root and plugin origins. Root nodes are presented as
  builtin. Patch nodes and some engine-owned builtin nodes have no bundle label. The new model
  must assign bundle identity to every node and distinguish external roots from builtin roots.
- `goofi-init` installs requirements from every checked-in bundle. `goofi-provision` supplies
  Python environments and pinned runtime tools, but `Tool` and the tool manifest have no Git.
  Package installs currently run at setup or boot, rather than as a library install transaction.
- The frontend's add menu uses the live node catalogue. App panels register in
  `frontend/src/lib/panels/register.ts` from the bridge's shared panel vocabulary; plugin panels
  use the plugin runtime. There is no library panel or external-source catalogue.
- `../goofi-nodes` currently contains only a README and licence. No bundles have moved.

## Order of work

1. **Storage and bundle identity.** Add the managed nodes root and `_local` to `layout`. Discover
   installed bundle folders and give every node a bundle identity. Update save, inspect, archive,
   source provenance and scan callers together. Remove the old custom path directly; no dual
   scan or compatibility path. Define how patch and plugin node folders count as bundles.
2. **One library owner and offline operations.** Separate library storage and operations from
   the active graph. Reuse the operation schema, parsing, help and dispatch rules from both CLI
   and server entry points. Catalogue operations must not start engines, audio hardware, a native
   window or a server. Builds and probes can use supervised child processes without a server.
   Define the shared lock and notification boundary before supporting concurrent CLI and app use.
3. **Git and authentication.** Add development Git checks, pinned distribution tools, credential
   interaction and the tools required by the supported transports. A library command must prepare
   the tools it needs even when it is the first command run after installation. Verify Linux,
   macOS and Windows; a server startup must not be required to provision library tools.
4. **Sources and indexing.** Persist registered repo sources. Read a repo's bundle folders and
   node descriptions without cloning or execution. Reject root-level nodes. Support private
   sources, refresh, removal, unavailable sources and cached browsing. Show repo and revision.
5. **Installation.** Implement node and bundle installation, update and removal. Reuse the
   existing build and probe paths. Stage work, report progress and errors, then publish a settled
   installed result. A failed install must not damage an existing bundle. Refresh running apps
   automatically, including when a separate CLI process changed the library.
6. **Move external bundles.** Move source, helper files, requirements, assets, licences and
   bundle-specific tests to `../goofi-nodes`. Keep core engine and transport contract tests here;
   replace external dependencies in those tests with controlled fixtures where needed. Restrict
   embedding, setup, boot checks and CI to the builtin set. Establish builds and tests in the
   external repo. Update `plugins.md`, `release-binaries.md` and bundle roadmap entries to match.
7. **Library panel.** Add a panel for source registration, browsing, node and bundle inspection,
   filters, curation and installed state. Use the shared library operations. Show install progress,
   errors and updates, and supply plus buttons for nodes and bundles. Keep library navigation and
   management outside patch undo and dirty state. Support touch, tablet and desktop layouts.
8. **Patch dependencies and verification.** Record external bundle requirements in `.gfi` files.
   Resolve missing bundles explicitly and keep unavailable nodes visible with their bundle named.
   Extend public CLI and app sessions for install, save/load, refresh and failures. Add browser
   sessions for the panel, socket updates, layout and gestures. Test private sources without live
   account secrets and bundled installs with host tools unavailable.

## Open decisions

- **Exact builtin set.** The proposed core set is `signal`, `audio` and `graphics`. This leaves
  ten external bundles: `biotuner`, `complexity`, `computer-vision`, `eeg`, `harmonic-geometry`,
  `image`, `image-generation`, `inception`, `ml` and `simulation`. Confirm this boundary and whether
  any individual nodes need to move between folders first. Engine-owned nodes also need builtin
  bundle labels. An external bundle can contain audio or graphics nodes.
- **Bundle and type identity.** Two repos can use the same folder name, but installed bundles
  share one `nodes/` parent. Choose the installed folder naming rule and stable source-qualified
  bundle ID. Node types currently use `engine:Name`; choose how to handle collisions across
  bundles, including builtin and `_local` collisions. Avoid accidental scan-order selection.
- **Node-level installation.** Does a node's plus button install its whole bundle, or only that
  node and its required files into the bundle folder? If partial installation is supported, define
  dependency closure, installed selections and later bundle update/removal behavior.
- **Index format and private source access.** Choose repo metadata versus static source parsing
  for descriptions, tags, ports, params, compatibility and bundle files. Parsing must cover Rust,
  Python and WGSL and report fields it cannot determine. Choose the remote index reader: GitHub's
  [tree API](https://docs.github.com/en/rest/git/trees) can list files without cloning, but private
  access requires API credentials. An SSH-authenticated Git alone does not settle that API path.
  Keep registration free of clone and execution; define refresh and offline cache behavior.
- **Git authentication in distributions.** Select the supported HTTPS credential helper/browser
  flow and SSH key/agent flow, credential storage, and required helper/SSH tools on each platform.
  [GitHub's credential guide](https://docs.github.com/en/get-started/git-basics/caching-your-github-credentials-in-git)
  describes HTTPS helpers and SSH keys. Define terminal and panel interaction, including private
  indexing, without putting credentials in source records or logs.
- **Checkout layout and revisions.** A repo can supply several bundles, but installed bundles
  must be direct children of `nodes/`. Choose where shared Git checkouts live and how selected
  folders become installed bundles. Define default branch/tag selection, commit pins, updates,
  local edits and whether bundles from one source can use different revisions. Commit pins are
  required, but their storage and update workflow are not implemented.
- **Bundle files and dependencies.** Define how helper modules, assets, repo-level shared files
  and requirements are included, especially for a node-only install. Decide whether the current
  shared Python environments are sufficient for conflicting requirements. Moving bundles does
  not solve missing platform wheels or native dependency tools. Define goofi/SDK compatibility
  and include required helper files in build cache inputs.
- **State, refresh and active use.** Choose source/install state storage, cross-process locking,
  recovery after interruption and app change notification. Define update/removal behavior for
  active nodes and open patches. Local library installation must not require an active session;
  saving an authored node or restarting a live instance still needs its patch/session context.
- **Patch representation.** Decide how all patch-authored and plugin-provided nodes get bundle
  identity, and how `.gfi` represents external requirements versus included `_local` source.
  Choose behavior for missing sources, private sources and a different installed revision.
- **Curation and initial catalogue.** Define whether curation means favourites, hidden bundles,
  source folder selection or another rule, and where these choices persist. Decide whether
  `KairosHive/goofi-nodes` is registered by default and whether any external bundle installs by
  default. No first-party privilege or automatic installation rule is set.
- **External repo checks.** Decide how bundle tests run against the matching goofi SDK/runtime
  and how the two repos verify changes together. Some core tests currently assume the full shipped
  node set; for example, the audio catalogue includes `BioFilter` from `biotuner`.
