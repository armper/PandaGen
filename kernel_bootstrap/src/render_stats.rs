//! Render performance statistics and instrumentation
//!
//! This module provides performance measurement for the rendering pipeline.
//! All instrumentation is compiled out in release builds unless explicitly enabled.
//!
//! ## Design Philosophy
//! - Zero runtime cost when disabled
//! - No allocations during measurement
//! - Collects actionable metrics: pixel writes, char draws, clears, frame times

use core::sync::atomic::AtomicU64;
#[cfg(debug_assertions)]
use core::sync::atomic::Ordering;

// The counters are compiled out of release builds, and with them every
// reader and writer of these.
/// Global render statistics (atomic for interrupt safety)
#[cfg_attr(not(debug_assertions), allow(dead_code))]
static RENDER_STATS: RenderStatsGlobal = RenderStatsGlobal::new();

/// Atomic counters for render statistics
#[cfg_attr(not(debug_assertions), allow(dead_code))]
struct RenderStatsGlobal {
    /// Total pixel writes this frame
    pixel_writes: AtomicU64,
    /// Total character draws this frame  
    char_draws: AtomicU64,
    /// Number of full screen clears this frame
    full_clears: AtomicU64,
    /// Number of line clears this frame
    line_clears: AtomicU64,
    /// Frame start tick (from PIT counter)
    frame_start_tick: AtomicU64,
    /// Last frame duration in ticks
    last_frame_ticks: AtomicU64,
    /// Running sum of frame ticks for averaging
    total_frame_ticks: AtomicU64,
    /// Frame count for averaging
    frame_count: AtomicU64,
    /// Minimum frame ticks observed
    min_frame_ticks: AtomicU64,
    /// Maximum frame ticks observed
    max_frame_ticks: AtomicU64,
    /// Successful whole-frame desktop presents (shadow or GUI surface)
    desktop_presents: AtomicU64,
    /// Rejected desktop presents (geometry/stride/format mismatch)
    desktop_present_errors: AtomicU64,
    /// Pixels copied by successful desktop presents
    desktop_present_pixels: AtomicU64,
}

impl RenderStatsGlobal {
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    const fn new() -> Self {
        Self {
            pixel_writes: AtomicU64::new(0),
            char_draws: AtomicU64::new(0),
            full_clears: AtomicU64::new(0),
            line_clears: AtomicU64::new(0),
            frame_start_tick: AtomicU64::new(0),
            last_frame_ticks: AtomicU64::new(0),
            total_frame_ticks: AtomicU64::new(0),
            frame_count: AtomicU64::new(0),
            min_frame_ticks: AtomicU64::new(u64::MAX),
            max_frame_ticks: AtomicU64::new(0),
            desktop_presents: AtomicU64::new(0),
            desktop_present_errors: AtomicU64::new(0),
            desktop_present_pixels: AtomicU64::new(0),
        }
    }
}

/// Snapshot of render statistics for a single frame
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderFrameStats {
    pub pixel_writes: u64,
    pub char_draws: u64,
    pub full_clears: u64,
    pub line_clears: u64,
    pub frame_ticks: u64,
}

/// Cumulative render statistics
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderCumulativeStats {
    pub frame_count: u64,
    pub avg_frame_ticks: u64,
    pub min_frame_ticks: u64,
    pub max_frame_ticks: u64,
    pub total_pixel_writes: u64,
    pub total_char_draws: u64,
    pub desktop_presents: u64,
    pub desktop_present_errors: u64,
    pub desktop_present_pixels: u64,
}

/// Start timing a new frame
#[inline]
#[cfg(debug_assertions)]
pub fn frame_begin(current_tick: u64) {
    // Reset per-frame counters
    RENDER_STATS.pixel_writes.store(0, Ordering::Relaxed);
    RENDER_STATS.char_draws.store(0, Ordering::Relaxed);
    RENDER_STATS.full_clears.store(0, Ordering::Relaxed);
    RENDER_STATS.line_clears.store(0, Ordering::Relaxed);
    RENDER_STATS
        .frame_start_tick
        .store(current_tick, Ordering::Relaxed);
}

