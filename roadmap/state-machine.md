# State machines: completion and selection rules

The work covers the state-machine, variable and expression changes through `258f8544`.
The user requested completion of all verified audit findings, on `main`, in tested commits.
This specification includes the user's decisions of 2026-10-05. It replaces the original
weighted-only selection rule and viewer-paced machine timing. Existing control-data,
expression and variable decisions remain in force.

## Model and architecture

- The shared patch clock is the only time owner. Thread wakes must not shift deadlines,
  travel ends, arrival order or branch decisions. Process simultaneous events as a batch.
  Viewer demand must not change execution. Audio applies timed control at sample positions.
- Keep one value carrier (`Data`), expression language, capability vocabulary and producer
  transport. Runtime state has one owner. Project its facts to consumers and the inspector.
  Do not maintain a separate UI simulation, audio machine or machine-variable transport.
- Preserve execution through renames. Reject results from replaced bindings and writes from
  replaced owners. Validate the model at the shared boundary, including load and replay.
- Keep flat states and multiple independent playheads. A playhead is resident in one state
  or traveling on one transition. Several machines compose through variables. Statechart
  hierarchy, parallel regions and history nodes require a different state model; they are
  outside this flat-machine feature, as in the original plan.
- Apply standard event, guard, action and conflict rules to this model. This is a goofi
  state machine, not an SCXML interpreter. Probabilities and interpolated travel are goofi
  extensions. Do not claim SCXML conformance.

## State inspector and transition selection

- Each state has a selection policy: `ordered` (default), `weighted`, or `uniform`.
  The inspector calls the shared `machine state edit` op to select the policy.
- The inspector shows outgoing transitions in their effective order, with move-up and
  move-down controls that work with touch and keyboard. The same controls order triggers
  within a transition. Order is authored state, undoable, saved, and exposed by the ops.
- A state's `order` is an ordered list of preferred transition IDs. It contains no duplicate,
  missing or non-outgoing ID. Unlisted outgoing transitions follow in authored transition
  order. This is a priority relation, not a second copy of transition membership. A wildcard
  transition can have a different priority in each state. One shared resolver produces the
  order for execution and the inspector. Editing/removing a transition removes invalid
  priority references; renaming a state preserves the relation.
- Every transition has `chance` in 0..1 (default 1) and a finite, nonnegative `weight`
  (default 1). Chance is the probability that an otherwise enabled transition participates
  in this decision. Weight is its relative share when the state's policy is `weighted`.
  A zero chance never participates; a zero weight excludes it from weighted selection.
- `ordered`: visit enabled transitions in effective order; take the first whose chance
  succeeds. A failed chance permits the next enabled transition to be considered.
- `weighted`: test each enabled transition's chance in effective order, then draw one
  surviving transition in proportion to weight. `uniform`: the same procedure, with one
  equal share per surviving transition. Order fixes draw order in both random policies;
  it does not silently override the selected policy.
- Several triggers on one transition are OR conditions. Collect each enabled transition
  once per playhead and event batch. Two simultaneous triggers must not double its chance
  or weight. Trigger order fixes evaluation and random-delay arming order, not extra votes.
- All automatic kinds use the same selection procedure: timers, conditions, named events,
  immediate transitions, `meet` and `alone`. Preserve FIFO/LIFO/all resident eligibility;
  those policies select which playheads are eligible, not a competing branch-selection path.
- Seeded draws are repeatable for the same authored model and timestamped input sequence.
  Chance 0/1 and a single eligible branch do not consume unnecessary draws. Iterate states,
  transitions, triggers and playheads in authored order; hash-map iteration must not decide
  behavior. Reset restarts the selected machine's generator and its playheads together.

## Triggers, guards and actions

- Keep `manual`, `after`, `when`, `meet` and `alone`. Add `event {name}` and `always`.
  Named events enter through `machine event {machine, event, playhead?}`. Without a playhead,
  the event is offered to all resident playheads as one batch. It does not dirty the patch.
  A named event is consumed once; it is not retained for a later state or flight arrival.
- `when` supports `rising` (default), `falling`, `change` and `level`. Edge modes establish
  their baseline on entry. `level` and `always` are checked on entry and after a timestamped
  change. Eventless chains settle before the next external event. A terminal state is a
  state with no outgoing transition; it holds its values until jump or reset.
- A transition may have a `guard`: an expression in the existing variables-and-`t`
  namespace. A trigger offers a transition; the guard decides whether it is enabled.
  Evaluate against the settled pre-decision state, before chance and branch selection.
  A false guard blocks every trigger kind. An evaluation failure is visible through the
  common runtime error surface; it does not become a successful transition or a zero delay.
- `machine fire` explicitly selects one transition. It validates the live source and guard,
  then bypasses chance and branch selection. It is the only transition effect that can
  redirect a playhead in flight. An invalid effect returns an error. Jump and reset remain
  explicit control effects, with no undo entry or dirty mark.
