//! # Lifecycle
//!
//! Deterministic cancellation and timeout primitives for PandaGen.
//!
//! ## Philosophy
//!
//! - **Explicit over implicit**: Cancellation is explicit, not hidden
//! - **Testability first**: Works with SimKernel time for deterministic testing
//! - **Mechanism not policy**: Provides primitives, services decide policies
//! - **No async runtime required**: Works in sync contexts
//!
//! ## Core Concepts
//!
//! - `CancellationToken`: Cloneable handle to check cancellation status
//! - `CancellationSource`: Controller that can trigger cancellation
//! - `CancellationReason`: Why cancellation occurred
//! - `Deadline`: Point in time when operation should timeout
//! - `Timeout`: Duration-based timeout with start time

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::rc::Rc;
use alloc::string::String;
use core::cell::RefCell;
use core::fmt;
use kernel_api::Instant;
use serde::{Deserialize, Serialize};

/// Reason for cancellation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CancellationReason {
    /// User-initiated cancellation
    UserCancel,
    /// Operation timed out
    Timeout,
    /// Supervisor/orchestrator cancelled the operation
    SupervisorCancel,
    /// Dependency failed, causing cascade cancellation
    DependencyFailed,
    /// Custom reason with description
    Custom(String),
}

impl fmt::Display for CancellationReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CancellationReason::UserCancel => write!(f, "user cancelled"),
            CancellationReason::Timeout => write!(f, "timeout"),
            CancellationReason::SupervisorCancel => write!(f, "supervisor cancelled"),
            CancellationReason::DependencyFailed => write!(f, "dependency failed"),
            CancellationReason::Custom(msg) => write!(f, "{}", msg),
        }
    }
}

/// Internal state of a cancellation token
#[derive(Debug, Clone, PartialEq, Eq)]
enum CancellationState {
    Active,
    Cancelled(CancellationReason),
}

/// Shared state between CancellationToken and CancellationSource
#[derive(Debug, Clone)]
struct SharedCancellationState {
    state: Rc<RefCell<CancellationState>>,
}

impl SharedCancellationState {
    fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(CancellationState::Active)),
        }
    }

    fn is_cancelled(&self) -> bool {
        matches!(*self.state.borrow(), CancellationState::Cancelled(_))
    }

    fn reason(&self) -> Option<CancellationReason> {
        match &*self.state.borrow() {
            CancellationState::Active => None,
            CancellationState::Cancelled(reason) => Some(reason.clone()),
        }
    }

    fn cancel(&self, reason: CancellationReason) {
        *self.state.borrow_mut() = CancellationState::Cancelled(reason);
    }
}

/// A cloneable token that can be checked for cancellation
///
/// CancellationToken is designed to be passed to operations that should
/// be cancellable. It's cheap to clone and check.
///
/// ## Example
///
/// ```
/// use lifecycle::{CancellationSource, CancellationReason};
///
/// let source = CancellationSource::new();
/// let token = source.token();
///
/// assert!(!token.is_cancelled());
///
/// source.cancel(CancellationReason::UserCancel);
/// assert!(token.is_cancelled());
/// assert_eq!(token.reason(), Some(CancellationReason::UserCancel));
/// ```
#[derive(Debug, Clone)]
pub struct CancellationToken {
    shared: SharedCancellationState,
}

impl CancellationToken {
    /// Creates a new token that is never cancelled
    ///
    /// Useful for operations that don't support cancellation.
    pub fn none() -> Self {
        Self {
            shared: SharedCancellationState::new(),
        }
    }

    /// Checks if cancellation has been requested
    pub fn is_cancelled(&self) -> bool {
        self.shared.is_cancelled()
    }

    /// Returns the reason for cancellation, if cancelled
    pub fn reason(&self) -> Option<CancellationReason> {
        self.shared.reason()
    }

    /// Throws an error if cancelled
    pub fn throw_if_cancelled(&self) -> Result<(), LifecycleError> {
        if let Some(reason) = self.reason() {
            Err(LifecycleError::Cancelled { reason })
        } else {
            Ok(())
        }
    }
}

