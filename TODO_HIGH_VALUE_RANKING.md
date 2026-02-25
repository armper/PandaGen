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

16. [x] `services_workspace_manager/src/lib.rs:1368` and `services_workspace_manager/src/lib.rs:1411`
   Implemented: `launch_package()` now returns `PackageLaunchReport` with `created_component_ids` plus structured `PackageLaunchFailure` entries, so component failures are recorded per-spec and package startup degrades gracefully instead of aborting at first error.

17. [x] `services_workspace_manager/src/lib.rs:1430` and `services_workspace_manager/src/lib.rs:1105`
   Implemented: `ComponentType::Custom` now resolves through a typed `CustomComponentRegistry` into a real interactive `CustomComponentRuntime` host (`ComponentInstance::Custom`) with rendered views, command handling (`status|meta|ping`), and metadata-aware handler selection instead of inert `None` shells.

18. [x] `services_workspace_manager/src/commands.rs:505` and `services_workspace_manager/src/lib.rs:2239`
   Implemented: command parsing/help now supports `open custom <entry>`; parser validates required entry, command execution maps entry into explicit custom metadata routing (`package.entry` + `custom.entry`), and CLI help advertises the custom launch pattern.

19. [x] `services_workspace_manager/src/command_registry.rs:11` and `services_workspace_manager/src/lib.rs:1806`
   Implemented: command palette registry now includes `Open Custom` (`open_custom`) as a first-class parametric descriptor with workspace category, keybinding hint, and prompt pattern (`open custom `), so command-mode previews expose custom-host launch discovery alongside existing open commands.

20. [x] `services_workspace_manager/src/command_registry.rs:14` and `services_workspace_manager/src/lib.rs:1806`
   Implemented: command palette registry now includes first-class `Open CLI` (`open_cli`) and `Open Pipeline` (`open_pipeline`) launch descriptors, and `open_editor` prompt metadata now reflects optional args (`requires_args = false`) while retaining `open editor ` invocation hints; launch-command prompt patterns are now aligned with accepted parser syntax and covered by registry tests.

21. [x] `services_workspace_manager/src/commands.rs:506` and `services_workspace_manager/src/workspace_status.rs:246`
   Implemented: command-surface parity now includes executable aliases for non-launch helpers (`recent`, `recent files`, `open recent`, `open file`, `open file-picker`), prompt validation now mirrors parser semantics (including `open custom` prefix/complete behavior), and suggestion output now advertises only executable helper variants; command-palette registry parity was tightened with explicit `open_file_picker` and canonical `recent` prompt patterns.

22. [x] `services_workspace_manager/src/commands.rs:506` and `services_workspace_manager/src/command_registry.rs:9`
   Implemented: introduced shared command-surface specs in `services_workspace_manager/src/command_surface.rs` (launch targets, helper aliases, palette metadata, suggestion specs, and validation prefix groups), and wired parser (`parse_command`), command-palette registry descriptor generation, and prompt suggestion/validation (`workspace_status`) to consume that single source of truth for invocation patterns, aliases, arg requirements, and categories.

23. [x] `services_workspace_manager/src/command_surface.rs:1` and `services_workspace_manager/src/command_registry.rs:13`
   Implemented: migrated all remaining non-launch palette descriptors (`help*`, `save`, `quit`, `boot profile*`, `list`, navigation, `close`) into shared `NON_LAUNCH_PALETTE_SPECS` and removed manual registration blocks, making command-palette registry composition fully data-driven from command-surface specs.

24. [x] `services_workspace_manager/src/commands.rs:510` and `services_workspace_manager/src/command_surface.rs:29`
   Implemented: extended shared command-surface grammar with non-launch executable rules (`help` topic aliases + `focus|close|status` component-id requirements), then wired parser and prompt validator to consume those grammar functions; CLI `help` now executes through parsed `WorkspaceCommand::Help` and category content instead of a hardcoded local output block.

25. [x] `services_workspace_manager/src/command_surface.rs:146` and `services_workspace_manager/src/help.rs:45`
   Implemented: `HelpCategory::workspace_help()` now generates its command section from shared command-surface descriptors and grammar (`LAUNCH_COMMAND_SPECS`, `HELPER_COMMAND_SPECS`, `NON_LAUNCH_PALETTE_SPECS`, `COMPONENT_ID_COMMAND_SPECS`, help-topic grammar), including alias/usage forms; parser help-usage errors also derive from shared help-topic grammar.

26. [x] `services_workspace_manager/src/help.rs:24` and `services_workspace_manager/src/command_surface.rs:356`
   Implemented: `Overview` and `System` help sections now render from shared command-surface metadata/grammar (`HELP_TOPIC_SPECS`, `help_usage_pattern()`, and `NON_LAUNCH_PALETTE_SPECS` system entries), removing stale static entries (`halt`, `reboot`, `mem`, `ticks`) and keeping docs synchronized with available command grammar.

27. [x] `services_workspace_manager/src/help.rs:24` and `services_workspace_manager/src/command_surface.rs:493`
   Implemented: eliminated help-topic parsing duplication by making `HelpCategory::parse()` delegate to shared `command_surface::parse_help_topic()` resolution, while `parse_help_topic()` remains canonical for alias decoding (including case-insensitive alias support and explicit `overview` mapping).

28. [ ] `services_workspace_manager/src/command_surface.rs:146` and `services_workspace_manager/src/workspace_status.rs:243`
   Track next: derive all static prompt suggestions (`list`, `next`, `prev`, `close <id>`) from shared command-surface descriptors/grammar so `generate_suggestions()` no longer contains hand-maintained non-help/non-open literals.
