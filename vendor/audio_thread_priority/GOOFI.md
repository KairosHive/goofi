# Local scheduling fix

Source: `audio_thread_priority` 0.35.1 from crates.io, under MPL-2.0.

Linux promotion first tries `sched_setscheduler` with the existing priority of 10
and reset-on-fork protection. Only a permission refusal uses the original RTKit
path. Direct scheduling leaves resource limits unchanged. This follows the
direct-first policy used by PipeWire and avoids an unnecessary process-wide
`RLIMIT_RTTIME` that killed goofi during audio viewer updates.

Demotion retains reset-on-fork, which an unprivileged thread cannot clear, and
checks pthread error codes rather than treating positive errors as success.

The public API and the other platform implementations are unchanged. Regression
coverage is in `backend/goofi-tests/tests/audio_priority.rs`. Remove this patch
when an upstream release provides direct-first Linux promotion.

The RTKit fallback still needs a finite CPU limit. This patch does not contain
native plugin hangs or memory faults; that requires process isolation.
