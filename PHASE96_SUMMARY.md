# Phase 96: Editor Rendering Performance Optimization

## Summary

Dramatically improved bare-metal editor rendering performance by implementing incremental dirty-region rendering. Before this change, every keystroke caused a full-screen clear + redraw of all lines. After this change, only changed cells are redrawn.

## Problem Analysis

### Identified Bottlenecks (from code inspection)

1. **Full screen clear every frame**: `sink.clear(normal_attr)` called whenever `output_dirty` was true
2. **Double writes per line**: Every row was first cleared with spaces (`for col in 0..cols { sink.write_at(col, viewport_row, b' ', normal_attr); }`), then content was written
3. **No dirty tracking**: All viewport lines were redrawn on every keystroke, even if only cursor moved
4. **Per-character cost**: Each character = 8×16 = 128 pixels × 4 bytes = 512 volatile writes

### Quantified Costs (before optimization)

For 80×25 VGA console:
- Full screen: 80 × 25 = 2,000 cells
- Per cell pixel cost: 8 × 16 = 128 pixels
- Per keystroke: ~2,000 cells × 128 pixels = **256,000 pixel writes**

For framebuffer (e.g., 1024×768):
- Text cells: 128 × 48 = 6,144 cells  
- Per keystroke: ~6,144 cells × 128 = **786,432 pixel writes**

## Solution: Incremental Dirty-Region Rendering

### New Module: `optimized_render.rs`

**Key Data Structures:**
- `EditorRenderCache`: Stores previous frame's cell content (char + attribute)
- `FrameRenderStats`: Tracks cells_written, lines_redrawn, full_clear for debugging

**Algorithm:**
1. Compare current editor state vs cached previous frame
2. Only redraw lines where content changed
3. For cursor-only moves: restore old cursor cell, draw new cursor cell
4. Update cache after each frame

### Performance Instrumentation: `render_stats.rs`

- Frame timing (start/end ticks)
- Pixel write counts
- Character draw counts
- Full clear vs incremental counters
- All instrumentation compiles out in release (`#[cfg(debug_assertions)]`)

## Results (measured via tests)

| Scenario | Before (cell writes) | After (cell writes) | Improvement |
|----------|---------------------|---------------------|-------------|
| Type 50 chars | ~100,000 | <15,000 | >6× faster |
| Cursor move (hjkl) | ~2,000 | <100 | >20× faster |
| No changes | ~2,000 | ~2 | 1000× faster |
| Mode change | ~2,000 | ~80 (status line only) | 25× faster |

## Tests Added

```
test optimized_render::tests::test_cache_initialization ... ok
test optimized_render::tests::test_incremental_vs_full_writes ... ok
test optimized_render::tests::test_cursor_only_change_minimal_writes ... ok
test optimized_render::tests::test_typing_50_characters_performance ... ok
test optimized_render::tests::test_hjkl_movement_performance ... ok
test render_stats::tests::test_render_stats_frame_tracking ... ok
test render_stats::tests::test_cumulative_stats ... ok
```

## Files Changed

### New Files
- `kernel_bootstrap/src/optimized_render.rs` - Incremental renderer with dirty-cell tracking
- `kernel_bootstrap/src/render_stats.rs` - Performance instrumentation (debug only)

### Modified Files
- `kernel_bootstrap/src/lib.rs` - Added new modules
- `kernel_bootstrap/src/main.rs`:
  - Added `editor_render_cache` state variable
  - Replaced inline editor render loop with `render_editor_optimized()` call
  - Both framebuffer and VGA paths now use optimized renderer

## Architecture Decisions

1. **Cache per-cell, not per-line**: Enables partial line updates (e.g., when typing in middle of line)
2. **Cursor tracked separately**: Allows O(1) cursor restore without line comparison
3. **Instrumentation behind cfg(debug_assertions)**: Zero runtime cost in release
4. **Deterministic**: Same editor state produces same rendered output
5. **No allocations during render**: Cache is pre-allocated, only resizes on dimension change

## Philosophy Alignment

- ✅ **Deterministic**: Same inputs → same visible output
- ✅ **Testability first**: All optimization logic tested under `cargo test`
- ✅ **No ambient authority**: No stdout prints, serial logs behind debug flag
- ✅ **Mechanism over policy**: Render cache is a mechanism; policy is in workspace_loop
- ✅ **Clean, modern, testable**: Small focused module with clear responsibility

## Future Optimizations (not implemented)

If further performance is needed:
- **Scroll optimization**: Use `scroll_up_text_lines()` when viewport scrolls, only redraw new lines
- **Batch pixel writes**: Write 4 pixels at once via 128-bit SSE stores
- **Row-level memset**: Use `rep stosq` for row clearing instead of per-cell writes
- **Glyph caching**: Cache rasterized font bitmaps (currently recalculated)

## Verification

```bash
# Build ISO (verifies bare-metal compilation)
cargo xtask iso

# Run library tests
cargo test --lib -p kernel_bootstrap

# Run optimization-specific tests
cargo test --lib -p kernel_bootstrap optimized_render
cargo test --lib -p kernel_bootstrap render_stats
```
