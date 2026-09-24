# Phase 367: the Calculator card (GFX-075)

## What changed

- `kernel_bootstrap/src/calculator.rs`: a Calculator that is a pure state
  machine. Exact decimal arithmetic on `i128` scaled by 10^12 (no
  floating point in the kernel, and `0.1 + 0.2` is `0.3`); `+ - * /`,
  parentheses, unary minus; errors say why ("Divide by zero",
  "Unbalanced ( )", "Incomplete", "Too big").
- The card is text like every card: expression and result on the first
  two lines, a five-by-four key grid (`[ 7 ]` cells), then a tape of the
  last six results. A content click is mapped back from line and column
  to the key, so the pointer and the keyboard reach the same `press`.
- Keyboard: digits and operators, Enter is `=`, Backspace edits, Esc
  clears, `x` is `*`. After a result, a digit starts over and an operator
  carries the result on.
- Desk: `DeskApp::Calculator` on the dock (fifth tile, "Ca"), a palette
  row, chips Clear and Close, an overview line with the current result.
- Tests no longer hard-code a four-tile dock: the row width is computed
  from `DeskApp::ALL`, so the next app does not re-pin seven tests.
- Gauntlet: the dock pixel pins moved with the fifth tile; a new shape
  opens the Calculator from the palette and types `12*3`.

## Why this shape

A calculator card with drawn buttons would need a widget toolkit the
compositor does not have and the desk has not needed. Text cells that
are hit-tested by line and column give a mouse-only calculator today
with no new primitive, and the same code path as typing.

## Tests

- `calculator::tests::arithmetic_is_exact_decimal_with_precedence_and_parentheses`
- `calculator::tests::keys_clicked_and_typed_reach_the_same_expression_and_the_tape`
- `desk::tests::the_calculator_takes_clicks_on_its_keys_and_typed_keys_alike`
- Machine: `cargo xtask gauntlet` exit 0.
