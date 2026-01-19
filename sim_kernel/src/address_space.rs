//! Address space management for SimulatedKernel
//!
//! This module implements the simulation-level address space management,
//! providing logical isolation without actual MMU integration.

use core_types::{
    AddressSpace, AddressSpaceCap, AddressSpaceId, MemoryAccessType, MemoryError, MemoryPerms,
    MemoryRegion, MemoryRegionCap, MemoryRegionId,
};
use identity::ExecutionId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Address space audit events (test-only)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddressSpaceEvent {
    /// Address space created
    SpaceCreated {
        space_id: AddressSpaceId,
        execution_id: ExecutionId,
        timestamp_nanos: u64,
    },
    /// Address space activated (context switch)
    SpaceActivated {
        space_id: AddressSpaceId,
        execution_id: ExecutionId,
        timestamp_nanos: u64,
    },
    /// Memory region allocated
    RegionAllocated {
        space_id: AddressSpaceId,
        region_id: MemoryRegionId,
        size_bytes: u64,
        permissions: MemoryPerms,
        timestamp_nanos: u64,
    },
    /// Memory region deallocated
    RegionDeallocated {
        space_id: AddressSpaceId,
        region_id: MemoryRegionId,
        timestamp_nanos: u64,
    },
    /// Memory access attempted
    AccessAttempted {
        space_id: AddressSpaceId,
        region_id: MemoryRegionId,
        access_type: MemoryAccessType,
        allowed: bool,
        timestamp_nanos: u64,
    },
    /// Address space destroyed
    SpaceDestroyed {
        space_id: AddressSpaceId,
        timestamp_nanos: u64,
    },
}

/// Audit log for address space operations
#[derive(Debug, Clone)]
pub struct AddressSpaceAuditLog {
    events: Vec<AddressSpaceEvent>,
}

impl AddressSpaceAuditLog {
    /// Creates a new empty audit log
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Records an event
    pub fn record(&mut self, event: AddressSpaceEvent) {
        self.events.push(event);
    }

    /// Returns all recorded events
    pub fn events(&self) -> &[AddressSpaceEvent] {
        &self.events
    }

    /// Clears all events
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Checks if an event matching the predicate exists
    pub fn has_event<F>(&self, predicate: F) -> bool
    where
        F: Fn(&AddressSpaceEvent) -> bool,
    {
        self.events.iter().any(predicate)
    }

    /// Counts events matching the predicate
    pub fn count_events<F>(&self, predicate: F) -> usize
    where
        F: Fn(&AddressSpaceEvent) -> bool,
    {
        self.events.iter().filter(|e| predicate(e)).count()
    }
}

impl Default for AddressSpaceAuditLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Address space manager for SimulatedKernel
///
/// This maintains the mapping between ExecutionIds and their address spaces,
/// as well as the capability tracking for memory operations.
pub struct AddressSpaceManager {
    /// Address spaces by ID
    spaces: HashMap<AddressSpaceId, AddressSpace>,
    /// Execution to address space mapping
    execution_to_space: HashMap<ExecutionId, AddressSpaceId>,
    /// Next capability ID (for AddressSpaceCap and MemoryRegionCap)
    next_cap_id: u64,
    /// Address space capabilities: cap_id -> (space_id, owner_execution_id)
    space_caps: HashMap<u64, (AddressSpaceId, ExecutionId)>,
    /// Memory region capabilities: cap_id -> (region_id, space_id, owner_execution_id)
    region_caps: HashMap<u64, (MemoryRegionId, AddressSpaceId, ExecutionId)>,
    /// Current active address space (for scheduler integration)
    current_space: Option<AddressSpaceId>,
    /// Audit log (test-only)
    audit_log: AddressSpaceAuditLog,
}

impl AddressSpaceManager {
    /// Creates a new address space manager
    pub fn new() -> Self {
        Self {
            spaces: HashMap::new(),
            execution_to_space: HashMap::new(),
            next_cap_id: 1,
            space_caps: HashMap::new(),
            region_caps: HashMap::new(),
            current_space: None,
            audit_log: AddressSpaceAuditLog::new(),
        }
    }

    /// Creates a new address space for the given execution
    ///
    /// Returns the AddressSpaceCap that grants control over this space.
    pub fn create_address_space(
        &mut self,
        execution_id: ExecutionId,
        timestamp_nanos: u64,
    ) -> AddressSpaceCap {
        let space = AddressSpace::new();
        let space_id = space.space_id;

        self.spaces.insert(space_id, space);
        self.execution_to_space.insert(execution_id, space_id);

        // Grant capability to the execution
        let cap_id = self.next_cap_id;
        self.next_cap_id += 1;

        self.space_caps.insert(cap_id, (space_id, execution_id));

        // Record audit event
        self.audit_log.record(AddressSpaceEvent::SpaceCreated {
            space_id,
            execution_id,
            timestamp_nanos,
        });

        AddressSpaceCap::new(space_id, cap_id)
    }

