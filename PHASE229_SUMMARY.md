# Phase 229: Layout Vocabulary

## Summary

This phase implements `GFX-029` from the graphics roadmap.

`services_gui_host::layout`:
- `LayoutNode::{Leaf, Stack, Overlay, Anchored, Padded}` with builders (`vstack`, `hstack`, `overlay`, `anchored`, `padded`), `Length::{Fixed, Weight}`, `Axis`, `Anchor` (nine positions), and `Insets`.
- `LayoutNode::solve(bounds)` resolves the tree to `(LayoutId, RasterRect)` pairs in tree order, in one pass with integer arithmetic.
- `distribute(total, gap, lengths)`: fixed lengths first (truncated in order on overflow), gaps reserved, remainder split by weight, and integer leftovers handed to the earliest weighted children so sizes always sum to the space.

`kernel_bootstrap::desktop_frame::DesktopLayout` now declares the desktop as a tree (margin, main over a fixed status strip with a one-cell gap, palette centred at half size) and solves it; a test asserts the solved rectangles equal the previous hand-computed geometry.

## Rationale

The roadmap asks for a "simple layout vocabulary," not a constraint solver. One-pass integer layout is cheap enough to run per frame inside the kernel and its rounding rule is written down and tested, which matters more for a desktop than expressive power. Re-expressing the existing desktop with it proves the vocabulary covers the shapes the shell needs before Epic 7 builds on it.

## Tests

- distribution: fixed then weights with rounding, gaps, overflow truncation, zero weights, empty input
- stacks in both axes with gaps and rounding
- overlay, padding (including padding larger than the rect), all anchor behaviours and clamping of oversized children
- a nested three-pane split matching a hand layout
- kernel: solved desktop equals the previous geometry; tiny surfaces degrade to empty rects

Validated with `cargo test -p services_gui_host -p kernel_bootstrap`, `cargo build -p services_gui_host --no-default-features`, and the bare-metal kernel build.
