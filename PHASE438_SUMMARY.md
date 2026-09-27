# Phase 438: the Calculator moves out of the kernel (PROC-006)

## What changed

The dock's Calculator is a program now. It runs in ring 3 in its own
address space and holds exactly one capability, its card. It cannot
reach a file, the network or even the console, because it was never
given them. Open it from the dock, the palette, the Apps grid or a
restored layout, and the desk opens its card and asks the kernel to start
it. The program takes the card over and draws the same Calculator the
kernel used to, pixel for pixel (the existing gauntlet's pixel checks
pass unchanged). The kernel no longer contains a calculator at all:
`kernel_bootstrap/src/calculator.rs` is gone.

This finishes step 4 of the road to programs that run on their own: a
card that used to be kernel code is a program, and a crash in it could
now take down only itself and its card.

### Programs get a heap

- **`free_list_heap`** is its own crate: the kernel's first-fit,
  coalescing allocator, with its own small spin lock instead of the HAL's.
  The kernel's global allocator and a program's are now the same code,
  checked by the same model tests (`kernel_bootstrap/tests/heap_model.rs`).
- **`pandagen_app`'s `heap` feature** gives a program 256 KiB of its own
  memory (zeroed pages its image lays out), managed by that allocator.
  `entry!` sets it up before `main`, so `alloc`'s `String`, `Vec` and
  `Box` just work.
- **Zeroed memory costs nothing in the image.** The link script keeps the
  GOT with the data, before `.bss`: an orphan `.got` after it had made
  the whole heap part of the file (287 KB; now 25 KB). `from_elf` also
  drops trailing zero bytes, since pages come zeroed. xtask folds the link
  script's hash into the programs' flags, so a changed script relinks
  every program (Cargo does not track it).

### `calculator_core`

The Calculator's arithmetic (exact decimal, `i128` scaled by 10^12), its
keys and its card, moved from the kernel and tested on the host. The card
is now an `app_protocol` view, drawn with the same geometry: a display
with the expression small and the result up to three times the font,
twenty keys with the operators as drawn signs, and the tape. A canvas too
small for it says "Make me bigger" instead of drawing outside the card.

### The protocol

`Op::Line` (a drawn sign, 1 to 8 pixels thick, both ends inside the card)
and text at scale 3, both of which the Calculator needs.

### Dock apps that are programs

- `DeskApp::program()` names the program behind an app; for now, the
  Calculator's.
- `launch` of such an app opens a card that waits for its program
  (thread 0, which tells no one anything) and queues the program to run.
  The kernel starts it (`desk: calculator is thread 1`), and
  `open_program` hands it the waiting card. `run calculator` in a
  Terminal opens a Calculator card of its own the same way.
- A program that can't start closes its waiting card with a notice.
- Because every way of opening an app goes through `launch`, the dock,
  palette, Apps grid and layout restore all work unchanged.

## Tests

- `calculator_core`: the arithmetic (moved); keys are buttons hit by
  pixel and typed keys reach the same code, checked through a decoded
  view; a full tape and a long result still make a view the desk
  accepts; too small a canvas never draws outside.
- `desk`: the Calculator is a program whose keys are clicked or typed. It
  launches, queues its run, and ignores keys while it waits. The program
  takes the same card, its real `calculator_core` view is drawn, and
  clicks on the drawn keys through the router reach the program as
  `7+8=`. A card whose program couldn't start closes.
- `app_protocol`: lines are drawn signs inside the card; bad thickness is
  refused. `program_image`: trailing zeros are implied. `free_list_heap`:
  its tests and the kernel's heap model, unchanged.
- Gauntlet: "the Calculator from the palette" now also expects `desk:
  calculator is thread 1`, and its pixel checks pass unchanged; the
  reboot shape expects the restored Calculator to start its program.
