//! # Memory Types
//!
//! This module defines the fundamental memory abstraction types for PandaGen.
//!
//! ## Philosophy
//!
//! - **Memory is authority, not a side effect**
//! - **Address spaces are objects, not process attributes**
//! - **No inheritance by default**
//! - **Isolation first, sharing only by explicit grant**
//! - **Deterministic behavior preserved in simulation**
//!
//! ## Key Types
//!
//! - [`AddressSpaceId`]: Unique identifier for an address space
//! - [`AddressSpace`]: Contains a set of non-overlapping memory regions
//! - [`MemoryRegion`]: Represents a memory region with permissions and backing
//! - [`MemoryPerms`]: Permission flags for memory regions (Read, Write, Execute)

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

/// Unique identifier for an address space
///
/// Each address space is associated with exactly one ExecutionId and contains
/// a set of non-overlapping memory regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AddressSpaceId(Uuid);

impl AddressSpaceId {
    /// Creates a new unique address space ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Returns the inner UUID value
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for AddressSpaceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AddressSpaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "aspace:{}", self.0)
    }
}

/// Unique identifier for a memory region within an address space
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryRegionId(Uuid);

impl MemoryRegionId {
    /// Creates a new unique memory region ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Returns the inner UUID value
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for MemoryRegionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MemoryRegionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "region:{}", self.0)
    }
}

/// Memory permission flags
///
/// Permissions for memory regions follow the principle of least privilege.
/// By default, no permissions are granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryPerms {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl MemoryPerms {
    /// No permissions
    pub fn none() -> Self {
        Self {
            read: false,
            write: false,
            execute: false,
        }
    }

    /// Read-only permission
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
        }
    }

    /// Read and write permissions
    pub fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            execute: false,
        }
    }

    /// Read and execute permissions (typical for code)
    pub fn read_execute() -> Self {
        Self {
            read: true,
            write: false,
            execute: true,
        }
    }

    /// All permissions (use sparingly)
    pub fn all() -> Self {
        Self {
            read: true,
            write: true,
            execute: true,
        }
    }

    /// Check if this has read permission
    pub fn can_read(&self) -> bool {
        self.read
    }

    /// Check if this has write permission
    pub fn can_write(&self) -> bool {
        self.write
    }

    /// Check if this has execute permission
    pub fn can_execute(&self) -> bool {
        self.execute
    }

    /// Check if this has no permissions
    pub fn is_none(&self) -> bool {
        !self.read && !self.write && !self.execute
    }
}

impl fmt::Display for MemoryPerms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}",
            if self.read { "R" } else { "-" },
            if self.write { "W" } else { "-" },
            if self.execute { "X" } else { "-" }
        )
    }
}

/// Memory region backing type
///
/// This is a logical enumeration for simulation. In a real MMU implementation,
/// these would map to different page table configurations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryBacking {
    /// Anonymous memory (e.g., heap, stack)
    Anonymous,
    /// Shared memory (explicitly shared between address spaces)
    Shared,
    /// Device memory (memory-mapped I/O)
    Device,
}

impl fmt::Display for MemoryBacking {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryBacking::Anonymous => write!(f, "Anonymous"),
            MemoryBacking::Shared => write!(f, "Shared"),
            MemoryBacking::Device => write!(f, "Device"),
        }
    }
}

/// Memory region
///
/// A memory region represents a contiguous range of memory within an address space.
/// Regions are immutable after creation - to change a region, you must create a new one.
///
/// ## Design Notes
///
/// - Size is in bytes (abstract units in simulation, real bytes on hardware)
/// - Base address is NOT stored here - this is a logical region
/// - Regions within an address space must not overlap
/// - The kernel enforces non-overlapping constraint when allocating
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRegion {
    /// Unique identifier for this region
    pub region_id: MemoryRegionId,
    /// Size in bytes (abstract in simulation)
    pub size_bytes: u64,
    /// Memory permissions
    pub permissions: MemoryPerms,
    /// Backing type
    pub backing: MemoryBacking,
}

