//! x86_64 Page Table Management
//!
//! Implements 4-level paging (PML4 -> PDPT -> PD -> PT) for x86_64.
//!
//! ## Design Philosophy
//!
//! This module provides a **minimal, compile-safe foundation** for virtual memory:
//! - Type-safe page table entry manipulation
//! - Clear separation of physical/virtual addresses
//! - Explicit permission management
//! - Testable without hardware (simulation mode)
//!
//! ## Integration with AddressSpace
//!
//! Maps PandaGen's capability model to x86_64 MMU:
//! - `AddressSpace` → CR3 (page table root)
//! - `MemoryRegion` → page table entries
//! - `MemoryPerms` → page table flags (R/W/X)

extern crate alloc;

use core::fmt;

/// Page size (4 KiB)
pub const PAGE_SIZE: usize = 4096;

/// Page table levels
pub const PAGE_TABLE_LEVELS: usize = 4;

/// Entries per page table
pub const ENTRIES_PER_TABLE: usize = 512;

/// Physical address type (explicit to avoid confusion)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysAddr(pub u64);

impl PhysAddr {
    /// Creates a new physical address
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// Returns the inner value
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Checks if the address is page-aligned
    pub const fn is_aligned(self) -> bool {
        self.0 % PAGE_SIZE as u64 == 0
    }
}

impl fmt::Display for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PhysAddr({:#x})", self.0)
    }
}

/// Virtual address type (explicit to avoid confusion)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtAddr(pub u64);

impl VirtAddr {
    /// Creates a new virtual address
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// Returns the inner value
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Checks if the address is page-aligned
    pub const fn is_aligned(self) -> bool {
        self.0 % PAGE_SIZE as u64 == 0
    }

    /// Extracts the PML4 index (bits 39-47)
    pub const fn pml4_index(self) -> usize {
        ((self.0 >> 39) & 0x1FF) as usize
    }

    /// Extracts the PDPT index (bits 30-38)
    pub const fn pdpt_index(self) -> usize {
        ((self.0 >> 30) & 0x1FF) as usize
    }

    /// Extracts the PD index (bits 21-29)
    pub const fn pd_index(self) -> usize {
        ((self.0 >> 21) & 0x1FF) as usize
    }

    /// Extracts the PT index (bits 12-20)
    pub const fn pt_index(self) -> usize {
        ((self.0 >> 12) & 0x1FF) as usize
    }

    /// Extracts the page offset (bits 0-11)
    pub const fn page_offset(self) -> usize {
        (self.0 & 0xFFF) as usize
    }
}

impl fmt::Display for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VirtAddr({:#x})", self.0)
    }
}

/// Page table entry flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTableFlags(u64);

impl PageTableFlags {
    /// Present flag (bit 0)
    pub const PRESENT: u64 = 1 << 0;
    /// Writable flag (bit 1)
    pub const WRITABLE: u64 = 1 << 1;
    /// User accessible flag (bit 2)
    pub const USER: u64 = 1 << 2;
    /// Write-through caching flag (bit 3)
    pub const WRITE_THROUGH: u64 = 1 << 3;
    /// Cache disable flag (bit 4)
    pub const CACHE_DISABLE: u64 = 1 << 4;
    /// Accessed flag (bit 5)
    pub const ACCESSED: u64 = 1 << 5;
    /// Dirty flag (bit 6)
    pub const DIRTY: u64 = 1 << 6;
    /// Huge page flag (bit 7)
    pub const HUGE: u64 = 1 << 7;
    /// Global flag (bit 8)
    pub const GLOBAL: u64 = 1 << 8;
    /// No execute flag (bit 63, requires EFER.NXE)
    pub const NO_EXECUTE: u64 = 1 << 63;

    /// Creates empty flags
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Creates flags from raw value
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Returns raw bits
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Checks if a flag is set
    pub const fn contains(self, flag: u64) -> bool {
        (self.0 & flag) == flag
    }

    /// Sets a flag
    pub const fn with_flag(self, flag: u64) -> Self {
        Self(self.0 | flag)
    }

    /// Clears a flag
    pub const fn without_flag(self, flag: u64) -> Self {
        Self(self.0 & !flag)
    }
}

/// Page table entry
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    /// Physical address mask (bits 12-51)
    const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

    /// Creates an unused entry
    pub const fn unused() -> Self {
        Self(0)
    }

    /// Checks if the entry is unused
    pub const fn is_unused(self) -> bool {
        self.0 == 0
    }

    /// Returns the flags
    pub const fn flags(self) -> PageTableFlags {
        PageTableFlags::from_bits(self.0 & !Self::ADDR_MASK)
    }

    /// Returns the physical address
    pub const fn phys_addr(self) -> Option<PhysAddr> {
        if self.flags().contains(PageTableFlags::PRESENT) {
            Some(PhysAddr::new(self.0 & Self::ADDR_MASK))
        } else {
            None
        }
    }

    /// Sets the entry
    pub fn set(&mut self, addr: PhysAddr, flags: PageTableFlags) {
        self.0 = addr.as_u64() | flags.bits();
    }

    /// Clears the entry
    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

