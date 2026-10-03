# The node library

Find, inspect, filter and install node bundles in goofi. The library includes builtin nodes,
installed external nodes, saved local nodes and nodes available from registered GitHub sources.
Updated 2026-10-03 after the second planning round. This entry owns node distribution; the plugin
interface remains in `sdk/README.md`. Implementation and bundle moves have not started.

## Decisions

- Only `signal`, `audio` and `graphics` stay builtin. The ten bundles that move to
  `../goofi-nodes` are `biotuner`, `complexity`, `computer-vision`, `eeg`, `harmonic-geometry`,
  `image`, `image-generation`, `inception`, `ml` and `simulation`. Its GitHub repo is
  `KairosHive/goofi-nodes`. Bundle-specific nodes stay with their bundle, regardless of engine.
- Every node belongs to a bundle. A bundle is a folder of node sources and required files; it
  can contain nodes for more than one engine. It does not require a plugin package.
- External repo checkouts live in `~/.goofi/nodes/<uname>/<repo>/`, with one folder per bundle.
  The installed bundle path and displayed identity are `<uname>/<repo>/<bundle>`. Node files
  at a repo root are invalid; repo-level README, licence, Cargo files and helper files are allowed.
- `~/.goofi/nodes/_local/` is the reserved local bundle, outside the repo hierarchy. goofi saves
  patch-authored nodes here. Replace `.goofi/custom/` directly, without a compatibility path.
  No repo can replace `_local`.
- `goofi_supervisor::layout` owns node paths and their home override. Sources and checkouts live
  in the home; compiled artifacts, index caches and tools live in the runtime. Ephemeral build
  and scan resources remain supervised.
- A source registration indexes a repo without creating an installed checkout or loading its
  nodes. Index Rust, Python and WGSL by static parsing, without executing code. The exact Git
  object-fetch boundary for private indexing remains open below.
- Only whole bundles can be installed. A node inspection can offer its containing bundle's
  install action, but there is no node-only selection or node-install operation.
- A bundle's plus button automatically obtains the repo, installs its Python requirements,
  compiles its Rust nodes and prepares/registers its node types. No separate user clone, build
  or refresh command is required. Installing another bundle reuses the same repo checkout.
- Repo updates apply to the whole checkout. Users can update or edit it directly, or use library
  operations and the panel. Show when the upstream has changes and offer to pull the repo.
  The checkout's current files are the source of truth; remove the earlier immutable-install-pin
  requirement. Git revisions describe state, but do not prevent manual updates or local edits.
- All library management operations work without an active goofi server. CLI, frontend, MCP and
  scripts share one operation vocabulary and implementation. A later app start indexes and
  loads/prepares modified installed bundles.
- This session targets the local, self-compiled version. Assume Git is installed and authenticated,
  including access to private repos. Add Git to development prerequisites beside uv, npm and
  rustup. Bundled Git, helper tools and distribution authentication belong to
  `release-binaries.md` and are outside this session's implementation scope.
- All nodes use the existing shared Python environments. Resolve external requirements through
  uv into those environments; do not create bundle-specific environments. External Rust bundles
  supply Cargo files for their dependencies, which Cargo resolves during compilation.
- Library install, update and removal are allowed during active use. Do not block them because
  a patch uses a bundle. Live refresh is useful, but restarting goofi is an accepted recovery
  path if changed code or dependencies break an active session.
- Keep the library functional: source registration, browsing, inspection, filtering, install,
  removal, update checks, pulls and preparation status. No favourites or hiding features.
- Delete all existing goofi test cases that depend on nodes moved to the external repo. Do not
  preserve those cases by installing the external bundles or replacing their nodes with fixtures.
  New library behavior tests can use small controlled bundle repos instead of product bundles.
- Website author instructions will go in `../goofi-website` later. This session defines the
  convention but does not change the website. A hosted library service is not required.

## Current implementation and required changes

- `backend/goofi-bridge/build.rs` embeds all 13 bundle folders and prebuilds their Rust files.
  `materialise_shipped`, development setup and boot requirements treat all of them as shipped.
  Restrict these paths and CI to the three builtin bundles.