- Existing `State.values` are entry assignments. Add `State.exit_values` and
  `Transition.values` as local attribute assignments using the same literal carrier and
  validation. A normal move samples the held values, applies source exit assignments, then
  transition assignments, then travels. Destination entry values land on arrival. A jump
  applies source exit and destination entry assignments without travel. Reset initializes
  defaults and enters the start state; it does not retain values from the previous run.
- A self-transition re-enters by default: exit, transition assignments, arrival/entry, and
  fresh timers and condition baselines. An `internal` self-transition applies transition
  assignments without exit, entry, or resetting dwell; it must have zero travel duration.
  This is local self-transition behavior, not hierarchical-state semantics.
- Select the moves for the entire simultaneous batch before applying any exit/transition/
  entry assignments. Subsequent decisions see the resulting settled batch. Actions must
  not call arbitrary ops or run Python with side effects inside the runtime.
- Stop a non-terminating zero-time chain and report a runtime error. Do not defer part of
  the chain to a later wake. Cycle detection must account for changed local values; revisiting
  a state alone is not proof of a cycle.

## Time and random durations

- Share one duration type between `after.seconds` and transition `duration`: fixed seconds,
  an expression, or `{min, max}` for a continuous uniform random range. Bounds are finite,
  nonnegative and ordered. Equal bounds behave as a fixed duration. The inspector labels
  the modes `fixed`, `expression`, and `random range`; both endpoints are editable.
- Sample a timer once when armed, using the machine's seeded generator. Evaluate an
  expression once at that same logical instant. Cache the resulting interval in the armed
  trigger; the deadline is entry time plus that interval. An edit rearms only the changed
  trigger, at the edit's timestamp. Unchanged triggers keep their deadlines and baselines.
- Sample/evaluate travel duration once when the transition is taken. Its end is departure
  time plus the sampled interval. Interpolation is a function of those absolute times and
  the shared curve. Redirection samples the old flight at the exact effect timestamp.
- A timer that fails its guard/chance or loses selection retries after the same sampled
  interval, from its previous deadline. Positive periods never restart from the wake time.
  A zero interval is one attempt per entry, not an unbounded probability retry loop.
- Inputs and manual effects carry global-clock timestamps. Process deadlines and inputs in
  time order. At one timestamp, settle arrivals and input changes, select all moves, then
  apply them. The common clock also defines deterministic observation points for timed
  condition expressions; arbitrary Python predicates cannot provide analytic root finding.
- Audio sample positions map to this clock. Scheduled edges and numeric travel must reach
  the relevant samples independently of UI updates and worker wake cadence. Reuse the
  existing data-plane binding; do not add a second expression evaluator in the callback.
- Project active transition identity, departure/end times and arrival order from runtime
  facts. The panel uses the same route for preview, connected edge and traveling dot,
  including parallel edges, self-transitions and wildcard routes.

## Standard basis

The selection and execution rules adapt ordered guarded transitions and run-to-completion
from [W3C SCXML, sections 3.5 and 3.13](https://www.w3.org/TR/scxml/).
The distinction between guards and triggers, and eventless stabilization, is also used by
[XState guards](https://stately.ai/docs/guards) and
[XState eventless transitions](https://stately.ai/docs/eventless-transitions).
Timer ownership follows entry/exit lifetime, as in
[XState delayed transitions](https://stately.ai/docs/delayed-transitions).
Uniform ranges, probability and interpolated travel are explicitly specified above as
goofi behavior; they are not attributed to those standards.

## Remaining implementation and verification

1. Complete clocked execution, ordered/weighted/uniform selection, transition chances,
   guards, trigger modes, events, internal/self transitions, local lifecycle assignments and
   random durations. Verify exact deadlines, independent triggers, simultaneous decisions,
   deterministic retry/catch-up/reset, source edits, input timestamps and audio samples.
2. Fix expression binding identity, stale-result rejection, producer-aware input carry,
   bounded worker notifications and deterministic blocking fixtures. Complete positional
   evaluation and common runtime error projection.
3. Preserve identity and execution through machine/state/playhead/attribute/group/element
   renames. Include every machine expression and lifecycle value field in shared traversals.
4. Complete common model validation, owner-safe publication and truthful effect acceptance.
5. Complete inspector controls above and restore endpoint editing. Fix preview/edge/dot
   geometry, arrival facts, parallel/self/wildcard routes, navigation and selection, bool/
   color/numeric conversion, resolved dependency overlays and responsive layout. Keep label
   selection and the single-playhead fire picker from the final Claude session.
6. Remove unused paint printing/timing and stale documentation. Strengthen existing sessions
   with held-worker replacements, successful clean-patch effects, followed-variable triggers,
   exact audio blocks and responsive panel scenes. Remove tests only when their behavior is
   obsolete or already covered by a stronger session.
7. Run warning-free workspace build/clippy/tests, frontend check/tests and relevant browser
   sessions. Re-review fixes, commit tested checkpoints and remove this entry when complete.