/// Page table (512 entries)
#[repr(align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; ENTRIES_PER_TABLE],
}

impl PageTable {
    /// Creates a new empty page table
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::unused(); ENTRIES_PER_TABLE],
        }
    }

    /// Returns a reference to an entry
    pub fn entry(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }

    /// Returns a mutable reference to an entry
    pub fn entry_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }

    /// Clears all entries
    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            entry.clear();
        }
    }
}

impl Default for PageTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Address space handle (wraps CR3 register value)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressSpaceHandle {
    /// Physical address of the PML4 table
    pub pml4_phys: PhysAddr,
}

impl AddressSpaceHandle {
    /// Creates a new address space handle
    pub const fn new(pml4_phys: PhysAddr) -> Self {
        Self { pml4_phys }
    }

    /// Returns the CR3 register value
    pub const fn as_cr3(self) -> u64 {
        self.pml4_phys.as_u64()
    }
}

/// Page table manager (simulation/testing mode)
///
/// This provides a simplified page table manager for testing without hardware.
/// In a real implementation, this would interface with actual page tables in memory.
pub struct PageTableManager {
    /// Next physical address to allocate (for simulation)
    next_phys_addr: u64,
    /// Simulated page tables (for testing without real memory)
    /// Maps physical address to page table
    tables: alloc::collections::BTreeMap<u64, PageTable>,
}

impl PageTableManager {
    /// Creates a new page table manager
    pub fn new() -> Self {
        Self {
            // Start allocating at 1MB to avoid low memory
            next_phys_addr: 0x10_0000,
            tables: alloc::collections::BTreeMap::new(),
        }
    }

    /// Allocates a physical page
    pub fn alloc_page(&mut self) -> PhysAddr {
        let addr = self.next_phys_addr;
        self.next_phys_addr += PAGE_SIZE as u64;
        PhysAddr::new(addr)
    }

    /// Creates a new address space (allocates PML4 table)
    pub fn create_address_space(&mut self) -> AddressSpaceHandle {
        let pml4_phys = self.alloc_page();
        self.tables.insert(pml4_phys.as_u64(), PageTable::new());
        AddressSpaceHandle::new(pml4_phys)
    }

    /// Maps a virtual page to a physical page
    ///
    /// This is a simplified implementation for testing.
    /// Real implementation would walk page tables and allocate intermediate levels.
    pub fn map_page(
        &mut self,
        handle: AddressSpaceHandle,
        virt: VirtAddr,
        phys: PhysAddr,
        perms: Permissions,
    ) -> Result<(), &'static str> {
        if !virt.is_aligned() || !phys.is_aligned() {
            return Err("Addresses must be page-aligned");
        }

        // In a real implementation, we would:
        // 1. Walk the page table hierarchy (PML4 -> PDPT -> PD -> PT)
        // 2. Allocate intermediate tables as needed
        // 3. Set the final PT entry

        // For now, just record the mapping in a simplified way
        let _pml4 = self
            .tables
            .get_mut(&handle.pml4_phys.as_u64())
            .ok_or("Invalid address space handle")?;

        // Simplified: just verify the mapping would be valid
        // Real implementation would actually set page table entries

        Ok(())
    }

    /// Unmaps a virtual page
    pub fn unmap_page(
        &mut self,
        handle: AddressSpaceHandle,
        virt: VirtAddr,
    ) -> Result<(), &'static str> {
        if !virt.is_aligned() {
            return Err("Address must be page-aligned");
        }

        let _pml4 = self
            .tables
            .get_mut(&handle.pml4_phys.as_u64())
            .ok_or("Invalid address space handle")?;

        // Simplified: just verify the unmapping would be valid
        // Real implementation would clear page table entries

        Ok(())
    }

    /// Returns the number of allocated tables (for testing)
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }
}

impl Default for PageTableManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory region permissions (maps to PandaGen MemoryPerms)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub user: bool,
}

