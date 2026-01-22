//! Tick source selection and kernel tick counter.
//!
//! Provides a small abstraction for choosing PIT/HPET as the periodic
//! tick source and a deterministic tick counter for scheduler plumbing.

use core::prelude::v1::*;

use crate::port_io::PortIo;
use crate::timer::{HpetTimer, PitTimer};
use hal::{TimerDevice, TimerInterrupt};

/// Hardware tick source selection.
#[derive(Debug)]
pub enum TickSource<P: PortIo> {
    Pit(PitTimer<P>),
    Hpet(HpetTimer),
}

impl<P: PortIo> TickSource<P> {
    /// Creates a PIT-backed tick source.
    pub fn pit(port_io: P) -> Self {
        TickSource::Pit(PitTimer::new(port_io))
    }

    /// Creates an HPET-backed tick source.
    pub fn hpet() -> Self {
        TickSource::Hpet(HpetTimer::new())
    }

    /// Returns true if interrupts are enabled on the active source.
    pub fn interrupts_enabled(&self) -> bool {
        match self {
            TickSource::Pit(timer) => timer.interrupts_enabled(),
            TickSource::Hpet(timer) => timer.interrupts_enabled(),
        }
    }

    /// Returns the configured periodic frequency (if any).
    pub fn configured_hz(&self) -> Option<u32> {
        match self {
            TickSource::Pit(timer) => timer.configured_hz(),
            TickSource::Hpet(timer) => timer.configured_hz(),
        }
    }
}

impl<P: PortIo> TimerDevice for TickSource<P> {
    fn poll_ticks(&mut self) -> u64 {
        match self {
            TickSource::Pit(timer) => timer.poll_ticks(),
            TickSource::Hpet(timer) => timer.poll_ticks(),
        }
    }
}

impl<P: PortIo> TimerInterrupt for TickSource<P> {
    fn configure_periodic(&mut self, hz: u32) {
        match self {
            TickSource::Pit(timer) => timer.configure_periodic(hz),
            TickSource::Hpet(timer) => timer.configure_periodic(hz),
        }
    }

    fn enable_interrupts(&mut self) {
        match self {
            TickSource::Pit(timer) => timer.enable_interrupts(),
            TickSource::Hpet(timer) => timer.enable_interrupts(),
        }
    }

    fn disable_interrupts(&mut self) {
        match self {
            TickSource::Pit(timer) => timer.disable_interrupts(),
            TickSource::Hpet(timer) => timer.disable_interrupts(),
        }
    }
}

/// Kernel tick counter derived from a timer device.
#[derive(Debug)]
pub struct KernelTickCounter<T: TimerDevice> {
    timer: T,
    last_ticks: u64,
    total_ticks: u64,
}

impl<T: TimerDevice> KernelTickCounter<T> {
    /// Creates a new counter from the given timer device.
    pub fn new(timer: T) -> Self {
        Self {
            timer,
            last_ticks: 0,
            total_ticks: 0,
        }
    }

    /// Polls the timer and advances the kernel tick counter.
    pub fn poll(&mut self) -> u64 {
        let current = self.timer.poll_ticks();
        let delta = current.saturating_sub(self.last_ticks);
        self.total_ticks = self.total_ticks.saturating_add(delta);
        self.last_ticks = current;
        self.total_ticks
    }

    /// Returns the total kernel ticks observed so far.
    pub fn total_ticks(&self) -> u64 {
        self.total_ticks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port_io::FakePortIo;
    use crate::timer::FakeTimerDevice;

    #[test]
    fn test_tick_source_hpet_config() {
        let mut source = TickSource::<FakePortIo>::hpet();
        source.configure_periodic(1000);
        source.enable_interrupts();
        assert_eq!(source.configured_hz(), Some(1000));
        assert!(source.interrupts_enabled());
    }

    #[test]
    fn test_tick_source_pit_config() {
        let io = FakePortIo::new();
        let mut source = TickSource::pit(io);
        source.configure_periodic(500);
        source.enable_interrupts();
        assert_eq!(source.configured_hz(), Some(500));
        assert!(source.interrupts_enabled());
        source.disable_interrupts();
        assert!(!source.interrupts_enabled());
    }

    #[test]
    fn test_kernel_tick_counter_with_fake_timer() {
        let ticks = vec![0, 10, 25, 40];
        let timer = FakeTimerDevice::new(ticks);
        let mut counter = KernelTickCounter::new(timer);

        assert_eq!(counter.poll(), 0);
        assert_eq!(counter.poll(), 10);
        assert_eq!(counter.poll(), 25);
        assert_eq!(counter.poll(), 40);
        assert_eq!(counter.total_ticks(), 40);
    }
}
