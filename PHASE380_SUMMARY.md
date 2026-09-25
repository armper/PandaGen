# Phase 380: the top bar tray (GFX-088)

## What changed

- The bar's right side is a tray: the unseen-notice count as an accent
  badge, today's date (`Thu 24 Sep`), a small memory meter, then the
  clock. Each part appears only once it is known: no date before the RTC
  has been read, no meter before the kernel has reported the heap.
- The top bar can carry an overlay (the `DesktopWindow.overlay` from
  Phase 378); the compositor draws it over the bar's text in the bar's
  own pixels. The badge is a rounded accent fill with its text in the
  background colour; the meter is a hairline track with an accent fill.
- The kernel asks for vitals every five seconds (`TRAY_EVERY`) on its
  own, so the meter is live without the Now card open. The desk's own
  polling is unchanged, which kept every tick-based test as it was.
- Click targets are unchanged: the badge opens Notices, the clock opens
  Now, anything else on the bar opens the palette.
- `cargo xtask qemu` prints the audio arguments it adds, so the printed
  command is the one that runs.

## Tests

- `desk::tests::the_tray_shows_the_date_a_memory_meter_and_a_badge`
- Machine: `cargo xtask gauntlet` exit 0.
