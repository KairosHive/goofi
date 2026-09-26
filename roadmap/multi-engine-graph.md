# The graph, with more than one engine

The seam is built (`backend-architecture.md` §1 owns the Patch/Runtime split that follows). What
remains at the seam:

- **`Kind` has no exhaustive match.** `NodeEntry::leaf`/`leaf_mut` and `Graph::stub`
  (`goofi-graph/src/lib.rs`) use `_ =>`, so a fourth variant compiles silently classified "not a
  leaf". Replace them with explicit `Kind::Facade | Kind::Port(_)`.
- **`keys_touching`** (`goofi-signal/src/engine.rs:132`) is O(N × (links + bindings)) and runs once
  per node reaching `Ready`. A large patch will notice.
- **`Graph::contains`** means "is a running leaf", `exists` "is any node", `wirable` "leaf or port";
  `Command::precondition` picks a different one per variant. One pass to name them for what they
  answer.
- **`goofi-graph` depends on `goofi-view`** beside `goofi-core` and `goofi-node`, for `ViewWant`
  alone. Decide whether the view vocabulary belongs below the graph or the dependency goes.
