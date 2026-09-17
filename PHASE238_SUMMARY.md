# Phase 238: Pipeline Runtime And Status Surface

## Summary

This phase implements `GFX-039` from the graphics roadmap on bare metal.

**Runtime** (`WorkspaceSession`): `pipeline run <cmd>[,<cmd>...]` creates a `PipelineRun` of `StageRun`s. Each loop iteration calls `pipeline_poll`, which submits the next pending stage as a `CommandRequest` to the kernel command service, then consumes the `CommandResponse` whose correlation id matches the running stage, recording `Succeeded { ticks, summary }` or `Failed { ticks, error }`. A failure finishes the run (fail-fast); completion pushes a summary line and a notice. `pipeline status` prints the trace; `pipeline clear` dismisses it.

Stage replies use a dedicated channel created at boot (`set_pipeline_channel`). The first attempt reused the workspace's response channel and never saw a reply: the console task also polls that channel and consumed them. This is the kind of ownership bug typed channels are meant to surface, and the fix is one owner per channel.

**Status surface**: `PipelineModel` renders the trace in the workspace window titled `Pipeline (running|done|failed)`, one line per stage (`[ .. ]`, `[ >> ]`, `[ ok ]`, `[FAIL]`, ticks, first output line), the running stage highlighted, the prompt pinned below, and a status strip `Pipeline: n/m stages done`.

## Rationale

Pipeline execution on bare metal means stages that go through the kernel's real message path rather than direct calls, so the runtime is genuinely asynchronous: the loop keeps rendering, presenting, and taking input between stages. Timings in ticks come from the same PIT counter everything else uses. The surface reuses the highlighted-row and pinned-prompt patterns of the other workspace apps.

## Verification

- `cargo test -p kernel_bootstrap` (pipeline model test: trace lines, running highlight, pinned prompt and caret, status text, failed/done titles).
- QEMU (`cargo xtask qemu-script`): `pipeline run help,ticks,mem,bogus` logged `stage 1 ok (11 ticks)` through `stage 4 failed (24 ticks)`; `pipeline status` printed the trace with real command outputs; the screendump shows the failed run surface and an ERROR notice.