- `AppState::node_roots` scans shipped/extra roots, plugin node folders and the custom folder;
  `rescan` adds patch `nodes_<engine>` folders last. A later file wins `engine:Name`. Root folder
  names supply labels and all roots appear builtin. Nested repo discovery and full bundle
  identity are missing. Folder identity alone does not solve runtime node type collisions.
- `goofi_node::describe` scans one flat folder for `.py`, `.rs` and `.wgsl`. Python discovery
  executes code; WGSL has a data header. Add static source parsing for catalogue indexing and
  retain real local probes for preparation. Unknown static fields must be reported as unknown.
- `goofi-build` generates one crate per Rust file using fixed SDK dependency lists. Its cache key
  does not include separate helper files. When the runtime has vendored crates, it replaces the
  whole crates.io source and forces offline builds. External Cargo dependencies need a build
  path that keeps SDK binding but permits dependency resolution and tracks all build inputs.
- `library list/get/save/refresh` currently depend on `AppState` and the graph; the CLI forwards
  them to a server. Separate persistent library management from live graph projection. Saving
  a node from an active patch still requires that patch's context.
- `layout::custom_nodes`, save, source inspection and `.gfi` packing/adoption use `.goofi/custom`.
  Move them together to `_local`. Retain saved-local-node inclusion in patch archives.
- There is no source registry, static remote index, installed bundle selection, repo update
  status, standalone installer or library panel. Some engine-owned builtin and patch nodes have
  no bundle identity. Include these in the new type/provenance model.
- Panels register in `frontend/src/lib/panels/register.ts` from the bridge's shared vocabulary.
  The add menu receives the live catalogue. Add the library panel through the app panel system
  and use the same operations for all controls.
- `../goofi-nodes` currently contains only a README and licence. Test dependencies include
  cross-engine cases: the audio catalogue expects `BioFilter` from `biotuner`.

## Proposed bundle convention

This is the concrete proposal for review, not a second package system:

```text
<uname>/<repo>/                 # Git checkout, one upstream/revision for its bundles
    README.md
    LICENSE
    <bundle>/
        README.md              # optional bundle description
        example.py             # Python node source
        Example.wgsl           # shader source with its existing data header
        requirements.txt       # optional, both shared Python environments
        requirements-gil.txt   # optional additions for the subprocess environment
        Cargo.toml             # if Rust is present: ordinary package/workspace
        Cargo.lock             # dependency resolution for this bundle
        rust/
            <node>/
                Cargo.toml     # ordinary library crate with node identity metadata
                src/lib.rs     # SDK export declaration and node code
            <helper>/          # optional shared library crate
        assets/                # optional assets and helper files
```

- Keep Python and WGSL node sources at bundle level, as now. Helper directories are not bundles.
  Rust node crates belong to the containing bundle and declare engine/source identity through
  Cargo metadata. Static parsing follows that declaration and reads source; it never runs
  `build.rs`, imports Python or expands executable code to index a source.
- Use ordinary Cargo dependency tables, features, path dependencies and workspaces. Shared repo
  helpers can be outside a bundle if referenced explicitly. Do not translate dependencies into
  another goofi-specific dependency list. Cargo has standard workspace and metadata facilities:
  [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html).
- goofi binds engine SDK dependencies to the SDK sources of the running version and generates
  the small cdylib wrapper around each author crate's export. Authors need no absolute path to
  a goofi checkout and do not maintain SDK hash or loader symbols. Builtin and `_local` standalone
  Rust files continue through the same build owner with their existing generated manifests.
- Put outputs in the runtime. Keep author Cargo sources intact. Resolve external crates online
  when needed; the builtin SDK vendor cache must not block dependencies it does not contain.
  Include Cargo files, lock resolution, source/helpers and SDK identity in build validity.
- Run uv with the bundle as the requirements context so local paths resolve there. Install the
  installed set's requirements into the shared environments. Report conflicts as preparation
  failures; no extra environments or dependency isolation are added.
- Preparation errors remain visible per bundle/node. Whole-bundle installation does not make
  an unbuildable node disappear from the library. Read the checkout again on app start or an
  explicit refresh, including manual edits and changes to dependency files.

## Order of work

1. Settle the open behavior below and the Cargo convention. No implementation before this plan
   is agreed. Distribution provisioning and website work remain deferred.
