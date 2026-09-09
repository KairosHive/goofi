# Sprint review — 2026-09-08

## Resolution pass

Resolved: R01–R14. The follow-up decisions below replace the proposed sample-progress contract.

| Findings | Result |
|---|---|
| R01 | A close holds its original session before flushing or waiting; manifest ordering belongs to that session. |
| R02 | WAV format changes open a new file; varying block sizes remain valid. |
| R03 | Every frame is appended in full. Removed overlap matching, causal output reuse, and excess history retention. Also corrected FreqShift's odd-length FFT mask. |
| R04 | Epoch captures supplied windows; Buffer owns window length. Rust and both Python tiers can clear inputs after a successful process call. |
| R05 | Resample handles independent windows. Fractional-rate conversion reports the actual rate, bounds filter factors, and validates its axis. |
| R06 | Control compatibility and locks are checked before a global value changes. |
| R07 | Rename refuses an occupied destination, including panel-only and lock-only groups; replay does not move a group whose members a peer changed. |
| R08 | Fresh commands remain strict; stale global replay leaves current state in place and does not block the stack. |
| R09 | Drawing echo suppression is consumed once; obsolete image loads are cancelled. |
| R10 | Parameter sockets retry while watched; release/reacquire is settled before closing. |
| R11 | A parameter row stays visible during its pointer gesture, then membership derives from current state. No clear revision is needed. |
| R12 | One shared statistics implementation; robust mode requires a positive window; unused held state removed. |
| R13 | Replacement is staged and synced before rename; the existing library filename is retained when the same type has another source filename. |
| R14 | The reviewed private audio/global test modules were removed, including five ignored hardware tests. Useful checks now run through existing external audio/editing sessions. This is not a claim that every older private test in the repository was removed. |

Follow-up decisions (user-approved):

- A frame is the complete input, including a rolling Buffer window. No sample identity, overlap annotation, or overlap inference is needed. A node that needs fresh samples is connected before Buffer.
- `NodeCtx::clear_input` and Python `self.clear_input` request consumption. The runtime validates every name, then clears held cells only on success, before its next input drain. A multi input clears all held wire values but retains its links. Failed calls preserve data; a successful call with no outputs still clears.
- Epoch consumes each trigger. A qualifying trigger captures an available data window once; later triggers require a new data arrival. Data arrivals alone do not capture. Rejected windows are consumed. Removed before/after timing, internal history, pending events, and pre-trigger baseline; kept averaging, whole-window baseline, rejection, and reset.
- Resample has no cross-frame phase or filter history. Each window's output length rounds up. A bounded rational ratio supports fractional rates; output metadata states the actual conversion rate. Extreme conversion factors are refused before filter allocation.

Follow-up validation:

- Codec 2/2, EEG 2/2, authored/library nodes 8/8, subprocess Python 11/11, and signals 9/9 passed through the normal runtime.
- The input-clearing error/recovery session also passed with embedded Python.
- The final EEG session also passed after adding labeled one-dimensional window coverage.
- The user's new simultaneous Osc/Gain WAV assertions passed; the full recording session file passed 4/4 serially. That test remains the user's uncommitted change.
- Final `cargo build --workspace --all-targets` and `cargo clippy --workspace --all-targets -- -D warnings` passed without warnings, with offline dependency resolution.
- Added checks for repeated equal scalars, complete repeated windows, causal filter progress, odd-length frequency shifts, fractional rates, repeated independent resampling, and invalid-axis recovery.
- Original Stream failed a public-source probe (2 samples held instead of 3); the replacement passed. Original Resample produced 52 samples at a claimed 25.5 Hz for a 200-sample 100 Hz window; the replacement produced 51. These two negative controls are component probes, not full manager sessions.
- Both local Python wheels were rebuilt and installed. The first sandboxed signal run timed out before its first output; normal shared-memory runs passed. An initial build warning for an unused import was fixed.
- No frontend changes or browser tests in this follow-up. Full workspace tests and the full embedded-Python suite were not run.

Validation completed:

- `CARGO_NET_OFFLINE=true cargo build --workspace --all-targets`: passed, no warnings.
- `CARGO_NET_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings`: passed.
- Editing: 15/15; node/library: 8/8; signals: 9/9; audio channel session: 1/1; recording: 4/4.
- Frontend: 723 module tests; Svelte and e2e type checks; desktop, phone-landscape, and tablet socket sessions each passed 5/5. All new checks also passed on phone.
- The original recorder and WAV variants failed the new recording assertions. The old embedded frontend failed the new reconnect assertion.
- Full workspace test and embedded-Python suites were not run; validation used the affected sessions.

Failed attempts and limits:

- The first nested Cargo build could not reach crates.io. Offline mode used the installed dependencies successfully.
- Sandboxed stream tests failed listener creation or timed out. Runs with normal system shared-memory access passed.
- A parallel recording run reported one dropped block in an existing assertion; serial runs passed twice.
- Adding control-kind operations exceeded the editing session's old fixed undo count. The count now includes those operations; the complete undo/redo session passes.
- The full phone socket run passed 4/5; its existing mouse-marquee step failed on the touch layout. The checked-in configuration runs these socket sessions on desktop. A temporary multi-project run also reused a test global; separate runs with fresh backends passed. Temporary configuration was removed.
- Vite retains its existing large-chunk warning; no compiler or typecheck warning remains.

The sections below preserve the original review evidence. Their line numbers and “no fixes” statement refer to the original reviewed HEAD.

## Review contract

- Requested output: filtered work for a later agent; no product fixes in this pass.
- Reviewed HEAD: `c394490493fb37838252371e1b45faf7125cf49a`.
- Window: `git log --since='5 days ago'` at review start, on the evening of September 8 in America/Toronto. This is a rolling 120-hour window, not five calendar dates.
- Inventory: 394 reachable commits, including merges; 497 changed paths; net diff 55,229 insertions and 5,131 deletions. Net comparison base: `7bb34d3326a95d19c2bf96aa240cf58560f04f75`. The inventory below is the fixed review input; do not recompute a moving cutoff when resuming.
- Evidence classes: **op reproduction** = current manager called through its normal op entry; **component reproduction** = actual source or public Rust API in a small probe; **source confirmed** = caller and guards checked, no end-to-end reproduction.
- P1: wrong data, wrong saved state, or a failed mutation contract. P2: narrower user failure or a concrete reduction worth making. These are work priorities, not a security score.
- No product fixes were applied. Temporary probe source was removed from the repository. No full workspace build, clippy, Rust suite, browser suite, or performance benchmark was run. The op probe compiled with Cargo and exited successfully.
- Existing working-tree changes were left alone: eleven deleted roadmap files and untracked `test-patches/`. Deleted roadmap files were read from HEAD where relevant.
- Coverage limit: all commit subjects and changed-path inventories were considered. This is not a line-by-line review of all 497 final files. Deep checks concentrated on signal history and scheduling, recording ownership, global commands and undo, inspector state, drawing, and live sockets. Individual simulation/ML/shader algorithms, every platform host, and all older engine internals have not been proved clear. Parallel reviewers reached a service usage limit before their final broad verification pass. This report records completed evidence only; it does not claim audit convergence.

## Work order

1. R01 and R02: recording integrity; independent fixes.
2. R03–R05: completed under the full-frame decisions above; no sample-progress contract.
3. R06–R08: global mutation planning and replay. Test these together in the existing editing session.
4. R09–R11: frontend state and reconnect behavior.
5. R12–R14: shared statistics, safe library replacement, and test/code reduction.

## R01 — P1 — A slow close publishes an old stream into a new recording

- Evidence: component reproduction, reviewed again against current `close_now` and both resulting manifests.
- Source: `backend/goofi-record/src/lib.rs:452` (`close_now`); `close_later`, `stop`, `start` in the same file.
- Sprint area: recording lifecycle; related commits `407fca9a`, `e07239d4`, `268c9742`. HEAD `roadmap/recording.md` already records a close/stop race; this reproduction adds cross-session corruption to the known lost-row failure.
- Path: close detaches a stream from session A; `finished` waits for its encoder without the session lock; stop A and start B; completion reacquires the lock and pushes the entry into whatever session is current. No session identity travels with the detached stream.
- Executed sequence: public Recorder API, injected encoder whose `finish` waits on a barrier, `close` on a thread, `stop`, `start`, release barrier, join close, `stop`.
- Result: A `manifest.json` has `streams: []`; B has A's `visual-out__…mkv` row. The row has A's timestamp and file name, under B's manifest.
- Action: keep detached closing work owned by its original session. Establish how a stopped session records pending finalization; never publish by reading the current session after a wait. Do not restore the old global-lock wait as a fix.
- Acceptance: extend the recording session with close/stop/start under a controlled slow encoder. A keeps its row; B gets no old row; unrelated stream drains continue during finish. Repeat with stop and no new session.

## R02 — P1 — A WAV accepts a new channel count or sample rate without opening a new file

- Evidence: component reproduction plus real audio caller trace.
- Source: `backend/goofi-record/src/stream.rs:242` (`takes`), `wav.rs`; `backend/goofi-record/src/frame.rs`; audio `plan.rs`, `runtime.rs`, `control.rs`.
- Sprint area: native recording formats (`e754a67e`) and later stream-format changes; use `git blame` on `takes` for the exact originating hunk.
- Path: `audio:Noise` exposes `noise.channels`; plan output width changes; the runtime ring and control half pass the new channel shape; record Frame derives a new Audio Kind. `takes` matches any Audio Kind and checks only WAV byte capacity. It does not compare rate/channels with the open Kind.
- Executed probe: create mono 48 kHz Stream; query stereo 48 kHz and mono 96 kHz; both return true. Write a stereo block: the header remains mono and the frame count follows that old header.
- Action: require equal audio format and enough file capacity. A changed format opens the next file. Keep varying block length legal.
- Acceptance: extend the recording session through mono→stereo and a sample-rate change. Open the files with an independent WAV reader and verify header, duration, sidecar row counts, and manifest format.

## R03 — P1 — Equal sample bytes are mistaken for already-consumed samples

- Evidence: component reproduction using current `goofi-core/src/stream.rs`; real caller checked.
- Commits: `2aaaef47`, `43352829`, `e429ef77`.
- Source: `backend/goofi-core/src/stream.rs:75` (`overlap`) and `Stream::push`; `node-bundles/signal/Delay.rs:65`. Also affects Smooth, Normalize, causal Filter, and other Stream consumers.
- Path: Stream infers overlap from equal bytes, with no sample position or explicit overlap input. A fresh Constant frame is a real arrival even if its value is unchanged. Runtime arrival handling does not turn equal samples into duplicate deliveries.
- Executed input: scalar samples `0,1,1,1,1`, Delay in samples mode with size 2. Stream history stays `[0,1]`; the two-sample delay stays 0. It must become 1 once the delay has elapsed. Repeated periodic blocks have the same ambiguity.
- Action: establish sample identity/range in the shared stream contract. Fresh samples advance even when equal; buffered windows state which samples they contain. Do not add exceptions for constants or guess identity from values.
- Acceptance: extend `signals.rs` situation `a_stitching_node_answers_from_the_past_and_a_transform_round_trips`: step 0→1, hold beyond delay, repeat periodic blocks, and compare direct input with the same samples through Buffer.

## R04 — P1 — Epoch consumes held data again when only the trigger arrives

