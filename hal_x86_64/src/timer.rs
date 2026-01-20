//! # x86_64 Timer Devices
//!
//! Hardware timer implementations for x86_64 architecture.
//!
//! ## Implementations
//!
//! - **FakeTimerDevice**: For testing without hardware
//! - **PitTimer**: 8254 Programmable Interval Timer (PIT)
//!
//! ## Design Notes
//!
//! - All implementations are non-blocking (polling only)
//! - Monotonicity is enforced
//! - Minimal unsafe code, isolated to port I/O

use hal::{TimerDevice, TimerInterrupt};

/// Fake timer device for testing
///
/// This device allows scripted tick sequences for testing
/// without real hardware. It enforces monotonicity.
///
/// # Examples
///
/// ```
/// use hal_x86_64::timer::FakeTimerDevice;
/// use hal::TimerDevice;
///
/// let ticks = vec![0, 100, 200, 350];
/// let mut timer = FakeTimerDevice::new(ticks);
///
/// assert_eq!(timer.poll_ticks(), 0);
/// assert_eq!(timer.poll_ticks(), 100);
/// assert_eq!(timer.poll_ticks(), 200);
/// assert_eq!(timer.poll_ticks(), 350);
/// assert_eq!(timer.poll_ticks(), 350); // Stays at last value
/// ```
#[derive(Debug)]
pub struct FakeTimerDevice {
    /// Scripted tick values to return
    ticks: Vec<u64>,
    /// Current index in the ticks array
    index: usize,
}

impl FakeTimerDevice {
    /// Creates a new fake timer with scripted tick values
    ///
    /// # Arguments
    ///
    /// * `ticks` - Sequence of tick values to return. Must be monotonic.
    ///
    /// # Panics
    ///
    /// Panics if `ticks` is not monotonic (each value must be >= previous).
    pub fn new(ticks: Vec<u64>) -> Self {
        // Verify monotonicity
        for i in 1..ticks.len() {
            assert!(
                ticks[i] >= ticks[i - 1],
                "Tick sequence must be monotonic: {} < {} at index {}",
                ticks[i],
                ticks[i - 1],
                i
            );
        }

        Self { ticks, index: 0 }
    }

    /// Returns the number of remaining scripted values
    pub fn remaining(&self) -> usize {
        if self.index < self.ticks.len() {
            self.ticks.len() - self.index
        } else {
            0
        }
    }
}

impl TimerDevice for FakeTimerDevice {
    fn poll_ticks(&mut self) -> u64 {
        if self.index < self.ticks.len() {
            let value = self.ticks[self.index];
            self.index += 1;
            value
        } else if !self.ticks.is_empty() {
            // Return last value if we've exhausted the sequence
            *self.ticks.last().unwrap()
        } else {
            0
        }
    }
}

/// 8254 Programmable Interval Timer (PIT)
///
/// The PIT is a legacy timer device present on all x86 systems.
/// This implementation uses Channel 0 in mode 3 (square wave generator).
///
/// ## Hardware Details
///
/// - Base frequency: ~1.193182 MHz
/// - We configure it for ~1ms ticks (divisor 1193)
/// - This gives us approximately 838 ticks per second (close enough to 1 kHz)
///
/// ## Implementation Notes
///
/// - Non-blocking: Always returns immediately
/// - Monotonic: Tracks cumulative ticks
/// - Minimal unsafe: Only for port I/O
///
/// ## Limitations
///
/// - Low resolution (~1ms)
/// - Can wrap around (but we track cumulative count)
/// - Legacy device (but universally available)
#[derive(Debug)]
pub struct PitTimer<P: super::port_io::PortIo> {
    /// Port I/O interface
    port_io: P,
    /// Cumulative tick count
    cumulative_ticks: u64,
    /// Last counter value we read (for delta calculation)
    last_counter: u16,
    /// Whether the timer has been initialized
    initialized: bool,
    /// Whether interrupts are enabled
    interrupts_enabled: bool,
    /// Configured periodic interrupt frequency
    configured_hz: Option<u32>,
}

impl<P: super::port_io::PortIo> PitTimer<P> {
    /// PIT Channel 0 data port
    const CHANNEL_0_DATA: u16 = 0x40;
    /// PIT command register
    const COMMAND: u16 = 0x43;
    /// Reload value for ~1ms ticks (1193182 Hz / 1193 ≈ 1000 Hz)
    const RELOAD_VALUE: u16 = 1193;

