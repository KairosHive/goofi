# State machines: deterministic timing

The machines shipped through `258f8544` run on the viewer cap: `goofi-machines` wakes on
`reducer::Pace`, and every arrival, flight start and `after` deadline is stamped with the wake
time. Each hop rounds up to the 33 ms grid and the error accumulates. Fire, jump and reset carry
no timestamp and land on the next wake. In-flight values are sampled and republished every tick.

The `state-machine` branch (`89e1bfba`) fixes the stamps inside a +2.5k-line rewrite that also
adds selection policies, chance, guards, events, edge modes, exit and transition values, random
ranges, order lists, a health projection, identity-preserving renames, a 48 kHz observation grid,
sample-span frames into audio, a Python purity whitelist and an expression-worker rewrite. It
polls at 1 kHz whenever a machine exists and settles up to 48,000 times a second. None of it is
merged; it stays as reference for the feature specifications.

## Decisions

### The machine produces data when it moves, never on a clock

- A playhead is in one of two states: **stationary** in a state, or **in flight** on a transition.
  Every change between them is one event at one logical instant, and each event produces one new
  `Data` per playhead variable. Nothing is published on a period.
- Stationary, an attribute's variable is a plain value. In flight, it is a **functional**: a new
  `Value::Functional { source, locals }`, a precompiled Python expression of `t` over bound
  constants, evaluated against the global clock by whoever consumes it. Entered with a `t` past
  the flight's end it yields the target state's value; before the start, the origin.
