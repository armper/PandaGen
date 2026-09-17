//! Degradation policy for low memory (GFX-048).
//!
//! When the heap runs low the desktop should shed optional work in a
//! predictable order instead of failing inside an allocation: first the
//! decorations (notice cards, pointer sprite), then the graphical desktop
//! itself, falling back to the text console which needs no per-frame
//! allocation. The policy is pure over heap numbers and uses hysteresis so
//! a level is only entered or left after several consistent samples.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum MemoryPressure {
    #[default]
    Normal,
    /// Shed decorations; keep the desktop.
    Low,
    /// Leave graphics mode.
    Critical,
}

/// Thresholds in permille of total heap that must remain free.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PressureThresholds {
    pub low_free_permille: u32,
    pub critical_free_permille: u32,
    /// Bytes a desktop frame needs at minimum; below this it is critical
    /// regardless of ratio.
    pub frame_reserve_bytes: usize,
}

impl PressureThresholds {
    pub const DEFAULT: PressureThresholds = PressureThresholds {
        low_free_permille: 150,
        critical_free_permille: 50,
        frame_reserve_bytes: 256 * 1024,
    };

    pub fn classify(&self, free: usize, total: usize) -> MemoryPressure {
        if total == 0 {
            return MemoryPressure::Critical;
        }
        let permille = (free as u128 * 1000 / total as u128) as u32;
        if free < self.frame_reserve_bytes || permille < self.critical_free_permille {
            MemoryPressure::Critical
        } else if permille < self.low_free_permille {
            MemoryPressure::Low
        } else {
            MemoryPressure::Normal
        }
    }
}

impl Default for PressureThresholds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What the desktop should stop doing at a pressure level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Degradation {
    pub drop_notices: bool,
    pub hide_pointer: bool,
    pub fall_back_to_text: bool,
}

impl Degradation {
    pub const fn for_pressure(pressure: MemoryPressure) -> Self {
        match pressure {
            MemoryPressure::Normal => Self {
                drop_notices: false,
                hide_pointer: false,
                fall_back_to_text: false,
            },
            MemoryPressure::Low => Self {
                drop_notices: true,
                hide_pointer: true,
                fall_back_to_text: false,
            },
            MemoryPressure::Critical => Self {
                drop_notices: true,
                hide_pointer: true,
                fall_back_to_text: true,
            },
        }
    }
}

/// Samples heap stats and reports the settled pressure level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PressureMonitor {
    thresholds: PressureThresholds,
    /// Samples a new level must persist before it is adopted.
    hysteresis: u32,
    settled: MemoryPressure,
    candidate: MemoryPressure,
    streak: u32,
    transitions: u32,
}

impl PressureMonitor {
    pub const fn new(thresholds: PressureThresholds, hysteresis: u32) -> Self {
        Self {
            thresholds,
            hysteresis,
            settled: MemoryPressure::Normal,
            candidate: MemoryPressure::Normal,
            streak: 0,
            transitions: 0,
        }
    }

    pub const fn pressure(&self) -> MemoryPressure {
        self.settled
    }

    pub const fn transitions(&self) -> u32 {
        self.transitions
    }

    pub fn degradation(&self) -> Degradation {
        Degradation::for_pressure(self.settled)
    }

    /// Feed one sample; returns `Some(new level)` when the settled level
    /// changes. Critical is adopted immediately: waiting for confirmation
    /// while a frame could fail is the wrong trade.
    pub fn sample(&mut self, free: usize, total: usize) -> Option<MemoryPressure> {
        let observed = self.thresholds.classify(free, total);
        if observed == self.candidate {
            self.streak = self.streak.saturating_add(1);
        } else {
            self.candidate = observed;
            self.streak = 1;
        }
        let adopt = observed == MemoryPressure::Critical || self.streak >= self.hysteresis.max(1);
        if adopt && observed != self.settled {
            self.settled = observed;
            self.transitions += 1;
            return Some(observed);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classification_by_ratio_and_reserve() {
        let t = PressureThresholds::DEFAULT;
        // Tiny heaps are always below the frame reserve.
        assert_eq!(t.classify(500, 1000), MemoryPressure::Critical);
        assert_eq!(
            t.classify(200 * 1024 * 1024, 1000 * 1024 * 1024),
            MemoryPressure::Normal
        );
        assert_eq!(
            t.classify(100 * 1024 * 1024, 1000 * 1024 * 1024),
            MemoryPressure::Low
        );
        assert_eq!(
            t.classify(40 * 1024 * 1024, 1000 * 1024 * 1024),
            MemoryPressure::Critical
        );
        // Plenty by ratio but below the frame reserve is still critical.
        assert_eq!(t.classify(100 * 1024, 200 * 1024), MemoryPressure::Critical);
        assert_eq!(t.classify(0, 0), MemoryPressure::Critical);
    }

    #[test]
    fn test_degradation_order_is_monotonic() {
        let n = Degradation::for_pressure(MemoryPressure::Normal);
        let l = Degradation::for_pressure(MemoryPressure::Low);
        let c = Degradation::for_pressure(MemoryPressure::Critical);
        assert_eq!(n, Degradation::default());
        assert!(l.drop_notices && l.hide_pointer && !l.fall_back_to_text);
        assert!(c.drop_notices && c.hide_pointer && c.fall_back_to_text);
    }

    #[test]
    fn test_monitor_hysteresis_and_immediate_critical() {
        let total = 100 * 1024 * 1024;
        let mut m = PressureMonitor::new(PressureThresholds::DEFAULT, 3);
        assert_eq!(m.sample(50 * 1024 * 1024, total), None);
        // Low must persist for three samples.
        assert_eq!(m.sample(10 * 1024 * 1024, total), None);
        assert_eq!(m.sample(10 * 1024 * 1024, total), None);
        assert_eq!(m.sample(10 * 1024 * 1024, total), Some(MemoryPressure::Low));
        assert_eq!(m.pressure(), MemoryPressure::Low);
        assert!(m.degradation().drop_notices);
        // A single normal sample does not recover; three do.
        assert_eq!(m.sample(50 * 1024 * 1024, total), None);
        assert_eq!(m.sample(50 * 1024 * 1024, total), None);
        assert_eq!(
            m.sample(50 * 1024 * 1024, total),
            Some(MemoryPressure::Normal)
        );
        // Critical is adopted on the first sample.
        assert_eq!(m.sample(1024 * 1024, total), Some(MemoryPressure::Critical));
        assert!(m.degradation().fall_back_to_text);
        assert_eq!(m.transitions(), 3);
        // Recovery from critical still needs the streak.
        assert_eq!(m.sample(60 * 1024 * 1024, total), None);
        assert_eq!(m.sample(60 * 1024 * 1024, total), None);
        assert_eq!(
            m.sample(60 * 1024 * 1024, total),
            Some(MemoryPressure::Normal)
        );
    }
}
