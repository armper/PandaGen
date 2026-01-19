# Repository Guidelines

## Project Philosophy (Apply to Every Task)
- **No legacy compatibility by design**: avoid POSIX assumptions (`fork`, `exec`, file descriptors, path-centric APIs).
- **Testability first**: logic should run under `cargo test`; keep kernel-facing work minimal and deterministic.
- **Modular and explicit**: use capabilities, explicit construction, and typed messages—no ambient authority.
- **Mechanism over policy**: primitives live in the kernel; services implement policy in user space.
- **Human-readable system**: small crates, clear names, minimal `unsafe`, and documentation that explains *why*.
- **Clean, modern, testable code**: readable implementations with fast, deterministic tests.

## Project Structure & Module Organization
PandaGen is a Rust workspace with crates at the repo root. Core building blocks live in `core_types/`, `ipc/`, `kernel_api/`, and `sim_kernel/`. Hardware abstractions are in `hal/` and `hal_x86_64/`. Services and hosts follow the `services_*` pattern (e.g., `services_storage/`, `services_network/`, `services_gui_host/`, `services_remote_ui_host/`). Higher-level systems include `packages/`, `package_registry/`, `remote_ipc/`, `distributed_storage/`, `workspace_access/`, and `formal_verification/`. Docs are in `docs/`, phase retrospectives in `PHASE*_SUMMARY.md`, examples in `examples/`, and integration-style tests in `contract_tests/`, `tests_resilience/`, and `tests_pipelines/`.

## Build, Test, and Development Commands
- `cargo build` — build the workspace.
- `cargo test` — run default tests.
- `cargo test --all` — run all workspace tests.
- `cargo test -p sim_kernel` — run a focused crate’s tests.
- `cargo fmt --check` — formatting check.
- `cargo clippy -- -D warnings` — lint with warnings as errors.

## Coding Style & Naming Conventions
- Rust 2021; Rustfmt defaults (4-space indentation).
- Naming: `snake_case` for modules/functions, `CamelCase` for types/traits, `SCREAMING_SNAKE_CASE` for constants.
- Prefer explicit capability-driven APIs and typed message schemas; avoid hidden global state or implicit privilege.

## Testing Guidelines
- Use the Rust test harness; keep tests deterministic and fast.
- Prefer `sim_kernel/` and in-process services over external dependencies.
- Name tests descriptively (e.g., `test_scheduler_preempts_on_budget`).
- Add tests alongside new logic and validate edge cases.

## Commit & Pull Request Guidelines
- Commits are short and imperative; common patterns include `Phase N: ...`, `feat: ...`, or `Add ...`.
- PRs should include a clear description, tests run, and doc/phase-summary updates when behavior changes.
- Include screenshots only for UI/UX changes.

## Documentation Expectations
- Update `docs/` and the relevant `PHASE*_SUMMARY.md` when adding subsystems or revising design.
- Document public APIs with rationale (the “why”), not just behavior.
