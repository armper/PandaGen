//! Animation timing hooks (GFX-030).
//!
//! PandaGen's desktop does not run a per-frame render loop. Rendering happens
//! when state changes and presenting is paced by ticks. Animation therefore
//! has to be expressed as *time-indexed state* the loop can sample and as a
//! *next-wake tick* the loop can schedule against, not as a callback per
//! frame. These types give exactly that:
//!
//! - `Transition`: a value moving from `from` to `to` over a tick window with
//!   an easing curve; sample it with `value_at(tick)`.
//! - `Blink`: a periodic on/off phase (caret, attention badge).
//! - `AnimationClock`: collects the next tick at which anything changes so the
//!   loop knows whether to mark the desktop dirty.
//!
//! Everything is integer-only over a monotonic `u64` tick counter and needs
//! no allocation.

use serde::{Deserialize, Serialize};

/// Fixed-point progress: 0 = start, `PROGRESS_ONE` = finished.
pub const PROGRESS_ONE: u32 = 1000;

/// Easing curves over fixed-point progress.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Easing {
    #[default]
    Linear,
    /// Decelerating: fast start, gentle stop (1 - (1-t)^2).
    EaseOut,
    /// Accelerating: gentle start, fast stop (t^2).
    EaseIn,
    /// Smooth both ends (3t^2 - 2t^3).
    EaseInOut,
    /// Jump to the end value at the last tick.
    Step,
}

impl Easing {
    /// Map linear progress (0..=1000) through the curve (0..=1000).
    pub fn apply(self, t: u32) -> u32 {
        let t = t.min(PROGRESS_ONE) as u64;
        let one = PROGRESS_ONE as u64;
        let out = match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t / one,
            Easing::EaseOut => {
                let inv = one - t;
                one - inv * inv / one
            }
            Easing::EaseInOut => {
                // 3t^2 - 2t^3 in fixed point.
                let t2 = t * t / one;
                let t3 = t2 * t / one;
                (3 * t2).saturating_sub(2 * t3)
            }
            Easing::Step => {
                if t >= one {
                    one
                } else {
                    0
                }
            }
        };
        out.min(one) as u32
    }
}

/// A value moving between two integers over a tick window.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transition {
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub from: i64,
    pub to: i64,
    pub easing: Easing,
}

impl Transition {
    pub const fn new(start_tick: u64, duration_ticks: u64, from: i64, to: i64) -> Self {
        Self {
            start_tick,
            duration_ticks,
            from,
            to,
            easing: Easing::Linear,
        }
    }

    pub const fn with_easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    pub const fn end_tick(&self) -> u64 {
        self.start_tick.saturating_add(self.duration_ticks)
    }

    /// Linear progress at `tick`, clamped to the window.
    pub fn progress_at(&self, tick: u64) -> u32 {
        if tick <= self.start_tick {
            return 0;
        }
        if self.duration_ticks == 0 || tick >= self.end_tick() {
            return PROGRESS_ONE;
        }
        ((tick - self.start_tick) * PROGRESS_ONE as u64 / self.duration_ticks) as u32
    }

    /// Eased value at `tick`; exactly `from` before the start and exactly
    /// `to` at or after the end.
    pub fn value_at(&self, tick: u64) -> i64 {
        let eased = self.easing.apply(self.progress_at(tick)) as i64;
        let span = self.to - self.from;
        // Round to nearest so the midpoint of 0..10 is 5, not 4.
        let offset = (span * eased + (PROGRESS_ONE as i64) / 2).div_euclid(PROGRESS_ONE as i64);
        if eased >= PROGRESS_ONE as i64 {
            self.to
        } else {
            self.from + offset
        }
    }

    pub fn is_finished_at(&self, tick: u64) -> bool {
        tick >= self.end_tick()
    }

    /// Next tick at which the value can change, if still running.
    pub fn next_change_after(&self, tick: u64) -> Option<u64> {
        if self.is_finished_at(tick) {
            None
        } else if tick < self.start_tick {
            Some(self.start_tick)
        } else {
            Some(tick + 1)
        }
    }
}

/// Periodic on/off phase.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Blink {
    pub period_ticks: u64,
    /// Phase origin; the blink is "on" starting here.
    pub phase_tick: u64,
}

impl Blink {
    pub const fn new(period_ticks: u64) -> Self {
        Self {
            period_ticks,
            phase_tick: 0,
        }
    }

    /// Restart the cycle so the blink is on at `tick` (e.g. after typing).
    pub fn restart_at(&mut self, tick: u64) {
        self.phase_tick = tick;
    }

    /// True during the first half of each period. A zero period is always on.
    pub fn is_on_at(&self, tick: u64) -> bool {
        if self.period_ticks == 0 {
            return true;
        }
        let elapsed = tick.wrapping_sub(self.phase_tick) % self.period_ticks;
        elapsed < self.period_ticks.div_ceil(2)
    }

    /// Next tick at which `is_on_at` flips.
    pub fn next_flip_after(&self, tick: u64) -> Option<u64> {
        if self.period_ticks == 0 {
            return None;
        }
        let half = self.period_ticks.div_ceil(2);
        let elapsed = tick.wrapping_sub(self.phase_tick) % self.period_ticks;
        let until = if elapsed < half {
            half - elapsed
        } else {
            self.period_ticks - elapsed
        };
        Some(tick + until)
    }
}

/// Collects the earliest tick at which any registered animation changes.
///
/// The loop calls `poll(now)`: `true` means something is due and the desktop
/// should be re-rendered; `next_wake()` tells the pacer how long it may idle.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AnimationClock {
    next_wake: Option<u64>,
}