impl MemoryRegion {
    /// Creates a new memory region
    pub fn new(size_bytes: u64, permissions: MemoryPerms, backing: MemoryBacking) -> Self {
        Self {
            region_id: MemoryRegionId::new(),
            size_bytes,
            permissions,
            backing,
        }
    }

    /// Checks if read access is allowed
    pub fn can_read(&self) -> bool {
        self.permissions.can_read()
    }

    /// Checks if write access is allowed
    pub fn can_write(&self) -> bool {
        self.permissions.can_write()
    }

    /// Checks if execute access is allowed
    pub fn can_execute(&self) -> bool {
        self.permissions.can_execute()
    }

    /// Returns the size in bytes
    pub fn size(&self) -> u64 {
        self.size_bytes
    }
}

/// Memory access type
///
/// Used to check if a particular access is allowed given region permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryAccessType {
    Read,
    Write,
    Execute,
}

impl fmt::Display for MemoryAccessType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryAccessType::Read => write!(f, "Read"),
            MemoryAccessType::Write => write!(f, "Write"),
            MemoryAccessType::Execute => write!(f, "Execute"),
        }
    }
}

/// Address space
///
/// An address space is a collection of non-overlapping memory regions.
/// Each address space is associated with exactly one ExecutionId (stored externally).
///
/// ## Design Notes
///
/// - Regions cannot overlap (enforced by the kernel at allocation time)
/// - Regions are immutable once created (but can be deallocated)
/// - No implicit sharing - sharing requires explicit MemoryRegionCap delegation
/// - Address spaces are capability-governed objects
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressSpace {
    /// Unique identifier
    pub space_id: AddressSpaceId,
    /// Memory regions in this space (region_id -> region)
    /// Note: We use a Vec for simplicity in simulation; real implementation
    /// would use a more efficient data structure for range queries
    regions: Vec<MemoryRegion>,
}

impl AddressSpace {
    /// Creates a new empty address space
    pub fn new() -> Self {
        Self {
            space_id: AddressSpaceId::new(),
            regions: Vec::new(),
        }
    }

    /// Returns all regions in this address space
    pub fn regions(&self) -> &[MemoryRegion] {
        &self.regions
    }

    /// Finds a region by ID
    pub fn find_region(&self, region_id: MemoryRegionId) -> Option<&MemoryRegion> {
        self.regions.iter().find(|r| r.region_id == region_id)
    }

    /// Adds a region to this address space
    ///
    /// Returns an error if adding this region would violate invariants.
    /// Note: This is public so that sim_kernel can add regions.
    pub fn add_region(&mut self, region: MemoryRegion) -> Result<(), MemoryError> {
        // For simulation, we don't track actual addresses, so we can't check for
        // real overlaps. In a real implementation with MMU, this would check
        // page table entries for conflicts.
        //
        // For now, we just add the region and trust the kernel's allocation logic.
        self.regions.push(region);
        Ok(())
    }

    /// Removes a region from this address space
    ///
    /// Returns the removed region if found.
    pub fn remove_region(&mut self, region_id: MemoryRegionId) -> Option<MemoryRegion> {
        if let Some(pos) = self.regions.iter().position(|r| r.region_id == region_id) {
            Some(self.regions.remove(pos))
        } else {
            None
        }
    }

    /// Returns the total number of regions
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// Returns the total size of all regions in bytes
    pub fn total_size_bytes(&self) -> u64 {
        self.regions.iter().map(|r| r.size_bytes).sum()
    }
}

