# 00. Principles and method

These rules are the result of the present program. They were found to work, and the rewrite keeps
them. Each part of the spec applies them; a design that breaks one of them is wrong even when it
works.

## 0.1 Design principles

1. **One capability interface.** Everything a program can do is an op. UI, CLI, MCP, scripts,
   plugins, the agent and tests use the same op vocabulary through one entry point. Device and
   client differences belong in presentation, not in separate behaviour paths.
2. **One state owner.** Each fact has one owner. Every other holder derives it or receives a
   one-way projection. No mirrored state, no duplicate counters, no cache that is kept in sync by
   hand. An index, a reference count and a summary are derived and can be rebuilt.
3. **Decide from settled state.** A batch of changes is applied whole. Scheduling, broadcasting
   and resource ownership are decided after the batch, from the difference between the state
   before and the state after. An intermediate mutation causes no decision.
4. **Smallest coherent design.** One mechanism serves each need. Rules that must agree are shared.
   Obsolete paths are deleted. Clear code replaces explanation.
5. **Fix the cause.** Types and shared schemas prevent invalid states. A boundary failure is
   reported and never causes a panic. A structural change is correct when it removes a class of
   errors.
6. **The patch is a value.** A write copies the one record it touches. A change is read off two
   snapshots. One history of snapshots serves every client.
7. **Nodes are a library; a mode schedules.** The graph, the nodes and the engines allow
   execution. When a node runs and what it is handed is the decision of the runtime mode of the
   application: live in goofi-pipe, Kahn in goofi-data.
8. **No global tick.** In the live mode each node schedules itself. Audio and graphics have their
   own clocks. Node processing never runs under the graph lock.
9. **One module per shared concern.** One time module, one owner of every path and tool, one
   session that owns every ephemeral resource, one transport, one recorder, one way a component is
   built (prepared apart, swapped in whole).
10. **A mode is detected, not chosen.** The same public code path serves both cases of a
    distinction the caller does not need to know (in process or across processes, terminal or
    pipe). The implementation detects the case.
11. **Observation does not disturb.** A viewer, a recorder tap or a status read never changes
    what the engines compute or when. Navigation never dirties the patch.
12. **Nothing fails silently.** A node with missing dependencies is listed as unavailable with
    the reason. An exception in a node goes to the error channel of that node. A refused op says
    why and names the help that answers it. A build that fails is a refusal its owner reports.
13. **Files are the interface for authors.** A node is one source file. Its stem names the type,
    its extension names the SDK, its folder names the bundle. No manifest list duplicates a
    directory.
14. **Append, then reclaim.** Durable data is written by append with the commit record last, so
    a crash leaves a valid file. Space is reclaimed by one explicit operation.
15. **Self-similar structure.** The same shape is used at every scale: a graph of transforms and
    queues describes a patch, a recipe and the scheduler; a dataset describes final data and
    spilled intermediates.
16. **Standalone units.** A shared component has a standard interface and no dependency on an
    application. A dataset never depends on another dataset. A program of the ecosystem does not
    depend on another program.

## 0.2 Policy before the first release

- There are no production users and no production data. The rewrite carries no compatibility
  code: no reader for a present file format, no alias for a present op name, no shim.
- An internal interface is not a contract. A rename, a schema field, a wire format or a shared
  rule changes in one commit with every reader.
- The example patches and the skills are ported once, by hand or by a one-time script that is not
  kept.

## 0.3 Method of work

- **Evidence.** A number or a result is stated only from tool output read in the same session. A
  measurement that returns zero or nothing has failed. An estimate is labelled. What was not
  verified is said.
- **Causes.** A cause is a hypothesis until a fixture fails on the broken variant and passes on
  the fix, five runs each way.
- **Failures.** A test failure is a defect. A failing guard is fixed in the code or the fixture;
  an assertion is never widened to pass.
- **Checks before a commit.** The build and the linter are free of warnings. Failures and skipped
  checks are reported.
- **Reviews.** An audit finding is verified against real callers. Review repeats after fixes
  until no substantive finding remains.
- **Commits.** A commit is a tested checkpoint. Rust uses four spaces; frontend code uses tabs
  and single quotes.
- **Language.** Documentation, op help, node docs and messages use ASD-STE100 Simplified
  Technical English.

## 0.4 Tests

- A test asserts observable behaviour through the public interface, which is the op vocabulary
  and the files a program writes.
- All backend tests of a program are named situations in one tests crate per program. A situation
  owns one surface. A defect is fixed with a fixture in the situation that owns the surface.
- A test asserts the state reached, never how fast: no rate floor, no latency ceiling, no sleep.
  A test polls against one harness wait.
- No test simulates load, opens audio hardware or opens a native window. Time in a test is
  driven by the harness.
- The browser is tested with Playwright against the real binary: socket integration, layout and
  gestures. Components are type checked.
- CI is a quick validation: one job per gate on a runner of its own, one build per job, each job
  under ten minutes. A test that fails only beside the load of another suite, or that proves a
  detail of a node or a library, is cut.
- Performance is measured by probes outside the test suite. A probe result is recorded with its
  date, machine and method.

## 0.5 Definition of done for a module

A module of the rewrite is done when:

1. Every statement of its part of the spec holds, and each is covered by a situation or a browser
   test that names it.
2. Every feature of the present program that the spec maps to the module works (part 12 holds the
   parity lists).
3. It has no dependency that the layer rule forbids (part 01).
4. The build, the linter and the type check are free of warnings.
5. Its ops have help text, and its user-facing behaviour is in the guides.

## 0.6 Conventions of this spec

- The spec is declarative: it says what the system is and does. It names no crate, type or
  function, except where a name is part of an interface a user or an author sees (an op, a file,
  a param, a manifest key).
- A statement is a requirement. "Open" marks a point that is not decided; part 13 lists them all.
- A section number (`8.4.2`) is a stable reference for tests, commits and reviews.
- Numbers that were measured carry their date and method. Other numbers are design values.
