//! # Capability-Based Permissions & Ownership
//!
//! This module implements PandaGen's capability-based permission system for storage objects.
//!
//! ## Philosophy
//!
//! **No POSIX permission bits.** Capabilities are unforgeable tokens that grant specific rights.
//!
//! ## Design Principles
//!
//! 1. **Capabilities over permissions**: Having a capability IS the permission
//! 2. **Explicit ownership**: Track which component/user created each object
//! 3. **Clear error messages**: Explain WHY access failed, not just "no"
//! 4. **Typed access**: Read/Write/Execute are distinct capabilities

use alloc::collections::BTreeMap;
use alloc::string::String;
use core::fmt;
use serde::{Deserialize, Serialize};
use core_types::new_uuid;
use uuid::Uuid;

use crate::{ObjectId, VersionId};

/// Unique identifier for a principal (component, user, or service)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PrincipalId(Uuid);

impl PrincipalId {
    /// Creates a new random principal ID
    pub fn new() -> Self {
        Self(new_uuid())
    }

    /// Creates a principal ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// Creates a well-known system principal
    pub fn system() -> Self {
        // Use a deterministic UUID for the system principal
        Self(Uuid::from_bytes([0; 16]))
    }
}

impl Default for PrincipalId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::system() {
            write!(f, "Principal(system)")
        } else {
            write!(f, "Principal({})", self.0)
        }
    }
}

/// Capability types for storage objects
///
/// Unlike POSIX rwx bits, these are unforgeable tokens granted explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityKind {
    /// Read access: Can read object data
    Read,

    /// Write access: Can modify object data (creates new version)
    Write,

    /// Execute access: Can invoke/run object (for executable content)
    Execute,

    /// Delete access: Can delete object
    Delete,

    /// Grant access: Can grant capabilities to others
    Grant,

    /// Own access: Full control (implies all other capabilities)
    Own,
}

impl fmt::Display for CapabilityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilityKind::Read => write!(f, "Read"),
            CapabilityKind::Write => write!(f, "Write"),
            CapabilityKind::Execute => write!(f, "Execute"),
            CapabilityKind::Delete => write!(f, "Delete"),
            CapabilityKind::Grant => write!(f, "Grant"),
            CapabilityKind::Own => write!(f, "Own"),
        }
    }
}

/// An unforgeable capability token
///
/// Having a capability proves authority to perform an action.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Capability {
    /// The object this capability grants access to
    pub object_id: ObjectId,

    /// The kind of access granted
    pub kind: CapabilityKind,

    /// Who was granted this capability
    pub holder: PrincipalId,

    /// Unique identifier for this capability (prevents forgery)
    capability_id: Uuid,
}

impl Capability {
    /// Creates a new capability
    pub fn new(object_id: ObjectId, kind: CapabilityKind, holder: PrincipalId) -> Self {
        Self {
            object_id,
            kind,
            holder,
            capability_id: new_uuid(),
        }
    }

    /// Returns the capability ID
    pub fn id(&self) -> Uuid {
        self.capability_id
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Cap[{} on {} for {}]",
            self.kind, self.object_id, self.holder
        )
    }
}

/// Ownership metadata for a storage object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ownership {
    /// Who created this object
    pub owner: PrincipalId,

    /// When it was created (Unix timestamp)
    pub created_at: u64,

    /// Who last modified it
    pub last_modified_by: PrincipalId,

    /// When it was last modified (Unix timestamp)
    pub last_modified_at: u64,

    /// Optional human-readable name/description
    pub description: Option<String>,
}

impl Ownership {
    /// Creates new ownership metadata
    pub fn new(owner: PrincipalId, timestamp: u64) -> Self {
        Self {
            owner,
            created_at: timestamp,
            last_modified_by: owner,
            last_modified_at: timestamp,
            description: None,
        }
    }

    /// Updates modification metadata
    pub fn update(&mut self, modifier: PrincipalId, timestamp: u64) {
        self.last_modified_by = modifier;
        self.last_modified_at = timestamp;
    }

    /// Sets description
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }
}

/// Reason why access was denied
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDenialReason {
    /// No capability for this operation
    MissingCapability {
        required: CapabilityKind,
        object_id: ObjectId,
        principal: PrincipalId,
    },

    /// Capability is for wrong object
    WrongObject {
        capability_object: ObjectId,
        requested_object: ObjectId,
    },

    /// Capability is for wrong principal
    WrongPrincipal {
        capability_holder: PrincipalId,
        requesting_principal: PrincipalId,
    },

    /// Capability has wrong kind
    WrongCapabilityKind {
        capability_kind: CapabilityKind,
        required_kind: CapabilityKind,
    },

    /// Object does not exist
    ObjectNotFound { object_id: ObjectId },

    /// Version does not exist
    VersionNotFound { version_id: VersionId },
}

