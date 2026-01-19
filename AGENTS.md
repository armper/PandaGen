# Repository Guidelines

## Project Philosophy (Apply to Every Task)
- **No legacy compatibility by design**: avoid POSIX assumptions (`fork`, `exec`, file descriptors, path-centric APIs).
- **Testability first**: prefer logic that runs under `cargo test`; kernel-facing work should be minimal and deterministic.
- **Modular and explicit**: use clear, typed interfaces; construct capabilities and services directly instead of relying on ambient state.
- **Mechanism over policy**: keep core primitives small; push policy decisions into services.
- **Message passing over shared state**: prefer structured messages with explicit schemas and correlation IDs.
- **Clean, modern, testable code**: keep implementations readable, small, and well-covered by fast, deterministic tests.

## Project Structure & Module Organization
PandaGen is a Rust workspace. Crates live at the repo root (e.g., `core_types/`, `ipc/`, `kernel_api/`, `sim_kernel/`, `hal/`, `services_*`, `input_types/`, `view_types/`, `cli_console/`, `pandagend/`). Cross-cutting docs live in `docs/`, phase retrospectives in `PHASE*_SUMMARY.md`, and runnable examples in `examples/`. Integration-style suites are isolated in `tests_resilience/`, `tests_pipelines/`, and `contract_tests/`.

## Build, Test, and Development Commands
- `cargo build` — build the full workspace.
- `cargo test` — run the default test suite.
- `cargo test --all` — run every crate’s tests, including integration suites.
- `cargo fmt --check` — verify formatting (Rustfmt defaults).
- `cargo clippy -- -D warnings` — lint with warnings treated as errors.

## Coding Style & Naming Conventions
- Rust 2021 edition; use standard Rustfmt output (4-space indentation).
- Naming: `snake_case` for modules/functions, `CamelCase` for types/traits, `SCREAMING_SNAKE_CASE` for constants.
- Prefer small, testable modules and explicit capability-driven APIs consistent with the project philosophy.

## Testing Guidelines
- Use the built-in Rust test harness; keep tests deterministic and fast.
- When possible, exercise services through the simulated kernel (`sim_kernel/`) instead of external dependencies.
- Name tests descriptively (e.g., `test_pipeline_executes_steps`) and keep new coverage close to the crate being changed.

## Commit & Pull Request Guidelines
- Commit subjects are short, imperative, and sentence case (e.g., “Add timer device trait”).
- Use phase-scoped prefixes when relevant (e.g., “Phase 23: Integrate scheduler”).
- PRs should include a clear description, test results, and doc updates when behavior changes. Link related issues if applicable.

## Documentation Expectations
- Update `docs/` or the relevant `PHASE*_SUMMARY.md` when introducing new subsystems or design decisions.
- Keep public APIs documented with rationale (“why”) in addition to “what”.
