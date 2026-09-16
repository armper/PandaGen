//! Frame pacing and present policy (GFX-018).
//!
//! Rendering and presenting are separate steps in PandaGen's display path:
//! services and the workspace loop *render* into a shadow surface whenever
//! state changes, but the hardware framebuffer is only *presented* to when
//! this policy says so. That keeps hardware writes explicit and bounded no
//! matter how many render passes happen between two ticks.
//!
//! The pacer is pure logic over a monotonic tick counter, so the policy is
//! fully covered by `cargo test` without a timer or a framebuffer.
//!
//! ## Model
//!
//! - `mark_dirty()` records that the shadow holds content not yet presented.
//! - `poll(now)` asks whether to present *now*. At most one present is
//!   allowed per `min_interval_ticks`; dirty marks that arrive inside the
//!   interval coalesce into the next present.
//! - `force(now)` presents immediately regardless of pacing. Used for
//!   mode switches where a stale frame must never be visible.

/// Presentation pacing parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentPolicy {
    /// Minimum ticks between two presents. `0` presents on every dirty poll.
    pub min_interval_ticks: u64,
}

impl PresentPolicy {
    /// Present as soon as content is dirty (no rate cap).
    pub const IMMEDIATE: PresentPolicy = PresentPolicy {
        min_interval_ticks: 0,
    };

    /// Default bare-metal policy: one present per PIT tick (100 Hz PIT means
    /// a 100 fps ceiling), which is well above what the text workspace needs
    /// while still bounding full-frame copies during key repeat bursts.
    pub const DEFAULT_BARE_METAL: PresentPolicy = PresentPolicy {
        min_interval_ticks: 1,
    };

    pub const fn new(min_interval_ticks: u64) -> Self {
        Self { min_interval_ticks }
    }
}

/// What the caller should do after a `poll`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentDecision {
    /// Nothing is dirty; do not touch the framebuffer.
    Idle,
    /// Content is dirty but the interval has not elapsed. `due_tick` is the
    /// earliest tick at which `poll` will return `Present`.
    Defer { due_tick: u64 },
    /// Present the shadow now. The pacer already recorded this present.
    Present,
}

/// Cumulative pacer statistics for observability (GFX-049 groundwork).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PacerStats {
    /// Presents granted by `poll` or `force`.
    pub presents: u64,
    /// Presents granted by `force` (subset of `presents`).
    pub forced_presents: u64,
    /// Dirty marks that were absorbed into an already-pending present.
    pub coalesced_marks: u64,
    /// Polls that returned `Defer`.
    pub deferred_polls: u64,
}

/// Stateful frame pacer over a monotonic tick counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePacer {
    policy: PresentPolicy,
    last_present_tick: Option<u64>,
    pending: bool,
    stats: PacerStats,
}

impl FramePacer {
    pub const fn new(policy: PresentPolicy) -> Self {
        Self {
            policy,
            last_present_tick: None,
            pending: false,
            stats: PacerStats {
                presents: 0,
                forced_presents: 0,
                coalesced_marks: 0,
                deferred_polls: 0,
            },
        }
    }

    pub const fn policy(&self) -> PresentPolicy {
        self.policy
    }

    pub const fn stats(&self) -> PacerStats {
        self.stats
    }

    /// True when the shadow holds content that has not been presented yet.
    pub const fn is_pending(&self) -> bool {
        self.pending
    }

    /// Record that a render pass produced content not yet on screen.
    pub fn mark_dirty(&mut self) {
        if self.pending {
            self.stats.coalesced_marks += 1;
        }
        self.pending = true;
    }

    /// Earliest tick at which a present is allowed. `None` when no present
    /// has happened yet, meaning the first present is always immediate.
    pub fn next_allowed_tick(&self) -> Option<u64> {
        self.last_present_tick
            .map(|last| last.saturating_add(self.policy.min_interval_ticks))
    }

    /// Decide whether to present at `now`.
    ///
    /// A `now` earlier than the last present (counter reset) is treated as
    /// due, so a broken clock degrades to "present when dirty" rather than
    /// freezing the display.
    pub fn poll(&mut self, now: u64) -> PresentDecision {
        if !self.pending {
            return PresentDecision::Idle;
        }
        match self.next_allowed_tick() {
            Some(due) if now < due && now >= self.last_present_tick.unwrap_or(0) => {
                self.stats.deferred_polls += 1;
                PresentDecision::Defer { due_tick: due }
            }
            _ => {
                self.grant(now, false);
                PresentDecision::Present
            }
        }
    }

