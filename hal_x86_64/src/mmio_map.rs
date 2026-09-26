//! Map a device page into the live x86_64 page tables.
//!
//! The bootloader's higher-half direct map only covers memory-map entries,
//! so MMIO windows such as the local APIC (0xFEE00000) fault when touched
//! through it. `map_4k` walks the tables from CR3, allocates intermediate
//! tables as needed, and installs a 4 KiB leaf. Table access goes through
//! `PhysMemory`, so the walk is host-tested against an in-memory arena.

use crate::paging::{PageTableFlags, VirtAddr};

#[cfg(test)]
const ENTRIES: usize = 512;
const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const PRESENT: u64 = PageTableFlags::PRESENT;
const WRITABLE: u64 = PageTableFlags::WRITABLE;
const HUGE: u64 = PageTableFlags::HUGE;

/// Access to page-table frames by physical address.
pub trait PhysMemory {
    fn read_entry(&self, table_phys: u64, index: usize) -> u64;
    fn write_entry(&mut self, table_phys: u64, index: usize, value: u64);
    /// A zeroed, page-aligned 4 KiB frame for a new table.
    fn alloc_table(&mut self) -> Option<u64>;
}

/// Page-table access through the higher-half direct map.
pub struct HhdmMemory<F: FnMut() -> Option<u64>> {
    hhdm: u64,
    alloc_frame: F,
}

impl<F: FnMut() -> Option<u64>> HhdmMemory<F> {
    /// # Safety
    /// `hhdm` must be the live direct-map offset and `alloc_frame` must
    /// return unused frames that the direct map covers.
    pub unsafe fn new(hhdm: u64, alloc_frame: F) -> Self {
        Self { hhdm, alloc_frame }
    }
}

impl<F: FnMut() -> Option<u64>> PhysMemory for HhdmMemory<F> {
    fn read_entry(&self, table_phys: u64, index: usize) -> u64 {
        let ptr = (self.hhdm + table_phys) as *const u64;
        // SAFETY: the table lies in memory covered by the direct map.
        unsafe { core::ptr::read_volatile(ptr.add(index)) }
    }

    fn write_entry(&mut self, table_phys: u64, index: usize, value: u64) {
        let ptr = (self.hhdm + table_phys) as *mut u64;
        // SAFETY: as above.
        unsafe { core::ptr::write_volatile(ptr.add(index), value) }
    }