- The machine builds the functional from the transition: `source` is the curve's closed form over
  `(t - start) / (end - start)`, clamped, between `a` and `b`; `locals` hold `a` (the value held at
  departure), `b` (the target state's value), `start` and `end` in patch seconds. `progress` is
  the same functional from 0 to 1. A string or bool is not interpolated: it switches when the
  arrival event lands. `state`, `prev` and `arrived` are plain values.
- One source text per curve, so a consumer compiles each once and evaluates with new locals.
  Equality of a functional is equality of source and locals; the codec carries both, the body a
  table-like encoding of the locals behind the source. Shared-memory and wire changes land with
  every consumer in one change.

### Logical instants, deadlines from deadlines

- `Time::now()` stays the one clock, f64 patch seconds. A flight ends at `start + duration`; a
  dwell is due at `arrived + seconds`, its interval evaluated once when armed; an arrival is
  stamped with the flight's end; a dwell that fires and is not drawn re-arms from its previous due.
- `advance(now)` processes every instant below `now` in order: the earliest of the next flight
  end, the next due and the earliest queued request. At one instant: land every arrival, collect
  every take, then depart, before the next instant. A zero-time chain is bounded at one instant and
  reports an error; nothing is deferred to the next wake.
- Requests are stamped where they are made: `Driver` stamps fire, jump and reset with the op's
  `time.now()`. A write to a variable a `when` gate reads is stamped by the store and wakes the
  thread. Every frame the machine writes carries `meta.time = at`.
- The thread wakes only for the earliest of a flight end, a dwell due, a stamped request and a
  watched write; with no machine, the idle sleep. `Pace` stays the reducers' tool.

### One evaluator, two functions, evaluated before the run

- The in-process free-threaded evaluator (`PyExprEvaluator`, 3.14t) is the one module for Python
  expressions: `compile(source) -> Compiled`, cached by source text, and
  `eval(compiled, t, locals) -> Data`. Param bindings, variable expressions, machine `when` and
  `after` expressions and functionals all go through it; no second harness.
- **An expression is evaluated when its value is needed: on the node's runtime thread, right
  before `process`.** Every computed binding, with or without a stream behind it, `t`-dependent
  `lfo()` and `noi()` included, is read at the run's `now`. The engine's expression worker, the
  `Cell` handover, `Bind::timed` and the runtime's 10 ms re-evaluation pace go. A slow expression
  delays its own node's run and nothing else; the free-threaded interpreter is what makes a node
  thread a sound place for it. Python still never runs in a device callback or the render ticker:
  the audio control half and the graphics half evaluate on their runtime threads before each run.
- **A functional is literally an expression.** A bare binding whose latest frame is a functional
  is evaluated as a computed binding with the functional's source and locals, through the same
  path; `control::read` elsewhere sees its target value `b`. The follower and the reducer read it
  the same way. One function in `goofi-node` turns a functional into a value at `t`.
- **A param never ticks a node.** `process` runs on `autotrigger`, on data arriving at a slot, or
  on the new `common.trigger` pulse, declared for the signal engine alone; audio and graphics run
  on their clocks. The expression `triggers` flag goes everywhere: `ExprDecl.trigger`, the document
  and op field, `Bind`/`Sub::Bind.trigger`, `params_changed(.., trigger)`, `trigger_pending` as a
  flag the pulse alone sets, and the `trig` toggle in `ParamField`. A node with a `t`-dependent
  expression and no run cause does not run; that is the model.

### Consumers evaluate at their own time

- **Signal plane.** A functional, like any computed binding, is read before `process` with
  `t = now`.
- **Audio plane.** Before each of its runs, the control half evaluates a functional at the block
  boundaries up to its next run, which the clock tie gives ahead of time, and hands the callback
  breakpoints `(sample, value)`. The callback ramps linearly between breakpoints in Rust and holds
  the last. The functional clamps at `end`, so a ramp never overshoots the target; the block that
  holds the flight's end reaches the target at its boundary. A block already rendered when the
  frame lands keeps the old value: one block of onset latency, no delay added.
- **Browser.** The reducer that serves `/data/variables/<name>` evaluates a functional at the
  viewer cap, as presentation; engine scheduling does not change. `PlayheadDots` keeps the CSS
  glide between samples and orders residents by `arrived`.
- **Recording** stores the functional frame once. A `when` condition over another machine's
  functional is read at events: the write, an arrival, a due.

## Stages

Shipped: expressions evaluated on the runtime thread before the run (the worker, `Cell`,
`Bind::timed` and the 10 ms pace removed); the `triggers` flag gone and `common.trigger` added.

1. **Carrier** (`goofi-core`, `goofi-codec`, `goofi-node`): `Value::Functional`, codec tag and
   golden, equality and hash, the functional-to-value call; the compile cache. About +110.
2. **Stepper** (`goofi-graph/src/machine.rs`): the two playhead states; `advance` as above;
   stamped requests; `arrived`; the functional writer. The sampling loop, `HOPS` deferral and
   `blend` go. About +60 net.
3. **Thread** (`goofi-bridge`): stamped messages, the deadline wait, the store's wake, the
   cap-paced loop removed. About +20 net.
4. **Audio breakpoints** (`goofi-audio`): boundary evaluation in the control half, the callback
   ramp. About +80.
5. **Tests** (`goofi-tests`, Playwright): drive `Machines` directly on explicit instants, dense
   against sparse against one late catch-up, identical frames and `arrived`; a fire stamped before
   a due lands first; a 1/48000 s ping-pong. Through the session: the k-th hop's `arrived` equals
   the jump instant plus k dwells; a functional read at `end + 1` is the target and at `start`
   the origin; an audio param bound to an interpolating attribute renders the ramp under the
   external clock and never passes the target; a signal node reads the value for its run; a
   `common.trigger` pulse runs a node that nothing else runs; an `lfo()` param moves only when
   its node runs. No rate or latency assertion.

Each stage ends at a tested commit.

## Open

- Which branch features to port afterwards, each as its own small change: selection policies and
  order, chance, guards, `event`/`always`, edge modes, exit and transition values, random ranges.
  Their specification is the branch's `roadmap/state-machine.md`.

## Not to be done

- A fixed-point tick type, a 48 kHz observation grid, a 1 kHz machine thread, or any periodic
  publication by the machine.
- Python in a device callback or the render ticker; a presentation delay; a sample-exact break
  inside an audio block.
- Re-evaluating alias expressions inside the machine; a followed variable's latest frame is what a
  condition reads. Root finding for a condition's crossing mid-flight.
- A Python purity whitelist, or a change to `time()`.