    /// Creates a new PIT timer with the given port I/O interface
    ///
    /// Note: This does not initialize the hardware. Call `initialize()`
    /// or let `poll_ticks()` initialize it on first call.
    pub fn new(port_io: P) -> Self {
        Self {
            port_io,
            cumulative_ticks: 0,
            last_counter: 0,
            initialized: false,
            interrupts_enabled: false,
            configured_hz: None,
        }
    }

    /// Initializes the PIT hardware
    ///
    /// Sets up Channel 0 in mode 3 (square wave) with our reload value.
    ///
    /// # Safety
    ///
    /// This is safe to call multiple times but will reset the hardware state.
    pub fn initialize(&mut self) {
        // Command byte: Channel 0, lobyte/hibyte, mode 3, binary
        // Binary: 00 (channel 0) 11 (lobyte/hibyte) 011 (mode 3) 0 (binary)
        const COMMAND_BYTE: u8 = 0b00110110;

        // Send command
        self.port_io.outb(Self::COMMAND, COMMAND_BYTE);

        // Send reload value (low byte, then high byte)
        self.port_io
            .outb(Self::CHANNEL_0_DATA, (Self::RELOAD_VALUE & 0xFF) as u8);
        self.port_io.outb(
            Self::CHANNEL_0_DATA,
            ((Self::RELOAD_VALUE >> 8) & 0xFF) as u8,
        );

        self.initialized = true;
    }

    /// Reads the current counter value from the PIT
    ///
    /// Uses the latch command to get a stable snapshot of the counter.
    ///
    /// # Returns
    ///
    /// The current counter value (counts down from RELOAD_VALUE to 0)
    fn read_counter(&mut self) -> u16 {
        // Latch counter 0
        self.port_io.outb(Self::COMMAND, 0b00000000);

        // Read low byte, then high byte
        let low = self.port_io.inb(Self::CHANNEL_0_DATA) as u16;
        let high = self.port_io.inb(Self::CHANNEL_0_DATA) as u16;

        (high << 8) | low
    }

    /// Updates cumulative ticks based on counter delta
    ///
    /// The PIT counts down from RELOAD_VALUE to 0. When it reaches 0,
    /// it reloads and continues. We track cumulative ticks by detecting
    /// these transitions.
    fn update_ticks(&mut self) {
        let current_counter = self.read_counter();

        // PIT counts DOWN, so if current < last, we've accumulated ticks
        // Handle wrap-around: if current > last, the counter reloaded
        let delta = if current_counter <= self.last_counter {
            // Normal case: counter decreased
            self.last_counter - current_counter
        } else {
            // Wrap-around case: counter reloaded
            // Ticks elapsed = (last_counter -> 0) + (RELOAD_VALUE -> current_counter)
            self.last_counter + (Self::RELOAD_VALUE - current_counter)
        };

        self.cumulative_ticks += delta as u64;
        self.last_counter = current_counter;
    }
}

impl<P: super::port_io::PortIo> TimerDevice for PitTimer<P> {
    fn poll_ticks(&mut self) -> u64 {
        // Initialize on first poll
        if !self.initialized {
            self.initialize();
            self.last_counter = self.read_counter();
            return 0;
        }

        self.update_ticks();
        self.cumulative_ticks
    }
}

impl<P: super::port_io::PortIo> TimerInterrupt for PitTimer<P> {
    fn configure_periodic(&mut self, hz: u32) {
        self.configured_hz = Some(hz);
    }

    fn enable_interrupts(&mut self) {
        self.interrupts_enabled = true;
    }

    fn disable_interrupts(&mut self) {
        self.interrupts_enabled = false;
    }
}

/// HPET timer (skeleton).
#[derive(Debug)]
pub struct HpetTimer {
    ticks: u64,
    interrupts_enabled: bool,
    configured_hz: Option<u32>,
}

impl HpetTimer {
    pub fn new() -> Self {
        Self {
            ticks: 0,
            interrupts_enabled: false,
            configured_hz: None,
        }
    }

