# Plugin system

The initial folder interface is implemented in `sdk/README.md`. Plugins use a Python backend,
compiled frontend assets, and an ordinary `nodes/` bundle with Rust or Python sources. All calls
use goofi's operation vocabulary. The SDK exposes `op`, `pre_op`, `post_op`, `on_start`, `on_stop`,
`register_panel`, `register_header`, and runtime `ctx.call` / `ctx.log`. Header registration returns
an update/disposal handle. No generic state/event bus, panel-open API, task manager, or resource
provider is planned for this version.

Remaining product work:

- Build the session/corpus plugin: people, corpora, acquisition sessions, tags, and catalogue.
- Choose the remote database/file service and its credential model.
- Add a persistent upload queue, restart reconciliation, and local download cache in that plugin.
- Supply its playback node and configure it through ordinary goofi operations.
- Verify source builds and lifecycle behavior on Windows and macOS as well as Linux.

Installation is folder discovery at startup. Live replacement, a marketplace, dependency relations
between plugins, and durable lifecycle subscriptions are outside the initial interface. The
library roadmap owns distribution; it must use plugin packages as the installation unit and
`nodes/` as their node contribution, rather than introduce a second installed bundle format.
