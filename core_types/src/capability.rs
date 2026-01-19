//! Capability system implementation
//!
//! This module implements PandaGen's capability-based security model.
//!
//! ## Design Principles
//!
//! 1. **Unforgeable**: Capabilities cannot be created except through authorized mechanisms
//! 2. **Transferable**: Capabilities can be explicitly passed between tasks
//! 3. **Typed**: Each capability has a phantom type ensuring type safety
//! 4. **Testable**: The entire system works under `cargo test`
//!
//! ## Example
//!
//! ```
//! use core_types::Cap;
//!
//! // Define a capability type
//! struct FileAccess;
//!
//! // Create a capability (in real code, only kernel can do this)
//! let cap: Cap<FileAccess> = Cap::new(42);
//!
//! // Type safety: cannot confuse different capability types
//! struct NetworkAccess;
//! let _net_cap: Cap<NetworkAccess> = Cap::new(43);
//! // These are different types and cannot be confused
//! ```

use crate::TaskId;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::marker::PhantomData;
use thiserror::Error;

/// A strongly-typed capability handle
///
/// `Cap<T>` represents a capability to perform operations related to `T`.
/// The type parameter `T` is a marker that ensures capabilities cannot be confused.
///
/// Capabilities are unforgeable: they can only be created by trusted code
/// (typically the kernel or a service with authority to grant capabilities).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cap<T> {
    /// Unique identifier for this capability
    id: u64,
    /// Phantom data to enforce type safety
    #[serde(skip)]
    _phantom: PhantomData<T>,
}

impl<T> Cap<T> {
    /// Creates a new capability
    ///
    /// # Security Note
    ///
    /// In a real system, this would only be callable by trusted kernel code.
    /// For testing and simulation, we allow it to be public.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            _phantom: PhantomData,
        }
    }

    /// Returns the capability ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Attempts to cast this capability to another type
    ///
    /// This always fails because capabilities are type-specific.
    /// This method exists to demonstrate type safety.
    pub fn try_cast<U>(self) -> Result<Cap<U>, CapabilityError> {
        // In a real system, there might be legitimate cross-type casts
        // based on privilege relationships, but by default we reject them
        Err(CapabilityError::InvalidCast)
    }
}

impl<T> PartialEq for Cap<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Cap<T> {}

impl<T> fmt::Display for Cap<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cap<{}>({})", std::any::type_name::<T>(), self.id)
    }
}

/// Errors related to capability operations
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapabilityError {
    /// Attempted to cast a capability to an incompatible type
    #[error("Invalid capability cast")]
    InvalidCast,
    /// Attempted to use a capability that has been revoked
    #[error("Capability has been revoked")]
    Revoked,
    /// Attempted to grant a capability without authority
    #[error("Insufficient authority to grant capability")]
    InsufficientAuthority,
}

/// Represents a capability grant operation
///
/// This is used when one task grants a capability to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGrant<T> {
    /// The capability being granted
    pub capability: Cap<T>,
    /// The grantor (optional, for audit trails)
    pub grantor: Option<u64>,
}

impl<T> CapabilityGrant<T> {
    /// Creates a new capability grant
    pub fn new(capability: Cap<T>, grantor: Option<u64>) -> Self {
        Self {
            capability,
            grantor,
        }
    }

    /// Extracts the capability from the grant
    pub fn into_capability(self) -> Cap<T> {
        self.capability
    }
}

/// Represents a capability transfer between tasks
///
/// Unlike granting (which may create copies), transfer moves ownership.
#[derive(Debug)]
pub struct CapabilityTransfer<T> {
    /// The capability being transferred
    capability: Cap<T>,
    /// The task transferring the capability
    from_task: u64,
    /// The task receiving the capability
    to_task: u64,
}

impl<T> CapabilityTransfer<T> {
    /// Creates a new capability transfer
    pub fn new(capability: Cap<T>, from_task: u64, to_task: u64) -> Self {
        Self {
            capability,
            from_task,
            to_task,
        }
    }

