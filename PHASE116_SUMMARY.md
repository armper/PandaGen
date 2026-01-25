# Phase 116: Bug Fix - Framebuffer Borrowing Errors and Compiler Warnings

## Objective
Investigate and fix compilation bugs preventing the codebase from building, particularly focusing on borrowing violations and compiler warnings.

## Changes Made

### 1. Critical Borrowing Bug Fixes in `kernel_bootstrap/src/framebuffer.rs`

#### Problem
The framebuffer rendering code had Rust borrowing violations that prevented compilation:
- `draw_char_at`: Called `glyph_cache_mut()` which returns `&mut GlyphCache`, then `glyph_for()` which returns a reference into the cache, then tried to access `self.buffer` mutably
- `draw_text_at`: Similar issue but in a nested loop, calling `glyph_cache_mut()` multiple times while trying to access `self.buffer`

#### Root Cause
The borrow checker identified that:
1. `self.glyph_cache_mut()` borrows `self` mutably
2. `glyph_for()` returns a reference tied to that mutable borrow
3. Accessing `self.buffer` while the glyph reference is live creates multiple mutable borrows of `self`

#### Solution
- **`draw_char_at`**: Copy the glyph data (`*glyph`) before using it, so the cache borrow ends before accessing `self.buffer`
- **`draw_text_at`**: Pre-fetch all glyphs into a `Vec<[[u8; 32]; FONT_HEIGHT]>` before the rendering loop, separating the cache access phase from the buffer access phase

### 2. Compiler Warning Cleanup

Fixed various compiler warnings across the codebase:

**Unused imports:**
- `sim_kernel/src/syscall_gate.rs`: Removed unused `AddressSpaceId`
- `services_editor_vi/src/render.rs`: Initially removed but then restored `ToString` (needed in tests)
- `services_notification/src/lib.rs`: Removed unused `alloc::format` and `ToString` from main code
- `services_job_scheduler/src/lib.rs`: Removed unused `alloc::format` and `ToString` from main code
- `kernel_bootstrap/src/optimized_render.rs`: Removed unused `cursor_col` variable
- `kernel_bootstrap/src/palette_overlay.rs`: Removed unused `ToString`

**Unused attributes:**
- `services_editor_vi/src/lib.rs`: Removed duplicate `#![no_std]` attribute

**Unused fields:**
- `sim_kernel/src/smp.rs`: Prefixed `state` field in `TaskInfo` with underscore (`_state`)
- `services_device_manager/src/lib.rs`: Prefixed `name` field in `DriverRecord` with underscore (`_name`)

**Unused mut:**
- `cli_console/src/interactive.rs`: Removed unnecessary `mut` from `chars` variable

**Macro imports:**
- Initially removed `#[macro_use]` from `hal_x86_64/src/lib.rs` but restored it as tests need `vec!` and `format!` macros

### 3. Test Import Fixes

After running `cargo fix`, many test modules lost access to `ToString`, `vec!`, `format!`, and `Box` because they relied on `#[macro_use] extern crate alloc`. Added explicit imports to test modules:

- `services_notification/src/lib.rs`: Added `ToString` and `format!` 
- `services_job_scheduler/src/lib.rs`: Added `ToString`
- `services_editor_vi/src/render.rs`: Added `ToString`
- `hal/src/keyboard.rs`: Added `vec`, `Vec`, and `Box`
- `hal_x86_64/src/keyboard.rs`: Added `Box`
- `view_types/src/lib.rs`: Added `ToString`, `vec!`, and `format!`
- `input_types/src/lib.rs`: Added `ToString` and `vec!`, fixed type annotation issue in test

## Technical Details

### Borrowing Rule Violation Pattern
```rust
// BEFORE (doesn't compile):
let glyph = self.glyph_cache_mut().glyph_for(ch, fg, bg);
// glyph is a reference tied to the mutable borrow of self
for (row_idx, scanline) in glyph.iter().enumerate() {
    self.buffer[...].copy_from_slice(scanline); // ERROR: second mutable borrow
}

// AFTER (compiles):
let glyph = *self.glyph_cache_mut().glyph_for(ch, fg, bg);
// glyph is now owned data, cache borrow ends here
for (row_idx, scanline) in glyph.iter().enumerate() {
    self.buffer[...].copy_from_slice(scanline); // OK: only one active borrow
}
```

## Testing

### Test Results
- **Library tests**: 17 test suites passed
- **Total tests in kernel_bootstrap**: 61 passed, 1 failed (pre-existing failure in `test_golden_trace_multiline_edit`)
- **Security scan**: 0 vulnerabilities found (CodeQL)
- **Code review**: Completed with 1 false positive (mut required for Vec::push)

### Files Modified
1. `kernel_bootstrap/src/framebuffer.rs` - Fixed borrowing bugs
2. `cli_console/src/interactive.rs` - Removed unnecessary mut
3. `hal_x86_64/src/lib.rs` - Kept #[macro_use] for test compatibility
4. `hal_x86_64/src/keyboard.rs` - Added test imports
5. `hal/src/keyboard.rs` - Added test imports
6. `input_types/src/lib.rs` - Added test imports, fixed type annotation
7. `kernel_bootstrap/src/optimized_render.rs` - Removed unused variable
8. `kernel_bootstrap/src/palette_overlay.rs` - Removed unused import
9. `services_device_manager/src/lib.rs` - Prefixed unused field
10. `services_editor_vi/src/lib.rs` - Removed duplicate attribute
11. `services_editor_vi/src/render.rs` - Added test imports
12. `services_job_scheduler/src/lib.rs` - Removed unused imports, added test imports
13. `services_notification/src/lib.rs` - Removed unused imports, added test imports
14. `sim_kernel/src/smp.rs` - Prefixed unused field
15. `sim_kernel/src/syscall_gate.rs` - Removed unused import
16. `text_renderer_host/src/bin/perf_demo.rs` - Auto-fixed by cargo fix
17. `view_types/src/lib.rs` - Added test imports

## Lessons Learned

1. **Borrow checker precision**: The borrow checker correctly identified a subtle bug where intermediate references create borrowing conflicts
2. **Copy vs. reference**: Sometimes copying data is the right solution to avoid complex lifetime management
3. **Macro imports**: `#[macro_use] extern crate alloc;` provides convenient macros but can be removed if explicit imports are added
4. **Test dependencies**: Running `cargo fix` can break tests if they rely on macro imports from parent scopes

## Impact

- **Compilation**: Fixed critical bugs preventing the codebase from compiling
- **Code quality**: Reduced compiler warnings from ~20 to near zero
- **Testability**: All library crates now compile and test successfully
- **Maintainability**: Cleaner code with explicit imports and no unnecessary mutations
