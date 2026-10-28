# State machines: what remains

The timing work shipped: a machine settles every instant it is owed, a flight is a functional
of the patch time read by whoever consumes it, and the thread wakes for deadlines, requests and
watched writes alone. Read `goofi_graph::machine`, `goofi_bridge::machines` and the
`machine::` situation for what the code says. The `state-machine` branch (`89e1bfba`) is not
merged; it holds the specification of the features below in its `roadmap/state-machine.md`.

## Remaining work

Each as its own small change against the shipped model, in this order of value:

1. An authored order of the transitions out of a state, editable on its card: `sequence` and an
   `expression`'s index read it. Today it is the transitions' authored order.
2. `event {name}` and `always` triggers with a `machine event` op. A transition's `condition` is
   the guard: one expression over `variables.*`, `me` and `st()`, taken on the turn to true.
3. Condition edge modes beyond the turn to true (`falling`, `change`, `level`).
4. `exit_values` on a state and `values` on a transition; `internal` self-transitions.
5. A random range `{min, max}` as a duration or an interval, sampled once from the machine's seed.

## Open

- **A test evaluator meets a flight.** The audio situation `a_patch_sounds_under_the_external_clock`
  reads a playhead's attribute through the test evaluator (`goofi_tests::FirstVar`), which runs
  no functional; after a `session load` the newborn playhead's attribute is a functional while it
  is in flight (an arrival or a transition), and the step asserting that the corpse's answer never
  lands on the newborn read that functional instead (CI 2026-10-23, run 277, once; five local runs
  passed). Decide whether the fixture waits the flight out before it asserts, or reads through an
  evaluator that runs motions; the assertion itself must not widen.
- **A rename restarts the renamed thing's run.** The machine thread keys its runs by playhead
  and state name, so `machine playhead rename` births the new name at its start and
  `machine state rename` sends a playhead resting in the state back to its start. Forward the
  rename to the thread (rekey the run, the head's state, its flight and its gates) before the
  replaced model lands.

- **Exact step onsets in audio.** A functional lands one wire hop after its event, so a `Step`
  curve sounds up to one block late and is ramped across that block. The branch's 10 ms
  presentation delay fixes the onset at the price of latency. Build it only if a patch asks.
- **A condition over a functional** is observed at events: the write, an arrival, a due. A
  crossing mid-flight is seen at that flight's end. Root finding per curve is not planned. The
  same holds for `t` in a condition: the thread does not poll.

## Not to be done

- A fixed-point tick type, a 48 kHz observation grid, a 1 kHz machine thread, or any periodic
  publication by the machine; a sample-exact break inside an audio block.
- Python in a device callback or the render ticker; a presentation delay.
- Re-evaluating alias expressions inside the machine; a Python purity whitelist; a change to
  `time()`.