    fn alloc_table(&mut self) -> Option<u64> {
        let frame = (self.alloc_frame)()?;
        let ptr = (self.hhdm + frame) as *mut u8;
        // SAFETY: a fresh frame covered by the direct map.
        unsafe { core::ptr::write_bytes(ptr, 0, 4096) };
        Some(frame)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    /// A huge page already covers the address at the given level (2 = 1 GiB, 3 = 2 MiB).
    HugePage(u8),
    /// A 4 KiB mapping to a different frame already exists.
    AlreadyMapped(u64),
    OutOfFrames,
}

/// Physical address `virt` translates to, if mapped (4 KiB, 2 MiB, or 1 GiB pages).
pub fn translate<M: PhysMemory>(mem: &M, cr3: u64, virt: u64) -> Option<u64> {
    let v = VirtAddr::new(virt);
    let mut table = cr3 & ADDR_MASK;
    let indices = [v.pml4_index(), v.pdpt_index(), v.pd_index(), v.pt_index()];
    for (level, &index) in indices.iter().enumerate() {
        let entry = mem.read_entry(table, index);
        if entry & PRESENT == 0 {
            return None;
        }
        let base = entry & ADDR_MASK;
        if level == 3 {
            return Some(base + (virt & 0xFFF));
        }
        if level >= 1 && entry & HUGE != 0 {
            let shift = if level == 1 { 30 } else { 21 };
            let mask = (1u64 << shift) - 1;
            return Some((base & !mask) + (virt & mask));
        }
        table = base;
    }
    None
}

/// Map the 4 KiB page at `virt` to `phys` with `leaf_flags` (PRESENT is added).
/// Intermediate tables are created PRESENT | WRITABLE. Mapping the same
/// frame twice is a no-op.
pub fn map_4k<M: PhysMemory>(
    mem: &mut M,
    cr3: u64,
    virt: u64,
    phys: u64,
    leaf_flags: u64,
) -> Result<(), MapError> {
    let v = VirtAddr::new(virt);
    let mut table = cr3 & ADDR_MASK;
    let indices = [v.pml4_index(), v.pdpt_index(), v.pd_index()];
    for (level, &index) in indices.iter().enumerate() {
        let entry = mem.read_entry(table, index);
        if entry & PRESENT == 0 {
            let new = mem.alloc_table().ok_or(MapError::OutOfFrames)?;
            mem.write_entry(table, index, new | PRESENT | WRITABLE);
            table = new;
            continue;
        }
        if level >= 1 && entry & HUGE != 0 {
            return Err(MapError::HugePage(level as u8 + 1));
        }
        table = entry & ADDR_MASK;
    }
    let existing = mem.read_entry(table, v.pt_index());
    let target = (phys & ADDR_MASK) | leaf_flags | PRESENT;
    if existing & PRESENT != 0 {
        if existing & ADDR_MASK == phys & ADDR_MASK {
            return Ok(());
        }
        return Err(MapError::AlreadyMapped(existing & ADDR_MASK));
    }
    mem.write_entry(table, v.pt_index(), target);
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::collections::BTreeMap;

    /// Sparse physical memory: tables keyed by frame address.
    #[derive(Default)]
    struct Arena {
        tables: BTreeMap<u64, [u64; ENTRIES]>,
        next: u64,
    }

    impl Arena {
        fn with_root() -> (Self, u64) {
            let mut a = Arena {
                next: 0x10_0000,
                ..Default::default()
            };
            let root = a.alloc_table().unwrap();
            (a, root)
        }
    }

    impl PhysMemory for Arena {
        fn read_entry(&self, t: u64, i: usize) -> u64 {
            self.tables[&t][i]
        }
        fn write_entry(&mut self, t: u64, i: usize, v: u64) {
            self.tables.get_mut(&t).unwrap()[i] = v;
        }
        fn alloc_table(&mut self) -> Option<u64> {
            let f = self.next;
            self.next += 4096;
            self.tables.insert(f, [0; ENTRIES]);
            Some(f)
        }
    }

    const LAPIC_VIRT: u64 = 0xffff_8000_fee0_0000;
    const LAPIC_PHYS: u64 = 0xfee0_0000;
    const UC: u64 = PageTableFlags::WRITABLE | PageTableFlags::CACHE_DISABLE;

    #[test]
    fn unmapped_translates_to_none_and_maps_creating_tables() {
        let (mut mem, cr3) = Arena::with_root();
        assert_eq!(translate(&mem, cr3, LAPIC_VIRT), None);
        map_4k(&mut mem, cr3, LAPIC_VIRT, LAPIC_PHYS, UC).unwrap();
        assert_eq!(mem.tables.len(), 4, "root + pdpt + pd + pt");
        assert_eq!(
            translate(&mem, cr3, LAPIC_VIRT + 0x20),
            Some(LAPIC_PHYS + 0x20)
        );
        let v = VirtAddr::new(LAPIC_VIRT);
        let pdpt = mem.read_entry(cr3, v.pml4_index()) & ADDR_MASK;
        let pd = mem.read_entry(pdpt, v.pdpt_index()) & ADDR_MASK;
        let pt = mem.read_entry(pd, v.pd_index()) & ADDR_MASK;
        assert_eq!(mem.read_entry(pt, v.pt_index()), LAPIC_PHYS | UC | PRESENT);
        assert_eq!(
            mem.read_entry(cr3, v.pml4_index()) & 0xFFF,
            PRESENT | WRITABLE
        );
    }

    #[test]
    fn remapping_same_frame_is_noop_and_different_frame_errors() {
        let (mut mem, cr3) = Arena::with_root();
        map_4k(&mut mem, cr3, LAPIC_VIRT, LAPIC_PHYS, UC).unwrap();
        assert_eq!(map_4k(&mut mem, cr3, LAPIC_VIRT, LAPIC_PHYS, UC), Ok(()));
        assert_eq!(
            map_4k(&mut mem, cr3, LAPIC_VIRT, 0x1000, UC),
            Err(MapError::AlreadyMapped(LAPIC_PHYS))
        );
    }

    #[test]
    fn huge_pages_are_translated_and_refused_for_mapping() {
        let (mut mem, cr3) = Arena::with_root();
        let v = VirtAddr::new(LAPIC_VIRT);
        let pdpt = mem.alloc_table().unwrap();
        mem.write_entry(cr3, v.pml4_index(), pdpt | PRESENT | WRITABLE);
        // 1 GiB page covering 0xC0000000..0x100000000
        mem.write_entry(
            pdpt,
            v.pdpt_index(),
            0xC000_0000 | PRESENT | WRITABLE | HUGE,
        );
        assert_eq!(
            translate(&mem, cr3, LAPIC_VIRT + 0x30),
            Some(LAPIC_PHYS + 0x30)
        );
        assert_eq!(
            map_4k(&mut mem, cr3, LAPIC_VIRT, LAPIC_PHYS, UC),
            Err(MapError::HugePage(2))
        );
        // 2 MiB page
        let pd = mem.alloc_table().unwrap();
        mem.write_entry(pdpt, v.pdpt_index(), pd | PRESENT | WRITABLE);
        mem.write_entry(pd, v.pd_index(), 0xfee0_0000 | PRESENT | WRITABLE | HUGE);
        assert_eq!(
            translate(&mem, cr3, LAPIC_VIRT + 0x1234),
            Some(LAPIC_PHYS + 0x1234)
        );
        assert_eq!(
            map_4k(&mut mem, cr3, LAPIC_VIRT, LAPIC_PHYS, UC),
            Err(MapError::HugePage(3))
        );
    }

    #[test]
    fn out_of_frames_is_reported() {
        struct Empty(Arena);
        impl PhysMemory for Empty {
            fn read_entry(&self, t: u64, i: usize) -> u64 {
                self.0.read_entry(t, i)
            }
            fn write_entry(&mut self, t: u64, i: usize, v: u64) {
                self.0.write_entry(t, i, v)
            }
            fn alloc_table(&mut self) -> Option<u64> {
                None
            }
        }
        let (arena, cr3) = Arena::with_root();
        let mut mem = Empty(arena);
        assert_eq!(
            map_4k(&mut mem, cr3, LAPIC_VIRT, LAPIC_PHYS, UC),
            Err(MapError::OutOfFrames)
        );
    }
}