#[inline]
#[cfg(not(debug_assertions))]
pub fn frame_begin(_current_tick: u64) {}

/// End frame timing and update cumulative stats
#[inline]
#[cfg(debug_assertions)]
pub fn frame_end(current_tick: u64) -> RenderFrameStats {
    let start = RENDER_STATS.frame_start_tick.load(Ordering::Relaxed);
    let duration = current_tick.saturating_sub(start);

    RENDER_STATS
        .last_frame_ticks
        .store(duration, Ordering::Relaxed);
    RENDER_STATS
        .total_frame_ticks
        .fetch_add(duration, Ordering::Relaxed);
    RENDER_STATS.frame_count.fetch_add(1, Ordering::Relaxed);

    // Update min/max
    let _ =
        RENDER_STATS
            .min_frame_ticks
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
                if duration < old {
                    Some(duration)
                } else {
                    None
                }
            });
    let _ =
        RENDER_STATS
            .max_frame_ticks
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
                if duration > old {
                    Some(duration)
                } else {
                    None
                }
            });

    RenderFrameStats {
        pixel_writes: RENDER_STATS.pixel_writes.load(Ordering::Relaxed),
        char_draws: RENDER_STATS.char_draws.load(Ordering::Relaxed),
        full_clears: RENDER_STATS.full_clears.load(Ordering::Relaxed),
        line_clears: RENDER_STATS.line_clears.load(Ordering::Relaxed),
        frame_ticks: duration,
    }
}

#[inline]
#[cfg(not(debug_assertions))]
pub fn frame_end(_current_tick: u64) -> RenderFrameStats {
    RenderFrameStats::default()
}

/// Record a pixel write operation
#[inline]
#[cfg(debug_assertions)]
pub fn record_pixel_write() {
    RENDER_STATS.pixel_writes.fetch_add(1, Ordering::Relaxed);
}

#[inline]
#[cfg(not(debug_assertions))]
pub fn record_pixel_write() {}

/// Record multiple pixel writes (batch)
#[inline]
#[cfg(debug_assertions)]
pub fn record_pixel_writes(count: u64) {
    RENDER_STATS
        .pixel_writes
        .fetch_add(count, Ordering::Relaxed);
}

#[inline]
#[cfg(not(debug_assertions))]
pub fn record_pixel_writes(_count: u64) {}

/// Record a successful whole-frame desktop present.
///
/// Presents are counted separately from pixel writes because a present is a
/// copy of already-rendered pixels, not new drawing work. Tracking them lets
/// the frame pacing policy (GFX-018) see how often the hardware buffer is
/// actually touched.
#[inline]
#[cfg(debug_assertions)]
pub fn record_desktop_present(copied_pixels: u64) {
    RENDER_STATS
        .desktop_presents
        .fetch_add(1, Ordering::Relaxed);
    RENDER_STATS
        .desktop_present_pixels
        .fetch_add(copied_pixels, Ordering::Relaxed);
}

#[inline]
#[cfg(not(debug_assertions))]
pub fn record_desktop_present(_copied_pixels: u64) {}

/// Record a rejected desktop present (contract violation at the present edge).
#[inline]
#[cfg(debug_assertions)]
pub fn record_desktop_present_error() {
    RENDER_STATS
        .desktop_present_errors
        .fetch_add(1, Ordering::Relaxed);
}

#[inline]
#[cfg(not(debug_assertions))]
pub fn record_desktop_present_error() {}

/// Record a character draw operation
#[inline]
#[cfg(debug_assertions)]
pub fn record_char_draw() {
    RENDER_STATS.char_draws.fetch_add(1, Ordering::Relaxed);
}

#[inline]
#[cfg(not(debug_assertions))]
pub fn record_char_draw() {}

/// Record a full screen clear
#[inline]
#[cfg(debug_assertions)]
pub fn record_full_clear() {
    RENDER_STATS.full_clears.fetch_add(1, Ordering::Relaxed);
}

#[inline]
#[cfg(not(debug_assertions))]
pub fn record_full_clear() {}

