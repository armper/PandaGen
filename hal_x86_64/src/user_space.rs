//! A program's own address space (PROC-002).
//!
//! Every program gets its own top-level page table. The upper half --
//! the kernel, the direct map of physical memory, the heap -- is shared:
//! its 256 top-level entries are copied from the kernel's table, so they
//! point at the same lower tables and the kernel is where it always is
//! whichever program is running. Those pages are not marked USER, so a
//! program cannot touch them. The lower half is the program's alone:
//! only the pages mapped here, with USER set, exist for it.
//!
//! The walk goes through `PhysMemory`, like `mmio_map`, so all of this
//! is tested on the host against an in-memory arena.
//!
//! A program's pointers are never followed by the kernel directly. A
//! system call that takes a buffer asks `read`/`write` here, which check
//! every page is the program's (mapped, USER, and writable if written)
//! and copy through the frames' physical addresses. A bad pointer is an
//! error returned to the program, never a fault in the kernel.

extern crate alloc;

use alloc::vec::Vec;

use crate::mmio_map::PhysMemory;
use crate::paging::{PageTableFlags, VirtAddr};

const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const PRESENT: u64 = PageTableFlags::PRESENT;
const WRITABLE: u64 = PageTableFlags::WRITABLE;
const USER: u64 = PageTableFlags::USER;
const HUGE: u64 = PageTableFlags::HUGE;
const NO_EXECUTE: u64 = PageTableFlags::NO_EXECUTE;
const PAGE: u64 = 4096;

/// The first address above a program's half: everything below is its.
pub const USER_END: u64 = 0x0000_8000_0000_0000;

/// Frames beyond page tables: user pages are freed and filled.
pub trait Frames: PhysMemory {
    /// A zeroed 4 KiB frame.
    fn alloc_frame(&mut self) -> Option<u64> {
        self.alloc_table()
    }
    fn free_frame(&mut self, frame: u64);
    fn read_bytes(&self, frame: u64, offset: usize, out: &mut [u8]);
    fn write_bytes(&mut self, frame: u64, offset: usize, data: &[u8]);
}

/// What a program may do with a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Access {
    pub write: bool,
    pub execute: bool,
}

