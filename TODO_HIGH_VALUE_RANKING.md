# High-Value TODO Ranking (Impact First, Effort Ignored)

This list is ordered by product/system value if completed, not by implementation cost.

1. [x] `services_pipeline_executor/src/lib.rs:571` and `services_pipeline_executor/src/lib.rs:494`
   Implemented: stage execution now uses real IPC/service invocation (`lookup_service` + `send_message` + `receive_message` + correlation check + deserialize).

2. [x] `pandagend/src/runtime.rs:255` and `pandagend/src/runtime.rs:260`
   Implemented: HAL mode now pumps input through `services_input_hal_bridge` + `InputService` into workspace routing, with deterministic HAL event injection support for tests.

3. [x] `services_input_hal_bridge/src/lib.rs:177` and `services_input_hal_bridge/src/lib.rs:240`
   Implemented: default `poll()` now performs real kernel message delivery with subscription validation and kernel error mapping (including budget/policy/channel failures), instead of placeholder counter-only behavior.

4. [x] `kernel_bootstrap/src/bare_metal_storage.rs:22`
   Implemented: boot storage now uses a typed backend that attempts `VirtioBlkDevice` MMIO initialization on bare-metal (with HHDM mapping + bounded probe), and falls back to `RamDisk` when unavailable.

5. [x] `services_workspace_manager/src/lib.rs:1501` and `services_workspace_manager/src/lib.rs:1514`
   Implemented: `save_settings()` / `load_settings()` now perform transactional `JournaledStorage` writes/reads with deterministic settings object resolution, optional fs-view path linking at `settings/user_overrides.json`, and corruption-safe load via `load_overrides_safe()` + `import_overrides()`.

6. [x] `services_workspace_manager/src/lib.rs:1088`
   Implemented: `Action::Save` now performs a real save on the focused editor instance (`save_current_document`), publishes updated editor views, reports concrete save failures, and no longer falls back to settings-only success.

7. [x] `services_workspace_manager/src/lib.rs:1297` and `services_workspace_manager/src/lib.rs:1267`
   Implemented: file picker now resolves selected `ObjectId` to a root-relative path before launching the editor (fallback to filename when unresolved), and breadcrumb rendering uses actual directory location (`ROOT/...`) instead of the `<root>` placeholder.

8. [x] `services_workspace_manager/src/boot_profile.rs:173` and `services_workspace_manager/src/boot_profile.rs:188`
   Implemented: boot profile manager now loads/saves `BootConfig` through transactional `JournaledStorage` using a deterministic object ID, with safe default fallback when storage is absent/missing/corrupt.

9. [x] `cli_console/src/commands.rs:187`
   Implemented: `CommandHandler::cat` now performs real content reads via `JournaledStorage` transactions (`read_data`) and returns file contents instead of object IDs.

10. [x] `services_workspace_manager/src/lib.rs:3320` and `services_workspace_manager/tests/runtime_tests.rs:140`
   Implemented: `WorkspaceRuntime::new` now loads persisted `BootConfig` through `BootProfileManager` and applies startup behavior deterministically (`Workspace` = no auto-launch, `Editor` = open editor at boot file, `Kiosk` = launch tagged custom kiosk component), with integration tests validating storage-backed profile activation.

11. [x] `services_workspace_manager/src/commands.rs:36` and `services_workspace_manager/src/commands.rs:539`
   Implemented: workspace command surface now supports boot profile management (`boot profile show|set|save`) with parser + formatter + execution handlers wired to `WorkspaceManager` boot-profile persistence APIs, plus command-registry/help updates and tests for parse, execute, and save/reload behavior.

12. [x] `services_workspace_manager/src/lib.rs:1024` and `services_workspace_manager/src/lib.rs:1304`
   Implemented: `ComponentType::Cli` and `ComponentType::PipelineExecutor` now create real interactive runtime instances (line input + view rendering + command/event processing) instead of `ComponentInstance::None`; CLI commands are parsed/executed through workspace command routing, and pipeline console drives real `PipelineExecutor::execute()` attempts with deterministic outcome logging.

13. [x] `services_workspace_manager/src/lib.rs:722` and `services_workspace_manager/src/lib.rs:2137`
   Implemented: pipeline runtime now uses a service-side harness kernel with registered echo stage handlers, so `run` auto-registers handler channels, executes real request/response IPC correlation, and returns successful stage outputs through `PipelineExecutor::execute()`.

14. [x] `services_workspace_manager/src/lib.rs:1662` and `services_workspace_manager/src/lib.rs:1673`
   Implemented: `Action::CommandMode` now enters a real command palette flow by opening/focusing CLI, rendering deterministic command-palette previews (`name/category/keybinding -> invocation pattern`), and updating workspace status with shown/total command counts.

15. [x] `services_workspace_manager/src/lib.rs:1110` and `services_workspace_manager/src/lib.rs:1284`
   Implemented: `launch_component()` now validates `FilePicker` launch prerequisites up-front and fails fast with `WorkspaceError::MissingLaunchContext` when storage/root context is missing, preventing partial component/view creation and replacing `ComponentInstance::None` fallback behavior; command-path feedback now surfaces actionable recovery hints.

16. [ ] `services_workspace_manager/src/lib.rs:1307` and `services_workspace_manager/src/lib.rs:1342`
   Track next: make `launch_package()` resilient to partial launch failure by returning created component IDs plus structured per-component errors (instead of aborting at first failure), so package startup can degrade gracefully when optional components are unavailable.