    /// Advances ticks (test helper).
    pub fn advance_ticks(&mut self, delta: u64) {
        self.ticks = self.ticks.saturating_add(delta);
    }

    /// Returns configured interrupt frequency (test helper).
    pub fn configured_hz(&self) -> Option<u32> {
        self.configured_hz
    }

    /// Returns whether interrupts are enabled.
    pub fn interrupts_enabled(&self) -> bool {
        self.interrupts_enabled
    }
}

impl Default for HpetTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerDevice for HpetTimer {
    fn poll_ticks(&mut self) -> u64 {
        self.ticks
    }
}

impl TimerInterrupt for HpetTimer {
    fn configure_periodic(&mut self, hz: u32) {
        self.configured_hz = Some(hz);
    }

    fn enable_interrupts(&mut self) {
        self.interrupts_enabled = true;
    }

    fn disable_interrupts(&mut self) {
        self.interrupts_enabled = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fake_timer_basic() {
        let ticks = vec![0, 10, 20, 30];
        let mut timer = FakeTimerDevice::new(ticks);

        assert_eq!(timer.poll_ticks(), 0);
        assert_eq!(timer.poll_ticks(), 10);
        assert_eq!(timer.poll_ticks(), 20);
        assert_eq!(timer.poll_ticks(), 30);
    }

    #[test]
    fn test_fake_timer_exhaustion() {
        let ticks = vec![100, 200];
        let mut timer = FakeTimerDevice::new(ticks);

        assert_eq!(timer.poll_ticks(), 100);
        assert_eq!(timer.poll_ticks(), 200);
        // After exhaustion, stays at last value
        assert_eq!(timer.poll_ticks(), 200);
        assert_eq!(timer.poll_ticks(), 200);
    }

    #[test]
    fn test_fake_timer_monotonic() {
        let ticks = vec![0, 100, 100, 150, 150, 200];
        let mut timer = FakeTimerDevice::new(ticks);

        let mut last = 0;
        for _ in 0..6 {
            let current = timer.poll_ticks();
            assert!(current >= last);
            last = current;
        }
    }

    #[test]
    #[should_panic(expected = "Tick sequence must be monotonic")]
    fn test_fake_timer_non_monotonic_panics() {
        let ticks = vec![0, 100, 50]; // 50 < 100, should panic
        FakeTimerDevice::new(ticks);
    }

    #[test]
    fn test_fake_timer_empty() {
        let ticks = vec![];
        let mut timer = FakeTimerDevice::new(ticks);

        assert_eq!(timer.poll_ticks(), 0);
        assert_eq!(timer.poll_ticks(), 0);
    }

    #[test]
    fn test_fake_timer_remaining() {
        let ticks = vec![0, 10, 20];
        let mut timer = FakeTimerDevice::new(ticks);

        assert_eq!(timer.remaining(), 3);
        timer.poll_ticks();
        assert_eq!(timer.remaining(), 2);
        timer.poll_ticks();
        assert_eq!(timer.remaining(), 1);
        timer.poll_ticks();
        assert_eq!(timer.remaining(), 0);
    }

    #[test]
    fn test_pit_timer_creation() {
        use crate::FakePortIo;
        let io = FakePortIo::new();
        let timer = PitTimer::new(io);
        assert!(!timer.initialized);
        assert_eq!(timer.cumulative_ticks, 0);
    }

    #[test]
    fn test_pit_timer_interrupt_config() {
        use crate::FakePortIo;
        let io = FakePortIo::new();
        let mut timer = PitTimer::new(io);
        timer.configure_periodic(1000);
        timer.enable_interrupts();
        assert!(timer.interrupts_enabled);
        timer.disable_interrupts();
        assert!(!timer.interrupts_enabled);
    }

    #[test]
    fn test_hpet_timer_scaffold() {
        let mut timer = HpetTimer::new();
        assert_eq!(timer.poll_ticks(), 0);
        timer.configure_periodic(2000);
        timer.enable_interrupts();
        timer.advance_ticks(10);
        assert_eq!(timer.poll_ticks(), 10);
        assert_eq!(timer.configured_hz(), Some(2000));
        assert!(timer.interrupts_enabled());
    }

    // Note: We can't test PitTimer::poll_ticks() without real hardware
    // or a hardware emulator. These tests verify the structure and
    // invariants only.
}