/// A controller that can trigger cancellation
///
/// CancellationSource creates tokens and can cancel them all at once.
///
/// ## Example
///
/// ```
/// use lifecycle::{CancellationSource, CancellationReason};
///
/// let source = CancellationSource::new();
/// let token1 = source.token();
/// let token2 = source.token();
///
/// // Both tokens see the same cancellation
/// source.cancel(CancellationReason::Timeout);
/// assert!(token1.is_cancelled());
/// assert!(token2.is_cancelled());
/// ```
#[derive(Debug, Clone)]
pub struct CancellationSource {
    shared: SharedCancellationState,
}

impl CancellationSource {
    /// Creates a new cancellation source
    pub fn new() -> Self {
        Self {
            shared: SharedCancellationState::new(),
        }
    }

    /// Creates a token from this source
    pub fn token(&self) -> CancellationToken {
        CancellationToken {
            shared: self.shared.clone(),
        }
    }

    /// Cancels all tokens from this source
    pub fn cancel(&self, reason: CancellationReason) {
        self.shared.cancel(reason);
    }

    /// Checks if this source has been cancelled
    pub fn is_cancelled(&self) -> bool {
        self.shared.is_cancelled()
    }
}

impl Default for CancellationSource {
    fn default() -> Self {
        Self::new()
    }
}

/// A deadline represents a point in time when an operation should timeout
///
/// Deadlines are absolute times, making them suitable for passing through
/// multiple layers without duration confusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Deadline {
    instant: Instant,
}

impl Deadline {
    /// Creates a deadline at the specified instant
    pub fn at(instant: Instant) -> Self {
        Self { instant }
    }

    /// Returns the instant of this deadline
    pub fn instant(&self) -> Instant {
        self.instant
    }

    /// Checks if the deadline has passed
    pub fn has_passed(&self, now: Instant) -> bool {
        now >= self.instant
    }

    /// Returns time remaining until deadline
    ///
    /// Returns None if deadline has passed.
    pub fn time_remaining(&self, now: Instant) -> Option<kernel_api::Duration> {
        if now < self.instant {
            Some(self.instant.duration_since(now))
        } else {
            None
        }
    }
}

/// Timeout specifies a duration-based timeout
///
/// Unlike Deadline, Timeout is relative and needs to be converted to a
/// Deadline for actual use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout {
    duration: kernel_api::Duration,
}

impl Timeout {
    /// Creates a timeout with the specified duration
    pub fn after(duration: kernel_api::Duration) -> Self {
        Self { duration }
    }

    /// Creates a timeout from milliseconds
    pub fn from_millis(millis: u64) -> Self {
        Self {
            duration: kernel_api::Duration::from_millis(millis),
        }
    }

    /// Creates a timeout from seconds
    pub fn from_secs(secs: u64) -> Self {
        Self {
            duration: kernel_api::Duration::from_secs(secs),
        }
    }

    /// Returns the duration of this timeout
    pub fn duration(&self) -> kernel_api::Duration {
        self.duration
    }

    /// Converts this timeout to a deadline starting from now
    pub fn to_deadline(&self, now: Instant) -> Deadline {
        Deadline::at(now + self.duration)
    }
}

/// Errors related to lifecycle operations
#[derive(Debug)]
pub enum LifecycleError {
    Cancelled { reason: CancellationReason },

    Timeout,
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LifecycleError::Cancelled { reason } => {
                write!(f, "Operation was cancelled: {}", reason)
            }
            LifecycleError::Timeout => write!(f, "Operation timed out"),
        }
    }
}