impl AnimationClock {
    pub const fn new() -> Self {
        Self { next_wake: None }
    }

    pub const fn next_wake(&self) -> Option<u64> {
        self.next_wake
    }

    /// Register a future change at `tick` (keeps the earliest).
    pub fn wake_at(&mut self, tick: u64) {
        self.next_wake = Some(match self.next_wake {
            Some(existing) => existing.min(tick),
            None => tick,
        });
    }

    /// Register whatever a transition or blink reports next.
    pub fn wake_after(&mut self, next: Option<u64>) {
        if let Some(tick) = next {
            self.wake_at(tick);
        }
    }

    /// Consume a due wake. Returns true when `now` has reached it.
    pub fn poll(&mut self, now: u64) -> bool {
        match self.next_wake {
            Some(tick) if now >= tick => {
                self.next_wake = None;
                true
            }
            _ => false,
        }
    }

    pub fn clear(&mut self) {
        self.next_wake = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_easing_endpoints_and_shapes() {
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
            Easing::Step,
        ] {
            assert_eq!(easing.apply(0), 0, "{easing:?}");
            assert_eq!(easing.apply(PROGRESS_ONE), PROGRESS_ONE, "{easing:?}");
            assert_eq!(easing.apply(5000), PROGRESS_ONE, "{easing:?} clamps");
            // Monotonic.
            let mut last = 0;
            for t in (0..=PROGRESS_ONE).step_by(50) {
                let v = easing.apply(t);
                assert!(v >= last, "{easing:?} not monotonic at {t}");
                last = v;
            }
        }
        assert_eq!(Easing::Linear.apply(500), 500);
        assert_eq!(Easing::EaseIn.apply(500), 250);
        assert_eq!(Easing::EaseOut.apply(500), 750);
        assert_eq!(Easing::EaseInOut.apply(500), 500);
        assert!(Easing::EaseInOut.apply(250) < 250);
        assert!(Easing::EaseInOut.apply(750) > 750);
        assert_eq!(Easing::Step.apply(999), 0);
    }

    #[test]
    fn test_transition_samples_exact_endpoints_and_midpoint() {
        let t = Transition::new(100, 10, 0, 10);
        assert_eq!(t.value_at(0), 0);
        assert_eq!(t.value_at(100), 0);
        assert_eq!(t.value_at(105), 5);
        assert_eq!(t.value_at(110), 10);
        assert_eq!(t.value_at(500), 10);
        assert!(!t.is_finished_at(109));
        assert!(t.is_finished_at(110));
        assert_eq!(t.next_change_after(50), Some(100));
        assert_eq!(t.next_change_after(104), Some(105));
        assert_eq!(t.next_change_after(110), None);

        // Reverse direction and negative values.
        let back = Transition::new(0, 4, 20, -20);
        assert_eq!(back.value_at(2), 0);
        assert_eq!(back.value_at(4), -20);

        // Zero duration jumps immediately after the start tick.
        let jump = Transition::new(7, 0, 1, 9);
        assert_eq!(jump.value_at(7), 1);
        assert_eq!(jump.value_at(8), 9);

        // Eased midpoint lands ahead of linear for EaseOut.
        let eased = Transition::new(0, 100, 0, 1000).with_easing(Easing::EaseOut);
        assert_eq!(eased.value_at(50), 750);
        assert_eq!(eased.value_at(100), 1000);
    }

    #[test]
    fn test_blink_phase_and_next_flip() {
        let mut blink = Blink::new(60);
        assert!(blink.is_on_at(0));
        assert!(blink.is_on_at(29));
        assert!(!blink.is_on_at(30));
        assert!(!blink.is_on_at(59));
        assert!(blink.is_on_at(60));
        assert_eq!(blink.next_flip_after(0), Some(30));
        assert_eq!(blink.next_flip_after(29), Some(30));
        assert_eq!(blink.next_flip_after(30), Some(60));
        assert_eq!(blink.next_flip_after(61), Some(90));

        // Restarting puts the blink in its "on" phase at that tick.
        blink.restart_at(45);
        assert!(blink.is_on_at(45));
        assert!(blink.is_on_at(74));
        assert!(!blink.is_on_at(75));

        // Odd periods round the on-phase up; zero period is always on.
        let odd = Blink::new(5);
        assert!(odd.is_on_at(2));
        assert!(!odd.is_on_at(3));
        let solid = Blink::new(0);
        assert!(solid.is_on_at(12345));
        assert_eq!(solid.next_flip_after(3), None);
    }

    #[test]
    fn test_clock_keeps_earliest_wake_and_fires_once() {
        let mut clock = AnimationClock::new();
        assert!(!clock.poll(10));
        clock.wake_at(50);
        clock.wake_at(30);
        clock.wake_after(None);
        clock.wake_after(Some(80));
        assert_eq!(clock.next_wake(), Some(30));
        assert!(!clock.poll(29));
        assert!(clock.poll(30));
        assert_eq!(clock.next_wake(), None, "a fired wake is consumed");
        assert!(!clock.poll(31));

        // Typical loop usage: register the blink's next flip each frame.
        let blink = Blink::new(10);
        let mut clock = AnimationClock::new();
        let mut redraws = 0;
        for tick in 0..30 {
            if clock.poll(tick) || tick == 0 {
                redraws += 1;
                clock.wake_after(blink.next_flip_after(tick));
            }
        }
        assert_eq!(
            redraws, 6,
            "one redraw per half period plus the first frame"
        );
    }
}
