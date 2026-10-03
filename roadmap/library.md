# The node library

Find, inspect, filter and install node bundles in goofi. The library includes builtin nodes,
installed external nodes, saved local nodes and nodes available from registered GitHub sources.
Implementation plan agreed 2026-10-03. This entry owns node distribution; the plugin interface
remains in `sdk/README.md`. All implementation stages below are pending. This planning session
changed documentation only; no nodes have moved and no implementation builds have run.

## Decisions

- Only `signal`, `audio` and `graphics` stay builtin. The ten bundles that move to
  `../goofi-nodes` are `biotuner`, `complexity`, `computer-vision`, `eeg`, `harmonic-geometry`,
  `image`, `image-generation`, `inception`, `ml` and `simulation`. Its GitHub repo is
  `KairosHive/goofi-nodes`. Bundle-specific nodes stay with their bundle, regardless of engine.
- Every node belongs to a bundle. A bundle is a folder of node sources and required files; it
  can contain nodes for more than one engine. It does not require a plugin package.
- Node type IDs include the engine, bundle identity and node name, so equal names in different
  bundles can coexist. An external example is `signal:KairosHive/goofi-nodes/eeg/Foo`. Use the
  same structured model for builtin, `_local`, patch and plugin bundles. Update internal callers
  and archives directly; do not add old-ID aliases.
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
  nodes. Index Rust, Python and WGSL by static parsing, without executing code. For a source
  without an installed checkout, use authenticated Git to fetch filtered objects into a
  supervised temporary object store, without a working-tree checkout. Read the file tree and
  required source blobs, then delete the temporary store. Retain only the parsed index with
  its source commit ID and index timestamp. No separate GitHub API login is required.
- Only whole bundles can be installed. A node inspection can offer its containing bundle's
  install action, but there is no node-only selection or node-install operation.
