# Phase 168: Pipeline Executor Real Stage IPC

## Summary

Implemented real stage invocation for `PipelineExecutor` by replacing the synthetic stub in `execute_stage_once()` with service discovery and IPC request/response flow.

## Rationale

The previous implementation always returned a synthetic success payload, which meant pipeline traces, retries, and policy-gated orchestration were not executing against real handlers.

This phase makes stage execution reflect actual runtime behavior:

- Resolve handler channel via `lookup_service`
- Send a stage invocation request message with typed input
- Receive a response (respecting stage timeout when configured)
- Verify response correlation with request ID
- Deserialize and return concrete `StageResult`

## Change Set

- `services_pipeline_executor/src/lib.rs`
  - Added stage IPC payload contract:
    - `StageInvokeRequest { input: TypedPayload }`
    - `StageInvokeResponse { result: StageResult }`
  - Replaced `execute_stage_once()` stub with real IPC flow.
  - Added error mapping for missing handler service to `ExecutorError::HandlerNotFound`.
  - Added correlation mismatch guard.
  - Added unit tests for:
    - Successful IPC stage invocation
    - Missing handler lookup
    - Mismatched response correlation

## Tests

Executed:

- `cargo test -p services_pipeline_executor`
  - Result: pass (all tests green)

Note:

- `cargo test -p tests_pipelines` currently fails due to pre-existing compile issues in `sim_kernel` unrelated to this phase.