impl Default for AddressSpace {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory-related errors
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MemoryError {
    #[error("Address space not found: {0}")]
    AddressSpaceNotFound(AddressSpaceId),

    #[error("Memory region not found: {0}")]
    RegionNotFound(MemoryRegionId),

    #[error("Permission denied: attempted {access_type} on region {region_id} with permissions {permissions}")]
    PermissionDenied {
        region_id: MemoryRegionId,
        access_type: MemoryAccessType,
        permissions: MemoryPerms,
    },

    #[error("Invalid region size: {0} bytes (must be > 0)")]
    InvalidRegionSize(u64),

    #[error("Region overlap detected")]
    RegionOverlap,

    #[error("Memory budget exhausted: attempted to allocate {requested} bytes, {available} bytes remaining")]
    BudgetExhausted { requested: u64, available: u64 },

    #[error("Cross-address-space access denied: region {region_id} belongs to address space {owner_space}, not {accessor_space}")]
    CrossSpaceAccess {
        region_id: MemoryRegionId,
        owner_space: AddressSpaceId,
        accessor_space: AddressSpaceId,
    },

    #[error("No capability for region: {0}")]
    NoCapability(MemoryRegionId),
}

/// Capability for an address space
///
/// Grants the holder the ability to allocate/deallocate regions within this space.
/// This is analogous to owning a page table in traditional systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AddressSpaceCap {
    /// The address space this capability grants access to
    pub space_id: AddressSpaceId,
    /// Internal capability ID (for tracking in capability table)
    pub cap_id: u64,
}

impl AddressSpaceCap {
    /// Creates a new address space capability
    pub fn new(space_id: AddressSpaceId, cap_id: u64) -> Self {
        Self { space_id, cap_id }
    }
}

/// Capability for a memory region
///
/// Grants the holder the ability to access (read/write/execute) this specific region.
/// Sharing a region requires delegating this capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryRegionCap {
    /// The address space containing this region
    pub space_id: AddressSpaceId,
    /// The specific region this capability grants access to
    pub region_id: MemoryRegionId,
    /// Internal capability ID (for tracking in capability table)
    pub cap_id: u64,
}

