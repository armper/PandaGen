//! Kernel error types

use alloc::string::String;

/// Errors that can occur when interacting with the kernel
#[derive(Debug, PartialEq, Eq)]
pub enum KernelError {
    /// Task spawn failed
    SpawnFailed(String),

    /// Channel operation failed
    ChannelError(String),

    /// Message send failed
    SendFailed(String),

    /// Message receive failed
    ReceiveFailed(String),

    /// Timeout occurred
    Timeout,

    /// Service not found
    ServiceNotFound(String),

    /// Service already registered
    ServiceAlreadyRegistered(String),

    /// Insufficient authority
    InsufficientAuthority(String),

    /// Invalid capability
    InvalidCapability(String),

    /// Resource exhausted (legacy - use ResourceBudgetExhausted for detailed errors)
    ResourceExhausted(String),

    /// Resource budget exceeded (pre-exhaustion warning)
    ResourceBudgetExceeded {
        resource_type: String,
        limit: u64,
        usage: u64,
        identity: String,
        operation: String,
    },

    /// Resource budget exhausted (hard limit reached)
    ResourceBudgetExhausted {
        resource_type: String,
        limit: u64,
        usage: u64,
        identity: String,
        operation: String,
    },
}

impl core::fmt::Display for KernelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            KernelError::SpawnFailed(msg) => write!(f, "Failed to spawn task: {}", msg),
            KernelError::ChannelError(msg) => write!(f, "Channel error: {}", msg),
            KernelError::SendFailed(msg) => write!(f, "Failed to send message: {}", msg),
            KernelError::ReceiveFailed(msg) => write!(f, "Failed to receive message: {}", msg),
            KernelError::Timeout => write!(f, "Operation timed out"),
            KernelError::ServiceNotFound(msg) => write!(f, "Service not found: {}", msg),
            KernelError::ServiceAlreadyRegistered(msg) => {
                write!(f, "Service already registered: {}", msg)
            }
            KernelError::InsufficientAuthority(msg) => {
                write!(f, "Insufficient authority: {}", msg)
            }
            KernelError::InvalidCapability(msg) => write!(f, "Invalid capability: {}", msg),
            KernelError::ResourceExhausted(msg) => write!(f, "Resource exhausted: {}", msg),
            KernelError::ResourceBudgetExceeded {
                resource_type,
                limit,
                usage,
                identity,
                operation,
            } => write!(
                f,
                "Resource budget exceeded: {} limit={}, usage={}, identity={}, operation={}",
                resource_type, limit, usage, identity, operation
            ),
            KernelError::ResourceBudgetExhausted {
                resource_type,
                limit,
                usage,
                identity,
                operation,
            } => write!(
                f,
                "Resource budget exhausted: {} limit={}, usage={}, identity={}, operation={}",
                resource_type, limit, usage, identity, operation
            ),
        }
    }
}

impl core::error::Error for KernelError {}
