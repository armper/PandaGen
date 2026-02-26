//! Integration between logical address spaces and hardware page tables
//!
//! This module provides the bridge between PandaGen's capability-based
//! AddressSpace abstraction and x86_64 page tables.
//!
//! ## Design Philosophy
//!
//! - **Separation of concerns**: Logical isolation (sim_kernel) and physical
//!   page tables (hal_x86_64) remain independent but can be linked.
//! - **Testability**: Code works without hardware page tables for testing.
//! - **Explicit**: Page table operations require capabilities, just like
//!   everything else in PandaGen.
//!
//! ## Usage
//!
//! In simulation mode (default), address spaces are purely logical.
//! When `PageTableBridge` is enabled, each AddressSpace gets backed by
//! actual page tables that could be loaded into CR3.

use core_types::{AddressSpaceId, MemoryError, MemoryPerms, MemoryRegionId};
use std::collections::HashMap;

/// Configuration for page table integration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTableMode {
    /// Simulation only (no hardware page tables)
    Simulation,
    /// Hardware page tables enabled (requires hal_x86_64)
    Hardware,
}

/// Bridge between logical address spaces and hardware page tables
///
/// This is optional and only used when integrating with real hardware.
/// In simulation mode, this is a no-op.
pub struct PageTableBridge {
    mode: PageTableMode,
    /// Mapping from AddressSpaceId to page table handle (when in Hardware mode)
    /// In a real implementation, this would store hal_x86_64::AddressSpaceHandle
    space_handles: HashMap<AddressSpaceId, u64>, // Simplified: just store a handle ID
}

impl PageTableBridge {
    /// Creates a new page table bridge
    pub fn new(mode: PageTableMode) -> Self {
        Self {
            mode,
            space_handles: HashMap::new(),
        }
    }

    /// Creates a page table for an address space
    pub fn create_page_table(&mut self, space_id: AddressSpaceId) -> Result<(), MemoryError> {
        match self.mode {
            PageTableMode::Simulation => {
                // No-op in simulation mode
                Ok(())
            }
            PageTableMode::Hardware => {
                // In a real implementation, we would:
                // 1. Allocate a PML4 table via hal_x86_64::PageTableManager
                // 2. Store the handle
                // For now, just mark that this space has a page table
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                space_id.hash(&mut hasher);
                self.space_handles.insert(space_id, hasher.finish());
                Ok(())
            }
        }
    }

    /// Maps a memory region in the page tables
    pub fn map_region(
        &mut self,
        space_id: AddressSpaceId,
        _region_id: MemoryRegionId,
        _virtual_base: u64,
        _size_bytes: u64,
        _perms: MemoryPerms,
    ) -> Result<(), MemoryError> {
        match self.mode {
            PageTableMode::Simulation => {
                // No-op in simulation mode
                Ok(())
            }
            PageTableMode::Hardware => {
                // Verify the space exists
                if !self.space_handles.contains_key(&space_id) {
                    return Err(MemoryError::AddressSpaceNotFound(space_id));
                }

                // In a real implementation, we would:
                // 1. Get the page table handle
                // 2. Map each page in the region
                // 3. Set permissions based on MemoryPerms

                Ok(())
            }
        }
    }

    /// Unmaps a memory region from the page tables
    pub fn unmap_region(
        &mut self,
        space_id: AddressSpaceId,
        _region_id: MemoryRegionId,
    ) -> Result<(), MemoryError> {
        match self.mode {
            PageTableMode::Simulation => {
                // No-op in simulation mode
                Ok(())
            }
            PageTableMode::Hardware => {
                // Verify the space exists
                if !self.space_handles.contains_key(&space_id) {
                    return Err(MemoryError::AddressSpaceNotFound(space_id));
                }

                // In a real implementation, we would:
                // 1. Get the page table handle
                // 2. Unmap each page in the region

                Ok(())
            }
        }
    }

    /// Destroys the page table for an address space
    pub fn destroy_page_table(&mut self, space_id: AddressSpaceId) -> Result<(), MemoryError> {
        match self.mode {
            PageTableMode::Simulation => {
                // No-op in simulation mode
                Ok(())
            }
            PageTableMode::Hardware => {
                self.space_handles.remove(&space_id);
                // In a real implementation, we would:
                // 1. Deallocate all page tables
                // 2. Return physical pages to the allocator
                Ok(())
            }
        }
    }

    /// Returns the CR3 value for an address space (for context switching)
    pub fn get_cr3(&self, space_id: AddressSpaceId) -> Option<u64> {
        match self.mode {
            PageTableMode::Simulation => None,
            PageTableMode::Hardware => self.space_handles.get(&space_id).copied(),
        }
    }

    /// Returns the current mode
    pub fn mode(&self) -> PageTableMode {
        self.mode
    }
}

impl Default for PageTableBridge {
    fn default() -> Self {
        Self::new(PageTableMode::Simulation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::AddressSpace;

    #[test]
    fn test_simulation_mode() {
        let mut bridge = PageTableBridge::new(PageTableMode::Simulation);
        let space = AddressSpace::new();

        // All operations should succeed but be no-ops
        assert!(bridge.create_page_table(space.space_id).is_ok());
        assert!(bridge
            .map_region(
                space.space_id,
                MemoryRegionId::new(),
                0x1000,
                4096,
                MemoryPerms::read_write(),
            )
            .is_ok());
        assert!(bridge.get_cr3(space.space_id).is_none());
        assert!(bridge.destroy_page_table(space.space_id).is_ok());
    }

    #[test]
    fn test_hardware_mode() {
        let mut bridge = PageTableBridge::new(PageTableMode::Hardware);
        let space = AddressSpace::new();

        // Create page table
        assert!(bridge.create_page_table(space.space_id).is_ok());
        assert!(bridge.get_cr3(space.space_id).is_some());

        // Map a region
        assert!(bridge
            .map_region(
                space.space_id,
                MemoryRegionId::new(),
                0x1000,
                4096,
                MemoryPerms::read_write(),
            )
            .is_ok());

        // Destroy
        assert!(bridge.destroy_page_table(space.space_id).is_ok());
        assert!(bridge.get_cr3(space.space_id).is_none());
    }

    #[test]
    fn test_hardware_mode_invalid_space() {
        let mut bridge = PageTableBridge::new(PageTableMode::Hardware);
        let space = AddressSpace::new();

        // Try to map without creating the page table first
        let result = bridge.map_region(
            space.space_id,
            MemoryRegionId::new(),
            0x1000,
            4096,
            MemoryPerms::read_write(),
        );
        assert!(result.is_err());
    }
}
