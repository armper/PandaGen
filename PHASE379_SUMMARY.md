# Phase 379: sound (GFX-087)

## What changed

- `kernel_bootstrap/src/speaker.rs`: the PC speaker, driven by PIT
  channel 2 through port 0x43/0x42 and gated by port 0x61, played note by
  note from the kernel's loop (`Speaker::poll` each iteration), so a
  chime never blocks anything. Notes are `(Hz, ticks)`; 0 Hz is a rest;
  the queue is bounded. Host-testable through `PortIo`, with a recording
  fake in the tests.
- `Sound::{Click, Chime, Blip}`: a short tick under a pressed button (a
  header chip or a drawn button that was hit), three rising notes when a
  countdown is up, a soft blip for a notice (not on top of a chime).
- The desk queues sounds and hands them to the kernel as
  `DeskRequest::Sound` at the end of `tick`; the kernel owns the speaker,
  the desk only asks, as with every other side effect.
- `cargo xtask qemu` gives the machine an audio backend for this host
  (CoreAudio on macOS, PulseAudio on Linux) and wires the PC speaker to it
  (`-machine pc,pcspk-audiodev=snd0`). `QEMU_AUDIO=none` keeps it quiet;
  the gauntlet and the smoke run never ask for one.

## Why this shape

The machine has no audio device the kernel knows, but it has always had
the speaker, and a desk needs only a few short sounds to feel present.
Driving it by the tick from the loop keeps the kernel's single thread
honest: no waiting, no interrupt, one compare per iteration when idle.

## Tests

- `speaker::tests::a_chime_is_played_note_by_note_on_the_tick_and_the_gate_closes_after`
- `desk::tests::the_desk_asks_the_kernel_to_click_blip_and_chime`
- Machine: `cargo xtask gauntlet` exit 0 (silent machine; the ports are
  written and nothing objects).