    /// Returns the source task ID
    pub fn from_task(&self) -> u64 {
        self.from_task
    }

    /// Returns the destination task ID
    pub fn to_task(&self) -> u64 {
        self.to_task
    }

    /// Completes the transfer and returns the capability
    pub fn complete(self) -> Cap<T> {
        self.capability
    }
}

/// Capability lifecycle event types
///
/// These events track the lifecycle of capabilities for audit purposes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityEvent {
    /// Capability was granted to a task
    Granted {
        cap_id: u64,
        grantor: Option<TaskId>,
        grantee: TaskId,
        cap_type: String,
    },
    /// Capability was delegated from one task to another
    Delegated {
        cap_id: u64,
        from_task: TaskId,
        to_task: TaskId,
        cap_type: String,
    },
    /// Capability was delegated across trust domain boundaries
    CrossDomainDelegation {
        cap_id: u64,
        from_task: TaskId,
        from_domain: String,
        to_task: TaskId,
        to_domain: String,
    },
    /// Capability was cloned (duplication allowed)
    Cloned {
        cap_id: u64,
        original_owner: TaskId,
        new_owner: TaskId,
        cap_type: String,
    },
    /// Capability was dropped by its owner
    Dropped {
        cap_id: u64,
        owner: TaskId,
        cap_type: String,
    },
    /// Attempted to use an invalid capability
    InvalidUseAttempt {
        cap_id: u64,
        task: TaskId,
        reason: CapabilityInvalidReason,
    },
    /// Capability invalidated due to owner termination
    Invalidated {
        cap_id: u64,
        owner: TaskId,
        cap_type: String,
    },
    /// Capability explicitly revoked
    Revoked {
        cap_id: u64,
        owner: TaskId,
        cap_type: String,
        reason: String,
    },
    /// Capability lease expired
    LeaseExpired {
        cap_id: u64,
        owner: TaskId,
        cap_type: String,
        expired_at_nanos: u64,
    },
}

/// Reasons why a capability use attempt failed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityInvalidReason {
    /// Owner task has terminated
    OwnerDead,
    /// Capability was never granted to this task
    NeverGranted,
    /// Capability was transferred away (move semantics)
    TransferredAway,
    /// Capability type mismatch
    TypeMismatch,
    /// Capability has been explicitly revoked
    Revoked,
    /// Capability lease expired
    LeaseExpired,
}

/// Capability status in the lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityStatus {
    /// Capability is valid and can be used
    Valid,
    /// Capability has been transferred to another task
    Transferred,
    /// Capability has been invalidated (owner dead, revoked, etc.)
    Invalid,
}

