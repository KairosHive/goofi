# State machines: what remains

The timing work shipped: a machine settles every instant it is owed, a flight is a functional
of the patch time read by whoever consumes it, and the thread wakes for deadlines, requests and
watched writes alone. Read `goofi_graph::machine`, `goofi_bridge::machines` and the
`machine::` situation for what the code says. The `state-machine` branch (`89e1bfba`) is not
merged; it holds the specification of the features below in its `roadmap/state-machine.md`.

## Remaining work

Each as its own small change against the shipped model, in this order of value:

1. Selection policies per state (`ordered`, `weighted`, `uniform`) with an authored transition
   order, and `chance` on a transition.
2. `guard` expressions; `event {name}` and `always` triggers with a `machine event` op.
3. `when` edge modes (`rising`, `falling`, `change`, `level`).
4. `exit_values` on a state and `values` on a transition; `internal` self-transitions.
5. A random range `{min, max}` as a duration or an `after`, sampled once from the machine's seed.

## Open

- **Exact step onsets in audio.** A functional lands one wire hop after its event, so a `Step`
  curve sounds up to one block late and is ramped across that block. The branch's 10 ms
  presentation delay fixes the onset at the price of latency. Build it only if a patch asks.
- **A `when` over another machine's functional** is observed at events: the write, an arrival,
  a due. A crossing mid-flight is seen at that flight's end. Root finding per curve is not planned.

## Not to be done

- A fixed-point tick type, a 48 kHz observation grid, a 1 kHz machine thread, or any periodic
  publication by the machine; a sample-exact break inside an audio block.
- Python in a device callback or the render ticker; a presentation delay.
- Re-evaluating alias expressions inside the machine; a Python purity whitelist; a change to
  `time()`.