    /// Allocates a region within an address space
    ///
    /// Requires a valid AddressSpaceCap.
    /// Returns a MemoryRegionCap granting access to the region.
    pub fn allocate_region(
        &mut self,
        space_cap: &AddressSpaceCap,
        region: MemoryRegion,
        caller_execution_id: ExecutionId,
        timestamp_nanos: u64,
    ) -> Result<MemoryRegionCap, MemoryError> {
        // Validate capability
        let (cap_space_id, cap_owner) = self
            .space_caps
            .get(&space_cap.cap_id)
            .ok_or(MemoryError::AddressSpaceNotFound(space_cap.space_id))?;

        if *cap_space_id != space_cap.space_id {
            return Err(MemoryError::AddressSpaceNotFound(space_cap.space_id));
        }

        if *cap_owner != caller_execution_id {
            return Err(MemoryError::AddressSpaceNotFound(space_cap.space_id));
        }

        // Get the address space
        let space = self
            .spaces
            .get_mut(&space_cap.space_id)
            .ok_or(MemoryError::AddressSpaceNotFound(space_cap.space_id))?;

        let region_id = region.region_id;
        let size_bytes = region.size_bytes;
        let permissions = region.permissions;

        // Add region to space
        space.add_region(region)?;

        // Create region capability
        let cap_id = self.next_cap_id;
        self.next_cap_id += 1;

        self.region_caps
            .insert(cap_id, (region_id, space_cap.space_id, caller_execution_id));

        // Record audit event
        self.audit_log.record(AddressSpaceEvent::RegionAllocated {
            space_id: space_cap.space_id,
            region_id,
            size_bytes,
            permissions,
            timestamp_nanos,
        });

        Ok(MemoryRegionCap::new(
            space_cap.space_id,
            region_id,
            cap_id,
        ))
    }

    /// Checks if an access to a region is allowed
    ///
    /// Requires a valid MemoryRegionCap and checks permissions.
    pub fn access_region(
        &mut self,
        region_cap: &MemoryRegionCap,
        access_type: MemoryAccessType,
        caller_execution_id: ExecutionId,
        timestamp_nanos: u64,
    ) -> Result<(), MemoryError> {
        // Validate capability
        let (cap_region_id, cap_space_id, cap_owner) = self
            .region_caps
            .get(&region_cap.cap_id)
            .ok_or(MemoryError::NoCapability(region_cap.region_id))?;

        if *cap_region_id != region_cap.region_id || *cap_space_id != region_cap.space_id {
            return Err(MemoryError::NoCapability(region_cap.region_id));
        }

        if *cap_owner != caller_execution_id {
            return Err(MemoryError::NoCapability(region_cap.region_id));
        }

        // Get the address space and region
        let space = self
            .spaces
            .get(&region_cap.space_id)
            .ok_or(MemoryError::AddressSpaceNotFound(region_cap.space_id))?;

        let region = space
            .find_region(region_cap.region_id)
            .ok_or(MemoryError::RegionNotFound(region_cap.region_id))?;

        // Check permissions
        let allowed = match access_type {
            MemoryAccessType::Read => region.can_read(),
            MemoryAccessType::Write => region.can_write(),
            MemoryAccessType::Execute => region.can_execute(),
        };

        // Record audit event
        self.audit_log.record(AddressSpaceEvent::AccessAttempted {
            space_id: region_cap.space_id,
            region_id: region_cap.region_id,
            access_type,
            allowed,
            timestamp_nanos,
        });

        if !allowed {
            return Err(MemoryError::PermissionDenied {
                region_id: region_cap.region_id,
                access_type,
                permissions: region.permissions,
            });
        }

        Ok(())
    }

    /// Activates an address space (context switch)
    ///
    /// This is called by the scheduler when switching tasks.
    pub fn activate_space(
        &mut self,
        execution_id: ExecutionId,
        timestamp_nanos: u64,
    ) -> Result<(), MemoryError> {
        let space_id = self
            .execution_to_space
            .get(&execution_id)
            .copied()
            .ok_or(MemoryError::AddressSpaceNotFound(AddressSpaceId::new()))?;

        self.current_space = Some(space_id);

        // Record audit event
        self.audit_log.record(AddressSpaceEvent::SpaceActivated {
            space_id,
            execution_id,
            timestamp_nanos,
        });

        Ok(())
    }

    /// Returns the currently active address space
    pub fn current_space(&self) -> Option<AddressSpaceId> {
        self.current_space
    }

    /// Returns the address space for an execution
    pub fn get_space_for_execution(
        &self,
        execution_id: ExecutionId,
    ) -> Option<&AddressSpace> {
        let space_id = self.execution_to_space.get(&execution_id)?;
        self.spaces.get(space_id)
    }

    /// Returns the audit log (test-only)
    pub fn audit_log(&self) -> &AddressSpaceAuditLog {
        &self.audit_log
    }

    /// Clears the audit log (test-only)
    pub fn clear_audit_log(&mut self) {
        self.audit_log.clear();
    }

