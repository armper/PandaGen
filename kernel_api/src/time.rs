//! Time abstractions

use core::ops::{Add, Sub};
use serde::{Deserialize, Serialize};

/// A point in time
///
/// Unlike POSIX time (seconds since epoch), this is an opaque type.
/// In simulated kernels, time can be virtual. In real kernels, it
/// maps to hardware time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Instant {
    /// Nanoseconds since some arbitrary epoch
    nanos: u64,
}

impl Instant {
    /// Creates an instant from nanoseconds
    pub fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }

    /// Returns nanoseconds since epoch
    pub fn as_nanos(&self) -> u64 {
        self.nanos
    }

    /// Returns the duration since another instant
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        Duration::from_nanos(self.nanos.saturating_sub(earlier.nanos))
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;

    fn add(self, duration: Duration) -> Self::Output {
        Instant::from_nanos(self.nanos + duration.as_nanos())
    }
}

impl Sub<Duration> for Instant {
    type Output = Instant;

    fn sub(self, duration: Duration) -> Self::Output {
        Instant::from_nanos(self.nanos.saturating_sub(duration.as_nanos()))
    }
}

/// A duration of time
///
/// This is explicit and type-safe. Unlike POSIX (where durations are
/// often implicit or confused with absolute times), Duration is distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Duration {
    /// Nanoseconds
    nanos: u64,
}

impl Duration {
    /// Creates a duration from nanoseconds
    pub const fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }

    /// Creates a duration from microseconds
    pub const fn from_micros(micros: u64) -> Self {
        Self {
            nanos: micros * 1_000,
        }
    }

    /// Creates a duration from milliseconds
    pub const fn from_millis(millis: u64) -> Self {
        Self {
            nanos: millis * 1_000_000,
        }
    }

    /// Creates a duration from seconds
    pub const fn from_secs(secs: u64) -> Self {
        Self {
            nanos: secs * 1_000_000_000,
        }
    }

    /// Returns the duration in nanoseconds
    pub const fn as_nanos(&self) -> u64 {
        self.nanos
    }

    /// Returns the duration in microseconds
    pub const fn as_micros(&self) -> u64 {
        self.nanos / 1_000
    }

    /// Returns the duration in milliseconds
    pub const fn as_millis(&self) -> u64 {
        self.nanos / 1_000_000
    }

    /// Returns the duration in seconds
    pub const fn as_secs(&self) -> u64 {
        self.nanos / 1_000_000_000
    }
}

impl Add for Duration {
    type Output = Duration;

    fn add(self, other: Duration) -> Self::Output {
        Duration::from_nanos(self.nanos + other.nanos)
    }
}

impl Sub for Duration {
    type Output = Duration;

    fn sub(self, other: Duration) -> Self::Output {
        Duration::from_nanos(self.nanos.saturating_sub(other.nanos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_creation() {
        let d1 = Duration::from_secs(1);
        let d2 = Duration::from_millis(1000);
        let d3 = Duration::from_micros(1_000_000);
        let d4 = Duration::from_nanos(1_000_000_000);

        assert_eq!(d1, d2);
        assert_eq!(d2, d3);
        assert_eq!(d3, d4);
    }

    #[test]
    fn test_duration_conversion() {
        let d = Duration::from_secs(1);
        assert_eq!(d.as_secs(), 1);
        assert_eq!(d.as_millis(), 1000);
        assert_eq!(d.as_micros(), 1_000_000);
        assert_eq!(d.as_nanos(), 1_000_000_000);
    }

    #[test]
    fn test_duration_arithmetic() {
        let d1 = Duration::from_millis(500);
        let d2 = Duration::from_millis(300);

        assert_eq!(d1 + d2, Duration::from_millis(800));
        assert_eq!(d1 - d2, Duration::from_millis(200));
    }

    #[test]
    fn test_instant_creation() {
        let i1 = Instant::from_nanos(1000);
        let i2 = Instant::from_nanos(2000);
        assert!(i2 > i1);
    }

    #[test]
    fn test_instant_duration_since() {
        let i1 = Instant::from_nanos(1000);
        let i2 = Instant::from_nanos(2000);
        assert_eq!(i2.duration_since(i1), Duration::from_nanos(1000));
    }

    #[test]
    fn test_instant_arithmetic() {
        let i = Instant::from_nanos(1000);
        let d = Duration::from_nanos(500);

        assert_eq!(i + d, Instant::from_nanos(1500));
        assert_eq!(i - d, Instant::from_nanos(500));
    }
}
