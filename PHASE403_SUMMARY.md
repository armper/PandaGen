# Phase 403: the rest screen's clock to the corner (GFX-111)

## What changed

- The rest screen (Ctrl+L, or five idle minutes) puts the time -- seven
  times the font (`REST_CLOCK_SCALE`) -- and the long date under it in
  the bottom-left corner, `REST_MARGIN` (64px) in, instead of in the
  middle of the screen. How to wake is centred along the foot.
- The veil and the waking rules are unchanged.

## Why

The default "Picture" wallpaper has its own mark and name in its middle;
a centred clock sat on top of them, and the two read as one smudge. In
the corner the clock is clear, and the picture is what the screen is
while the desk rests.

## Tests

- `desk::tests::the_desk_rests_on_ctrl_l_or_when_idle_and_wakes_on_any_input`:
  the clock at its scale, at the left margin, in the lower half.
- Machine: `cargo xtask gauntlet` exit 0.
