//! Graphics observability (GFX-049).
//!
//! One place to count what the display path does: frames rendered, presents
//! (with tick latency), rejected presents, and the pacing decisions that
//! turn renders into presents. The kernel feeds it from the single present
//! point and the render path; `gfx stats` reads a `GfxSnapshot`.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::degrade::MemoryPressure;

/// Running counters for the display path.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GfxTelemetry {
    /// Desktop or shadow renders completed.
    pub frames_rendered: u64,
    /// Successful hardware presents.
    pub presents: u64,
    /// Presents rejected by the framebuffer contract.
    pub presents_rejected: u64,
    /// Sum of present latencies in ticks (for the average).
    pub present_ticks_total: u64,
    /// Worst single present in ticks.
    pub present_ticks_max: u64,
    /// Presents that took more than one tick (a frame that missed its slot).
    pub slow_presents: u64,
    /// Sum of present latencies in TSC cycles (finer than ticks).
    pub present_cycles_total: u64,
    /// CPUs that shared the most recent present (1 = boot CPU alone).
    pub present_workers: u32,
    /// Redraws requested by the animation clock.
    pub animation_wakes: u64,
    /// Pointer events routed to the desktop.
    pub pointer_events: u64,
    /// Time spent building and compositing frames (GFX-120): ticks in
    /// all, the worst frame, and TSC cycles in all.
    pub render_ticks_total: u64,
    pub render_ticks_max: u64,
    pub render_cycles_total: u64,
    /// Pixels repainted, and the pixels the frames had in all: how much
    /// of the screen damage-limited rendering actually touched.
    pub repainted_pixels: u64,
    pub frame_pixels: u64,
    /// Frames that had to repaint the whole screen.
    pub full_repaints: u64,
}

impl GfxTelemetry {
    pub const fn new() -> Self {
        Self {
            frames_rendered: 0,
            presents: 0,
            presents_rejected: 0,
            present_ticks_total: 0,
            present_ticks_max: 0,
            slow_presents: 0,
            present_cycles_total: 0,
            present_workers: 1,
            animation_wakes: 0,
            pointer_events: 0,
            render_ticks_total: 0,
            render_ticks_max: 0,
            render_cycles_total: 0,
            repainted_pixels: 0,
            frame_pixels: 0,
            full_repaints: 0,
        }
    }

    /// A frame rendered (GFX-120): how long it took, how many pixels it
    /// repainted of how many, and whether it was the whole screen.
    pub fn record_render(&mut self, ticks: u64, cycles: u64, repainted: u64, of: u64) {
        self.render_ticks_total += ticks;
        self.render_ticks_max = self.render_ticks_max.max(ticks);
        self.render_cycles_total += cycles;
        self.repainted_pixels += repainted;
        self.frame_pixels += of;
        if repainted >= of {
            self.full_repaints += 1;
        }
    }

    pub fn record_frame(&mut self) {
        self.frames_rendered += 1;
    }

    /// Record a present that took `ticks`.
    pub fn record_present(&mut self, ticks: u64) {
        self.presents += 1;
        self.present_ticks_total += ticks;
        self.present_ticks_max = self.present_ticks_max.max(ticks);
        if ticks > 1 {
            self.slow_presents += 1;
        }
    }

    /// Cycle-level timing for the present just recorded and how many CPUs
    /// shared it.
    pub fn record_present_cycles(&mut self, cycles: u64, workers: u32) {
        self.present_cycles_total += cycles;
        self.present_workers = workers.max(1);
    }

    /// Average present latency in cycles.
    pub fn present_cycles_avg(&self) -> u64 {
        if self.presents == 0 {
            0
        } else {
            self.present_cycles_total / self.presents
        }
    }

    pub fn record_present_rejected(&mut self) {
        self.presents_rejected += 1;
    }

    pub fn record_animation_wake(&mut self) {
        self.animation_wakes += 1;
    }

    pub fn record_pointer_event(&mut self) {
        self.pointer_events += 1;
    }

