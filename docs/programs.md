# Writing a PandaGen program

A program runs in ring 3, in an address space of its own. It can do
nothing outside that address space but ask the kernel, and it can only
ask about things it was given: each is a *handle*, a small number that
means something only in the program's own table of capabilities. There
are no paths and no global names, and nothing like a "standard output"
that every program has.

## The pieces

- **`pandagen_app`**: the SDK. It provides the system calls (`exit`,
  `yield_now`, `sleep_ms`, `time_ms`), the handles (`Handle`,
  `Console`), `say!` for formatted lines, and `entry!` for the entry
  point and the panic handler. Programs need no heap (lines are formatted
  in a fixed buffer), but can have one: the `heap` feature gives a
  program 256 KiB of its own memory, managed by the same allocator as
  the kernel's (`free_list_heap`), so `alloc`'s `String`, `Vec` and `Box`
  work.
- **`program_image`**: the format a program ships in. An image holds a
  name, an entry point, pieces of memory (each writable or runnable,
  never both, all above the first 4 MiB), and the capabilities the
  program asks for. The kernel checks all of it before it maps anything.
- **`apps/<name>`**: the programs. Each is its own Cargo workspace,
  built for `x86_64-unknown-none` and linked by `apps/app.ld` at
  `0x400000`.
- **`cargo xtask iso`** builds every program in `PROGRAMS`, turns each
  ELF into an image with the capabilities listed there, and ships the
  images in the ISO as Limine boot modules. The kernel finds them at
  boot; `programs` in the Terminal lists them and `run <name>` starts one.

## A program

```rust
#![no_std]
#![no_main]

use pandagen_app::{entry, say, Console};

entry!(main);

fn main(console: Console) -> u64 {
    let _ = say!(console, "hello at {} ms", pandagen_app::time_ms());
    0 // the exit code
}
```

A panic sends `panicked: ...` to the console and exits with 101.

## Other capabilities and calls

- **Notices** (`Ask::Notices`): `Notices::say(text)` puts a line in the
  desk's notices centre, with a chime.
- **`random_bytes` / `random_u64`**: bytes from the machine's generator.
  Any program may ask: it reveals nothing and reaches nothing.

## A card on the desk

A program that asks for a card (`program_image::Ask::Card`) gets one
when it runs: `run tally` opens a card for it. The program never draws
pixels. It describes the card with `ViewWriter`: a title, a footer, and
fills, outlines, lines, text (left, right or centred) and buttons, in
colour *roles* (surface, raised, text, muted, accent, ...) or, for what
the theme has no role for, a colour by value (`Role::Rgb`: Tiles' tiles). The desk draws that with its own widgets in
its own theme. A button stands for a key, so a click and a key press are
the same event to the program, and anything a program does works with
the mouse alone or the keyboard alone.

```rust
const CARD: Card = Card(Handle(1)); // handles come in the order asked

loop {
    match CARD.next_event() {          // sleeps until something happens
        Event::Size { w, h } => { /* lay out for w x h */ }
        Event::Key(b'+') => count += 1, // typed, or the "+" button clicked
        Event::Closed => return 0,      // two seconds to finish
        _ => continue,
    }
    let mut buf = [0u8; 1024];
    let mut view = ViewWriter::new(&mut buf, "Tally", "+ adds one");
    view.op(Op::Button { area: Area::new(16, 16, 80, 40), kind: Kind::Primary, key: b'+', label: "+" });
    CARD.present(view.finish().unwrap()).ok();
}
```

The desk checks every view against the card before drawing it: every
widget inside the canvas, every text valid, short and free of control
characters, at most 256 widgets and 8 KiB. A view that fails is refused,
the card keeps the last good one, and its footer says why. A program the
kernel ends takes its card with it, and the desk leaves a notice saying
why.

## Dock apps that are programs

The Calculator, the Timer and Tiles are programs (`apps/calculator`,
`apps/timer`, `apps/tiles`, with everything testable in the host-tested
`calculator_core`, `timer_core` and `tiles_core`). The Calculator and
Tiles ask for exactly one capability, their card; the Timer asks for its
card and **notices**, so a countdown that is up is said in the desk's
notices centre, with a chime, wherever the person is. None of them can
reach a file, the network or the console. `DeskApp::program` names the program behind a dock app. Opening
that app (from the dock, the palette, the Apps grid, or a restored
layout) opens a card at once and asks the kernel to start the program,
which then takes the card over. If the program can't start, the card
closes and a notice says why.

## Supervision: what a program may use, and what happens when it breaks

Every program runs within limits (`kernel_bootstrap/src/supervision.rs`):

- **Memory.** Its image and its stack must fit in 16 MiB, checked before
  anything is mapped. A program cannot grow past what it was loaded
  with, so the check at load is the whole of it.
- **The processor.** At most three quarters of it: 15 timer ticks in
  every 20 (`sched::Budget`). A program that has used its share waits
  for the next window, however it loops, so the desk always has the
  rest. `threads` shows each program's share and how often it was held
  back.
- **Crashes.** When the kernel ends a program with a card, its card
  stays and the program is started again into it, up to three times in
  a minute. At the fourth crash in a minute the card says it has given
  up and offers a "Start it again" button (or R). `apps/fragile` breaks
  when you press B, to show this.

## Adding one

1. Copy `apps/ticker` to `apps/<name>` and change the package name.
2. Add `("<name>", &[program_image::Ask::Console])` to `PROGRAMS` in
   `xtask/src/main.rs`.
3. `cargo xtask iso`, boot, and `run <name>` in a Terminal.

## What a program is refused

| It tries                                   | What happens                                         |
|--------------------------------------------|------------------------------------------------------|
| a handle it was not given                  | `Error::NoSuchHandle`: the handle does not exist      |
| a pointer outside its memory in a call     | `Error::BadPointer`, and nothing is read or written   |
| touching the kernel's memory               | a page fault; the kernel ends the program             |
| `cli`, `hlt`, `in`, `out`, `int` (but 0x80) | a general-protection fault; the kernel ends it        |
| looping forever                            | it is preempted like any thread, and `stop` ends it   |
| drawing outside its card, or spoofing text | the view is refused; the last good one stays          |

The machine carries on in every case. How a program ended is shown in
the Terminal: `exited with 0`, `stopped`, or `ended by the kernel: ...`.