impl core::error::Error for LifecycleError {}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel_api::{Duration, Instant};

    #[test]
    fn test_cancellation_token_none() {
        let token = CancellationToken::none();
        assert!(!token.is_cancelled());
        assert_eq!(token.reason(), None);
    }

    #[test]
    fn test_cancellation_source_basic() {
        let source = CancellationSource::new();
        let token = source.token();

        assert!(!token.is_cancelled());
        assert!(!source.is_cancelled());

        source.cancel(CancellationReason::UserCancel);

        assert!(token.is_cancelled());
        assert!(source.is_cancelled());
        assert_eq!(token.reason(), Some(CancellationReason::UserCancel));
    }

    #[test]
    fn test_cancellation_multiple_tokens() {
        let source = CancellationSource::new();
        let token1 = source.token();
        let token2 = source.token();

        assert!(!token1.is_cancelled());
        assert!(!token2.is_cancelled());

        source.cancel(CancellationReason::Timeout);

        assert!(token1.is_cancelled());
        assert!(token2.is_cancelled());
        assert_eq!(token1.reason(), Some(CancellationReason::Timeout));
        assert_eq!(token2.reason(), Some(CancellationReason::Timeout));
    }

    #[test]
    fn test_cancellation_reason_display() {
        assert_eq!(CancellationReason::UserCancel.to_string(), "user cancelled");
        assert_eq!(CancellationReason::Timeout.to_string(), "timeout");
        assert_eq!(
            CancellationReason::SupervisorCancel.to_string(),
            "supervisor cancelled"
        );
        assert_eq!(
            CancellationReason::DependencyFailed.to_string(),
            "dependency failed"
        );
        assert_eq!(
            CancellationReason::Custom("test".to_string()).to_string(),
            "test"
        );
    }

    #[test]
    fn test_throw_if_cancelled() {
        let source = CancellationSource::new();
        let token = source.token();

        assert!(token.throw_if_cancelled().is_ok());

        source.cancel(CancellationReason::UserCancel);
        let result = token.throw_if_cancelled();
        assert!(result.is_err());
        match result {
            Err(LifecycleError::Cancelled { reason }) => {
                assert_eq!(reason, CancellationReason::UserCancel);
            }
            _ => panic!("Expected Cancelled error"),
        }
    }

    #[test]
    fn test_deadline_basic() {
        let now = Instant::from_nanos(1000);
        let future = Instant::from_nanos(2000);
        let deadline = Deadline::at(future);

        assert!(!deadline.has_passed(now));
        assert_eq!(deadline.instant(), future);

        let later = Instant::from_nanos(2000);
        assert!(deadline.has_passed(later));

        let much_later = Instant::from_nanos(3000);
        assert!(deadline.has_passed(much_later));
    }

    #[test]
    fn test_deadline_time_remaining() {
        let now = Instant::from_nanos(1000);
        let future = Instant::from_nanos(2000);
        let deadline = Deadline::at(future);

        let remaining = deadline.time_remaining(now);
        assert_eq!(remaining, Some(Duration::from_nanos(1000)));

        let at_deadline = Instant::from_nanos(2000);
        let remaining = deadline.time_remaining(at_deadline);
        assert_eq!(remaining, None);

        let past_deadline = Instant::from_nanos(3000);
        let remaining = deadline.time_remaining(past_deadline);
        assert_eq!(remaining, None);
    }

    #[test]
    fn test_timeout_basic() {
        let timeout = Timeout::from_millis(100);
        assert_eq!(timeout.duration(), Duration::from_millis(100));

        let timeout = Timeout::from_secs(5);
        assert_eq!(timeout.duration(), Duration::from_secs(5));

        let timeout = Timeout::after(Duration::from_millis(500));
        assert_eq!(timeout.duration(), Duration::from_millis(500));
    }

    #[test]
    fn test_timeout_to_deadline() {
        let now = Instant::from_nanos(1000);
        let timeout = Timeout::from_millis(100);
        let deadline = timeout.to_deadline(now);

        let expected = Instant::from_nanos(1000 + 100_000_000); // 100ms in nanos
        assert_eq!(deadline.instant(), expected);
        assert!(!deadline.has_passed(now));
        assert!(deadline.has_passed(expected));
    }

    #[test]
    fn test_cancellation_reasons() {
        assert_eq!(
            CancellationReason::UserCancel,
            CancellationReason::UserCancel
        );
        assert_ne!(CancellationReason::UserCancel, CancellationReason::Timeout);
        assert_eq!(
            CancellationReason::Custom("a".to_string()),
            CancellationReason::Custom("a".to_string())
        );
        assert_ne!(
            CancellationReason::Custom("a".to_string()),
            CancellationReason::Custom("b".to_string())
        );
    }
}
