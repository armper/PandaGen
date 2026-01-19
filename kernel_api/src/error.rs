//! Kernel error types

use thiserror::Error;

/// Errors that can occur when interacting with the kernel
#[derive(Debug, Error, PartialEq, Eq)]
pub enum KernelError {
    /// Task spawn failed
    #[error("Failed to spawn task: {0}")]
    SpawnFailed(String),

    /// Channel operation failed
    #[error("Channel error: {0}")]
    ChannelError(String),

    /// Message send failed
    #[error("Failed to send message: {0}")]
    SendFailed(String),

    /// Message receive failed
    #[error("Failed to receive message: {0}")]
    ReceiveFailed(String),

    /// Timeout occurred
    #[error("Operation timed out")]
    Timeout,

    /// Service not found
    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    /// Service already registered
    #[error("Service already registered: {0}")]
    ServiceAlreadyRegistered(String),

    /// Insufficient authority
    #[error("Insufficient authority to perform operation")]
    InsufficientAuthority,

    /// Invalid capability
    #[error("Invalid capability: {0}")]
    InvalidCapability(String),

    /// Resource exhausted
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),
}
