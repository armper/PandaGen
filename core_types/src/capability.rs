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
}