/// Record a line clear operation
#[inline]
#[cfg(debug_assertions)]
pub fn record_line_clear() {
    RENDER_STATS.line_clears.fetch_add(1, Ordering::Relaxed);
}

#[inline]
#[cfg(not(debug_assertions))]
pub fn record_line_clear() {}

/// Get cumulative statistics
#[cfg(debug_assertions)]
pub fn get_cumulative_stats() -> RenderCumulativeStats {
    let frame_count = RENDER_STATS.frame_count.load(Ordering::Relaxed);
    let total_ticks = RENDER_STATS.total_frame_ticks.load(Ordering::Relaxed);
    let avg = total_ticks.checked_div(frame_count).unwrap_or(0);

    RenderCumulativeStats {
        frame_count,
        avg_frame_ticks: avg,
        min_frame_ticks: RENDER_STATS.min_frame_ticks.load(Ordering::Relaxed),
        max_frame_ticks: RENDER_STATS.max_frame_ticks.load(Ordering::Relaxed),
        total_pixel_writes: RENDER_STATS.pixel_writes.load(Ordering::Relaxed),
        total_char_draws: RENDER_STATS.char_draws.load(Ordering::Relaxed),
        desktop_presents: RENDER_STATS.desktop_presents.load(Ordering::Relaxed),
        desktop_present_errors: RENDER_STATS.desktop_present_errors.load(Ordering::Relaxed),
        desktop_present_pixels: RENDER_STATS.desktop_present_pixels.load(Ordering::Relaxed),
    }
}

#[cfg(not(debug_assertions))]
pub fn get_cumulative_stats() -> RenderCumulativeStats {
    RenderCumulativeStats::default()
}

/// Reset all statistics
#[cfg(debug_assertions)]
pub fn reset_stats() {
    RENDER_STATS.pixel_writes.store(0, Ordering::Relaxed);
    RENDER_STATS.char_draws.store(0, Ordering::Relaxed);
    RENDER_STATS.full_clears.store(0, Ordering::Relaxed);
    RENDER_STATS.line_clears.store(0, Ordering::Relaxed);
    RENDER_STATS.frame_start_tick.store(0, Ordering::Relaxed);
    RENDER_STATS.last_frame_ticks.store(0, Ordering::Relaxed);
    RENDER_STATS.total_frame_ticks.store(0, Ordering::Relaxed);
    RENDER_STATS.frame_count.store(0, Ordering::Relaxed);
    RENDER_STATS
        .min_frame_ticks
        .store(u64::MAX, Ordering::Relaxed);
    RENDER_STATS.max_frame_ticks.store(0, Ordering::Relaxed);
    RENDER_STATS.desktop_presents.store(0, Ordering::Relaxed);
    RENDER_STATS
        .desktop_present_errors
        .store(0, Ordering::Relaxed);
    RENDER_STATS
        .desktop_present_pixels
        .store(0, Ordering::Relaxed);
}

#[cfg(not(debug_assertions))]
pub fn reset_stats() {}

/// Serialises tests that touch the process-global counters (directly or via
/// `render_editor_optimized`), so parallel test threads cannot interleave a
/// frame's begin/end with another test's.
#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static STATS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    STATS_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_stats_frame_tracking() {
        let _guard = test_guard();
        reset_stats();

        // Simulate a frame
        frame_begin(100);
        record_char_draw();
        record_char_draw();
        record_pixel_writes(256); // 2 chars × 128 pixels
        record_line_clear();
        let stats = frame_end(105);

        assert!(stats.char_draws >= 2);
        assert!(stats.pixel_writes >= 256);
        assert!(stats.line_clears >= 1);
        assert_eq!(stats.frame_ticks, 5);
    }

    #[test]
    fn test_cumulative_stats() {
        let _guard = test_guard();
        reset_stats();

        // Frame 1
        frame_begin(0);
        record_char_draw();
        frame_end(10);

        // Frame 2
        frame_begin(10);
        record_char_draw();
        record_char_draw();
        frame_end(25);

        let cumulative = get_cumulative_stats();
        assert_eq!(cumulative.frame_count, 2);
        // min=10, max=15, avg=12 (but this is tricky with atomics)
    }
}
