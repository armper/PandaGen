# Phase 400: Now, drawn -- a clock, a memory bar, CPUs and tiles (GFX-108)

## What changed

- The Now card is drawn (`now_ui`) instead of eight lines of text: the
  clock three times the font's size, the uptime and the look to its
  right; memory as a bar filled by the share in use, with `used of total
  MiB` above it (or "asking..." until the kernel answers); a dot per CPU,
  lit in the accent when online (up to 16, the count says the rest); and
  three tiles -- files, cards (and on how many spaces), notices kept --
  each a large number over its name.
- The facts are gathered once per frame (`NowFacts`, `Desk::now_facts`)
  before the cards are drawn, as the lines were. Its chips (Notices,
  Look, Shortcuts) and keys are unchanged. The card is 40px taller.
- Tiles loses its New chip: the card draws a New button (one control per
  action, as in Phase 395). The sound test clicks the Calendar's Today
  chip instead.

## Why

Now is the desk's system monitor; next to the drawn Calculator, Calendar
and Timer it was still a text dump.

## Tests

- `desk::tests::the_clock_opens_now_which_asks_the_kernel_once_a_second`:
  the drawn texts (the clock at scale 3), the facts, and the memory
  bar's fill as the used share of the track.
- Machine: `cargo xtask gauntlet` exit 0.