- Evidence: component reproduction and runtime caller/guard trace.
- Commit: `792da094`.
- Source: `node-bundles/eeg/epoch.py:24`, `:83`; `backend/signal/goofi-pymod/src/params.rs:47`; signal runtime input storage and process assembly.
- Path: both Epoch inputs default to `trigger=true`. Runtime keeps each input's latest cell and passes all cells to process after a triggering arrival. Epoch appends the full held data and advances `written` on every call, before it handles the trigger. The required-data guard succeeds once a first data frame exists.
- Executed sequence: data `[10,20]` with low trigger, then high trigger with the same held data. `written` changes 2→4; an epoch is reported complete without two more data samples arriving.
- Action: consume a data frame once and give trigger events a defined sample position. Merely setting the trigger slot to non-triggering can lose short pulses and does not solve alignment. Coordinate with R03's progress contract.
- Acceptance: extend the EEG session with independent controlled data and trigger sources. Trigger-only wakes must not advance sample count. Test trigger before/after data, held high, fall/rise, and exact pre/post-event samples. The current EEG tests do not cover this Epoch session.

## R05 — P1 — Resample restarts phase and filter history for every input frame

- Evidence: component reproduction calling the actual Python process body.
- Commit: `8541feb7`.
- Source: `node-bundles/signal/resample.py:26`.
- Path: each frame independently enters `resample_poly`; no fractional output phase or FIR history survives. Inputs accept arbitrary chunk lengths, including scalar chunks from streams. There is no guard requiring a block divisible by the conversion ratio.
- Executed sequence: 1,000 one-sample frames at 1,000 Hz, target 250 Hz. Result: 1,000 output samples labelled 250 Hz, expected 250. One second becomes four downstream. Uneven larger blocks also accumulate count error; independent filter padding adds boundary transients.
- Action: preserve rate phase and filter state for fresh stream chunks. Resolve fresh chunks versus complete windows with R03. Also ensure the advertised rate matches the integer-rounded conversion ratio currently used.
- Acceptance: extend the analysis session in `signals.rs`; compare uneven chunks against a contiguous reference, total output count, and boundary behavior. The existing exact 2:1, 512-sample window test checks central amplitude only.

## R06 — P1 — A refused global edit mutates hidden graph state

- Evidence: op reproduction; independently checked by a second reviewer.
- Commits: `b949bdf0`, subsequent control ops.
- Source: `backend/goofi-graph/src/lib.rs:555` (`apply_global_change`); `command.rs:482`; bridge `arms.rs` (`global_add`, `global_edit`); `lib.rs:1286` (`AppState::call`).
- Path: apply value first, then validate/apply control. If the control cannot draw the value, `set_control` returns Err. History records no inverse; the write tail broadcasts only successful calls. No caller checks `Control::fits` before this mutation.
- Executed op: `global entry add {name:"audit.bad",type:"float",value:1,control:{kind:"toggle",x:0,y:0,w:2,h:2}}`.
- Result: Err `a toggle cannot draw a float`; document has no `audit.bad`; retry valid add gets `already exists`. The hidden entry appears on a later successful write. The same sequence can change an existing value before a failed widget edit.
- Action: validate and plan the complete global change before applying it. A single command must be atomic itself; Compound cannot roll back an inverse it never received.
- Acceptance: extend `editing.rs` with invalid add, invalid combined value/control edit, then a valid edit and undo. Refusal preserves graph, document, history, and dirty state. Include a compound containing the rejected edit.

## R07 — P1 — Group-rename undo moves entries that were never renamed

- Evidence: op reproduction; independent source verification.
- Commit: `e48118cb`.
- Source: `backend/goofi-core/src/globals.rs:635` (`rename_group`); graph `lib.rs:633`; `command.rs:519`.
- Path: destination group can already exist if individual member names do not collide. Inverse is a whole reverse group rename. It therefore moves the destination's old members and panels too. Destination group-lock state can also be overwritten by the move.
- Executed session: A adds `first.one`; B adds `second.two`; A renames first→second; A undoes. Result contains `first.one` and `first.two`; B's original `second.two` is gone.
- Action: choose a coherent rename domain. The smallest choice is to refuse an occupied destination group, including panel-only and lock-only groups. If merging is required, give it an inverse that targets exactly the moved entries and replans against current state. Do not restore raw layout state.
- Acceptance: extend the two-actor editing session with occupied, panel-only, and lock-only destinations, then peer changes between rename and undo. Undo must not move unrelated entries.

## R08 — P2 — Peer deletion blocks global-command undo

- Evidence: op reproduction; independent source verification.
- Commits: new global command family (`b949bdf0`, `e48118cb`, `b98518b1`, `030af8b8`).
- Source: graph `command.rs:493` (`RemoveGlobal`) and `:877` (`flip`); related rename/source/lock commands call the strict store methods directly.
- Executed session: actor A adds `audit.peer`; actor B removes it; A calls undo. Result: Err `no such global audit.peer`. A's last entry stays applied, so another undo repeats the failure and cannot reach earlier work.
- Guard check: replay intentionally bypasses `precondition`; the error is inside command execution. RemoveNode already has tolerant replay, but RemoveGlobal does not.
- Action: put fresh-caller requirements in the fresh path; give stale global replay defined, non-blocking behavior. Audit the new command family for missing, renamed, and newly locked targets. Do not suppress all real errors.
- Acceptance: extend the existing two-actor editing session through peer removal/rename/lock and repeated undo/redo. Earlier entries remain reachable; a fresh invalid op is still refused.

## R09 — P2 — DrawPad ignores redo of its last local image

- Evidence: source confirmed by two reads; browser scenario not run.
- Commit area: `3c77743f`, `156b7ecf`.
- Source: `frontend/src/lib/ui/DrawPad.svelte:44` (`mine` and value effect).
- Path: commit picture A stores `mine=A`. Undo sets value empty and clears canvas but leaves mine=A. Redo sets value=A; `url === mine` exits before drawing. Document is correct and canvas stays empty. Loading peer image B then restoring A has the same stale comparison.
- Action: make echo suppression refer to the current pending write/current canvas state, not the last image ever authored. Invalidate pending image loads when the value changes; an old onload callback must not replace newer state.
- Acceptance: extend the drawing gesture session: stroke→undo→redo; peer replacement→undo; rapid image changes. Compare rendered pixels with the current global value.

## R10 — P2 — A closed param socket remains registered and never reconnects

- Evidence: source confirmed; no forced browser drop run.
- Commit: `c3944904`.
- Source: `frontend/src/lib/api/paramLive.ts:20`; graph singleton `stores/graph.svelte.ts:882`; watcher in `inspector/ParamForm.svelte`.
- Path: watch adds a WebSocket and a holder count. Only message is handled. Close/error leave the closed socket in the map. Further watchers increase holders on the dead entry. A mounted inspector never gets its live-rate updates back after a transient close. The slower control sweep is a fallback, so this is loss of update rate, not necessarily permanently stale values.
- Action: reconnect while settled demand remains positive; cancel retries when no holders remain. Keep one socket per watched node. Batch release/reacquire decisions to avoid closing a shared socket on a transient zero.
- Acceptance: extend the socket session: observe live values, interrupt only /params, keep inspector mounted, require updates to resume; two watchers must share one replacement connection.

## R11 — P2 — Clear cannot remove a sticky row that returned to its old baseline

- Evidence: source confirmed; no browser run.
- Source: `frontend/src/lib/inspector/paramFilters.ts:126` (`settleNonDefault`); `ParamForm.svelte:259`; graph baseline command.
- Path: membership stays when held baseline values equal current baseline values. Start baseline 0, move 0→1→0: row stays by design during editing. Clear snapshots 0 again. `sameZero` is still true, so the row remains. On later clears the document can contain no delta at all for equal baseline content.
- Action: define a clear event/revision with one owner, distinct from baseline value equality, or limit sticky membership to the active gesture. Retain the intended rule that a row does not disappear under an active drag.
- Acceptance: extend the existing param-filter situation with move-away→return→clear twice, plus clear from a peer. Require an empty list and correct subsequent re-entry.

## R12 — P2 — Two copies of normalization logic share the same unsupported-mode bug

- Evidence: source confirmed; numerical implementation and manifests checked.
- Commits: `43352829`, `03ee549f`.
- Source: `node-bundles/signal/Normalize.rs:10-88`; `TableNormalize.rs:10-95`.
- Duplication: Scale, Welford updates, median/interpolation, minmax/zscore/robust window rules, and scale application. About 80–90 lines carry the same numerical policy in each node; frame layout is legitimately different.
- Bug: Running::scale treats every mode except minmax as zscore. Both manifests offer robust with window size 0. TableNormalize defaults to size 0. Thus a valid robust selection uses mean/standard deviation despite promising median/interquartile range.
- Dead state: `TableNormalize.held` is assigned but never read.
- Action: share the numerical policy below the authored node files; keep per-node layout/history local. Define robust running support or explicitly refuse that combination. Remove held. Do not abstract unrelated manifest syntax.
- Acceptance: extend the signal/table session with outliers, robust size 0 and positive size, hold/release/reset, and equivalent array/table inputs.

## R13 — P2 — Library overwrite removes the old file before the replacement is ready

- Evidence: source confirmed; I/O failure injection not run.
- Commit: `52edf599`.
- Source: `backend/goofi-bridge/src/arms.rs:230-244` (`library_save`).
- Path: overwrite removes the held file, then copies from the workspace. Copy failure leaves the prior private-library source deleted; a different filename for the same type makes this explicit. The workspace source may survive, but the previous library version does not. Caller authorization to overwrite does not make a failed replacement successful.
- Action: stage the replacement in the destination directory and complete it before replacing/removing the old source. Handle the same-type/different-filename case without losing the only old copy. Keep cross-filesystem workspace moves supported.
- Acceptance: extend the library session with a controlled copy failure and both same/different filenames. Old library content survives refusal; successful replacement leaves one source and correct scan provenance.

## R14 — P2 — New Rust unit-test islands violate the suite boundary and repeat policy in test fixtures

- Evidence: source inventory; no runtime failure claimed.
- Scope examples: new tests in `backend/audio/goofi-audio/src/chanmap.rs`, `host.rs`, `control.rs`; `backend/goofi-core/src/globals.rs:687`. Existing older unit tests are not all sprint additions.
- Concrete issue: these tests access implementation in the crates they judge, contrary to the single external `goofi-tests` crate rule. The globals tests maintain extra lists of control kinds and check those lists against other lists. They do not replace the command session that missed R06–R08.
- Action: move useful behavior into the existing external audio/editing situations; delete redundant private-function checks. Prefer an exhaustive enum match/shared declaration for control-kind coverage. Do not expose internals merely to transplant tests.
- Adjacent reduction: touched files contain long comments that restate behavior/history, including >2-line comment blocks in DrawPad, globals, and CI. Thin these when changing their owner; retain only a short deviation reason. Do not run a repo-wide formatting pass or treat comment count alone as a major defect.

## Known work and rejected candidates

