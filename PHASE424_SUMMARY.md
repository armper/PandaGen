# Phase 424: a framebuffer with red in the low byte is drawn right (FB-001)

## What changed

The framebuffer path knew one pixel layout, `Rgb32` (0xXXRRGGBB, blue in
the low byte), and assumed it whatever the bootloader said: the colour
masks Limine reports were never read. QEMU's display and most PC firmware
use that layout; firmware that sets up 0xXXBBGGRR (red in the low byte)
would have shown the whole desk with red and blue swapped. The early
"only 32 bpp, BGRX hardcoded" note (Phase 71's framebuffer) had been open
since.

- `BootInfo` carries the red, green and blue shifts from Limine's
  framebuffer response; the kernel logs them when they are not the usual
  16/8/0.
- `PixelFormat::Bgr32` is the second 32-bit layout; `to_bytes` and the
  present conversion write red where red goes.
- `PixelFormat::from_shifts(bpp, shifts)` decides: 32-bit with 16/8/0 or
  0/8/16 is drawn; anything else (24 bpp, 10-bit, other orders) is
  refused as 24 bpp already was, and the kernel falls back to the VGA text
  console -- readable, where the wrong layout was not.
- The present conversion chooses the layout once per band and runs a
  loop with constant shifts (`convert_rows::<RED, BLUE>`), so the usual
  layout costs what it did.

## Tests

- `red_in_the_low_byte_is_drawn_as_such_and_other_orders_are_refused`:
  `from_shifts` for both layouts and two it refuses; a framebuffer from
  boot info with shifts 0/8/16 is `Bgr32`, one with 8/16/24 is refused;
  a present writes 200/100/50 as `[200, 100, 50, 0]`.
- The existing band-conversion and 24 bpp refusal tests pass unchanged.
- QEMU: the desk's colours are unchanged on the usual layout.
- `cargo xtask gauntlet`: exit 0.