impl MemoryRegionCap {
    /// Creates a new memory region capability
    pub fn new(space_id: AddressSpaceId, region_id: MemoryRegionId, cap_id: u64) -> Self {
        Self {
            space_id,
            region_id,
            cap_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_space_id_unique() {
        let id1 = AddressSpaceId::new();
        let id2 = AddressSpaceId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_address_space_id_display() {
        let id = AddressSpaceId::new();
        let display = format!("{}", id);
        assert!(display.starts_with("aspace:"));
    }

    #[test]
    fn test_memory_region_id_unique() {
        let id1 = MemoryRegionId::new();
        let id2 = MemoryRegionId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_memory_perms_none() {
        let perms = MemoryPerms::none();
        assert!(!perms.can_read());
        assert!(!perms.can_write());
        assert!(!perms.can_execute());
        assert!(perms.is_none());
        assert_eq!(perms.to_string(), "---");
    }

    #[test]
    fn test_memory_perms_read_only() {
        let perms = MemoryPerms::read_only();
        assert!(perms.can_read());
        assert!(!perms.can_write());
        assert!(!perms.can_execute());
        assert!(!perms.is_none());
        assert_eq!(perms.to_string(), "R--");
    }

    #[test]
    fn test_memory_perms_read_write() {
        let perms = MemoryPerms::read_write();
        assert!(perms.can_read());
        assert!(perms.can_write());
        assert!(!perms.can_execute());
        assert_eq!(perms.to_string(), "RW-");
    }

    #[test]
    fn test_memory_perms_read_execute() {
        let perms = MemoryPerms::read_execute();
        assert!(perms.can_read());
        assert!(!perms.can_write());
        assert!(perms.can_execute());
        assert_eq!(perms.to_string(), "R-X");
    }

    #[test]
    fn test_memory_perms_all() {
        let perms = MemoryPerms::all();
        assert!(perms.can_read());
        assert!(perms.can_write());
        assert!(perms.can_execute());
        assert_eq!(perms.to_string(), "RWX");
    }

    #[test]
    fn test_memory_region_creation() {
        let region = MemoryRegion::new(4096, MemoryPerms::read_write(), MemoryBacking::Anonymous);
        assert_eq!(region.size(), 4096);
        assert!(region.can_read());
        assert!(region.can_write());
        assert!(!region.can_execute());
    }

    #[test]
    fn test_memory_region_permissions() {
        let region = MemoryRegion::new(4096, MemoryPerms::read_execute(), MemoryBacking::Anonymous);
        assert!(region.can_read());
        assert!(!region.can_write());
        assert!(region.can_execute());
    }

    #[test]
    fn test_address_space_creation() {
        let space = AddressSpace::new();
        assert_eq!(space.region_count(), 0);
        assert_eq!(space.total_size_bytes(), 0);
    }

    #[test]
    fn test_address_space_add_region() {
        let mut space = AddressSpace::new();

        let region = MemoryRegion::new(4096, MemoryPerms::read_write(), MemoryBacking::Anonymous);
        let region_id = region.region_id;

        space.add_region(region).unwrap();

        assert_eq!(space.region_count(), 1);
        assert_eq!(space.total_size_bytes(), 4096);
        assert!(space.find_region(region_id).is_some());
    }

    #[test]
    fn test_address_space_multiple_regions() {
        let mut space = AddressSpace::new();

        let region1 = MemoryRegion::new(4096, MemoryPerms::read_write(), MemoryBacking::Anonymous);
        let region2 =
            MemoryRegion::new(8192, MemoryPerms::read_execute(), MemoryBacking::Anonymous);

        space.add_region(region1).unwrap();
        space.add_region(region2).unwrap();

        assert_eq!(space.region_count(), 2);
        assert_eq!(space.total_size_bytes(), 12288);
    }

    #[test]
    fn test_address_space_remove_region() {
        let mut space = AddressSpace::new();

        let region = MemoryRegion::new(4096, MemoryPerms::read_write(), MemoryBacking::Anonymous);
        let region_id = region.region_id;

        space.add_region(region).unwrap();
        assert_eq!(space.region_count(), 1);

        let removed = space.remove_region(region_id);
        assert!(removed.is_some());
        assert_eq!(space.region_count(), 0);
        assert_eq!(space.total_size_bytes(), 0);
    }

    #[test]
    fn test_address_space_find_nonexistent_region() {
        let space = AddressSpace::new();
        let nonexistent_id = MemoryRegionId::new();
        assert!(space.find_region(nonexistent_id).is_none());
    }

    #[test]
    fn test_memory_backing_display() {
        assert_eq!(MemoryBacking::Anonymous.to_string(), "Anonymous");
        assert_eq!(MemoryBacking::Shared.to_string(), "Shared");
        assert_eq!(MemoryBacking::Device.to_string(), "Device");
    }

    #[test]
    fn test_memory_access_type_display() {
        assert_eq!(MemoryAccessType::Read.to_string(), "Read");
        assert_eq!(MemoryAccessType::Write.to_string(), "Write");
        assert_eq!(MemoryAccessType::Execute.to_string(), "Execute");
    }

    #[test]
    fn test_address_space_cap_creation() {
        let space_id = AddressSpaceId::new();
        let cap = AddressSpaceCap::new(space_id, 42);
        assert_eq!(cap.space_id, space_id);
        assert_eq!(cap.cap_id, 42);
    }

    #[test]
    fn test_memory_region_cap_creation() {
        let space_id = AddressSpaceId::new();
        let region_id = MemoryRegionId::new();
        let cap = MemoryRegionCap::new(space_id, region_id, 43);
        assert_eq!(cap.space_id, space_id);
        assert_eq!(cap.region_id, region_id);
        assert_eq!(cap.cap_id, 43);
    }

    #[test]
    fn test_memory_error_display() {
        let space_id = AddressSpaceId::new();
        let err = MemoryError::AddressSpaceNotFound(space_id);
        let display = err.to_string();
        assert!(display.contains("Address space not found"));

        let region_id = MemoryRegionId::new();
        let err = MemoryError::PermissionDenied {
            region_id,
            access_type: MemoryAccessType::Write,
            permissions: MemoryPerms::read_only(),
        };
        let display = err.to_string();
        assert!(display.contains("Permission denied"));
        assert!(display.contains("Write"));
    }
}