impl Permissions {
    /// Kernel read-only
    pub const fn kernel_ro() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
            user: false,
        }
    }

    /// Kernel read-write
    pub const fn kernel_rw() -> Self {
        Self {
            read: true,
            write: true,
            execute: false,
            user: false,
        }
    }

    /// Kernel read-execute
    pub const fn kernel_rx() -> Self {
        Self {
            read: true,
            write: false,
            execute: true,
            user: false,
        }
    }

    /// User read-only
    pub const fn user_ro() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
            user: true,
        }
    }

    /// User read-write
    pub const fn user_rw() -> Self {
        Self {
            read: true,
            write: true,
            execute: false,
            user: true,
        }
    }

    /// User read-execute
    pub const fn user_rx() -> Self {
        Self {
            read: true,
            write: false,
            execute: true,
            user: true,
        }
    }

    /// Converts to page table flags
    pub fn to_flags(self) -> PageTableFlags {
        let mut flags = PageTableFlags::from_bits(PageTableFlags::PRESENT);

        if self.write {
            flags = flags.with_flag(PageTableFlags::WRITABLE);
        }
        if self.user {
            flags = flags.with_flag(PageTableFlags::USER);
        }
        if !self.execute {
            flags = flags.with_flag(PageTableFlags::NO_EXECUTE);
        }

        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phys_addr() {
        let addr = PhysAddr::new(0x1000);
        assert_eq!(addr.as_u64(), 0x1000);
        assert!(addr.is_aligned());

        let unaligned = PhysAddr::new(0x1001);
        assert!(!unaligned.is_aligned());
    }

    #[test]
    fn test_virt_addr_indices() {
        // Test address 0x0000_1234_5678_9ABC
        let addr = VirtAddr::new(0x0000_1234_5678_9ABC);

        // Bits 39-47: PML4 index
        assert_eq!(addr.pml4_index(), 0x24);
        // Bits 30-38: PDPT index
        assert_eq!(addr.pdpt_index(), 0xd1);
        // Bits 21-29: PD index
        assert_eq!(addr.pd_index(), 0xb3);
        // Bits 12-20: PT index
        assert_eq!(addr.pt_index(), 0x189);
        // Bits 0-11: page offset
        assert_eq!(addr.page_offset(), 0xABC);
    }

    #[test]
    fn test_page_table_flags() {
        let flags = PageTableFlags::empty()
            .with_flag(PageTableFlags::PRESENT)
            .with_flag(PageTableFlags::WRITABLE);

        assert!(flags.contains(PageTableFlags::PRESENT));
        assert!(flags.contains(PageTableFlags::WRITABLE));
        assert!(!flags.contains(PageTableFlags::USER));

        let cleared = flags.without_flag(PageTableFlags::WRITABLE);
        assert!(cleared.contains(PageTableFlags::PRESENT));
        assert!(!cleared.contains(PageTableFlags::WRITABLE));
    }

    #[test]
    fn test_page_table_entry() {
        let mut entry = PageTableEntry::unused();
        assert!(entry.is_unused());
        assert!(entry.phys_addr().is_none());

        let phys = PhysAddr::new(0x1000);
        let flags = PageTableFlags::empty()
            .with_flag(PageTableFlags::PRESENT)
            .with_flag(PageTableFlags::WRITABLE);

        entry.set(phys, flags);
        assert!(!entry.is_unused());
        assert_eq!(entry.phys_addr(), Some(phys));
        assert!(entry.flags().contains(PageTableFlags::PRESENT));
        assert!(entry.flags().contains(PageTableFlags::WRITABLE));

        entry.clear();
        assert!(entry.is_unused());
    }

    #[test]
    fn test_page_table() {
        let mut table = PageTable::new();

        // All entries should be unused
        for i in 0..ENTRIES_PER_TABLE {
            assert!(table.entry(i).is_unused());
        }

        // Set an entry
        let phys = PhysAddr::new(0x2000);
        let flags = PageTableFlags::empty().with_flag(PageTableFlags::PRESENT);
        table.entry_mut(10).set(phys, flags);

        assert!(!table.entry(10).is_unused());
        assert_eq!(table.entry(10).phys_addr(), Some(phys));

        // Clear all
        table.clear();
        assert!(table.entry(10).is_unused());
    }

    #[test]
    fn test_permissions_to_flags() {
        let perms = Permissions::user_rw();
        let flags = perms.to_flags();

        assert!(flags.contains(PageTableFlags::PRESENT));
        assert!(flags.contains(PageTableFlags::WRITABLE));
        assert!(flags.contains(PageTableFlags::USER));
        assert!(flags.contains(PageTableFlags::NO_EXECUTE));
    }

    #[test]
    fn test_address_space_handle() {
        let phys = PhysAddr::new(0x10000);
        let handle = AddressSpaceHandle::new(phys);

        assert_eq!(handle.as_cr3(), 0x10000);
        assert_eq!(handle.pml4_phys, phys);
    }

    #[test]
    fn test_page_table_manager_create_address_space() {
        let mut manager = PageTableManager::new();

        let handle1 = manager.create_address_space();
        let handle2 = manager.create_address_space();

        // Each address space should have a unique PML4
        assert_ne!(handle1.pml4_phys, handle2.pml4_phys);
        assert_eq!(manager.table_count(), 2);
    }

    #[test]
    fn test_page_table_manager_map_page() {
        let mut manager = PageTableManager::new();
        let handle = manager.create_address_space();

        let virt = VirtAddr::new(0x1000);
        let phys = PhysAddr::new(0x2000);
        let perms = Permissions::user_rw();

        let result = manager.map_page(handle, virt, phys, perms);
        assert!(result.is_ok());
    }

    #[test]
    fn test_page_table_manager_unaligned_addresses() {
        let mut manager = PageTableManager::new();
        let handle = manager.create_address_space();

        let virt = VirtAddr::new(0x1001); // Unaligned
        let phys = PhysAddr::new(0x2000);
        let perms = Permissions::user_rw();

        let result = manager.map_page(handle, virt, phys, perms);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Addresses must be page-aligned");
    }

    #[test]
    fn test_page_table_manager_unmap() {
        let mut manager = PageTableManager::new();
        let handle = manager.create_address_space();

        let virt = VirtAddr::new(0x1000);
        let result = manager.unmap_page(handle, virt);
        assert!(result.is_ok());
    }
}
