# Phase 280: One Work Queue, Two Owners

## The finding

`static WORK: WorkQueue<32>` had two independent owners:

- the boot CPU, submitting frame-band conversion jobs from `run_present_bands`;
- an application processor, submitting `smp run` jobs from `run_smp_jobs`.

Both began by calling `WORK.reset()`, which set `next_id` back to zero and
cleared the `done` flag on every slot. Either owner resetting mid-flight gave
the other owner's outstanding ids to fresh jobs, so a `wait` could be
satisfied by a completion it never asked for: wrong `cpu=` and `MISMATCH`
lines from `smp run`, and a frame band reported converted that never was.

`reset` existed because `submit` refused once `next_id` reached `N` — the
queue could serve only 32 jobs per lifetime. The reset was the workaround, and
the workaround was the bug.

## The fix

Ids are now monotonic and a slot is claimed by id:

- `submit` fails only when the ring is genuinely full, never because the id
  space ran out, so the queue serves any number of jobs.
- Each slot records which id owns it. `complete` and `result` check that id
  first, so a late worker reporting an abandoned job cannot be attributed to
  whoever holds that slot now.
- `reset` is gone. Nothing can invalidate another owner's results, which is a
  property of the API rather than of the callers' discipline.
- `cancel(id)` reclaims a ring slot when the submitter gives up waiting.
  `run_present_bands` now cancels before falling back, instead of leaving the
  job to occupy a slot for the rest of the uptime.

Separately, and from the same critic: a band job re-read `PRESENT_BANDS` at
the moment it ran, not when it was queued, so a worker that woke after the
boot CPU had given up could paint a band belonging to a frame that had already
moved on. Band jobs now carry a `PRESENT_GENERATION` in their argument and do
nothing if it no longer matches.

`run_smp_jobs` also spun 200 million times waiting on `u32::MAX`, the
sentinel for "never queued". It now reports that job as not queued and
continues.

## Verification

Tier 1, and the old implementation fails the first of these at round four:

- `the_queue_outlives_its_slot_count_without_a_global_reset` — 200 jobs
  through a four-slot queue.
- `two_owners_interleave_without_disturbing_each_other` — owner A's completed
  results survive owner B starting a batch, and ids never collide.
- `a_recycled_slot_ignores_the_previous_owners_completion`.
- `cancelling_returns_the_ring_slot`.
- `capacity_is_the_ring_not_the_lifetime`, replacing a test that asserted the
  defect as intended behaviour ("ids are exhausted even though a slot freed up").

End to end: graphics mode with continuous presents, six rounds of `smp run 32`
interleaved with mouse movement. All 192 jobs completed, zero `MISMATCH`, zero
present fallbacks.

**Honest note on reproduction.** The end-to-end race was not caught in the act.
Triggering it needs a present's `reset` to land inside an `smp run` batch, and
those jobs finish in microseconds, so the window is far narrower than the
33 ms present interval. The defect is established by inspection and by the
tier-1 contract tests; the end-to-end run is evidence of no regression, not of
the original race.