- Bundles can be installed individually from a repo. Use Git's partial clone
  (`--filter=blob:none`) with cone-mode sparse checkout to obtain selected bundle folders,
  repo-root files and explicitly required shared helper folders. GitHub supports this combination:
  [GitHub sparse checkout and partial clones](https://github.blog/open-source/git/bring-your-monorepo-down-to-size-with-sparse-checkout/).
  Keep one Git checkout per repo. Installing another bundle expands that checkout; it does not
  make a second clone or install the repo's other bundles. Git metadata is still repo-wide.
  Store installed bundle IDs once; derive goofi's sparse selection from them and their required
  helper folders. A checked-out helper folder does not itself become an installed bundle.
- A bundle's plus button automatically obtains the repo, installs its Python requirements,
  compiles its Rust nodes and prepares/registers its node types. No separate user clone, build
  or refresh command is required. Installing another bundle reuses the same repo checkout.
- Repo updates apply to the whole checkout. Users can update or edit it directly, or use library
  operations and the panel. Show when the upstream has changes and offer to pull the repo.
  The checkout's current files are the source of truth; remove the earlier immutable-install-pin
  requirement. Git revisions describe state, but do not prevent manual updates or local edits.
- The first bundle install creates the persistent partial clone. Later installs and pulls reuse
  it; a pull fetches changes and populates the selected folders without cloning again or obtaining
  all unrelated file contents. Initial installation can fetch objects already read by a temporary
  index scan because that temporary store was discarded. This is limited repeated transfer, not
  a requirement for a full repo clone. Use existing repo objects when indexing an installed repo.
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
- Rust bundles use ordinary author crates/workspaces plus goofi's generated ABI wrapper, as
  specified below. goofi supplies the SDK binding; Cargo owns author dependency resolution.
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

## Bundle convention

Use ordinary source folders, Python requirements and Cargo files:

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
- Each bundle's Cargo workspace must be usable without the other bundles being checked out.
  Do not put all bundle crates in one repo-root workspace whose members must all be present.
  Required shared helper directories must be identifiable without executing build code so goofi
  can include them in the sparse checkout. Unrelated bundles remain uninstalled.
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

The Git selection uses the standard commands below; these are examples, not actions taken in
this planning session:

```sh
git clone --filter=blob:none --sparse <url> <repo-path>
git -C <repo-path> sparse-checkout set --cone <bundle>
git -C <repo-path> sparse-checkout add <another-bundle>
```

Partial clone defers file-content downloads until needed; sparse checkout selects the working
folders. Sparse checkout alone does not reduce the initial clone's file-content transfer.
Pulls still update the shared repo branch, so all installed bundles in it share one revision.
See [Git clone](https://git-scm.com/docs/git-clone) and
[Git sparse checkout](https://git-scm.com/docs/git-sparse-checkout).

## Source lifecycle

1. **Register/index.** Read the remote tip. If the cached index already describes that commit,
   reuse it. Otherwise fetch the tree and source needed for static parsing into a temporary Git
   object store. Keep the index, commit ID and index timestamp; delete the temporary objects.
   Do not create a working-tree checkout, install dependencies or run node/build code.
2. **First bundle install.** Create the persistent filtered, sparse clone under the repo's home
   path. Select the bundle and required helper folders, then prepare it. Cached catalogue entries
   describe the scanned commit; if the upstream changed, refresh the index for the actual revision.
3. **Another bundle install.** Expand the existing sparse checkout and prepare that whole bundle.
   Do not create another clone. Other bundles remain available without becoming installed.
4. **Update.** Fetch into the existing partial clone and report upstream changes. On the user's
   pull action, update its branch and prepare its installed bundles. Keep the sparse selection.
5. **Manual changes or restart.** Read installed selections and current checkout files, including
   user edits. Index and load/prepare them again as needed; no remote clone is required.

Partial clone supports filtered fetches and downloads missing objects as needed; checkout can
fetch required file contents in a batch. See [Git partial clone](https://git-scm.com/docs/partial-clone).

## Implementation rules

Follow `AGENTS.md` throughout this change:

- Work on `main`. Read the current diff before each stage; the shared checkout contains other
  active work, including files this feature will touch. Preserve that work. Commit only this
  feature's changes at tested checkpoints, with the required model `Co-Authored-By` trailer.
- Add only the owners needed by this feature. One library owner supplies all management paths;
  the manager still owns the live graph and patch. Do not create a separate CLI registry, frontend
  business rules, plugin distribution layer, database or general-purpose package manager.
- Use shared types for repo, bundle and node identity. Parse and validate them once at boundaries.
  Derive display names, source paths and sparse selections from these types; do not duplicate
  string parsing across engines, the bridge and frontend.
- Store registered sources and installed selections once. Git owns branch/revision/local edits.
  The node files own their declarations. The remote index and compiled artifacts are derived
  caches, not independent installation state. Publish events from settled operation results.
- Remove obsolete code in the stage that replaces it. No old-ID aliases, dual custom-node scans,
  duplicate installers, old/new catalogue stores, backfills or retained compatibility formats.
  Development patches and test data can be recreated if their type format changes.
- Share scan, build, probe and failure rules across engines. Preparation happens outside the
  graph lock. Keep the existing supervised resource ownership and shutdown order.
- Use ASD-STE100 Simplified Technical English. Match Rust/frontend style and shared UI tokens;
  do not reformat unrelated files or run Prettier. Keep panel mechanics in `panelty`.
- Verify observable behavior through public library, CLI and app interfaces. Rust tests belong
  in `goofi-tests`. Use controlled Git bundle fixtures, test clocks and hosts; no audio hardware,
  native windows, load simulation, timing assertions or sleeps for synchronization.
- Review each stage against real callers and upstream guards after its checks pass. Fix the cause
  of substantive findings, remove the obsolete paths and review again. Do not suppress warnings.

## State ownership

| State | Owner and storage | Derived consumers |
| --- | --- | --- |
| Registered repo URLs and installed bundle IDs | One library state file in the home, written by the library owner | CLI, app, sparse selection |
| Repo source, branch, commit and local edits | Git checkout under `nodes/<uname>/<repo>/` | Update status, source inspection |
| Authored local node source | `nodes/_local/`, managed through the library | Saved patches and node preparation |
| Patch-authored source and graph | Existing patch workspace and manager | Patch bundle and live node catalogue |
| Remote descriptions, scanned commit and index timestamp | Disposable index cache in the runtime | Available bundle/node browsing |
| Dependency preparation and compiled artifacts | Existing provision/build owners in the runtime | Availability and preparation errors |
| Live instances and their health | Existing engines and manager | Add menu, viewers and runtime status |

Keep one typed operation contract and one phrase registry. Handlers that manage the library need
library context; handlers that save patch source or act on live instances need patch context.
CLI library management must select the library path before server resolution or engine startup.
The app adapter projects settled library results into the existing graph/catalogue path.

## Implementation stages

Complete these stages in order. A stage is done only when its listed result and relevant checks
pass, its obsolete paths are removed, and its tested changes are committed. Keep the stage status
and remaining work in this file current. Do not start implementation in this planning turn.

### Stage 1 — Bundle identity and local storage

Status: pending.

- Introduce typed repo, bundle and node identities. Use `<uname>/<repo>/<bundle>` for external
  bundles and include bundle identity in runtime node type IDs. Give builtin, `_local`, patch
  and plugin nodes explicit bundle identities under the same model.
- Add the managed nodes root and `_local` through `goofi_supervisor::layout`. Replace
  `custom_nodes`, `AppState::custom` and their scan/save/inspect/archive callers together.
- Update engine registration, graph resolution, schemas, generated frontend types, node editing,
  copy/paste, undo and `.gfi` serialization where they consume node type IDs. Keep file stems as
  declaration names; do not confuse a node's full ID with its source file name.
- Remove last-root-wins selection between different bundles. Equal engine/name pairs can coexist
  in separate bundles; a duplicate declaration within one bundle must report an error.
- Normalize scan roots to explicit bundle identity. Remove duplicated folder-name/provenance
  guessing; do not leave engine callers to derive their own identities.
- Delete existing test cases that depend on the ten external product bundles when first affected.
  Do not rewrite those cases for the new type IDs only to delete them at the bundle move stage.

Checkpoint: two bundles with equal node names resolve independently. `_local` save, rename,
inspect and `.gfi` save/load use the new path; builtin and patch nodes retain correct provenance.
Existing instances, expressions, undo and source editing use the same type model. Extend the
owning node/contract/patch sessions, and check frontend consumers changed by the new schema.

### Stage 2 — Standalone library owner and shared operations

Status: pending.

- Add the smallest coherent library module/crate for persistent source registration, installed
  selections, catalogue access and management effects. It must be usable without a live graph.
- Use one home state file for sources/selections, with typed validation, serialized writes and
  whole-file replacement. Use the same state lock from app and CLI processes. Keep index/build
  caches in the runtime; do not introduce a database or migration chain.
- Reuse the existing typed argument schemas, phrase parsing, help, completion, operation list and
  dispatch rules. Refactor their context boundary only as needed; do not make a second op table.
- Route standalone library commands before the CLI resolves a server or starts a window/engine.
  Create a short-lived supervisor session only when commands need children or scratch resources.
- Declare operations as their handlers become usable: source list/add/remove/refresh, bundle
  list/get/install/remove, repo check/pull, node list/get, and installed-library refresh. Keep
  `library save` tied to its required patch context. Settle exact phrases in the shared registry.
- Remove bridge-only management implementations as shared handlers replace them. Retain the
  manager adapter for live catalogue projection; it must not own a second persistent library.

Checkpoint: public CLI library reads and implemented effects work with no server, without opening
engines or windows. CLI and app resolve the same operation schema and state. Extend the CLI and
operation contract sessions; verify resource release through session status where resources exist.

### Stage 3 — Static catalogue scanner

Status: pending.

- Add one scanner for repo trees and local bundle sources. Parse Python declarations/docstrings,
  Rust source and Cargo metadata, and WGSL data headers. Reuse existing declaration validation.
- Detect top-level bundle folders, reject node declarations at a repo root and distinguish helper
  directories from bundles. Read metadata statically; never run Python, `build.rs` or node code.
- Follow declared node source paths and statically identifiable helper files. Read only the files
  needed for the catalogue, not model weights or unrelated binary assets.
- Represent computed declarations as unknown fields until local preparation obtains them. Do not
  invent empty ports, params or tags as if they were known. An incomplete index can still offer
  installation; malformed declarations report their real source/bundle/file boundary.
- Keep one description schema for builtin, installed and available nodes. Runtime preparation can
  supply known availability/details; uninstalled index entries do not enter an engine registry.

Checkpoint: a controlled mixed-language repo indexes without executing its deliberately failing
import/build code. Root nodes are rejected, helpers do not appear as bundles, partial declarations
stay inspectable and equal node names preserve their bundle IDs. Test through public library APIs
in `goofi-tests`; retain real local probes for preparation rather than a second execution scanner.

### Stage 4 — Cargo and shared Python preparation

Status: pending.

- Support the accepted bundle-local Cargo package/workspace convention and per-node author
  crates. Define the small Cargo identity/source metadata schema. Authors use ordinary dependency
  tables, features and helper crates; goofi supplies runtime SDK bindings and generated ABI wrappers.
- Keep one build owner for standalone builtin/patch/`_local` source and external author crates.
  Preserve SDK version/hash validation and the existing dynamic-library loading boundary.
- Permit Cargo to resolve dependencies outside the builtin vendor set. Do not apply the current
  blanket offline/source replacement to external dependencies. Keep artifacts out of checkouts.
- Make build validity account for Cargo manifests, lock resolution, node/helper source and SDK
  inputs. A helper edit or dependency change must rebuild the affected node; unchanged input
  must reuse a valid artifact. Remove the source-file-only assumption for author crates.
- Resolve installed bundles' `requirements.txt` and `requirements-gil.txt` with uv into the
  existing shared environments. Resolve relative paths from the correct bundle context. Report
  conflicts and failures; do not add bundle venvs or manually maintained dependency counters.
- Share preparation results and diagnostics between install, refresh and boot. Keep unusable
  nodes visible and keep compilation/probes outside the graph lock.

Checkpoint: one controlled Rust bundle with an external dependency and a path helper builds and
loads through the public interface; changing the helper changes observable output. A Python bundle
prepares in the shared environments. Dependency/build failures remain inspectable. Extend hosted,
node and preparation sessions using fixture nodes, not the product bundles that will move out.
Use controlled dependency sources for these checks; do not require live registry access in tests.

### Stage 5 — Authenticated Git indexing and sparse installation

Status: pending.

- Add Git resolution/checks through `layout::Tool` for the local version and update development
  prerequisites. Use the user's installed authenticated Git. Do not build a GitHub login flow.
- Implement source registration/refresh using temporary filtered Git object stores, with no
  working checkout. Keep only index data, commit ID and index timestamp after cleanup. Reuse
  an unchanged committed index; use existing repo objects when a persistent checkout exists.
- Implement first installation as a persistent filtered clone with cone-mode sparse checkout at
  `nodes/<uname>/<repo>`. Subsequent installs expand that checkout. Include required helper
  folders derived from static dependency metadata; do not load helpers as installed bundles.
- Keep the installed bundle selection as the one installation answer. Publish it after the
  checkout succeeds. If preparation fails, retain visible source and error state rather than
  claiming that all nodes are usable or silently omitting them.
- Implement repo check/pull using the existing clone. Show upstream/local status, use explicit
  fast-forward-only pulls and prepare the installed bundles together after the repo changes.
- Implement bundle removal separately from source unregistration and explicit repo deletion.
  Preserve user edits, downloaded Git objects and shared Python packages. Contract sparse paths
  only where doing so preserves user work.
- Supervise Git, uv, Cargo, probes and temporary paths. Report real failures and leave a settled,
  recoverable state after interruption. Do not make resource decisions from intermediate writes.

Checkpoint: a public CLI session registers a fixture source without an installed checkout, installs
one of two bundles, expands the same clone for the other, detects/pulls a fixture commit and removes
one selection. Verify installed types, remaining bundle files, retained index metadata, errors and
resource cleanup. Use a controlled Git remote with filtering enabled; no live credentials or
GitHub account is required for tests. Existing Git authentication remains the transport's concern.

### Stage 6 — App, boot and patch integration

Status: pending.

- Discover only selected installed repo bundles plus builtin, `_local`, patch and plugin bundles.
  Use the library owner and current files as the source of the scan; do not mirror install state
  in `AppState`, engines or frontend stores.
- Remove the superseded `--extra-nodes` route and its configuration when shared bundle loading
  is available. Do not retain a second unmanaged external-node path beside library operations.
- On boot, index/load/prepare changed installed bundles, including manual repo edits. Share the
  same preparation path with library operations. Do not require remote access to load unchanged,
  already prepared installed nodes.
- After app library effects settle, project the resulting catalogue once and broadcast through
  the existing node-type event path. Keep effects out of patch history and dirty-state changes.
- Permit effects while nodes are active. Use existing refresh/restart behavior where it applies;
  a goofi restart is accepted recovery for unreplaced code/shared dependencies. Do not add a
  watcher, hot-swap framework or active-patch operation lockout.
- Record external bundle/type requirements and observed revision in `.gfi`. Load the current
  installed checkout, keep missing nodes visible with their bundle named, and never silently
  install or pull on patch load. Keep `_local` and patch-authored source in archives as before.
- Remove duplicate custom/root/provenance assumptions and old graph-dependent library state
  paths when their shared replacements are in use.

Checkpoint: app effects update its catalogue; a CLI effect followed by refresh/restart is loaded;
manual source changes take effect after preparation. Verify `_local` portability, missing external
bundle visibility, current-checkout loading and clean patch state through public sessions. Extend
relevant socket sessions when the catalogue payload or event behavior changes.

### Stage 7 — Move the ten external bundles and delete their core tests

Status: pending.

- Inspect both checkouts and preserve other work. Move the ten decided bundle folders into
  `../goofi-nodes`, including helpers, requirements, assets, documentation and licences.
  Convert their Rust nodes to the agreed Cargo convention. Do not change the website.
- Embed and prebuild only `signal`, `audio` and `graphics`. Restrict development setup, shipped
  requirements, boot checks, CI caches and release smoke tests to that builtin set. Remove
  obsolete in-repo references and assumptions rather than adding source-path fallbacks.
- Delete every existing goofi test case that depends on the moved product nodes, including
  browser cases, that was not already removed in an earlier stage. Preserve unrelated work in
  mixed test files. Do not keep those cases alive
  with automatic external installs, copied product code or replacement fixtures.
- Keep new library contract tests based on their small controlled repos; these test the library,
  not the removed external product behavior. No external product test project or cross-repo
  CI service is required in this feature.
- Register `KairosHive/goofi-nodes` as the default available source. Install no external bundle
  automatically. Verify that the external checkout fits the scanner/build convention before
  considering the move complete. Local verification can use the checkout before it is published.
- Update bundle roadmap references, setup instructions and plugin/library documentation. Keep
  bundled Git/authentication work in `release-binaries.md` and website instructions deferred.

Checkpoint: a fresh goofi contains only builtin nodes and runs without the moved bundles' packages.
The separate repo indexes correctly; individual bundle installs use the standard library path.
Run the remaining core suite and the focused library sessions. Commit tested changes in each repo
without including unrelated files. Publication or a push is a separate action, not required here.

### Stage 8 — Frontend library panel

Status: pending.

- Register a library app panel through the existing shared panel vocabulary. Keep `panelty` in
  charge of panel layout/mechanics and use existing UI primitives and root tokens.
- Show builtin, `_local`, installed and available bundle/node records with inspection and useful
  search/filter controls. Display external bundles as `<uname>/<repo>/<bundle>`.
- Add source register/unregister, whole-bundle plus/remove, repo update check/pull and preparation
  status/errors. A node detail offers its bundle's install action. No node-only installation,
  favourites, hiding, publishing service or second dependency management UI.
- Use shared operation results for installed/update/availability state. Local filter text and
  selection are presentation state; do not copy library rules into stores or components.
- Keep available uninstalled nodes out of the runtime add menu until installed/prepared. Keep
  library navigation and management out of patch undo/dirty state. Preserve cable dragging and
  workspace layout, and use container queries for panel sizing.

Checkpoint: typecheck and frontend tests pass. A relevant Playwright session browses/filters a
fixture source, inspects nodes, installs a bundle, sees it in the add menu, observes preparation
failure/update state and removes it. Verify touch/desktop layouts and socket catalogue updates
through existing layout/gesture sessions. Do not test throughput or add timing thresholds.

### Stage 9 — Final audit and completion

Status: pending.

- Audit from real CLI/app/MCP callers through the common operation path. Check ownership,
  batching, source selection, graph projection, errors, resource release and patch behavior.
- Search for old custom paths, flat unmanaged roots, bare-name collision selection, duplicate
  op declarations, external product test dependencies and shipped external requirements. Delete
  obsolete code/schemas/configuration and ensure each shared rule has one owner.
- Run final workspace and frontend checks, plus the relevant browser suite. Fix warnings and
  substantive failures, then review the fixes again. Report failed/skipped checks explicitly.
- Verify ordinary authenticated Git use locally. Do not implement deferred distribution login,
  hosted services, website author guides or external product test infrastructure.
- Update documentation and stage status. Remove this roadmap entry only when the local feature
  is complete; leave deferred release work in the release roadmap. Commit the final tested state.

Final required checks from `AGENTS.md`:

```sh
cargo build --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
npm --prefix frontend run check
npm --prefix frontend run test
```

Run the relevant Playwright sessions from `tests/e2e` with the real backend. During development,
run checks scoped to the stage and affected public sessions; run the full set at completion.
Do not repeat broad checks after they pass unless a change or failure gives a reason.

## Accepted behavior defaults

- Static fields that require evaluation stay unknown until a local probe supplies them.
- Check upstream on explicit refresh or panel entry. Pull only on user action, with fast-forward
  behavior. Leave conflicts/divergence to manual Git; no automatic stash, reset or merge.
- Uninstall removes the selection and reduces sparse paths when safe. Keep the shared checkout,
  Git objects and Python packages. Source unregistration does not uninstall its bundles.
- The first-party repo is available by default; no external bundles install by default.
- Patch load uses current installed source, reports missing bundles and does not automatically
  install/pull. `_local` and patch-authored source remain portable inside the archive.

## Continuation after context compaction

- Read this file and `AGENTS.md`, then inspect the current diffs in goofi and `../goofi-nodes`.
- All stages are pending. The approved architecture is the decisions/convention/lifecycle above;
  no additional product approval is needed to implement it when the user resumes the build.
- This turn creates the staged plan only. Do not interpret the pending statuses as permission
  to start implementation before that resume instruction.
- Work through the stage checkpoints on `main`, preserve shared changes, and commit tested work
  with the required model trailer. Resolve routine implementation choices from the agreed design;
  ask only if evidence requires a material change to the product contract.