    /// Present immediately, ignoring the interval. Clears any pending mark.
    pub fn force(&mut self, now: u64) {
        self.grant(now, true);
    }

    fn grant(&mut self, now: u64, forced: bool) {
        self.pending = false;
        self.last_present_tick = Some(now);
        self.stats.presents += 1;
        if forced {
            self.stats.forced_presents += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idle_pacer_never_presents() {
        let mut pacer = FramePacer::new(PresentPolicy::DEFAULT_BARE_METAL);
        for now in 0..10 {
            assert_eq!(pacer.poll(now), PresentDecision::Idle);
        }
        assert_eq!(pacer.stats(), PacerStats::default());
    }

    #[test]
    fn test_first_present_is_immediate() {
        let mut pacer = FramePacer::new(PresentPolicy::new(50));
        pacer.mark_dirty();
        assert_eq!(pacer.poll(7), PresentDecision::Present);
        assert!(!pacer.is_pending());
        assert_eq!(pacer.next_allowed_tick(), Some(57));
    }

    #[test]
    fn test_dirty_marks_within_interval_coalesce_into_one_present() {
        let mut pacer = FramePacer::new(PresentPolicy::new(2));
        pacer.mark_dirty();
        assert_eq!(pacer.poll(10), PresentDecision::Present);

        // Three renders inside the interval: all deferred, one present later.
        pacer.mark_dirty();
        assert_eq!(pacer.poll(10), PresentDecision::Defer { due_tick: 12 });
        pacer.mark_dirty();
        assert_eq!(pacer.poll(11), PresentDecision::Defer { due_tick: 12 });
        pacer.mark_dirty();
        assert_eq!(pacer.poll(12), PresentDecision::Present);
        assert_eq!(pacer.poll(12), PresentDecision::Idle);

        let stats = pacer.stats();
        assert_eq!(stats.presents, 2);
        assert_eq!(stats.coalesced_marks, 2);
        assert_eq!(stats.deferred_polls, 2);
    }

    #[test]
    fn test_present_rate_is_bounded_under_continuous_dirtying() {
        let mut pacer = FramePacer::new(PresentPolicy::new(3));
        let mut presents = 0;
        for now in 0..30 {
            pacer.mark_dirty();
            if pacer.poll(now) == PresentDecision::Present {
                presents += 1;
            }
        }
        // ticks 0,3,6,...,27 => 10 presents for 30 ticks.
        assert_eq!(presents, 10);
        assert!(pacer.is_pending());
    }

    #[test]
    fn test_immediate_policy_presents_every_dirty_poll() {
        let mut pacer = FramePacer::new(PresentPolicy::IMMEDIATE);
        for _ in 0..5 {
            pacer.mark_dirty();
            assert_eq!(pacer.poll(42), PresentDecision::Present);
        }
        assert_eq!(pacer.stats().presents, 5);
        assert_eq!(pacer.stats().deferred_polls, 0);
    }

    #[test]
    fn test_force_presents_inside_interval_and_clears_pending() {
        let mut pacer = FramePacer::new(PresentPolicy::new(100));
        pacer.mark_dirty();
        assert_eq!(pacer.poll(0), PresentDecision::Present);
        pacer.mark_dirty();
        assert_eq!(pacer.poll(1), PresentDecision::Defer { due_tick: 100 });
        pacer.force(1);
        assert!(!pacer.is_pending());
        assert_eq!(pacer.poll(2), PresentDecision::Idle);
        assert_eq!(pacer.stats().forced_presents, 1);
        assert_eq!(pacer.stats().presents, 2);
        // The forced present restarts the interval.
        assert_eq!(pacer.next_allowed_tick(), Some(101));
    }

    #[test]
    fn test_clock_going_backwards_does_not_freeze_display() {
        let mut pacer = FramePacer::new(PresentPolicy::new(5));
        pacer.mark_dirty();
        assert_eq!(pacer.poll(1_000), PresentDecision::Present);
        pacer.mark_dirty();
        assert_eq!(pacer.poll(3), PresentDecision::Present);
    }

    #[test]
    fn test_interval_saturates_near_tick_max() {
        let mut pacer = FramePacer::new(PresentPolicy::new(u64::MAX));
        pacer.mark_dirty();
        assert_eq!(pacer.poll(10), PresentDecision::Present);
        assert_eq!(pacer.next_allowed_tick(), Some(u64::MAX));
        pacer.mark_dirty();
        assert_eq!(
            pacer.poll(u64::MAX - 1),
            PresentDecision::Defer { due_tick: u64::MAX }
        );
        assert_eq!(pacer.poll(u64::MAX), PresentDecision::Present);
    }
}
