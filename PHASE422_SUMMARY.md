# Phase 422: no warnings, clippy clean, and what the lints found (DEV-002)

## What changed

The workspace built with about forty compiler warnings (host and kernel
target) and a hundred-odd clippy lints, two of them errors that stopped
`cargo clippy` before it reached most crates. Now:

- `cargo test --workspace --all-targets --no-run`: **no warnings.**
- `cargo xtask iso` (the kernel's own target, release): **no warnings.**
- `cargo clippy --workspace --all-targets -- -D warnings`: **exit 0.**

### Real fixes the lints led to

- **`read_idtr` ran `sidt` under `options(nomem)`**, which tells the
  compiler the asm touches no memory -- so it could assume `idtr` still
  held the zeros it was made with. `nomem` is gone. `gdt::load` claimed
  `nostack` while its far return pushes and pops; that is gone too.
- **`PageTableManager::map_page` and `unmap_page` were stubs**: they
  checked alignment and the handle and wrote nothing, so every "mapping"
  succeeded and none existed. They now walk PML4, PDPT and PD, make the
  tables they need, write the leaf (refusing a page already mapped), and
  set USER on every level above a user page; `translate` does the MMU's
  walk. Unmapping a page that is not mapped is an error.
- **A test that asserted nothing** (`tests_pipelines`: policy and
  pipelines) now asserts what its comments said: the policy does not deny.
- **The kernel's `mem*` intrinsics** are `unsafe fn`, with the contract in
  their docs: they take raw pointers on trust, as C's do.
- **Dead code removed**: the Phase-15 keyboard demo (`editor_loop`,
  `EditorState`, `render_editor`) and its `output` module, never called;
  `save_undo_snapshot` in `editor_core`; nine redundant redraw flags in
  the pointer path (graphics-mode pointer events always redraw); dead
  resets in the text renderer. Constants only tests use are `cfg(test)`.
- **An unused `[patch]` and vendored crate**: `third_party/serde_core`
  patched 1.0.228 while the lockfile resolves 1.0.229, so it was never
  used, by the kernel build or any other. Removed; the lockfile is
  unchanged.

### Optimizations

- **Scrollback** (`console_fb`, `console_vga`) dropped the oldest line with
  `remove(0)` on every push once full -- shifting every line kept. Dropped
  lines are now freed in bulk, once per `max_lines` pushes.
- `clippy --fix` took the mechanical ones across 40 files: needless
  clones of `Copy` types, `is_multiple_of`, `div_ceil`, `abs_diff`,
  `rfind`, `repeat`, collapsed `if let`s and the like.

### Stale docs

- `docs/qemu_boot.md` said storage falls back to RAM for want of PCI
  probing; virtio-blk over PCI has worked since Phases 252-253, and the
  gauntlet now proves a reboot keeps the disk (Phase 421).
- `docs/architecture.md`'s Phase 20 limitations are marked as of their
  time, with the phases that closed them.

## Tests

- `test_map_page_writes_a_walk_the_mmu_can_follow`,
  `test_a_user_page_under_kernel_tables_makes_the_path_user`, and the
  unmap test now requiring a mapping first (`hal_x86_64`).
- `test_a_long_run_keeps_the_newest_and_frees_the_rest` (`console_fb`).
- Every existing test, unchanged in meaning.
- `cargo xtask gauntlet`: exit 0.
