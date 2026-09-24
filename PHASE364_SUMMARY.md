# Phase 364: the Now card and the shortcut sheet (GFX-072)

## What changed

- **Now.** Clicking the clock (or the palette row) opens a card with the
  time, uptime, heap in use, CPUs online, file count, cards and spaces in
  use, the kept look and the notice count. While open it asks the kernel
  for `Vitals` once a second through `Desk::tick`; chips open Notices,
  Look and Shortcuts.
- **Shortcuts.** One card listing the desk's own keys (`DESK_KEYS`) and
  then every palette row that has a shortcut, generated from
  `PaletteAction::ALL` so it never drifts from the palette.
- The sheet is longer than a card and scrolls: arrows, PageUp/Down, the wheel; the footer counts `N of M keys`.
- Neither is a dock tile.

## Design

`DeskRequest::Vitals` is served in the kernel from the same globals the
console's `mem` and `cpus` read (`GLOBAL_HEAP.stats()`, `CPUS.online()`,
`CPU_TOTAL`) plus a filesystem listing count. The desk throttles the ask
to `VITALS_EVERY` ticks and never has two outstanding. `clock_at_column`
mirrors the compositor's right-alignment so the clock is a click target
without a new hit region.

## Tests

- `desk::tests::the_clock_opens_now_which_asks_the_kernel_once_a_second`
  (no ask without a card, the clock click, throttling, rows, the sheet's
  content, dock unchanged).
- Machine: `cargo xtask gauntlet` exit 0.