/// Metadata about a capability's lifecycle
///
/// This tracks the ownership and status of capabilities in the system.
/// Used by the kernel to enforce capability semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityMetadata {
    /// The capability ID
    pub cap_id: u64,
    /// Current owner of the capability
    pub owner: TaskId,
    /// Type name of the capability (for audit/debug)
    pub cap_type: String,
    /// Current status
    pub status: CapabilityStatus,
    /// Original grantor (None for kernel-created caps)
    pub grantor: Option<TaskId>,
    /// Explicit revocation flag
    #[serde(default)]
    pub revoked: bool,
    /// Lease expiration timestamp (None for no lease)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_nanos: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Marker types for testing
    #[derive(Debug, Clone)]
    struct FileRead;
    #[derive(Debug, Clone)]
    struct FileWrite;
    #[derive(Debug, Clone)]
    struct NetworkAccess;

    #[test]
    fn test_capability_creation() {
        let cap: Cap<FileRead> = Cap::new(1);
        assert_eq!(cap.id(), 1);
    }

    #[test]
    fn test_capability_type_safety() {
        let file_cap: Cap<FileRead> = Cap::new(1);
        let net_cap: Cap<NetworkAccess> = Cap::new(2);

        // These are different types - they cannot be compared
        // This won't even compile: assert_ne!(file_cap, net_cap);
        assert_eq!(file_cap.id(), 1);
        assert_eq!(net_cap.id(), 2);
    }

    #[test]
    fn test_capability_equality() {
        let cap1: Cap<FileRead> = Cap::new(1);
        let cap2: Cap<FileRead> = Cap::new(1);
        let cap3: Cap<FileRead> = Cap::new(2);

        assert_eq!(cap1, cap2);
        assert_ne!(cap1, cap3);
    }

    #[test]
    fn test_capability_cast_fails() {
        let file_cap: Cap<FileRead> = Cap::new(1);
        let result: Result<Cap<FileWrite>, _> = file_cap.try_cast();
        assert_eq!(result, Err(CapabilityError::InvalidCast));
    }

    #[test]
    fn test_capability_grant() {
        let cap: Cap<FileRead> = Cap::new(1);
        let grant = CapabilityGrant::new(cap.clone(), Some(100));

        assert_eq!(grant.grantor, Some(100));
        assert_eq!(grant.capability, cap);

        let extracted = grant.into_capability();
        assert_eq!(extracted, cap);
    }

    #[test]
    fn test_capability_transfer() {
        let cap: Cap<FileRead> = Cap::new(1);
        let transfer = CapabilityTransfer::new(cap.clone(), 100, 200);

        assert_eq!(transfer.from_task(), 100);
        assert_eq!(transfer.to_task(), 200);

        let transferred = transfer.complete();
        assert_eq!(transferred, cap);
    }

    #[test]
    fn test_capability_display() {
        let cap: Cap<FileRead> = Cap::new(42);
        let display = format!("{}", cap);
        assert!(display.contains("Cap"));
        assert!(display.contains("42"));
    }

    #[test]
    fn test_capability_event_granted() {
        use crate::TaskId;
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let event = CapabilityEvent::Granted {
            cap_id: 42,
            grantor: Some(task1),
            grantee: task2,
            cap_type: "FileRead".to_string(),
        };

        match event {
            CapabilityEvent::Granted { cap_id, .. } => assert_eq!(cap_id, 42),
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_capability_event_delegated() {
        use crate::TaskId;
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let event = CapabilityEvent::Delegated {
            cap_id: 42,
            from_task: task1,
            to_task: task2,
            cap_type: "FileRead".to_string(),
        };

        match event {
            CapabilityEvent::Delegated {
                from_task, to_task, ..
            } => {
                assert_eq!(from_task, task1);
                assert_eq!(to_task, task2);
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_capability_event_invalidated() {
        use crate::TaskId;
        let task = TaskId::new();
        let event = CapabilityEvent::Invalidated {
            cap_id: 42,
            owner: task,
            cap_type: "FileRead".to_string(),
        };

        match event {
            CapabilityEvent::Invalidated { cap_id, owner, .. } => {
                assert_eq!(cap_id, 42);
                assert_eq!(owner, task);
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_capability_invalid_reason() {
        let reason = CapabilityInvalidReason::OwnerDead;
        assert_eq!(reason, CapabilityInvalidReason::OwnerDead);

        let reason2 = CapabilityInvalidReason::TransferredAway;
        assert_ne!(reason, reason2);
    }

    #[test]
    fn test_capability_status() {
        let status = CapabilityStatus::Valid;
        assert_eq!(status, CapabilityStatus::Valid);

        let status2 = CapabilityStatus::Invalid;
        assert_ne!(status, status2);
    }

    #[test]
    fn test_capability_metadata() {
        use crate::TaskId;
        let task = TaskId::new();
        let metadata = CapabilityMetadata {
            cap_id: 42,
            owner: task,
            cap_type: "FileRead".to_string(),
            status: CapabilityStatus::Valid,
            grantor: None,
            revoked: false,
            lease_expires_at_nanos: None,
        };

        assert_eq!(metadata.cap_id, 42);
        assert_eq!(metadata.owner, task);
        assert_eq!(metadata.status, CapabilityStatus::Valid);
    }
}
