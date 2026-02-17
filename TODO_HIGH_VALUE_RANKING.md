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

5. `services_workspace_manager/src/lib.rs:1501` and `services_workspace_manager/src/lib.rs:1514`
   Wire `save_settings()` / `load_settings()` to `StorageService` (actual read/write + safe import). Current behavior only validates/announces without persistence.

6. `services_workspace_manager/src/lib.rs:1088`
   Replace placeholder `Action::Save` behavior with real focused-editor document save. Current save action can report success while only saving settings.

7. `services_workspace_manager/src/lib.rs:1297` and `services_workspace_manager/src/lib.rs:1267`
   Complete file picker/editor handoff by mapping `ObjectId` to path and real breadcrumbs. This closes an important UX/data-model gap in file workflows.

8. `services_workspace_manager/src/boot_profile.rs:173` and `services_workspace_manager/src/boot_profile.rs:188`
   Implement persistent boot profile load/save instead of default/no-op behavior. This enables stable boot preferences across sessions.

9. `cli_console/src/commands.rs:187`
   Implement real `cat` content reads instead of returning object IDs. This is lower-level than the platform/runtime items above but high leverage for CLI usability and debugging.