impl Access {
    pub const CODE: Access = Access {
        write: false,
        execute: true,
    };
    pub const DATA: Access = Access {
        write: true,
        execute: false,
    };
    pub const READ_ONLY: Access = Access {
        write: false,
        execute: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceError {
    OutOfFrames,
    /// Not page-aligned, or not in the program's half.
    BadAddress,
    AlreadyMapped,
}

/// A program's address space: its top-level table and the frames it owns.
#[derive(Debug)]
pub struct UserSpace {
    pml4: u64,
    /// Every frame this space allocated -- tables and pages -- freed with it.
    owned: Vec<u64>,
    /// Whether the CPU honours no-execute (EFER.NXE); without it the bit
    /// is reserved and setting it faults.
    nx: bool,
}

impl UserSpace {
    /// A new space sharing `kernel_pml4`'s upper half.
    pub fn new<M: Frames>(mem: &mut M, kernel_pml4: u64, nx: bool) -> Result<Self, SpaceError> {
        let pml4 = mem.alloc_table().ok_or(SpaceError::OutOfFrames)?;
        for index in 256..512 {
            let entry = mem.read_entry(kernel_pml4 & ADDR_MASK, index);
            mem.write_entry(pml4, index, entry);
        }
        Ok(Self {
            pml4,
            owned: alloc::vec![pml4],
            nx,
        })
    }

    /// The value for CR3 while this program runs.
    pub fn cr3(&self) -> u64 {
        self.pml4
    }

    /// Frames this space holds, tables included.
    pub fn frames(&self) -> usize {
        self.owned.len()
    }

    /// Map a fresh zeroed page at `virt`; its frame.
    pub fn map_new<M: Frames>(
        &mut self,
        mem: &mut M,
        virt: u64,
        access: Access,
    ) -> Result<u64, SpaceError> {
        if !virt.is_multiple_of(PAGE) || virt >= USER_END {
            return Err(SpaceError::BadAddress);
        }
        let v = VirtAddr::new(virt);
        let mut table = self.pml4;
        for index in [v.pml4_index(), v.pdpt_index(), v.pd_index()] {
            let entry = mem.read_entry(table, index);
            if entry & PRESENT == 0 {
                let new = mem.alloc_table().ok_or(SpaceError::OutOfFrames)?;
                self.owned.push(new);
                // Every level above a user page must allow USER too.
                mem.write_entry(table, index, new | PRESENT | WRITABLE | USER);
                table = new;
            } else {
                table = entry & ADDR_MASK;
            }
        }
        if mem.read_entry(table, v.pt_index()) & PRESENT != 0 {
            return Err(SpaceError::AlreadyMapped);
        }
        let frame = mem.alloc_frame().ok_or(SpaceError::OutOfFrames)?;
        self.owned.push(frame);
        let mut flags = PRESENT | USER;
        if access.write {
            flags |= WRITABLE;
        }
        if !access.execute && self.nx {
            flags |= NO_EXECUTE;
        }
        mem.write_entry(table, v.pt_index(), frame | flags);
        Ok(frame)
    }

    /// Map pages covering `virt..virt+bytes.len()` and fill them with
    /// `bytes` (the rest zero): a program's code or data.
    pub fn load<M: Frames>(
        &mut self,
        mem: &mut M,
        virt: u64,
        bytes: &[u8],
        access: Access,
    ) -> Result<(), SpaceError> {
        let pages = (bytes.len() as u64).div_ceil(PAGE).max(1);
        for page in 0..pages {
            let frame = self.map_new(mem, virt + page * PAGE, access)?;
            let start = (page * PAGE) as usize;
            let end = bytes.len().min(start + PAGE as usize);
            if start < end {
                mem.write_bytes(frame, 0, &bytes[start..end]);
            }
        }
        Ok(())
    }

    /// The frame and leaf flags behind the program's page at `virt`, if
    /// it is the program's (present and USER at every level).
    pub fn page<M: Frames>(&self, mem: &M, virt: u64) -> Option<(u64, u64)> {
        if virt >= USER_END {
            return None;
        }
        let v = VirtAddr::new(virt);
        let mut table = self.pml4;
        for index in [v.pml4_index(), v.pdpt_index(), v.pd_index()] {
            let entry = mem.read_entry(table, index);
            if entry & (PRESENT | USER) != PRESENT | USER || entry & HUGE != 0 {
                return None;
            }
            table = entry & ADDR_MASK;
        }
        let leaf = mem.read_entry(table, v.pt_index());
        if leaf & (PRESENT | USER) != PRESENT | USER {
            return None;
        }
        Some((leaf & ADDR_MASK, leaf & !ADDR_MASK))
    }

    /// Copy the program's bytes at `addr` into `out`, if every one is
    /// its. Allocates nothing, so a system call can use it with
    /// interrupts off.
    pub fn read_into<M: Frames>(&self, mem: &M, addr: u64, out: &mut [u8]) -> bool {
        let Some(end) = addr.checked_add(out.len() as u64) else {
            return false;
        };
        if end > USER_END {
            return false;
        }
        let mut at = addr;
        while at < end {
            let Some((frame, _)) = self.page(mem, at & !(PAGE - 1)) else {
                return false;
            };
            let offset = (at % PAGE) as usize;
            let n = ((PAGE - at % PAGE).min(end - at)) as usize;
            let done = (at - addr) as usize;
            mem.read_bytes(frame, offset, &mut out[done..done + n]);
            at += n as u64;
        }
        true
    }

    /// The program's `len` bytes at `addr`, if every one is its.
    pub fn read<M: Frames>(&self, mem: &M, addr: u64, len: usize) -> Option<Vec<u8>> {
        let mut out = alloc::vec![0u8; len];
        self.read_into(mem, addr, &mut out).then_some(out)
    }

    /// Copy `data` into the program's memory at `addr`, if every byte
    /// there is its and writable. Nothing is written unless all can be.
    pub fn write<M: Frames>(&self, mem: &mut M, addr: u64, data: &[u8]) -> bool {
        let Some(end) = addr.checked_add(data.len() as u64) else {
            return false;
        };
        if end > USER_END {
            return false;
        }
        let mut page = addr & !(PAGE - 1);
        while page < end {
            match self.page(mem, page) {
                Some((_, flags)) if flags & WRITABLE != 0 => {}
                _ => return false,
            }
            page += PAGE;
        }
        let mut at = addr;
        while at < end {
            let (frame, _) = self.page(mem, at & !(PAGE - 1)).expect("checked above");
            let offset = (at % PAGE) as usize;
            let n = ((PAGE - at % PAGE).min(end - at)) as usize;
            let done = (at - addr) as usize;
            mem.write_bytes(frame, offset, &data[done..done + n]);
            at += n as u64;
        }
        true
    }

    /// Give every frame back. The space must not be running anywhere.
    pub fn free<M: Frames>(self, mem: &mut M) {
        for frame in self.owned {
            mem.free_frame(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Ram {
        frames: BTreeMap<u64, [u8; 4096]>,
        next: u64,
        freed: Vec<u64>,
    }

    impl Ram {
        fn new() -> Self {
            Ram {
                next: 0x10_0000,
                ..Default::default()
            }
        }
        fn frame(&self, f: u64) -> &[u8; 4096] {
            &self.frames[&f]
        }
    }

    impl PhysMemory for Ram {
        fn read_entry(&self, t: u64, i: usize) -> u64 {
            u64::from_le_bytes(self.frame(t)[i * 8..i * 8 + 8].try_into().unwrap())
        }
        fn write_entry(&mut self, t: u64, i: usize, v: u64) {
            self.frames.get_mut(&t).unwrap()[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        fn alloc_table(&mut self) -> Option<u64> {
            let f = self.next;
            self.next += 4096;
            self.frames.insert(f, [0; 4096]);
            Some(f)
        }
    }

    impl Frames for Ram {
        fn free_frame(&mut self, f: u64) {
            assert!(self.frames.remove(&f).is_some(), "freed twice");
            self.freed.push(f);
        }
        fn read_bytes(&self, f: u64, o: usize, out: &mut [u8]) {
            out.copy_from_slice(&self.frame(f)[o..o + out.len()]);
        }
        fn write_bytes(&mut self, f: u64, o: usize, d: &[u8]) {
            self.frames.get_mut(&f).unwrap()[o..o + d.len()].copy_from_slice(d);
        }
    }

    /// A kernel table with something in its upper half.
    fn kernel(ram: &mut Ram) -> u64 {
        let k = ram.alloc_table().unwrap();
        ram.write_entry(k, 256, 0xAAA000 | PRESENT | WRITABLE);
        ram.write_entry(k, 511, 0xBBB000 | PRESENT | WRITABLE);
        ram.write_entry(k, 0, 0xCCC000 | PRESENT); // lower half: not shared
        k
    }

    #[test]
    fn the_kernel_half_is_shared_and_the_program_half_starts_empty() {
        let mut ram = Ram::new();
        let k = kernel(&mut ram);
        let space = UserSpace::new(&mut ram, k, true).unwrap();
        let pml4 = space.cr3();
        assert_eq!(ram.read_entry(pml4, 256), 0xAAA000 | PRESENT | WRITABLE);
        assert_eq!(ram.read_entry(pml4, 511), 0xBBB000 | PRESENT | WRITABLE);
        assert_eq!(
            ram.read_entry(pml4, 0),
            0,
            "the kernel's lower half is not the program's"
        );
        assert_eq!(space.page(&ram, 0x40_0000), None);
    }

    #[test]
    fn loaded_code_is_the_programs_readable_and_not_writable() {
        let mut ram = Ram::new();
        let k = kernel(&mut ram);
        let mut space = UserSpace::new(&mut ram, k, true).unwrap();
        let code: Vec<u8> = (0..5000u32).map(|i| i as u8).collect();
        space
            .load(&mut ram, 0x40_0000, &code, Access::CODE)
            .unwrap();
        assert_eq!(space.read(&ram, 0x40_0000, 5000).unwrap(), code);
        let (_, flags) = space.page(&ram, 0x40_1000).unwrap();
        assert_eq!(flags & (USER | WRITABLE | NO_EXECUTE), USER);
        assert!(
            !space.write(&mut ram, 0x40_0010, b"no"),
            "code is read-only"
        );
        // The rest of the second page is zero.
        assert_eq!(space.read(&ram, 0x40_1000 + 904, 8).unwrap(), [0; 8]);
    }

    #[test]
    fn data_pages_are_writable_and_not_executable() {
        let mut ram = Ram::new();
        let k = kernel(&mut ram);
        let mut space = UserSpace::new(&mut ram, k, true).unwrap();
        space.map_new(&mut ram, 0x80_0000, Access::DATA).unwrap();
        space.map_new(&mut ram, 0x80_1000, Access::DATA).unwrap();
        // A write across the page boundary.
        assert!(space.write(&mut ram, 0x80_0ffe, b"abcd"));
        assert_eq!(space.read(&ram, 0x80_0ffe, 4).unwrap(), b"abcd");
        let (_, flags) = space.page(&ram, 0x80_0000).unwrap();
        assert_ne!(flags & NO_EXECUTE, 0);
        // Without NXE the bit is never set (it would be reserved).
        let mut old = UserSpace::new(&mut ram, k, false).unwrap();
        old.map_new(&mut ram, 0x80_0000, Access::DATA).unwrap();
        assert_eq!(old.page(&ram, 0x80_0000).unwrap().1 & NO_EXECUTE, 0);
    }

    #[test]
    fn pointers_outside_the_program_are_refused() {
        let mut ram = Ram::new();
        let k = kernel(&mut ram);
        let mut space = UserSpace::new(&mut ram, k, true).unwrap();
        space.map_new(&mut ram, 0x80_0000, Access::DATA).unwrap();
        // Unmapped, running off the end, the kernel's half, wrapping.
        assert!(space.read(&ram, 0x90_0000, 1).is_none());
        assert!(space.read(&ram, 0x80_0ff0, 32).is_none());
        assert!(space.read(&ram, 0xffff_8000_0000_0000, 8).is_none());
        assert!(space.read(&ram, u64::MAX - 3, 8).is_none());
        assert!(space.read(&ram, USER_END - 4, 8).is_none());
        assert!(!space.write(&mut ram, 0x80_0ff0, &[1; 32]));
        // And a refused write wrote nothing.
        assert_eq!(space.read(&ram, 0x80_0ff0, 16).unwrap(), [0; 16]);
        // Mapping the kernel's half, or twice, or unaligned.
        assert_eq!(
            space.map_new(&mut ram, 0xffff_8000_0000_0000, Access::DATA),
            Err(SpaceError::BadAddress)
        );
        assert_eq!(
            space.map_new(&mut ram, 0x80_0000, Access::DATA),
            Err(SpaceError::AlreadyMapped)
        );
        assert_eq!(
            space.map_new(&mut ram, 0x80_0001, Access::DATA),
            Err(SpaceError::BadAddress)
        );
    }

    #[test]
    fn a_kernel_page_is_not_the_programs_even_through_its_table() {
        let mut ram = Ram::new();
        let k = kernel(&mut ram);
        let mut space = UserSpace::new(&mut ram, k, true).unwrap();
        space.map_new(&mut ram, 0x80_0000, Access::DATA).unwrap();
        // A leaf without USER under the program's tables (as the kernel
        // might have made) is not the program's.
        let pml4 = space.cr3();
        let pdpt = ram.read_entry(pml4, 0) & ADDR_MASK;
        let pd = ram.read_entry(pdpt, 0) & ADDR_MASK;
        let pt = ram.read_entry(pd, 4) & ADDR_MASK;
        ram.write_entry(pt, 1, 0xDDD000 | PRESENT | WRITABLE);
        assert_eq!(space.page(&ram, 0x80_1000), None);
    }

    #[test]
    fn freeing_gives_back_every_frame_and_nothing_of_the_kernels() {
        let mut ram = Ram::new();
        let k = kernel(&mut ram);
        let mut space = UserSpace::new(&mut ram, k, true).unwrap();
        space
            .load(&mut ram, 0x40_0000, &[0x90; 100], Access::CODE)
            .unwrap();
        space.map_new(&mut ram, 0x5f_f000, Access::DATA).unwrap();
        let held = space.frames();
        // pml4, pdpt, pd, pt (shared by both), code page, stack page.
        assert_eq!(held, 6);
        let before = ram.frames.len();
        space.free(&mut ram);
        assert_eq!(ram.frames.len(), before - held);
        assert!(ram.frames.contains_key(&k), "the kernel's table stays");
    }
}