- **Known recording close race:** R01 is one task with the existing note, not a second backlog item. The new evidence is that a row can move into B, not only disappear from A.
- **Known VST scan cost:** committed `roadmap/vst3-scan-boot-cost.md` says warm caching/source keys are fixed; cold serial child scans and a deliberate retry door for cached refusals remain open. Retain those tasks. Do not add an unconditional cache clear to library refresh. That roadmap file is currently deleted in the user's working tree.
- **Known graphics readback fixes:** `roadmap/graphics-engine.md`, “What the frame path cost”: polling order, second pacing flag, U8 demand, and forwarding already corrected measured 9–10 fps to 29–30 fps. Do not re-file these fixed causes. Remaining socket bandwidth is documented, not a new measured bottleneck from this review.
- **Known Python stop gap:** HEAD `roadmap/python-subprocess-stop.md` records missing graceful stop in the child and MIDI note-offs. Keep one task there if the deletion is reconsidered; not newly discovered here.
- **Metadata-only viewer demand:** `viewers/capacity.ts:57` asks for viewer-size pixels even for ranks it only describes. This looks inconsistent with the one-texel rule. Backend demand-fold verification was not completed, so it is NOT a confirmed performance finding. Trace capacity→fold→ViewWant and measure before promoting it.
- **Filter phase switching:** possible retained causal history across causal→zero-phase→causal; not verified as an independent bug. R03 already proves shared sample loss. Do not add a speculative second Filter rewrite.
- **Multiple DrawPad painters:** control draw broadcasts to open clients; exact-once ownership with several pads deserves a browser check. No duplicate-stroke reproduction was completed. Headless/no-open-pad doing nothing is explicitly accepted in AGENTS and is not a finding.
- **Python import serialization:** retain; the code prevents a real cyclic-import deadlock. Lengthy comments alone do not justify changing its lock behavior.
- **Platform CI removal:** Windows/macOS omission is an explicit cost decision. Green Linux is not cross-platform proof, but reinstating the matrix is not an unrequested cleanup fix.
- **One source file per node / separate signal and graphics simulation views:** accepted architecture. Similar node declarations or names alone do not establish bloat.

## Evidence available in this session

These `/tmp` files are disposable evidence, not another backlog. Findings above contain the failure sequence needed to rebuild them.

- `/tmp/goofi-root-probe.log`: Cargo compilation and manager op results for R06–R08.
- `/tmp/goofi-root-probe.rs`: temporary manager probe source, copied out of the repo after execution.
- `/tmp/goofi-stream-check.rs`, `/tmp/goofi-epoch-check.py`, `/tmp/goofi-resample-check.py`: direct current-source probes for R03–R05. These are not full manager tests.
- `/tmp/goofi-wav-review.rs`, `/tmp/goofi-reaper-review.rs`: public recording API probes. Rust probes linked existing built crates; source traces were checked against reviewed HEAD.
- `/tmp/goofi-reaper-review/`: the two manifests demonstrating R01.

## Fixed commit inventory

Subjects are retained verbatim as identifiers. A listed commit was included in triage; listing does not claim every changed line was proved correct.