    /// Destroys an address space when an execution terminates
    ///
    /// This removes all state associated with the address space.
    pub fn destroy_address_space(
        &mut self,
        execution_id: ExecutionId,
        timestamp_nanos: u64,
    ) -> Result<(), MemoryError> {
        let space_id = self
            .execution_to_space
            .remove(&execution_id)
            .ok_or(MemoryError::AddressSpaceNotFound(AddressSpaceId::new()))?;

        // Remove the space itself
        self.spaces.remove(&space_id);

        // Invalidate all capabilities for this space
        self.space_caps.retain(|_, (sid, _)| *sid != space_id);
        self.region_caps.retain(|_, (_, sid, _)| *sid != space_id);

        // Clear current space if it was active
        if self.current_space == Some(space_id) {
            self.current_space = None;
        }

        // Record audit event
        self.audit_log.record(AddressSpaceEvent::SpaceDestroyed {
            space_id,
            timestamp_nanos,
        });

        Ok(())
    }
}

impl Default for AddressSpaceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::MemoryBacking;

    #[test]
    fn test_create_address_space() {
        let mut manager = AddressSpaceManager::new();
        let exec_id = ExecutionId::new();

        let _cap = manager.create_address_space(exec_id, 1000);

        assert!(manager.get_space_for_execution(exec_id).is_some());
        assert_eq!(manager.audit_log().events().len(), 1);
        assert!(manager.audit_log().has_event(|e| matches!(
            e,
            AddressSpaceEvent::SpaceCreated { .. }
        )));
    }

    #[test]
    fn test_allocate_region() {
        let mut manager = AddressSpaceManager::new();
        let exec_id = ExecutionId::new();

        let space_cap = manager.create_address_space(exec_id, 1000);

        let region = MemoryRegion::new(4096, MemoryPerms::read_write(), MemoryBacking::Anonymous);
        let region_id = region.region_id;

        let region_cap = manager
            .allocate_region(&space_cap, region, exec_id, 2000)
            .unwrap();

        assert_eq!(region_cap.region_id, region_id);
        assert_eq!(manager.audit_log().events().len(), 2);
        assert!(manager.audit_log().has_event(|e| matches!(
            e,
            AddressSpaceEvent::RegionAllocated { .. }
        )));
    }

    #[test]
    fn test_access_region_allowed() {
        let mut manager = AddressSpaceManager::new();
        let exec_id = ExecutionId::new();

        let space_cap = manager.create_address_space(exec_id, 1000);

        let region = MemoryRegion::new(4096, MemoryPerms::read_write(), MemoryBacking::Anonymous);
        let region_cap = manager
            .allocate_region(&space_cap, region, exec_id, 2000)
            .unwrap();

        // Read should be allowed
        let result = manager.access_region(&region_cap, MemoryAccessType::Read, exec_id, 3000);
        assert!(result.is_ok());

        // Write should be allowed
        let result = manager.access_region(&region_cap, MemoryAccessType::Write, exec_id, 4000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_access_region_denied() {
        let mut manager = AddressSpaceManager::new();
        let exec_id = ExecutionId::new();

        let space_cap = manager.create_address_space(exec_id, 1000);

        let region = MemoryRegion::new(4096, MemoryPerms::read_only(), MemoryBacking::Anonymous);
        let region_cap = manager
            .allocate_region(&space_cap, region, exec_id, 2000)
            .unwrap();

        // Read should be allowed
        let result = manager.access_region(&region_cap, MemoryAccessType::Read, exec_id, 3000);
        assert!(result.is_ok());

        // Write should be denied
        let result = manager.access_region(&region_cap, MemoryAccessType::Write, exec_id, 4000);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MemoryError::PermissionDenied { .. }
        ));
    }

    #[test]
    fn test_activate_space() {
        let mut manager = AddressSpaceManager::new();
        let exec_id = ExecutionId::new();

        let _cap = manager.create_address_space(exec_id, 1000);

        assert!(manager.activate_space(exec_id, 2000).is_ok());
        assert!(manager.current_space().is_some());
        assert!(manager.audit_log().has_event(|e| matches!(
            e,
            AddressSpaceEvent::SpaceActivated { .. }
        )));
    }

    #[test]
    fn test_destroy_address_space() {
        let mut manager = AddressSpaceManager::new();
        let exec_id = ExecutionId::new();

        let _cap = manager.create_address_space(exec_id, 1000);

        assert!(manager.get_space_for_execution(exec_id).is_some());

        assert!(manager.destroy_address_space(exec_id, 3000).is_ok());
        assert!(manager.get_space_for_execution(exec_id).is_none());
        assert!(manager.audit_log().has_event(|e| matches!(
            e,
            AddressSpaceEvent::SpaceDestroyed { .. }
        )));
    }

    #[test]
    fn test_cross_execution_access_denied() {
        let mut manager = AddressSpaceManager::new();
        let exec_id1 = ExecutionId::new();
        let exec_id2 = ExecutionId::new();

        let space_cap = manager.create_address_space(exec_id1, 1000);

        let region = MemoryRegion::new(4096, MemoryPerms::read_write(), MemoryBacking::Anonymous);
        let region_cap = manager
            .allocate_region(&space_cap, region, exec_id1, 2000)
            .unwrap();

        // exec_id2 trying to access exec_id1's region should fail
        let result = manager.access_region(&region_cap, MemoryAccessType::Read, exec_id2, 3000);
        assert!(result.is_err());
    }
}