    /// Average present latency in hundredths of a tick.
    pub fn present_ticks_avg_centi(&self) -> u64 {
        if self.presents == 0 {
            0
        } else {
            self.present_ticks_total * 100 / self.presents
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// Everything `gfx stats` shows, gathered once per frame by the loop.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GfxSnapshot {
    pub telemetry: GfxTelemetry,
    /// Pacer counters.
    pub pacer_presents: u64,
    pub pacer_deferred: u64,
    pub pacer_coalesced: u64,
    pub pacer_forced: u64,
    pub pressure: MemoryPressure,
    pub pressure_transitions: u32,
    pub heap_used: usize,
    pub heap_free: usize,
    pub heap_total: usize,
    pub budget_used: usize,
    pub budget_limit: usize,
    /// Tick the snapshot was taken.
    pub tick: u64,
}

impl GfxSnapshot {
    /// Human-readable lines for the workspace output.
    pub fn lines(&self) -> Vec<String> {
        let t = &self.telemetry;
        let avg = t.present_ticks_avg_centi();
        alloc::vec![
            format!(
                "gfx: frames={} presents={} rejected={} slow={} avg_present={}.{:02} ticks max={} ticks",
                t.frames_rendered,
                t.presents,
                t.presents_rejected,
                t.slow_presents,
                avg / 100,
                avg % 100,
                t.present_ticks_max
            ),
            format!(
                "pacer: presents={} deferred={} coalesced={} forced={} anim_wakes={} pointer_events={}",
                self.pacer_presents,
                self.pacer_deferred,
                self.pacer_coalesced,
                self.pacer_forced,
                t.animation_wakes,
                t.pointer_events
            ),
            format!(
                "present: workers={} avg_cycles={}",
                t.present_workers,
                t.present_cycles_avg()
            ),
            {
                let frames = t.frames_rendered.max(1);
                let ticks = t.render_ticks_total * 100 / frames;
                format!(
                    "render: avg={}.{:02} ticks max={} ticks avg_cycles={} repainted={}% full={}",
                    ticks / 100,
                    ticks % 100,
                    t.render_ticks_max,
                    t.render_cycles_total / frames,
                    (t.repainted_pixels * 100).checked_div(t.frame_pixels).unwrap_or(0),
                    t.full_repaints
                )
            },
            format!(
                "memory: pressure={:?} transitions={} heap {}/{} KiB used, budget {}/{} KiB, tick={}",
                self.pressure,
                self.pressure_transitions,
                self.heap_used / 1024,
                self.heap_total / 1024,
                self.budget_used / 1024,
                self.budget_limit / 1024,
                self.tick
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counters_and_average() {
        let mut t = GfxTelemetry::new();
        assert_eq!(t.present_ticks_avg_centi(), 0);
        t.record_frame();
        t.record_frame();
        t.record_present(0);
        t.record_present(1);
        t.record_present(4);
        t.record_present_rejected();
        t.record_animation_wake();
        t.record_pointer_event();
        assert_eq!(t.frames_rendered, 2);
        assert_eq!(t.presents, 3);
        assert_eq!(t.presents_rejected, 1);
        assert_eq!(
            t.slow_presents, 1,
            "only the 4-tick present missed its slot"
        );
        assert_eq!(t.present_ticks_max, 4);
        assert_eq!(t.present_ticks_avg_centi(), 166);
        t.reset();
        assert_eq!(t, GfxTelemetry::new());
    }

    #[test]
    fn test_snapshot_lines_are_complete_and_readable() {
        let mut snapshot = GfxSnapshot::default();
        snapshot.telemetry.record_present(2);
        snapshot.pacer_deferred = 7;
        snapshot.pressure = MemoryPressure::Low;
        snapshot.heap_used = 8 * 1024 * 1024;
        snapshot.heap_total = 32 * 1024 * 1024;
        snapshot.budget_limit = 24 * 1024 * 1024;
        snapshot.tick = 1234;
        let lines = snapshot.lines();
        assert_eq!(lines.len(), 5);
        assert!(
            lines[0].starts_with("gfx: frames=0 presents=1 rejected=0 slow=1 avg_present=2.00"),
            "{}",
            lines[0]
        );
        assert!(lines[1].contains("deferred=7"));
        assert!(lines[2].starts_with("present: workers="));
        assert!(
            lines[3].starts_with("render: avg=0.00 ticks"),
            "{}",
            lines[3]
        );
        assert!(lines[4].contains("pressure=Low"));
        assert!(lines[4].contains("heap 8192/32768 KiB"));
        assert!(lines[4].contains("budget 0/24576 KiB"));
        assert!(lines[4].ends_with("tick=1234"));
    }

    #[test]
    fn render_telemetry_says_how_much_of_the_screen_was_repainted() {
        let mut t = GfxTelemetry::new();
        t.record_frame();
        t.record_render(30, 1_000, 100, 100);
        t.record_frame();
        t.record_render(2, 100, 10, 100);
        let snapshot = GfxSnapshot {
            telemetry: t,
            ..Default::default()
        };
        let line = &snapshot.lines()[3];
        assert!(
            line.starts_with("render: avg=16.00 ticks max=30 ticks"),
            "{line}"
        );
        assert!(line.ends_with("repainted=55% full=1"), "{line}");
    }

    #[test]
    fn present_cycles_average_and_workers() {
        let mut t = GfxTelemetry::new();
        assert_eq!(t.present_workers, 1);
        assert_eq!(t.present_cycles_avg(), 0);
        t.record_present(0);
        t.record_present_cycles(1_000, 4);
        t.record_present(1);
        t.record_present_cycles(3_000, 4);
        assert_eq!(t.present_cycles_avg(), 2_000);
        assert_eq!(t.present_workers, 4);
        let snapshot = GfxSnapshot {
            telemetry: t,
            ..GfxSnapshot::default()
        };
        assert!(snapshot
            .lines()
            .iter()
            .any(|l| l == "present: workers=4 avg_cycles=2000"));
    }
}
