# Phase 180 Summary: Interactive CLI and Pipeline Runtime Instances

## What Changed
- Replaced placeholder component instances for non-editor components in workspace runtime:
  - `services_workspace_manager/src/lib.rs`
  - `ComponentType::Cli` now creates a real in-process interactive console instance.
  - `ComponentType::PipelineExecutor` now creates a real in-process interactive pipeline console instance (std builds).
- Added internal interactive runtime state and rendering for text-oriented components:
  - `InlineConsole` line editor (input buffer, cursor, history, output log, revisions).
  - Keyboard event handling for text entry/navigation/editing.
  - Deterministic `ViewFrame` publishing for main/status views.
- Wired CLI input to real workspace command execution:
  - CLI command lines are parsed via `parse_command`.
  - Valid commands are executed through `WorkspaceManager::execute_command`.
  - Command results are formatted and appended to CLI output log.
- Wired pipeline console input to real executor invocation path (std builds):
  - `run` command builds a typed one-stage `PipelineSpec`.
  - Executes through `services_pipeline_executor::PipelineExecutor::execute`.
  - Appends success/failure output back into component views.
- Updated crate feature wiring for std-only pipeline runtime dependencies:
  - `services_workspace_manager/Cargo.toml`
  - `std` feature now enables optional `services_pipeline_executor`, `pipeline`, and `sim_kernel`.
- Added tests:
  - `test_cli_component_executes_workspace_commands`
  - `test_pipeline_component_runs_executor_path`

## Rationale
- Workspace previously spawned `Cli`/`PipelineExecutor` components as metadata-only shells (`ComponentInstance::None`), which made those component types non-interactive and low-value.
- This phase upgrades both into deterministic interactive surfaces that consume input and publish views, making multi-component workspace behavior materially more useful.
- Pipeline command path now exercises real typed pipeline orchestration code from workspace UI entrypoints, establishing a direct bridge for future service-backed handler execution.

## Validation
- `cargo test -p services_workspace_manager` passed.
- `cargo test -p services_workspace_manager --lib` passed.
