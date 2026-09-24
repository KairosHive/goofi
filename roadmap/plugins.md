# Plugin system

The package contract is `sdk/README.md`; `library.md` owns the installation unit and distribution.
No generic state/event bus, panel-open API, task manager, or resource provider is planned for this
version. Live replacement, a marketplace, dependency relations between plugins, and durable
lifecycle subscriptions are outside the initial interface.

Remaining product work:

- Build the session/corpus plugin: people, corpora, acquisition sessions, tags, and catalogue.
- Choose the remote database/file service and its credential model.
- Add a persistent upload queue, restart reconciliation, and local download cache in that plugin.
- Supply its playback node and configure it through ordinary goofi operations.
- Verify source builds and lifecycle behavior on Windows and macOS as well as Linux.