```text
c394490493fb37838252371e1b45faf7125cf49a 2026-09-08T18:45:54-04:00 fix(params): a param's error has a plane, and its value has a clock
9ca09928aff4669a3359b3b9128dc75299360312 2026-09-08T18:12:47-04:00 feat(nodes): one crossing per ordered pair, named for the plane it crosses from
861bf4f85a1eab5a964eb0403c54fce674f760c8 2026-09-08T18:12:33-04:00 refactor(node): a node cannot PRODUCE another engine's kind, and an input of one is a crossing
25fec8b55d463bc884d01660d62530f940cd0d76 2026-09-08T18:12:23-04:00 fix(audio): a texture enters through the one inbox every input shares
0263c165557b18689fef0185436618afd1807bb1 2026-09-08T18:08:25-04:00 feat(record): an array's .npy is flat, so a reshape stays in one file
6f6081c7b13e8e2622342a3620a30677387d5d7a 2026-09-08T18:02:35-04:00 feat(inspector): a param keeps its own control in every mode
f55738848f0f76234b0f1046a86c7779743b1b0a 2026-09-08T18:02:27-04:00 feat(ui): a slider, a toggle and a dropdown that read out without taking input
3fda92153b004dca3d12e263474623a81943cdc7 2026-09-08T17:51:44-04:00 docs(iceoryx2): the startup reclaim was already taken, and it was not working
cc5105872bf24bb5a346ef2abe01a597b34fa3ab 2026-09-08T17:51:13-04:00 fix(transport): the refused reclaim names each file, and the monitor files too
a3d99b88eedf90f01f1106896faa20937025abd9 2026-09-08T17:50:59-04:00 fix(bridge): the reducers are one port owner, so they hold one iceoryx2 node
ccda53ef2607b04d45479915d38b7aa08cad570b 2026-09-08T17:46:53-04:00 feat(inspector): a param row takes a dropped node, and asks which link it is
02107e1d3161f93be05249fe957ec4d133e80dae 2026-09-08T17:15:59-04:00 ci: the demo advances once a day, not once a push
bc0ff2d3782ecc26b0e38e5fe9c6eafd020ee614 2026-09-08T17:10:47-04:00 feat(inspector): a param row that opens onto where its value comes from
7f1cd872a083792f54760cdee968dec24c63e2fd 2026-09-08T16:46:01-04:00 docs: main is the branch, and we all commit on it
5c7026f1b0e1d9feb1617633d9310173e252747d 2026-09-08T16:40:36-04:00 fix(inspector): the filter row is on every node, not the big ones
2739e69fbdae635b4d550217c2d1bb4b30efa0a3 2026-09-08T16:34:01-04:00 fix(docker): bindgen needs libclang, which the image never carried
b9431376431788f9f77697a936c37eafd2748ab7 2026-09-08T16:14:20-04:00 feat(inspector): one header block, and filters that keep the tabs up
585ea25848d0e01457172ab3675053baa55b1bf1 2026-09-08T16:14:07-04:00 feat(ui): a disclosure that can be seated on a ground of its own
98497f29dd4e10bcc2b868d2a1244c31006ec677 2026-09-08T16:14:00-04:00 feat(ui): a segmented strip that lights a set, not one segment
1f570ccc143a4e505288910bbec62819a1228be1 2026-09-08T16:10:42-04:00 feat(demo): one instance per example, and the chooser that leaves for another
14093583226ba782c74e11da726fe8c5aaf6dc71 2026-09-08T15:59:46-04:00 fix(record): no frame is destroyed between the producer and the file
cff578591a24611122d77e6a9284014ae68616bc 2026-09-08T15:58:38-04:00 docs(skills): the emergent-shader skill is principles, and nothing a node already explains
6a4562e596f3eba8a4978cb53dde43fa0191fa73 2026-09-08T15:08:36-04:00 feat(skills): a skill rides the workspace, and a load brings the ones added since
c2d991b58830c494a6cb23daae7afaca54f3ba3e 2026-09-08T15:04:50-04:00 feat(inception): pigments that warp their own time, and the pools they sink through
a98d126aca8bed7065662a921c3f7bee10da94dc 2026-09-08T14:46:48-04:00 docs(skills): the test said less than I said it did
6cb1e27aedfd2735df7a29cb6e00c38fcd96e041 2026-09-08T14:38:54-04:00 feat(inception): the thought field, drawn in its own ink
f18849da8091893173eb6971cc8f793afb94401d 2026-09-08T14:14:04-04:00 feat(ml): a trained model on a wire, and a generator the patch steers
0d7b46bf4fb5bb39ef37b4ef4160fedf9eeb4976 2026-09-08T14:13:50-04:00 feat(init): a bundle may name packages the free-threaded interpreter cannot hold
afc1b6c67213e73892d757ec1b1171b179e659c8 2026-09-08T14:13:41-04:00 feat(graphics): a rule a folder of images can teach, and the weights that carry it
65813e7ef51eb19c9189d4f69de4127ba53cf123 2026-09-08T14:18:13-04:00 ci: Linux alone, as a matrix of one
ff8ed5d61df6601ec82fb7e12bcf5cff84919dff 2026-09-08T14:18:02-04:00 docs(skills): the difference between a shader that is animated and one that is alive
48720394ca5208593d037f57f0f580a5ed731faa 2026-09-08T14:04:02-04:00 docs(iceoryx2): a stranded service outlives a restart, because a restart keeps the uid
b985d1479e89e03ee6334a44d9d95c702600ce38 2026-09-08T13:43:20-04:00 feat(graphics): the plane divided, and the picture that divides it
52edf5990de7b0dd578512179645d90aa54b84d9 2026-09-08T13:28:35-04:00 feat(library): a save that may replace, and the file it would land on named first
e429ef77e33a7e3f74f7301e331af16e5dd9bef0 2026-09-08T13:28:03-04:00 feat(signal): a Filter that answers now, beside the one that shifts nothing
7ef085e0c5e87c0e711c0a95d1eb83c2a29c9e44 2026-09-08T13:27:40-04:00 fix(tests): a palette row, two nutshells and a clippy lint the tree already had
7f73826fa41ae4cdf32b61c9f8d69a72965679d4 2026-09-08T13:27:27-04:00 docs: the branch that is, and the files that are
c9b9b581ec2a1b78a0755255034fcc17cdafe520 2026-09-08T13:27:18-04:00 perf(graphics): a frame in the width its readers draw, one tick after it is drawn
04833e5ba5508ef0a24e24179a0f4693dd4eee5e 2026-09-08T13:26:58-04:00 perf(record): a lane per stream, and a line that says only what moved
d4dbb1b349e9c4921f69a3d2b6fe561e15ef112e 2026-09-08T04:15:25-04:00 style(inception): FluidPigments in the tree's own idiom
95ac81924a451f78ec87127167e125c5b34b926b 2026-09-08T04:03:00-04:00 feat(inception): a bundle of complex visuals, and the three nodes that open it
f93e413c63b13b5b491c6125df1454d5337a0b37 2026-09-08T04:02:47-04:00 feat(graphics): a lattice of Lenia's own, and the kernel that gives it species
60c5e514314d07f6f6cd667fe2fd17b13870197f 2026-09-08T01:22:33-04:00 feat(simulation): every model ships the picture it is read from
460ddf4e53c7a4a851b9a27475e536c2ba2c6460 2026-09-08T01:22:08-04:00 fix(graphics): the newest frame alone, where a half draws what it is handed
ed4743e486651eaafbfb3198fa10670c3a5dcc0e 2026-09-08T01:05:21-04:00 Merge branch 'main' of https://github.com/KairosHive/goofi
e754a67ebbf84592ea0f2fcfe269480e2869e2e3 2026-09-08T01:03:57-04:00 feat(record): a stream is written in the format its own shape takes
b786865d70b0a17b72fb9e434df78c27e8b8ff97 2026-09-08T01:03:37-04:00 refactor(core): one projection of a frame's meta, and a borrowed view of its samples
656675b8aeadd2ff0df637ce313aac3f140466e3 2026-09-08T00:50:55-04:00 feat(inspector): three param filters, and a zero point the reader can move
a0255b9a45984a726a176506a5f098f60f47b59c 2026-09-08T00:22:13-04:00 fix(audio): ASIO is offered in both directions, and warmed before plugins load
1fdb781d5fd3b3ba3d897bc37a7fe55a8ccc35dc 2026-09-08T00:21:33-04:00 feat(audio): a node names which of a device's channels it uses
3cd1f2adfdcc22c73c26d72a0042362715996614 2026-09-07T23:44:21-04:00 fix(record): a video that plays, at the rate the frames were made
01649551601f7248275a9a7099fdc323e6ba5303 2026-09-07T23:44:06-04:00 fix(graphics): the clock renders at 30, and the rate owns the period
2d8643627e4711faac3ce258a77a53c775946671 2026-09-07T22:11:22-04:00 The README says what goofi is now, and the image builds again
11a0a6957fc97cd92cd558ed60eb5aa98decf44e 2026-09-07T20:52:46-04:00 feat(biotuner): a signal's own peaks as the bands of a dynamic EQ
1e5755fffb6f2056cdb117748dfe5b33751c6b35 2026-09-07T16:04:05-04:00 test(audio): a chord is four voices, and the bank stands on every one
81bb1199e27a22e7b3b7e5737c5dc6c7e8325621 2026-09-07T15:46:31-04:00 feat(audio): a shape carries its own width, and a voice can be let go
d79e2bd658f5d4e8e98b0d503b565bc3051197e7 2026-09-07T14:38:45-04:00 fix(tests): the harmonic step reads a settled bank, not the one before it
8be3295d6e1c2b7e1752efc7df9c62ffb959d66b 2026-09-07T02:54:56-04:00 feat(audio): a bank can stand on the partials of a pitch
cec766d65c68af39586902226ec1c2446b8e8e2d 2026-09-07T02:42:42-04:00 feat(audio): one signal's bands are what another speaks in
7d2be867a506a447aaf905fb4fda7416c106cf82 2026-09-07T20:42:58-04:00 feat(ml): an agent that learns from every sample and never settles
4c225e6435c50ae67536cbeb33dfda6fdaf35820 2026-09-07T20:30:52-04:00 perf(vst3): the scanner's identity in a cache key is its SOURCES
53f26613a25870738930819150a76d8782be7426 2026-09-07T19:10:02-04:00 docs(roadmap): the reaper that races a stop loses a row, and why the fix was withdrawn
11ef8e9efff9330321544c5e7087a57a8e17f692 2026-09-07T19:04:49-04:00 fix(record): a frame's gap is counted with the write that reveals it
e9e328961b35bf7a264da18551850b1286fe2d3f 2026-09-07T18:33:17-04:00 test(record): zero crossings are counted per block, never across a splice
83b4ce3eef109af4a8a8aa940ce125ae90d2491a 2026-09-07T18:22:34-04:00 docs(record): the audio anchor is tied under the runtime lock
268c9742bd8f5ff69f977e57bf32d5cbfcab65a4 2026-09-07T18:22:28-04:00 fix(record): one owner closes a stream, and a stop keeps its tail
cdf83800f9e913df79a1ac3ca49c4cb722098bc0 2026-09-07T18:22:15-04:00 fix(graphics): a recorded video frame is dated by the tick that DREW it
8d989c7be0a758919c5e5ba65affa1f89c084a4c 2026-09-07T17:31:21-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into claude/simulation-node-bundle-proposals-073a70
a765dc2f99e1c59ad6b64efddc0022ae8e4e1873 2026-09-07T17:15:36-04:00 fix(library): a save never overwrites a node already in the library
7f51dae46d833898e281bc9bbc8671a3ca1a30b0 2026-09-07T17:10:40-04:00 fix(library): the private library is called `custom` everywhere
a6eca12aac0aedf4e1c7900c827832060a35fb20 2026-09-07T17:09:16-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into claude/simulation-node-bundle-proposals-073a70
dca0916935a7cb7ec8e281ff63b2a0bc0d1637f8 2026-09-07T17:09:04-04:00 test(eeg): a montage re-referenced, related and read as a graph
03ee549ff098da1aaa28f14f11eb637b0949adec 2026-09-07T17:09:04-04:00 feat(signal): a table's members, each scaled against its own past
82a1397a7a7eb43eb597f4440741461f0f8b403b 2026-09-07T17:09:04-04:00 feat(complexity): c1 and c2, the log-cumulants of the multifractal spectrum
792da0946ff565a16a2d09d0c69dc41df485708b 2026-09-07T17:08:45-04:00 feat(eeg): connectivity, graph metrics, a reference and an epoch
e07239d4d894980d8a0e7c61c5dc3a31227a721b 2026-09-07T16:50:45-04:00 fix(record): nothing holds a lock across the wait for an encoder
7b36f30993b7916b10f55d9a2481571f21760e4e 2026-09-07T17:00:01-04:00 fix(record): the ring's header is read without being eaten
7933b3472d8bc79566d3601fb0c0a2c6bcd81fc6 2026-09-07T16:37:58-04:00 fix(record): a block's number is the block's own, carried in the ring header
498e2a2b6b3b3db0481ab665f890ff4c82273b2b 2026-09-07T16:43:38-04:00 docs(roadmap): state the ffmpeg choice, not a reason nobody gave
6623dc7ae43ab918d8636a3346ed7354d6212b8c 2026-09-07T16:38:35-04:00 docs(roadmap): the recorder is built, and what it decided
310caf2fce5a88ee0b878ce44defecb197bbfad5 2026-09-07T16:26:45-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into HEAD
d2dd35af8e7666211b9a54562c66c1b57657c272 2026-09-07T16:25:40-04:00 fix(record): the running session announces itself, and an arm that changes nothing records nothing
407fca9a7eb53d4780ecef661dfb6f2cb45ee044 2026-09-07T16:22:47-04:00 fix(record): finalizing a video never happens on the render thread
9fdd577deb54c69c2ef796e6650d9c92c555903c 2026-09-07T16:14:00-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into HEAD
f71d026da014d38811625ae330da782e1cdf449a 2026-09-07T16:10:23-04:00 test(e2e): the swept scene holds an armed recorder
0b8f151604ce7c1a4df77b75cebce18b16b184bb 2026-09-07T16:03:17-04:00 feat(ui): the recorder is a panel, and the header says it is running
fd7583bcc37f3e80fb6214a94adc477a0b0804c2 2026-09-07T16:10:20-04:00 test(record): a machine with no encoder loses the video and nothing else
19b9592de17176864899208ae37fe7b226aa18fd 2026-09-07T16:01:36-04:00 fix(record): a missing encoder costs the graphics stream alone
0022f880d5ba2ab8f11ba9dc6041737378fe1fe6 2026-09-07T16:09:27-04:00 fix(record): the audio anchor is derived from the count, never from the wake
05c3ea0fca17702395de7f5680530af75d984056 2026-09-07T15:39:11-04:00 feat(record): the audio engine records through the one recorder
ab93546d7650c7417d47ead1d4087e1ca7d68b71 2026-09-07T15:54:15-04:00 feat(record): the graphics engine records through an encoder
b7959d29e297a2cb0c6c442f2d2137a923e4ab82 2026-09-07T15:51:12-04:00 fix(record): a departing feed is read to exhaustion before it is closed
f11b2454afeaec653a5cde74db8ee13b1fb25558 2026-09-07T15:13:52-04:00 fix(record): a frame the subscriber overflowed is counted, never silent
7e96175bd69889a8fc46ca82577a156fe4475ece 2026-09-07T15:24:12-04:00 fix(record): the budget buys depth, since a slow reader loses the oldest frame
2fc7a1f9aed1f689feae4609da35ddf320c1eb85 2026-09-07T15:13:52-04:00 feat(record): one thread drains every armed signal stream
28e926fc4cccba02c349fec46cab564b28afe285 2026-09-07T15:16:24-04:00 fix(record): the recording segment is a stated budget, and a lost frame is said
a4d8aab60cd571c3643b93d0bf23600330c6451d 2026-09-07T15:00:53-04:00 style(record): the load's comment states the deviation in two lines
004040d917147bdc01026247691c3394cababe27 2026-09-07T14:56:30-04:00 fix(record): a stranded recording is said, not a reason to refuse the load
b40bd96d4abee47d307480e9219d6784b7015265 2026-09-07T14:53:57-04:00 feat(record): an armed signal slot publishes its bytes twice
bfa7dbc5c272233ec584fd8c87a7bfca96b0b43c 2026-09-07T14:51:04-04:00 fix(record): the session lock is the one authority on a running recording
8ae8ade79230cfc25a967666bf00217d8d5bccd8 2026-09-07T14:45:00-04:00 feat(record): arming is the node's own record, and an op group
d37976398bf17e6c0740f5ff4f4fc6af3186d1e6 2026-09-07T14:43:51-04:00 fix(record): a re-arm mints a new file, and one owner holds the start
87c684e215ee64047e686eed661b550e7be4e1bd 2026-09-07T14:35:27-04:00 feat(record): a recording is a folder of frame streams and one manifest
66ff1360a3bc4412424c1ea358cf0a1a402b28a4 2026-09-07T14:29:27-04:00 feat(time): the clock reports a UTC anchored once at the origin
51036f4cedb3a3cae411c73c02aff3932c4b38d8 2026-09-07T13:17:53-04:00 test(graphics): the cascade concentrates the field without dimming it
4a568be053c4089a0da757b7999e751de4a508d8 2026-09-07T13:17:43-04:00 feat(graphics): noise folds its octaves six ways, and one of them is a cascade
1cbe888aa78ae68ff9d83d5ff093cc2babe6aa26 2026-09-07T05:43:57-04:00 Merge branch 'rust-rewrite' of github.com:dav0dea/goofi-pipe into rust-rewrite
1bbbc0af5beac368b15fcf4bd75e4437430076f9 2026-09-07T05:43:51-04:00 feat(graphics): the plot is drawn in the shader, and the engine hands it the range
430e4ff61f32408819eb17201eb48c87cd2a3ce7 2026-09-07T04:26:28-04:00 feat(graphics): a texture with nothing to follow is 1024 square
8be078d5eb83eed267bf864ca2be69fb1fd907bb 2026-09-07T04:18:30-04:00 fix(audio): a device is opened in every word cpal has a type for
f9883c97fa4e55854bdd8857dfa1cb472eecd3f5 2026-09-07T04:18:30-04:00 fix(audio): a device is opened in every word cpal has a type for
5f7766b4005f797af048cb008d6d11499466a660 2026-09-07T04:06:39-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into claude/simulation-node-bundle-proposals-073a70
8b56f778ac3f0f7cd3b28e648dc880f1a30c4b2d 2026-09-07T04:06:23-04:00 docs(roadmap): the simulation bundle, and what it deliberately left open
3e227b3cbdeeb41779b175ea6eace940b0bbf913 2026-09-07T04:06:15-04:00 test(simulation): a model is judged on the behaviour it is named for
e50595e79f6d5671d1b326fa38be9fda65d9010c 2026-09-07T04:06:04-04:00 feat(graphics): seven automata that keep a grid of their own
9e25a37a20c1f3e45cf4664f1172bba5960dc15b 2026-09-07T04:05:54-04:00 feat(simulation): a bundle of models that make their own dynamics
f0b2b454ff6e9f857491ea1ca90c5b5c4ee62c73 2026-09-07T03:56:33-04:00 feat(graphics): an ARRAY input's transfer has modes, and they are the viewers' own
0616113dfaeb271e4b2680dce327a67acebb0e8a 2026-09-07T03:01:22-04:00 feat(ui): the add-node menu opens on the tab its opening asks for
bd4e7624d718262a81dfda4d388eeb3145803d66 2026-09-07T03:01:16-04:00 feat(ui): Escape leaves a maximized panel, once nothing nearer wants it
b871b9ef4a7283b4464b7643171bf5069f128408 2026-09-07T03:00:26-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into claude/simulation-node-bundle-proposals-073a70
d6a6164ce83adab563cfe82af993ba8e00431d2b 2026-09-07T02:59:16-04:00 feat(graphics): noise speaks the vocabulary a visual toolchain already taught
327ca025919f7db3ffb16f6bcf9977b548fbaf83 2026-09-07T02:59:06-04:00 fix(graphics): two docs still named the size group by its old spelling
b91c1e2cacffe76b461e22a63dc69eb84d72382d 2026-09-07T02:53:04-04:00 fix(tests): a stamp moves when the BYTES move, not when a file is copied
86f497857f1802dfa92d07f38781cc4aba9154aa 2026-09-07T02:24:44-04:00 fix(nodes): a measure per channel says which channel it measured
12a10b9f14c370e8efae3d79349d5468a933ec69 2026-09-07T02:21:18-04:00 fix(ci): the Windows libclang step is one step, not two
2807df77ea31e59d42095bbf5722a7c55add9a1a 2026-09-07T02:17:21-04:00 feat(signal): a drawing widget's picture leaves the pad as a frame
156b7ecfad0fd7aecbaf59e51b714552b0120f9e 2026-09-07T02:17:10-04:00 feat(control): a turtle script is another hand on the pad, not a second painter
f833100d58c74ad688f26c87a59c91ad86591ad5 2026-09-07T02:11:33-04:00 feat(audio): ASIO is compiled in, because nothing about it was the user's to supply
826a779fa8b478c1111432c20b0addafa9b40dba 2026-09-07T01:56:12-04:00 feat(audio): a build says which audio APIs it carries, and which it left out
401ac5984d8eb33450ff5cf79d8e05b9e4c0b4e2 2026-09-07T01:48:59-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into claude/simulation-node-bundle-proposals-073a70
7a9d46a6cc5ff899a61c4089e74c933fb9320ca1 2026-09-07T01:43:49-04:00 fix(cli): a port that is taken names the goofi that holds it
daaaaa19be0fbebe675e0bea41045fc86ce12c63 2026-09-07T01:43:44-04:00 perf(vst3): a scan is remembered by the binary's stamp, refusal included
074c0eab30fc484cf041af8000beb191660c8fca 2026-09-07T01:43:24-04:00 fix(vst3): a Windows plugin finds its own DLLs, in an apartment that is open
e854deb0b789a4cb28b4e609ae6ec835dd76c946 2026-09-07T01:26:39-04:00 feat(graphics): the size is a param, and a node keeps buffers of its own
0c604998de99a85a52b8e1936ba49b9641c5ee0c 2026-09-07T01:25:58-04:00 fix(control): a panel's edit mode is the panel's own view
d473d1a65a41ac264c91501b1970744e9f8e8763 2026-09-07T00:24:20-04:00 fix(transport): a thread that opens a service gets a stack that can
28b6b71e40b405be53732b3a85e260f663aded3f 2026-09-07T00:24:10-04:00 fix(bridge): an accessory never widens what an engine makes
d251642d0f692e1ac4171f5405cc4741d9c2991a 2026-09-07T00:23:58-04:00 perf(graphics): the GPU makes each reader's own frame, and the render thread never waits
d5e277b8a73e14491e224ec51dd5fa4b6d6f8994 2026-09-07T00:23:46-04:00 fix(graphics): the noise hash mixes integers, so a cell boundary is not a seam
4a8d063bd1860c59e543379d8925455963183e7c 2026-09-07T00:15:11-04:00 fix(init): the audio libraries are a precondition, so setup names them
ceb71af880f4569a23a7bd0a0f68571cdad619cc 2026-09-06T23:48:48-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into HEAD
be7c841afaaccd04aaec2ab12417cdd9f3207fa3 2026-09-06T23:46:34-04:00 Merge remote-tracking branch 'origin/claude/py-import-serialize' into rust-rewrite
6995541af700f530c41cbb68e0c38a5a4733b772 2026-09-06T23:44:41-04:00 docs(roadmap): the host is the user's choice, and the name is where it lives
28106e456acf0f036ee8e253f6a60848a985429e 2026-09-06T23:44:41-04:00 feat(audio): every cpal host the machine runs, and the device name says which
28846e2160d674ed86e0f43a858e2b1dd72a8542 2026-09-06T23:24:46-04:00 docs(roadmap): time is built, and a clock is the thing that comes later
2859bfd81fc42a460c1e570da66daf16116638b0 2026-09-06T23:23:49-04:00 feat(lfo): phase is a function of patch time, so two LFOs run together
e3970e57eb99439426097e9b7041cb61f5f8bc9b 2026-09-06T23:23:40-04:00 refactor(time): one patch time, and eight holders become one
47bbaacf8aa994a63ad2ee1da7bf01f5a916332f 2026-09-06T23:22:04-04:00 docs(roadmap): pipewire-pulse gives the mixer, and cannot give priority
d0fb56baba7ea5a228217327e2d456a187be9eb6 2026-09-06T23:22:04-04:00 fix(audio): the callback thread asks for real-time priority
7303e773b9411f903769dd57496522a147ead16c 2026-09-06T22:56:53-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into HEAD
a922bbd7dd8bc38dafcd5ca9c9d5fba756811d34 2026-09-06T22:56:47-04:00 fix(test): the parallelism scenario times the runs, not the loads
4123af15c572135380124eeb1c9815efc41738f3 2026-09-06T22:55:10-04:00 Merge pull request #27 from dav0dea/pr/asio
e939fe3adfc527dbed4d68ab3f49b132a480f3ad 2026-09-06T22:11:24-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into rust-rewrite
114b9b4219a799797e48a8425f40fff4494125e8 2026-09-06T22:10:20-04:00 fix(python): a poisoned body lock is taken, not spun on
7a86da5d3bd7cef90b04a0b0dd8e3f52a5ee352e 2026-09-06T22:09:43-04:00 Merge remote-tracking branch 'origin/claude/meme-parity-nodes' into HEAD
0d74f77bf01d50be6ca13cb8f1c11f683d7bdfdb 2026-09-06T22:09:42-04:00 Merge remote-tracking branch 'origin/pr-vst3-tuning' into HEAD
2826e1bf719b81323aaecad4871c15b9a69b9802 2026-09-06T22:09:42-04:00 Merge remote-tracking branch 'origin/claude/py-import-serialize' into HEAD
f02b29b381623943b6b9bf772f1206752be1b84d 2026-09-06T22:00:15-04:00 feat(examples): the main-branch patches that the rewrite's node set can express
2f5e24729bd39e9eff7934484ef45d8bdac4b892 2026-09-06T21:57:57-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into HEAD
aef4d94b248d46975cc08fdd2f6fc52901fef56f 2026-09-06T20:59:56-04:00 fix(test): the gate's fall is a state nothing sees undriven
565f969e9f00af9181632664990e27d41741ded4 2026-09-06T20:28:52-04:00 test(vst3): a fixture that can be detuned, and both spellings of the detune
80d203031df7b093d3decee6661fefb409bf2ef5 2026-09-06T20:28:44-04:00 fix(vst3): a pitch between two semitones reaches the plugin
905566c62ba09499f2f4c148fb9ced7c09c0db96 2026-09-06T21:37:46-04:00 docs(roadmap): one Windows failure was never this family, and the file said it was
b82dfc6e61f179acac613da069c86d69ce03c5bd 2026-09-06T21:37:03-04:00 test(subpatches): a refused rename is judged on the expressions, not on a clock
4ef70799a676a23541297ddf12fa41cbc04bcb1a 2026-09-06T20:58:47-04:00 fix(python): a node body runs alone, so a cyclic package cannot deadlock
b9a55519cf445ee7f342667aa6513f24575b669a 2026-09-06T18:08:59-04:00 fix: untrack two worktree symlinks that never belonged in the tree
9c2ed2334144b96a39ed000a5efbd740043fc112 2026-09-06T18:04:20-04:00 feat(globals): an engine publishes its own facts, and `machine` says what it does
5653a59b0d104c43718d2c0b304b0ae6a41dea20 2026-09-06T17:20:47-04:00 docs(roadmap): the backlog holds what is unbuilt, and git holds the rest
91e6492303608d19c3ca4160b38fd19ec674de66 2026-09-06T16:41:08-04:00 docs(roadmap): recording and its clock are one item, not two
aac174b8e44b92e1baa88d2290122034f2bcf1e9 2026-09-06T16:35:41-04:00 fix(audio): what ASIO found — a wider device, a slower open, and a list that ends
bd2db3e72b0009e0c27b3ed70623cbb550b6cfd3 2026-09-06T16:07:00-04:00 feat(audio): ASIO, as a build the user makes, and one driver for the patch
54802dd5cb2242f72f067f17f956804626ca1c92 2026-09-06T16:32:38-04:00 docs(roadmap): what a boot actually costs, and the reclaim that does not work
b628223739157722009c4dc5255a367e0d5d5686 2026-09-06T16:02:51-04:00 feat(biotuner): a tuning is held until a related one arrives
88f2ce35741931ea0472df02ceb017f3521817e8 2026-09-06T16:02:39-04:00 feat(nodes): a state you have to hold, a value you can catch, a set to land on
d2c81098a02773d8d912fe5113368acdc454b0f1 2026-09-06T16:02:20-04:00 feat(nodes): a clock the whole patch can share
b8cc02d0020c643684616f13128072698d3dd61e 2026-09-06T16:02:09-04:00 feat(nodes): Math says what happens to a value the range cannot hold
3c3b4f85c07649a153eb1782c546e2365f239ea3 2026-09-06T16:22:57-04:00 Merge remote-tracking branch 'origin/pr-biotuner-rhythm-color' into HEAD
c7f9e4b471eaac77de8563b4f5f1ecbb7d79a7ad 2026-09-06T16:22:57-04:00 Merge remote-tracking branch 'origin/pr-biotuner' into HEAD
3e55aed0ba252540c417723a7eecf27011b93d92 2026-09-06T16:22:57-04:00 Merge remote-tracking branch 'origin/pr-audio-watchdog' into HEAD
62813701d1c9e63f26e28b5488e16f6a96eca082 2026-09-06T16:22:57-04:00 Merge remote-tracking branch 'origin/pr/audio-input-format' into HEAD
5e15915bb281c81fa5e563dbc52afa5ff87756a6 2026-09-06T16:22:57-04:00 Merge remote-tracking branch 'origin/pr/control-widgets' into HEAD
cdf45dccaa08c265c3af4a47291deaaab589befb 2026-09-06T16:20:03-04:00 docs(roadmap): Windows is accepted red on iceoryx2, and no lever here is taken
327b31b665152fe247417cea3e995bd7c8a274f0 2026-09-06T16:17:33-04:00 docs(roadmap): the spliced window was seen ONCE, and the record should say so
b5d1b2db67e4d02c6b4e27db2d466ed9d4d63f14 2026-09-06T16:13:10-04:00 docs(roadmap): recording, frame time, and the panel add-on door's decisions
66b406fca5f88d8eaf60356b27d0fdbf20a50586 2026-09-06T15:53:34-04:00 Merge remote-tracking branch 'origin/pr-biotuner' into pr-biotuner
ca91da13feef2aa6b954a9bda21e963c77d3f917 2026-09-06T15:53:18-04:00 fix(nodes): five findings from the review, and one input that had to go
019bec9bffaf65db88aefc14d5e4fd60e22e0532 2026-09-06T15:51:22-04:00 Merge branch 'pr-biotuner' into pr-biotuner-rhythm-color
9d3f2d691c676e80d7a08d3eeed2d781bac2c761 2026-09-06T15:51:21-04:00 Merge branch 'rust-rewrite' into pr-biotuner
5dccb8e1905b88a2e32e53fa4b39b50a184e4955 2026-09-06T15:51:20-04:00 test(running): the pacing bar follows the load it is judging
c5069ea0292ed6fdc012f673a7808197a0fa5212 2026-09-06T15:33:42-04:00 docs(roadmap): the third platform bug this round, and what stands behind it
61c5e75d1de8554406976454814225bedc6e5338 2026-09-06T15:29:29-04:00 Merge branch 'pr-biotuner' into pr-biotuner-rhythm-color
a8aa9fec6f2e7e679864d02ef3a6c7af928b83cf 2026-09-06T15:29:28-04:00 Merge branch 'rust-rewrite' into pr-biotuner
97b75c86fd5739b9e05e5745974073cb93056db5 2026-09-06T15:29:20-04:00 fix(globals): a global with no source has no follower
3c77743fee2ea49420f64eaf770db8e8a8cce515 2026-09-06T11:53:52-04:00 feat(control): a field is a text area, and a drawing is a widget of its own
ca138177643af1db0321efa31cf9cfbc98834933 2026-09-06T11:56:43-04:00 fix(audio): an input speaks the driver's format, and a rate refusal names the rates
b2c601bb11a85c07fb074ffffbb73c58fd7e4faf 2026-09-06T15:14:45-04:00 docs(roadmap): Windows can deliver a spliced window, not only refuse to start
2bad7b75826b5fe04126c15436072f7881280532 2026-09-06T14:55:28-04:00 Merge branch 'pr-biotuner' into pr-biotuner-rhythm-color
fad5afba1a5dbfef39db296f1c4dfae33e2fb609 2026-09-06T14:52:19-04:00 Merge branch 'rust-rewrite' into pr-biotuner
1af23703180e6b30e27f37c33c05f0024ecb1e64 2026-09-06T14:52:13-04:00 test(audio): the take's head outlasts a command that lands late
d84eeff7120f1d4c9295e4d9e3a22568abf748da 2026-09-06T14:41:08-04:00 Merge branch 'rust-rewrite' into pr-biotuner
41db0103e8f41bd16ec4ba0fa582b943a8c61a3d 2026-09-06T14:40:32-04:00 docs(roadmap): what the three platforms are red on, and what is upstream
97535def8ae6e476dbdeecf1ff89ecfe60a5acc6 2026-09-06T14:40:32-04:00 fix(e2e): a panel's type is the fifth leakable global, and touch hands it back
c1fe4893c3547ff521ec501d7d0bfac161584a87 2026-09-06T14:40:32-04:00 fix(audio): a wrap is not an end, so a looped file falls silent when it stops
385ca48b61cedf4dc5a712a90147a92868656834 2026-09-06T13:22:05-04:00 Merge branch 'pr-biotuner' into pr-biotuner-rhythm-color
45bcfd2cd8301bda5427097c166f9b7f9b7715c4 2026-09-06T13:21:57-04:00 fix(nodes): a nutshell fits, and a node waiting for its producer is not a node that failed
d9e5d85ffd7ecdee7f2f785eab80e1b9ed8b88c5 2026-09-06T13:10:26-04:00 Merge branch 'rust-rewrite' into pr-biotuner
b6eb98ab3b64e704cc59bb95408e59a5236943d9 2026-09-06T13:10:18-04:00 fix: an op is answered, never waited out, and a case-fold pair needs a filesystem that has one
eb9a9f151a5fb00ab25687ca66313b96acc05b09 2026-09-06T12:43:16-04:00 test(nodes): the rhythm, colour and element nodes get a scenario of their own
bf1786bd0cecb4c3a1ded745b3043425659688d0 2026-09-06T12:36:55-04:00 Merge branch 'pr-biotuner' into pr-biotuner-rhythm-color
8bec9ca2844d0c3337739c4b13b0b5e479708db7 2026-09-06T12:36:49-04:00 feat(nodes): the biotuner bundle names its package, and a scenario drives it
49453866992994d0acb48a9766c87aadc9e68ec2 2026-09-06T12:33:13-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into pr-biotuner
da9cb0358a14c2031b3e6de9d26321179eecdab9 2026-09-06T12:33:01-04:00 fix: the audit's open list, closed where a machine here could judge it
fd2648967217eeb614fb9651b75528f243104d64 2026-09-06T12:04:56-04:00 Merge remote-tracking branch 'origin/rust-rewrite' into rust-rewrite
6dd2b805c08028b2a3ad0f3f7748b621a1aa48bd 2026-09-06T11:58:45-04:00 fix: what a three-way cross-platform audit found, and what it left standing
9291902d43ac17ea6625ae16ab047592b5f19537 2026-09-06T11:52:27-04:00 Merge branch 'pr-biotuner-rhythm-color' into rust-rewrite
8c2349cd2f6c9865b60c32fb4cfa8ea72563e4bf 2026-09-06T11:52:24-04:00 feat(nodes): BioColors keeps the original physical reading beside the perceptual ones
fbf8fc07fad8d8537852acd0619d28a42546c1d6 2026-09-06T11:31:15-04:00 feat(graphics): a Window node puts its texture on the machine's own screen
79f75eb579ab27ac13ab4ced826291d196a06e1f 2026-09-06T11:05:07-04:00 Merge branch 'pr-biotuner-rhythm-color' into rust-rewrite
f8cb506e88eda4051996b8422536d257d401afc5 2026-09-06T11:05:05-04:00 docs(nodes): the same move for the rhythm, colour and element nodes
d54eb5a9e5f01811845271536dc409dd4cce04f8 2026-09-06T11:05:04-04:00 Merge branch 'pr-biotuner' into pr-biotuner-rhythm-color
4586a66f6c228f28322831875cd7ed6ae5bb5469 2026-09-06T11:04:53-04:00 docs(nodes): the slot reference belongs where goofi actually reads it
c6ee69e9a18be088e94ab71b73c5bf71bdeca647 2026-09-06T10:55:20-04:00 Merge branch 'pr-biotuner-rhythm-color' into rust-rewrite
a452c3a94452480e8f2405bd35429a67fb779d47 2026-09-06T10:55:07-04:00 docs(nodes): the rhythm, colour and element nodes name their inputs too
2b6d7e61af15e6547d7f9bec93bbbcb6fc732510 2026-09-06T10:55:06-04:00 Merge branch 'pr-biotuner' into pr-biotuner-rhythm-color
b539b75dc4c1ec0b5dcb3a30d35f4bac7cbb7625 2026-09-06T10:54:53-04:00 docs(nodes): every node names its inputs, not only its outputs
56f964e4c6fcc0b6bef9c753ef5e484a864ea491 2026-09-06T10:49:48-04:00 Merge branch 'pr-biotuner-rhythm-color' into rust-rewrite
29f3c4a67a4cc0a3a155b54ba78d594288fe32e8 2026-09-06T10:48:20-04:00 feat(view): the wire serves at the rate the app paints, and the browser says so
c1f4345a70f0688c0dababd6b808371a6b9726e3 2026-09-06T01:40:01-04:00 fix(python): the probe memo keeps answers, never failures
faefd8428a9ca31d54c296259a591185380daffd 2026-09-06T01:34:28-04:00 docs(roadmap): a later directory does not win a shared type name
62d40477a8dbe7c243b0f34f6677a1f1b28655c7 2026-09-06T01:25:04-04:00 fix(audio): a node costing about one block is heavy, not broken
7262a22b3b6a8fa3f1093a96a5ff15cf504b6374 2026-09-06T00:48:21-04:00 docs(roadmap): the signature that blocks every later step of the audio scenario
05e1c488dea3187e860c204e5d5990dd401666aa 2026-09-05T23:21:09-04:00 feat(audio): the pitch wheel bends the notes a keyboard is holding
10f3144d476ba4503ebb1594bd725435c8047e0f 2026-09-06T06:52:43-04:00 fix(graphics): the program audit's findings, each as a rule
78cf66629a5591258bdafafafc8f1ef6f9bdf416 2026-09-06T06:19:34-04:00 docs: the graphics suite needs an adapter, and says which package
592f451b5919dfe9718c7e3ec5dce61617dc6c85 2026-09-06T06:18:48-04:00 docs: the orientation names the third engine
9edd872aa339d3478bf82f1462e4ccbed6c676b7 2026-09-06T06:18:08-04:00 docs(graphics): what the binary measured on real hardware
ba2a944817bc24cfcea798c21a3178734d3e4dae 2026-09-06T06:13:21-04:00 docs(graphics): the roadmap says what was built, and what is still open
ef5e98c695f347822dba4e22bd5557053a683984 2026-09-06T06:12:59-04:00 feat(graphics): the timer clock has a test, and a reader is told the graph's dtype
25e923b20be5d594d26bd3c0d8082c7412e878d2 2026-09-06T05:59:06-04:00 feat(graphics): the shipped set, and one device the whole process takes a turn on
adb304367bf2d67793fd288c88cf47d0869704fa 2026-09-06T04:25:41-04:00 fix(graphics): uv is top-left, so a pass-through body is a copy
6e475cdbaedf1f6f632384e894ab4c62374ab3bd 2026-09-06T04:16:52-04:00 feat(graphics): the shader tier reaches the health pill, and CI gets an adapter
0d23cef5d10d74323e4537f4e325a6073901d117 2026-09-06T04:14:00-04:00 feat(graphics): a wgsl file is a node, and a chain of them renders on the gpu
6690502040c4108930d463113551a6eee7929f60 2026-09-06T03:43:29-04:00 refactor(control): the control half of a scheduled engine is one crate, and audio keeps only what is audio
4ec336df9b2a8ce7d99d593514b8ae1186125947 2026-09-06T03:33:05-04:00 fix(view): a texel is not a value, and every reader of the folded stream now says so
614977352b63af909171578314532ee9664ebdbb 2026-09-06T03:16:52-04:00 feat(core): SlotType::Texture crosses every layer, and a texture feeds a texture or an array
7a8bdbb11d88a5d3b74e9b6d916202f9c7c417f8 2026-09-06T03:08:36-04:00 feat(view): a viewer that draws 8-bit texels is served them, and one stream folds to what every viewer can draw
005b265e6be2615ac4e59519aa7a6706eb404732 2026-09-06T03:03:47-04:00 feat(nodes): a signal's colour and its elements, at the width the libraries have
6399e2a2f50ec12a1433c0e66430868b3a48c004 2026-09-06T03:03:47-04:00 feat(nodes): a ratio is a rhythm read at another rate
2ae71973e2e1f5a3f5fd3e625f412462c51e7843 2026-09-06T02:55:28-04:00 docs(roadmap): a VST3 note-on rounds the pitch, and the tuning field is where the rest goes
31473e9be798201592068038359497f47a38f00a 2026-09-06T02:48:50-04:00 docs(roadmap): the graphics engine redesigned as a .wgsl node engine, with the uint8 viewer hop
1c746aa6121ea4fa56f85490715b2e435aeb30d9 2026-09-06T02:27:05-04:00 feat(nodes): a tuning reaches a synth three ways — a preset, a stream, a keyboard
6192b3eb0f8f3c945d402831dbe69fde07265ff3 2026-09-06T02:26:48-04:00 feat(nodes): a tuning is one of several constructions, not just the peak ratios
7c1ee194703176ae718745e01e8cb687a0347fec 2026-09-06T01:54:15-04:00 feat(ops): a node is addressed by its name, and every read answers one
6b9629803cb1845359eca277c0d9c2830feb99e6 2026-09-06T01:23:46-04:00 feat(ops): a node doc opens with a nutshell, and a list of records is a line each
1fa164adee86b7e0dfe3362b283381a0a1e43a41 2026-09-06T01:07:22-04:00 feat(audio): AudioPlayback reads a take back, and a file keeps what a device drops
4a95d0dfe0b30849c90a7d31ad1d0fafe10cbb8c 2026-09-06T01:06:15-04:00 feat(audio): a take is params on AudioOut, and the blocks in the ring are the request
0122f8e2e638c9d2f13cebecc541129738f57d19 2026-09-06T00:47:29-04:00 feat(ops): a read answers an index, and the detail behind it is a flag
5dce6ba1e2255eed07ade967a8a615d7bbcb3038 2026-09-06T00:38:44-04:00 feat(nodes): a buffer told an axis past the rank grows one, so a vector buffers into channels by time
94ed57d379cfce74f440a4af527057736336d00b 2026-09-05T23:39:29-04:00 feat(control): learn lives on the widget, and a node is picked from a list that typing filters
7d5f8aa38892b75aab49fb3ada439a63abea810e 2026-09-05T23:35:54-04:00 feat(inspector): the pulse button is a word, and the number gives back a rem
15838079d2be6a7c54ff74a2e7ed8a6ea9f6ae80 2026-09-05T23:33:48-04:00 Merge PR #20: VST parameters, a keyboard cable, and two crashes
3d8a79a16dbde981e14059a22251727a76c07947 2026-09-05T22:04:52-04:00 feat(nodes): a biotuner bundle — peaks, harmonicity and the tuning they make
0d02fc04a37ca30e46cf74df5fa75cb457d2e708 2026-09-05T23:12:07-04:00 feat(inspector): a source is C, E or R, and every switch says the same kind of thing
755be60fc84373b552aa034631dfe637a59503ad 2026-09-05T23:10:49-04:00 fix(audio): the macOS window host names every feature its imports need
e89e3af2799d4523ffa80c41f96b290c62c690b7 2026-09-05T23:10:49-04:00 fix(audio): the Duration a plugin's runloop needs is Linux's alone
bd62eaf93dfa22ad476ffda662e821e6267c085e 2026-09-05T23:05:20-04:00 feat(control): a node dropped onto a widget links it, and the source switch is one strip
6b19c18675f03aa7abb62e46f4ac2c400b17fda5 2026-09-05T22:43:33-04:00 fix(control): a drop lands anywhere in the board's scroll area, and the ghost snaps to its cell
c709b6b2082720bf0d8640c1c765ebbaf39627ac 2026-09-05T22:42:12-04:00 feat(inspector): a param is one row, its source a thin strip, and a pulse fills what it can
2135fa284901c2b369c25bba97e6b9a215be0811 2026-09-05T22:40:19-04:00 refactor(signal): the one node whose struct did not match its file
e530c5683587dc72f42ca405cf49b1a9d2e8edac 2026-09-05T22:39:03-04:00 fix(control): one field per row in the widget's form, and the delete on the widget's corner
b44afb8c3a06def73fec39e50b56ea4591ce99e8 2026-09-05T22:36:52-04:00 fix(control): the widget's form stays open under a press in another panel
fb7b64396d59e23c1acea6104610d03d7472dbe9 2026-09-05T22:32:39-04:00 feat(control): the palette in a strip above the board, the form in a popover on the widget
6ac3b05f4919947d25f1fe35819d36cea67870b7 2026-09-05T22:19:00-04:00 fix(editor): a delta mid-drag no longer empties the marquee
030af8b824e3382112445dea4c223f7fcc5fb97f 2026-09-05T22:15:02-04:00 feat(control): a widget follows a producer, an inspector pane edits it, and agents have a door
56158119764fb87a4c22d4bbb6d545b86fcc4c7d 2026-09-05T22:14:27-04:00 fix(vst3): a plugin's program list is its presets, and it was filtered away
8bf84f977f502c2bfeb91c728a61880c27ce3a1f 2026-09-05T22:04:40-04:00 feat(audio): one cable carries a keyboard into a plugin, and the pedal is heard
b5d219ced28e49d7cebe7dd58c12df9cbf9cf902 2026-09-05T20:25:28-04:00 fix(inspector): the touched filter searches within itself, and never strands a node
7da4e71f121508b581c558c4fbae0bef7f9ebd74 2026-09-05T20:25:28-04:00 fix(graph): a load rebinds every binding, not only the ones naming a port
c4f8f92a41b294eaef79cd2a4d9b78b6a1ea2cba 2026-09-05T20:25:28-04:00 fix(audio): a control half ticks before its params arrive, so reading one must not panic
5525dfa6810d1411b92944d1ba141b8482873d00 2026-09-05T15:16:42-04:00 fix(inspector): touched-only spans the groups, because the tab is what you do not know
2c8fd28fb3cb701e5d3b498c3939477b4c7aad06 2026-09-05T14:47:19-04:00 feat(inspector): a touched-only filter, so a plugin's hundreds collapse to the few in play
c7a1c7bedfa85c89fe5303a38b4eac150b15ef7f 2026-09-05T14:47:10-04:00 fix(audio): drop an unused import the editor left behind
9c30da731f5ed2408fc3c5f426c060a7b856f2c8 2026-09-05T14:47:10-04:00 fix(vst3): a param the plugin cannot automate is not a param goofi offers
c2dfcc2f038809d79f35f7aef3680d73fe034bd3 2026-09-05T11:53:02-04:00 feat(audio): a control-rate param costs no port, so a plugin can declare thousands
1c05e1abcf7af4e6212a76474f3ea7d1725b50ab 2026-09-05T11:23:33-04:00 feat(inspector): a search over every parameter family at once
ca352421a2126804b69ba52658c2d62206c8897c 2026-09-05T11:19:28-04:00 feat(vst3): a plugin's own parameter families become the node's param tabs
2433ec77bf0cd26e7b317b649b3bda0e0d4b8ebb 2026-09-05T21:51:45-04:00 feat(topbar): the public instance says so
983d13a81d0e0fe0db298819344e1913c6832a26 2026-09-05T21:32:09-04:00 feat(control): a new control panel is born naming control0, and no panel waits on a name
b98518b1c86be19ed296764ca0d281f159b2689a 2026-09-05T21:30:12-04:00 feat(globals): a lock on a group or an entry, and a control panel's edit mode is one
f0b91342119f6c7ea68485d38e97477f15ab9e35 2026-09-05T21:17:32-04:00 feat(control): the ghost is the widget, Delete deletes, and the grid is sixteen wide
b26c6f8a8aea5984d95d4dc33794aff2d1e545bb 2026-09-05T21:17:32-04:00 fix(nodes): the shipped tree is keyed by its embed, and a type names its bundle
b5c6db4ff1aa424779a30cf9abff66ee8ee955dc 2026-09-05T21:08:00-04:00 feat(palette): a row names the bundle it came from, and a search finds it there
b943c5b112f2ee8d6f365c50b628357d24536a34 2026-09-05T21:04:03-04:00 fix(control): a panel with no group can be named, and a palette bears the widgets
e982742d750b3e984b4ecf110c4a20fcf739446e 2026-09-05T20:21:39-04:00 feat(palette): Tab walks the engines, and the field never loses the caret
1002f51c5e23f553ba3829a4dab7edaff912fc23 2026-09-05T20:11:57-04:00 feat(palette): one filter, and every engine tab in its own ink
977e2f1bf7c8bf99595038acc6b8d3eb1aeb8409 2026-09-05T19:37:00-04:00 fix(inspector): the resize band no longer covers the first param tab
ea9420f8a4566462c94a692f27a9dfdd26675563 2026-09-05T19:19:19-04:00 fix(signal): the rate cap reads the global by the name it now has
3265305aea9f8333c6610d62311d2b132c5f0f9a 2026-09-05T19:16:06-04:00 fix(frontend): a group nobody has toggled is closed, not undefined
a41f17a10ba591f967b0e0740212c46aded34aa3 2026-09-05T19:03:37-04:00 test(e2e): the browser suite meets the renamed slots and the grouped globals
2174c6e01eefab3967b7e3e78e2b457d7c1c02f0 2026-09-05T18:46:23-04:00 feat(audio): seven new nodes, and one gate rule across both engines
407ef3227f6eaa40affaf0e2dcea31fda971676f 2026-09-05T18:40:03-04:00 test(e2e): a control widget turns under a finger, and moves in edit mode
8541feb70a92dad95306ca6244b5f52fffd1084c 2026-09-05T18:29:08-04:00 feat(nodes): the core's Python half — resampling, eigen, and the four I/O pairs
ae30385147e2fb7bf15e9be1f1e65ad4aa9ebd2b 2026-09-05T18:28:16-04:00 feat(frontend): a control panel draws one group of globals
f0fcb6f74a9d48ad93fbc6f663e247133a236118 2026-09-05T18:14:03-04:00 feat(frontend): globals are grouped, and a control-owned one is read-only
d294a3660fb40fd9e19ab1ddc04c3c14aa929f66 2026-09-05T18:08:11-04:00 fix(build): the machine's fetch settings reach the nested node build
d3033996a3366eaa341ca54481a93699dda2d102 2026-09-05T18:08:11-04:00 feat(nodes): the spectrum by segments, and the text and table families
3d1113039368809a984ea0e5dabb248650835418 2026-09-05T18:07:59-04:00 refactor(nodes): the main input is `input`, in every node and every caller
b949bdf0225c5095969f514670d48650b86094ba 2026-09-05T18:06:15-04:00 feat(globals): an entry may carry a control record
e48118cb6e70089ae8f4940d001bb95a46a9da08 2026-09-05T17:54:34-04:00 feat(globals): rename an element or a group, and every expression follows
f7c9582f01f7085f2960136fc73042b9c63241db 2026-09-05T17:50:18-04:00 Revert "feat(demo): an untouched demo goes quiet rather than billing all night"
b4a8684d94df13eb353d8ca622db835221168ab3 2026-09-05T17:49:37-04:00 feat(nodes): the filter runs both ways, and scipy says what comes out
c35b6c8959cfd5fb07d82016c003293e51caae3e 2026-09-05T17:45:55-04:00 feat(globals): a global's name is a group and an element
65d434b22d8e1122c82bde05d9eb8577b200f771 2026-09-05T17:32:51-04:00 feat(nodes): four analysis nodes, read against a sine whose answers are known
db483ce7d94f47f3d69b160aba5f8543bd1fc25b 2026-09-05T16:17:33-04:00 test(audio): the suite's window host has no screen
433528297b38572a8b683f5b425f3dad0041214a 2026-09-05T17:26:29-04:00 feat(nodes): the stitching nodes, and the spectrum both ways
d890ec225b585253ce699ed74c2de7dabba3eb63 2026-09-05T17:17:53-04:00 feat(demo): an untouched demo goes quiet rather than billing all night
eccde346ef60b0ac6f9794e8e5f77e90f770e993 2026-09-05T17:17:41-04:00 fix(demo): `session status` panicked where there is no audio engine
b205b1b944a9466d4d4a6058802235817b65a7a9 2026-09-05T16:58:03-04:00 feat(canvas): a node's header carries its engine's colour
3d5be03a320e12014cf1f8ede78362952b371ebf 2026-09-05T16:57:00-04:00 feat(palette): a colour per family, and a plugin is a family of its own
8cac5d41d5b4b0c8a010c7713bd5e6254385ab2b 2026-09-05T16:55:26-04:00 refactor(nodes): the oscillator becomes the LFO, and its rate moves to the output page
b7cfec5b83df350d9572bc0925c6639021357d29 2026-09-05T16:43:57-04:00 build(docker): node 24, because the lockfile is gitignored
69a7dd81f252b20d44726e40ca1de42f9396a593 2026-09-05T16:24:57-04:00 build(docker): node from NodeSource, not apt
8c6ad718243f487ed5ad6207435200b6ecc675c9 2026-09-05T16:21:33-04:00 feat(nodes): a constant is a producer, and a gate is high above zero
ceac17324862ed956fcd989512719f4bcea0997d 2026-09-05T14:15:17-04:00 feat(nodes): the three control nodes, which carry no signal of their own
d5a05f981f4ad0814d5a030a5f23e99d8571c936 2026-09-05T14:00:48-04:00 fix(init): uv's listing is what the gap is read from, so it must not be coloured
19bcd674873e9d02882b2325b141b102719f306f 2026-09-05T13:57:22-04:00 feat(nodes): the array family, and the labels that follow an axis through it
cf742b267522f4b708968947d72f16f8e43864a7 2026-09-05T13:40:08-04:00 docs: plugin editors are hosted, on the main thread, off the live instance
cc7dcde4d258c71f7368011769e9ff3c7369f045 2026-09-05T13:40:08-04:00 feat(app): an "Open plugin editor" action, drawn from the palette's capability
883e258af8a162f98dc9b9c4b6b16ffbc9f06ce2 2026-09-05T13:38:30-04:00 feat(vst3): a plugin's own editor, off the live instance, on the window thread
549ee829b4a7da57ae184a6b005436fd2c12b35a 2026-09-05T13:27:14-04:00 feat(seam): an editor window is a capability an engine answers per type
8b7a90ccf96462e94a179ce923b0194cf31f0d06 2026-09-05T13:22:08-04:00 feat(audio): the main thread is a window loop, and a plugin is made on it
cb1ca202907d5dac441544b37d46be1433da8122 2026-09-05T13:32:09-04:00 feat(nodes): the four generators, and the rule that runs a source nothing can ring
2aaaef47108bb7cc2197f8dea494bbb5bb1e1c65 2026-09-05T13:16:59-04:00 feat(core): the meta rules and the stream a stitching node keeps instead of state
533debb169b3a11ee4562e0a4b7c5672402c43ca 2026-09-05T12:07:12-04:00 test(harness): the nested cargo target is this binary's, not the machine's
5d2ea59ee09c7d4cf2f5159375377bcd8c2aadac 2026-09-05T12:43:45-04:00 fix(seam): the review and audit round of the node library's first step
6b9105cf8a7f6adc68ee82bbba74f211392cb796 2026-09-05T12:43:45-04:00 refactor(palette): the engine is read off the type id, and one fixture row serves every test
231140c3d1cd8cf733f2d34cce78133317ee89e1 2026-09-05T12:43:29-04:00 docs(roadmap): the words the seam made false
bd249bfafbadcf24b56ade183eafa396f41277f6 2026-09-05T12:00:52-04:00 docs(roadmap): the seam decisions of the node library program
63fdcdfa74e1f5922cb91edebb0c4d67714e7f32 2026-09-05T12:00:52-04:00 feat(signal): a multi slot carries its senders as node.slot, and a rename follows
0faea986bec41f7a324dea37f4244a4631623a48 2026-09-05T12:00:52-04:00 fix(params): a pulse's evaluation stops at the graph, and the audio raise outlives a wake
b6d3e2a83ca7484c81ac54ae5025d787093abec6 2026-09-05T11:25:28-04:00 test(subpatches): the dump is read once the node has settled
0770897864fc11052e81c66d0f2a6e62fb1f6f6b 2026-09-05T11:27:54-04:00 fix(params): a node's pages arrive in declared order, common last
98e0a0b24e1591570bd29e99d0c045449cf3ce32 2026-09-05T11:27:54-04:00 feat(params): a pulse reaches a Python node, a dylib node and the audio control half
be9d2614838a2444d9c411e75519a75c909ff212 2026-09-05T11:24:05-04:00 fix(inspector): a driven pulse shows the source it is driven by
d50faa336ba150ca666cb7e4868014d4131701cf 2026-09-05T11:19:34-04:00 fix(vst3): a scan's memo is keyed on the scanner too
c834290b5284e5714a00a705f5b6b12e97d04212 2026-09-05T02:41:26-04:00 test(vst3): the fixture withholds its params until it is connected
e628c33b56230a1edc19066f0642b9df037e7625 2026-09-05T02:41:26-04:00 fix(vst3): show a separate controller's parameters
052995fb62f314ec982f89a9af0d5633edfa1152 2026-09-05T11:12:01-04:00 feat(inspector): a pulse is a button, and pages keep their declared order
773e07e30e258a17a79488f1a1b2efd59fa4eee6 2026-09-05T11:07:32-04:00 build: a rebuilt SPA says nothing
af8808facbae1a272c03afeb41e97af42269a5ac 2026-09-05T10:12:35-04:00 perf(probe): a probe's answer outlives the process
a16066e28ee35a03a79a67b97815b3eb464a1808 2026-09-05T10:12:35-04:00 test(contracts): one palette for the tags walk, not one per type
348d4a5d3388b9e7f92d2306a8a3f6043a4fc34b 2026-09-05T11:05:13-04:00 fix(params): a pulse leaks no gate value, and no literal reaches it
418ecf16033094c51bdd0f913d6b2f72a4d3ea68 2026-09-05T02:10:54-04:00 refactor(ops): one predicate says what a mode withholds
d49ef9852fafbbc756c8b59fe2bba17f46776934 2026-09-05T02:00:54-04:00 build(docker): the container a public goofi runs in
e4b8599cf7317b236c658b5c54a65e5ab366ff80 2026-09-05T02:00:54-04:00 feat(frontend): the demo flag rides hello, and a withheld affordance is not drawn
eda827bbaf3f1a8e61919fe1615a8155481be427 2026-09-05T02:00:41-04:00 feat(demo): a public goofi serves the graph and none of the host around it
e0e9399d382c073b8852fd68525faf718310e70b 2026-09-05T10:20:42-04:00 feat(params): a pulse param is a request, fired by an op or a rising edge
7c822792c88259e823e971ee8efe9f7d148f560a 2026-09-05T07:42:28-04:00 test(browser): a lag that fills the socket, not one that hopes to
8120e4c8e9e2fc318ea1d7afe369991397262404 2026-09-05T07:13:40-04:00 fix(session): the replacement's snapshot goes out under the lock that replaced the graph
6f7f92b2cd60a149677678e1c229e8454a0b545a 2026-09-05T06:34:54-04:00 test(library): a fixture plugin that is an instrument, and a tag list that cannot be empty
cde10eaf79152058883356e2290fbc1841179e31 2026-09-05T04:30:54-04:00 ci: a ceiling the list fits under
e7f336cca180905e2e947b52ac8152e3efbf7df4 2026-09-05T03:31:29-04:00 test(agent): the one-line report's reason, as the diagnostic run showed it
3abd1c61cf2089a75aacaff502693f5e855a81bd 2026-09-05T03:21:29-04:00 test(agent): the workspace report in one line
ef7816b5c445174cc1b5d8bfa209882b0c553546 2026-09-05T03:21:28-04:00 fix(scan): a copy that kept its mtime is still another file
1fda5a7538fe7eadd015f040eddefd51049be71a 2026-09-05T03:21:28-04:00 test(audio): a born-at reading no control tick can split
86176f8a7cf88a5738710b4a5f56e53acca634d6 2026-09-05T03:21:28-04:00 ci: keep the cargo cache on a red run
b83f3f5e8185cd5c047cb1738fe527ebbcd5a184 2026-09-05T03:21:28-04:00 ci: the Linux runner has no sequencer, and the suite says so instead
93b012fb72f0bdd218c2f46033b9c7b42dd25856 2026-09-05T03:10:30-04:00 refactor(frontend): the tag type is the vocabulary, and a complete fixture drops its cast
e2cbf0fe8cd11249d742367105f2a16f90afa056 2026-09-05T02:59:44-04:00 feat(frontend): tags replace category, from the generated vocabulary
a005ed2f3537a2e0aa92f214f58f8a52684918d7 2026-09-05T02:58:23-04:00 feat(library): tags from one closed vocabulary replace category
88c1b86ba5996fd6398e6c41a04033975b278729 2026-09-05T02:42:06-04:00 ci: the sequencer module comes with the kernel's extra modules
189bdcbf1877e94acf7a6c07218cd675fa9dd2f9 2026-09-05T02:41:07-04:00 ci: every platform reports on its own, and every test binary runs
48b8349411d95dff6f5a68a9d6106831b6ba1d49 2026-09-05T02:35:32-04:00 fix(client): only a refusal sweeps a session record
dc2c7a41ea5a8c6b43c1275d37e40ee8ce6fec7d 2026-09-05T02:35:32-04:00 ci: the Linux runner loads the ALSA sequencer
528abc79814cc96f2d683bb6b1d1fe4b302ff26b 2026-09-05T02:19:29-04:00 ci: the Linux runner gets ALSA's headers
e587478a50de3c5db27ca57442bb9afc8ce31df9 2026-09-05T02:12:43-04:00 refactor(nodes): a bundle is a flat folder, and a file names its own engine
f79e964fac1da65bf4526734126b80768e12d0cf 2026-09-05T02:09:39-04:00 ci: Node 24, because 22's npm cannot resolve the frontend
71b4a374c3c78266ee15ba9e7f1699f4b19b962a 2026-09-05T00:46:39-04:00 fix(editor): a card keeps its measured size across a re-derivation, and blinks only when its handles change
9aea7ffdce35ae93f363d61157e1e76d28f24e69 2026-09-05T00:35:10-04:00 feat(frontend): the palette and the doc key on engine:Name
a747395e3e522496549eb8f9e4e589eb84b2e7a3 2026-09-05T00:27:23-04:00 refactor(nodes): every node-bundles/<bundle>/ is shipped, and the built-ins are two of them
886c4820f3005fba43d229ecaf8cf2d6ba3609ce 2026-09-05T00:15:04-04:00 fix(graph): an engine id is claimed once, and the suite pins it
f1b28c7202d208047d6d856289b33de8165221f2 2026-09-05T00:09:15-04:00 perf(audio): the dev profile optimizes the sound-server client
17cbd18d341731c2d24e5a68808b196090c4b81d 2026-09-04T23:58:53-04:00 feat(graph): a type is engine:Name, and two engines may share one name
04207ea1f4aa88e7041b711eb8cdde0180648ac6 2026-09-04T23:47:25-04:00 test(e2e): the integrity scene streams a spectrum on a log axis and tolerates no page error
218d2709b979808c7a892e18d41eabcbcd8d30fe 2026-09-04T23:47:25-04:00 fix(audio): a NaN never enters the plan, and a node that makes one is named
904021e163561d41a33c084f3211870a4221b4bd 2026-09-04T23:24:25-04:00 test(e2e): a delta landing mid-drag leaves the node under the pointer
586c8ce99b167cac8844fb6e5ef96db1bd9b6ff7 2026-09-04T23:24:25-04:00 fix(viewer): a log axis draws its own decades, never uPlot's walk
33732c5d35dbf4857e3fcda9ae6dfda0b4174a42 2026-09-04T22:26:37-04:00 fix(editor): a node mid-drag keeps the position the pointer has it at
366ce74e7ca9d21db2649115ee73deafaeddf178 2026-09-04T22:18:32-04:00 fix(viewers): a log y-axis takes a floor above zero
c990195514c73a2d40e27e009676b6461d31f5c0 2026-09-04T21:03:28-04:00 refactor(transport): the graph is one owner, so it is one iceoryx2 node
0144ef62768f32b86eb9ed2c0fd9f815d21581ea 2026-09-04T20:41:20-04:00 fix(transport): goofi asks the OS for the descriptors a patch costs
c21b6bdf554a4ea38d58d0e03db9fb2b082cea2b 2026-09-04T19:25:35-04:00 fix(audio): the host's buffer, never a block of ours
01b108447dd233edb4bbd8f02752bb1a963e3c5f 2026-09-04T19:00:07-04:00 fix(viewers): an undeclared viewer draws a preview, never the full frame
eae873d6a8f5598af3a618e10f8e7ec5747c07a7 2026-09-04T17:08:03-04:00 build: one binary a bare `cargo run` means

```