impl fmt::Display for AccessDenialReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccessDenialReason::MissingCapability {
                required,
                object_id,
                principal,
            } => {
                write!(
                    f,
                    "Access denied: {} requires {} capability on {}, but none was provided",
                    principal, required, object_id
                )
            }
            AccessDenialReason::WrongObject {
                capability_object,
                requested_object,
            } => {
                write!(
                    f,
                    "Access denied: Capability is for {}, but access to {} was requested",
                    capability_object, requested_object
                )
            }
            AccessDenialReason::WrongPrincipal {
                capability_holder,
                requesting_principal,
            } => {
                write!(
                    f,
                    "Access denied: Capability is held by {}, but {} is requesting access",
                    capability_holder, requesting_principal
                )
            }
            AccessDenialReason::WrongCapabilityKind {
                capability_kind,
                required_kind,
            } => {
                write!(
                    f,
                    "Access denied: Operation requires {} capability, but only {} was provided",
                    required_kind, capability_kind
                )
            }
            AccessDenialReason::ObjectNotFound { object_id } => {
                write!(f, "Access denied: Object {} does not exist", object_id)
            }
            AccessDenialReason::VersionNotFound { version_id } => {
                write!(f, "Access denied: Version {} does not exist", version_id)
            }
        }
    }
}

/// Permission checker for validating capabilities
pub struct PermissionChecker {
    /// Map of object ID to ownership
    ownership: BTreeMap<ObjectId, Ownership>,
}

impl PermissionChecker {
    /// Creates a new permission checker
    pub fn new() -> Self {
        Self {
            ownership: BTreeMap::new(),
        }
    }

    /// Registers an object's ownership
    pub fn register_object(&mut self, object_id: ObjectId, ownership: Ownership) {
        self.ownership.insert(object_id, ownership);
    }

    /// Checks if a capability is valid for an operation
    pub fn check_access(
        &self,
        capability: &Capability,
        object_id: ObjectId,
        required_kind: CapabilityKind,
        principal: PrincipalId,
    ) -> Result<(), AccessDenialReason> {
        // Check if object exists
        if !self.ownership.contains_key(&object_id) {
            return Err(AccessDenialReason::ObjectNotFound { object_id });
        }

        // Check if capability is for the right object
        if capability.object_id != object_id {
            return Err(AccessDenialReason::WrongObject {
                capability_object: capability.object_id,
                requested_object: object_id,
            });
        }

        // Check if capability is held by the right principal
        if capability.holder != principal {
            return Err(AccessDenialReason::WrongPrincipal {
                capability_holder: capability.holder,
                requesting_principal: principal,
            });
        }

        // Check if capability kind is sufficient
        // Own capability implies all others
        if capability.kind == CapabilityKind::Own {
            return Ok(());
        }

        // Otherwise, must match exactly
        if capability.kind != required_kind {
            return Err(AccessDenialReason::WrongCapabilityKind {
                capability_kind: capability.kind,
                required_kind,
            });
        }

        Ok(())
    }

    /// Gets ownership information for an object
    pub fn get_ownership(&self, object_id: ObjectId) -> Option<&Ownership> {
        self.ownership.get(&object_id)
    }

    /// Checks if a principal is the owner of an object
    pub fn is_owner(&self, object_id: ObjectId, principal: PrincipalId) -> bool {
        self.ownership
            .get(&object_id)
            .map(|o| o.owner == principal)
            .unwrap_or(false)
    }
}

impl Default for PermissionChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn test_principal_id_creation() {
        let p1 = PrincipalId::new();
        let p2 = PrincipalId::new();
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_system_principal() {
        let sys1 = PrincipalId::system();
        let sys2 = PrincipalId::system();
        assert_eq!(sys1, sys2);
        assert_eq!(format!("{}", sys1), "Principal(system)");
    }

    #[test]
    fn test_capability_creation() {
        let obj_id = ObjectId::new();
        let principal = PrincipalId::new();
        let cap = Capability::new(obj_id, CapabilityKind::Read, principal);

        assert_eq!(cap.object_id, obj_id);
        assert_eq!(cap.kind, CapabilityKind::Read);
        assert_eq!(cap.holder, principal);
    }

    #[test]
    fn test_capability_display() {
        let obj_id = ObjectId::new();
        let principal = PrincipalId::new();
        let cap = Capability::new(obj_id, CapabilityKind::Write, principal);

        let display = format!("{}", cap);
        assert!(display.contains("Write"));
    }

    #[test]
    fn test_ownership_creation() {
        let owner = PrincipalId::new();
        let ownership = Ownership::new(owner, 1000);

        assert_eq!(ownership.owner, owner);
        assert_eq!(ownership.created_at, 1000);
        assert_eq!(ownership.last_modified_by, owner);
        assert_eq!(ownership.last_modified_at, 1000);
    }

