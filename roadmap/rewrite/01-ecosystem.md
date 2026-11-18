# 01. The ecosystem

## 1.1 Programs

goofi is an ecosystem of programs for biosignals, audio, graphics and data: a meeting point and a
marketplace for autopoietic research and arts.

| Program | Short | Function |
| --- | --- | --- |
| goofi-pipe | `gp` | Real-time patches. A person builds a patch of nodes in a browser. The graph that is edited is the graph that runs. It records. |
| goofi-data | `gd` | Datasets. It keeps, labels, derives, records and shares what was recorded. |
| goofi-build | — | Models. It makes models from goofi-data datasets. Later; part 11. |

- "goofi" names only what is shared: the repository, the home, the account, the goofi layer and
  the shared UI library.
- Each program serves its own web app from its own binary and is its own CLI.
- Each program is also a library: another program or a script uses it natively.
- No program depends on another program. Programs meet through the goofi layer, the home, the
  transport and the account.

## 1.2 Layers

- **The goofi layer** holds what does not know an application: time, paths and tools, the
  session, the build of components, logging, configuration, payload types and their metadata,
  the codec, the transport, the recorder, the dataset file (its format, its reader and its
  writer), the graph and the document, the node contract and the SDKs, the node library, the three engines, the op framework, the agent harness, the account
  client, and the shared UI library.
- **goofi-pipe** holds the live runtime mode, its server and ops, the patch editor and its
  panels.
- **goofi-data** holds the Kahn runtime mode, the management of datasets (locations, adapters,
  exchange, search), the scheduler, the studio, its server and ops, and its dashboard.
- The dataset file is in the goofi layer because three users need it and no program depends on
  another: the dataset recorder type (part 02.9.4), the loader and writer nodes (part 04.9)
  and goofi-build (part 11).
- **The layer rule:** the goofi layer depends on no application; an application depends on the
  goofi layer and on no other application. A test asserts the rule from the dependency graph.

## 1.3 Repository

- One repository, one Rust workspace, one lock file, one version. The repository is the unit of
  atomic change.
- All Rust crates lie in one flat folder. A prefix names the layer: `goofi-` for the goofi layer,
  `gp-` for goofi-pipe, `gd-` for goofi-data.
- One npm workspace: the shared UI library, and one frontend per program.

  ```
  crates/      goofi-*  gp-*  gd-*
  web/ui/      the shared UI library
  web/pipe/    the goofi-pipe frontend
  web/data/    the goofi-data frontend
  nodes/       the shipped node bundles
  plugins/     the shipped plugins, as source
  skills/      the agent guides
  examples/    the example patches and recipes
  roadmap/     one file per open feature
  ```

- Each program has its own tests crate, so the CI job of goofi-data does not build what only
  goofi-pipe needs.
- The panel mechanics (panelty) and the plotting (plotluck) are part of the shared UI library.
  Their separate names do not persist.

## 1.4 Running

- The ecosystem is development-only until its first release. It builds with `cargo`, `npm` and
  `uv` from PATH and ships no installer.
- Two commands start a clone: one that provisions everything `cargo` cannot provide (the Python
  interpreters, the Python package, the frontend dependencies, the build configuration), and
  `cargo run`. The provisioning command is a program of the workspace, so it is the same command
  in every shell. It is idempotent. It checks the tools and system libraries a build needs and
  names each missing one.
- The web app of a program is compiled into its binary. A build with a stale or missing app
  fails; a binary without its app refuses to start, unless it was built headless.
- A program prints its URL and does not open a browser.
- Platforms: Linux, macOS and Windows for the backend. Any current browser for the app, on
  touch, tablet and desktop, in both orientations.
- Python is part of goofi: a node can be written in it, and an expression is Python. Two
  interpreters live in the runtime directory: a free-threaded one and one with the GIL. goofi
  uses no other interpreter.

## 1.5 The home, the runtime and the session

- **The home** (`~/.goofi`) holds what the user keeps: `config.toml`, recordings, datasets (the
  default data directory), plugins and their data, the private node library, the downloaded node
  bundles, and the credentials of the account. Every program reads the same home.
- **The runtime** holds what one version needs to run: the interpreters and the build caches. It
  is disposable. Build artifacts that were not touched for three days are removed.
- **The session** holds what one running program owns and that ends with it: children, threads,
  scratch paths, devices, agent instances. A lock is the one answer to "is it alive".
- One module owns every path and every tool lookup. Nothing else reads a path or a tool from the
  environment.

## 1.6 Trust

- A program targets one user on localhost or a trusted LAN. WebSockets have no authentication.
- An origin guard (a Sec-Fetch allowlist and a Host check) protects every route.
- To bind beyond the machine prints a warning: there is no authentication and a terminal route
  is a real shell.
- A demo mode withholds every door a public instance cannot offer (part 08.1). It is not a
  sandbox.
- A plugin is trusted local code. It runs with the permissions of the user.

## 1.7 The account and the online library

- One goofi account serves the whole ecosystem.
- The account gives access to: the online node library, the remote data repository, shared
  datasets between people, and remote storage that needs a login (Drive, Cloudflare and others).
- The credentials live in the home. Ops log in, log out and show the status.
- The online node library is in the spirit of VCV Rack and ComfyUI: a person uploads a node
  bundle under the account; another person browses, downloads and uses it. Part 04.8 specifies
  it.

## 1.8 Documentation site

- The site holds the install guide, the authoring guide, the op vocabulary and every shipped node
  with its params and its source.
- The op pages and the node pages are generated from the op table and the node manifests. Nothing
  on them is written twice.

## 1.9 Licence

- AGPL-3.0-only. A commercial licence is available on request.

## 1.10 Open decisions

1. Two binaries (`goofi-pipe`, `goofi-data`) are assumed. The names of the commands a person
   types (`goofi-pipe` and `goofi-data`, or `gp` and `gd`, or one `goofi` with a first word) are
   not decided.
2. The word for a standalone program of the ecosystem, as against a plugin.
3. The account service and its credential model. The hosting service is not chosen.
4. The licence of panelty and plotluck against AGPL-3.0-only is not checked.
5. Whether the Python package `goofi` stays one package or splits into the transport bindings
   and the node API.
6. Where the new tree lives while the present program still runs (part 13).