2. Add repo hierarchy discovery, `_local` and structured bundle identity to the layout/scan/save/
   inspect/archive callers. Define builtin, patch and plugin bundle identities with the same model.
3. Add a persistent library owner and shared operations usable directly from the CLI without
   booting a server, engines, audio hardware or windows. Hold a short-lived supervisor session
   for builds/probes. Serialize goofi's library writes; manual Git edits remain user-controlled.
4. Add source registration, static indexing and refresh using installed authenticated Git. Source
   registration must not install a checkout. Preserve cached catalogue inspection when offline.
5. Add whole-bundle install/remove, repo update check/pull and preparation. Keep one checkout per
   repo and a settled installed state. Reuse uv, Cargo, probe, resource and error handling owners.
   After an app operation, refresh its catalogue where possible; explicit refresh/restart covers
   external CLI changes or code that cannot be replaced live. No watcher is required initially.
6. Move the ten external bundle folders with requirements, helpers, assets and licences into
   `../goofi-nodes`. Delete dependent existing goofi test cases. Keep the remaining checks focused
   on builtin nodes and core behavior. Remove obsolete boot/setup/CI dependencies. Define external
   test tooling separately; it is not required to retain the deleted goofi tests in this session.
7. Add the library panel with source registration, bundle/node details, filters, plus/remove
   actions, repo update status/pull and preparation errors. No favourites or hiding. Keep patch
   dirty state and undo independent of library navigation/management; support touch and tablets.
8. Verify standalone CLI and app operations using controlled repo fixtures, shared dependency
   preparation, `_local` save/load and restart recovery. Add relevant browser sessions for panel
   behavior and socket updates. Do not build or run these checks during this planning round.

## Second-round questions and proposed defaults

1. **Clone versus installed selection.** A clone contains every bundle folder. Recommended:
   only bundles explicitly installed with plus are prepared and loaded; the others are available
   in the catalogue. Store selected bundle IDs once. Removing one bundle keeps the checkout for
   its siblings; source removal unregisters the source and is separate from uninstall.
2. **Runtime type identity.** Recommended: include bundle identity in the node type ID so two
   bundles can contain the same engine/name. For example, `signal:KairosHive/goofi-nodes/eeg/Foo`.
   Folder identity alone cannot provide this. Define short builtin IDs and `_local`, patch and
   plugin IDs consistently; remove old internal spellings directly, without aliases.
3. **Private static indexing.** Existing Git authentication can fetch repo objects, but cannot
   supply a remote file tree through `ls-remote`. Recommended: fetch into a supervised temporary
   object store, statically read trees/source blobs, then retain only the index cache. No installed
   checkout is created. [Git fetch](https://git-scm.com/docs/git-fetch.html) supports shallow and
   filtered object transfers. If registration must also forbid object fetches, private indexing
   needs a separate GitHub API authentication path, outside the stated Git-only assumption.
4. **Rust author layout.** Confirm ordinary author crates/workspaces plus goofi's generated ABI
   wrapper, as proposed above, rather than flattening an author's Cargo dependencies into every
   generated single-file node crate. Define exact metadata keys and SDK binding during design.
5. **Static parser limits.** Recommended: index declarations that can be read without evaluation;
   mark computed ports/params/tags unknown until a local probe succeeds. Installation remains
   possible. Do not demand an additional authored catalogue manifest for arbitrary code.
6. **Repo pull behavior.** Recommended: track the repo's current branch/upstream, check on explicit
   refresh or panel entry, and pull only on user action. Use fast-forward-only pulls; report local
   conflicts/divergence and leave them for manual Git resolution. No automatic reset, stash,
   merge or background pull. A pull prepares all installed bundles in that repo. Record actual
   revision and local modification status for inspection, without an independent version counter.
7. **Removal and defaults.** Recommended: uninstall stops future loading of that bundle; keep the
   user-editable checkout and shared Python packages. Offer explicit repo deletion separately.
   Register `KairosHive/goofi-nodes` by default, but install no external bundle automatically.
8. **Patch source semantics.** Recommended: keep `_local`/patch-authored source inside `.gfi` as
   today; external nodes name their bundle/type and show missing dependencies on load. Describe
   the revision used but load the current installed checkout. Do not silently pull or install on
   patch load. Assign a patch-owned bundle identity without moving its workspace into the home.