    #[test]
    fn test_ownership_update() {
        let owner = PrincipalId::new();
        let modifier = PrincipalId::new();
        let mut ownership = Ownership::new(owner, 1000);

        ownership.update(modifier, 2000);

        assert_eq!(ownership.owner, owner);
        assert_eq!(ownership.last_modified_by, modifier);
        assert_eq!(ownership.last_modified_at, 2000);
    }

    #[test]
    fn test_permission_checker_valid_access() {
        let mut checker = PermissionChecker::new();
        let obj_id = ObjectId::new();
        let principal = PrincipalId::new();
        let ownership = Ownership::new(principal, 1000);

        checker.register_object(obj_id, ownership);

        let cap = Capability::new(obj_id, CapabilityKind::Read, principal);
        let result = checker.check_access(&cap, obj_id, CapabilityKind::Read, principal);

        assert!(result.is_ok());
    }

    #[test]
    fn test_permission_checker_wrong_object() {
        let mut checker = PermissionChecker::new();
        let obj_id1 = ObjectId::new();
        let obj_id2 = ObjectId::new();
        let principal = PrincipalId::new();
        let ownership = Ownership::new(principal, 1000);

        checker.register_object(obj_id2, ownership);

        let cap = Capability::new(obj_id1, CapabilityKind::Read, principal);
        let result = checker.check_access(&cap, obj_id2, CapabilityKind::Read, principal);

        assert!(result.is_err());
        match result.unwrap_err() {
            AccessDenialReason::WrongObject { .. } => {}
            _ => panic!("Expected WrongObject error"),
        }
    }

    #[test]
    fn test_permission_checker_wrong_principal() {
        let mut checker = PermissionChecker::new();
        let obj_id = ObjectId::new();
        let principal1 = PrincipalId::new();
        let principal2 = PrincipalId::new();
        let ownership = Ownership::new(principal1, 1000);

        checker.register_object(obj_id, ownership);

        let cap = Capability::new(obj_id, CapabilityKind::Read, principal1);
        let result = checker.check_access(&cap, obj_id, CapabilityKind::Read, principal2);

        assert!(result.is_err());
        match result.unwrap_err() {
            AccessDenialReason::WrongPrincipal { .. } => {}
            _ => panic!("Expected WrongPrincipal error"),
        }
    }

    #[test]
    fn test_permission_checker_wrong_kind() {
        let mut checker = PermissionChecker::new();
        let obj_id = ObjectId::new();
        let principal = PrincipalId::new();
        let ownership = Ownership::new(principal, 1000);

        checker.register_object(obj_id, ownership);

        let cap = Capability::new(obj_id, CapabilityKind::Read, principal);
        let result = checker.check_access(&cap, obj_id, CapabilityKind::Write, principal);

        assert!(result.is_err());
        match result.unwrap_err() {
            AccessDenialReason::WrongCapabilityKind { .. } => {}
            _ => panic!("Expected WrongCapabilityKind error"),
        }
    }

    #[test]
    fn test_permission_checker_own_capability() {
        let mut checker = PermissionChecker::new();
        let obj_id = ObjectId::new();
        let principal = PrincipalId::new();
        let ownership = Ownership::new(principal, 1000);

        checker.register_object(obj_id, ownership);

        // Own capability should grant all access
        let cap = Capability::new(obj_id, CapabilityKind::Own, principal);

        assert!(checker
            .check_access(&cap, obj_id, CapabilityKind::Read, principal)
            .is_ok());
        assert!(checker
            .check_access(&cap, obj_id, CapabilityKind::Write, principal)
            .is_ok());
        assert!(checker
            .check_access(&cap, obj_id, CapabilityKind::Execute, principal)
            .is_ok());
    }

    #[test]
    fn test_permission_checker_object_not_found() {
        let checker = PermissionChecker::new();
        let obj_id = ObjectId::new();
        let principal = PrincipalId::new();

        let cap = Capability::new(obj_id, CapabilityKind::Read, principal);
        let result = checker.check_access(&cap, obj_id, CapabilityKind::Read, principal);

        assert!(result.is_err());
        match result.unwrap_err() {
            AccessDenialReason::ObjectNotFound { .. } => {}
            _ => panic!("Expected ObjectNotFound error"),
        }
    }

    #[test]
    fn test_is_owner() {
        let mut checker = PermissionChecker::new();
        let obj_id = ObjectId::new();
        let owner = PrincipalId::new();
        let other = PrincipalId::new();
        let ownership = Ownership::new(owner, 1000);

        checker.register_object(obj_id, ownership);

        assert!(checker.is_owner(obj_id, owner));
        assert!(!checker.is_owner(obj_id, other));
    }

    #[test]
    fn test_access_denial_reason_display() {
        let obj_id = ObjectId::new();
        let principal = PrincipalId::new();

        let reason = AccessDenialReason::MissingCapability {
            required: CapabilityKind::Write,
            object_id: obj_id,
            principal,
        };

        let display = format!("{}", reason);
        assert!(display.contains("Access denied"));
        assert!(display.contains("Write"));
    }
}
