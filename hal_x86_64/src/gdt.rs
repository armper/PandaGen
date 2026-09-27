//! Global descriptor table and task state segment for 64-bit mode.
//!
//! The kernel installs its own GDT (instead of keeping the bootloader's)
//! so every CPU can load a TSS with a dedicated interrupt stack for
//! double faults. Descriptor encoding is host-tested bit for bit.
//!
//! Programs run in ring 3 (PROC-002), so there are user code and data
//! segments too, with descriptor privilege 3. Their order -- data before
//! code -- is the one `sysret` would expect, should system calls move to
//! it from `int 0x80`.

/// Kernel code selector in [`Gdt`].
pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
/// Kernel data selector in [`Gdt`].
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
/// User data selector in [`Gdt`], with requested privilege 3.
pub const USER_DATA_SELECTOR: u16 = 0x18 | 3;
/// User code selector in [`Gdt`], with requested privilege 3.
pub const USER_CODE_SELECTOR: u16 = 0x20 | 3;
/// TSS selector in [`Gdt`] (a 16-byte system descriptor).
pub const TSS_SELECTOR: u16 = 0x28;

/// 64-bit code segment: present, DPL 0, code, readable, long mode.
pub const KERNEL_CODE_DESCRIPTOR: u64 = 0x00AF_9A00_0000_FFFF;
/// Data segment: present, DPL 0, data, writable.
pub const KERNEL_DATA_DESCRIPTOR: u64 = 0x00CF_9200_0000_FFFF;
/// Data segment: present, DPL 3, data, writable.
pub const USER_DATA_DESCRIPTOR: u64 = 0x00CF_F200_0000_FFFF;
/// 64-bit code segment: present, DPL 3, code, readable, long mode.
pub const USER_CODE_DESCRIPTOR: u64 = 0x00AF_FA00_0000_FFFF;

/// 64-bit task state segment.
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct Tss {
    _reserved0: u32,
    /// Stack pointers for privilege levels 0-2.
    pub rsp: [u64; 3],
    _reserved1: u64,
    /// Interrupt stack table entries 1-7 (index 0 is IST1).
    pub ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    /// I/O map base; pointing past the segment limit disables the map.
    pub iomap_base: u16,
}

impl Tss {
    pub const fn new() -> Self {
        Self {
            _reserved0: 0,
            rsp: [0; 3],
            _reserved1: 0,
            ist: [0; 7],
            _reserved2: 0,
            _reserved3: 0,
            iomap_base: core::mem::size_of::<Tss>() as u16,
        }
    }
}

impl Default for Tss {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = assert!(core::mem::size_of::<Tss>() == 104);

/// Encode the two halves of a 64-bit available-TSS system descriptor.
pub const fn tss_descriptor(base: u64, limit: u32) -> [u64; 2] {
    let low = (limit as u64 & 0xFFFF)
        | (base & 0xFF_FFFF) << 16
        | 0x89 << 40 // present, type 9 = available 64-bit TSS
        | (limit as u64 >> 16 & 0xF) << 48
        | (base >> 24 & 0xFF) << 56;
    let high = base >> 32;
    [low, high]
}

/// A seven-entry GDT: null, kernel code, kernel data, user data, user
/// code, TSS (two slots).
#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct Gdt {
    entries: [u64; 7],
}

impl Gdt {
    pub const fn new() -> Self {
        Self {
            entries: [
                0,
                KERNEL_CODE_DESCRIPTOR,
                KERNEL_DATA_DESCRIPTOR,
                USER_DATA_DESCRIPTOR,
                USER_CODE_DESCRIPTOR,
                0,
                0,
            ],
        }
    }

    /// Point the TSS descriptor at `tss`.
    pub fn set_tss(&mut self, tss: *const Tss) {
        let [low, high] = tss_descriptor(tss as u64, core::mem::size_of::<Tss>() as u32 - 1);
        self.entries[5] = low;
        self.entries[6] = high;
    }

    pub fn entries(&self) -> &[u64; 7] {
        &self.entries
    }

    /// Descriptor-table register value for `lgdt`.
    pub fn pointer(&self) -> DescriptorTablePointer {
        DescriptorTablePointer {
            limit: (core::mem::size_of::<[u64; 7]>() - 1) as u16,
            base: self.entries.as_ptr() as u64,
        }
    }
}

impl Default for Gdt {
    fn default() -> Self {
        Self::new()
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct DescriptorTablePointer {
    pub limit: u16,
    pub base: u64,
}

/// Load `gdt`, reload the segment registers, and load the TSS.
///
/// # Safety
/// `gdt` must stay valid for as long as the CPU uses it, and its TSS
/// descriptor must point at a valid `Tss`.
#[cfg(target_arch = "x86_64")]
pub unsafe fn load(gdt: &'static Gdt) {
    let pointer = gdt.pointer();
    core::arch::asm!(
        "lgdt [{ptr}]",
        // Reload CS with a far return.
        "push {code}",
        "lea {tmp}, [rip + 2f]",
        "push {tmp}",
        "retfq",
        "2:",
        "mov ds, {data:x}",
        "mov es, {data:x}",
        "mov ss, {data:x}",
        "xor {tmp:e}, {tmp:e}",
        "mov fs, {tmp:x}",
        "mov gs, {tmp:x}",
        "ltr {tss:x}",
        ptr = in(reg) &pointer,
        code = const KERNEL_CODE_SELECTOR as u64,
        data = in(reg) KERNEL_DATA_SELECTOR as u64,
        tss = in(reg) TSS_SELECTOR as u64,
        tmp = out(reg) _,
        // Not `nostack`: the far return pushes and pops.
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_descriptors_match_known_encodings() {
        let gdt = Gdt::new();
        assert_eq!(gdt.entries()[0], 0);
        assert_eq!(gdt.entries()[1], 0x00AF_9A00_0000_FFFF);
        assert_eq!(gdt.entries()[2], 0x00CF_9200_0000_FFFF);
        let limit = gdt.pointer().limit;
        assert_eq!(limit, 55);
    }

    #[test]
    fn user_segments_are_privilege_3_and_match_their_selectors() {
        let gdt = Gdt::new();
        let dpl = |d: u64| (d >> 45) & 3;
        let data = gdt.entries()[(USER_DATA_SELECTOR >> 3) as usize];
        let code = gdt.entries()[(USER_CODE_SELECTOR >> 3) as usize];
        assert_eq!((dpl(data), dpl(code)), (3, 3));
        assert_eq!((USER_DATA_SELECTOR & 3, USER_CODE_SELECTOR & 3), (3, 3));
        assert_ne!(code & (1 << 53), 0, "long-mode code");
        assert_ne!(code & (1 << 43), 0, "executable");
        assert_eq!(data & (1 << 43), 0, "data");
        // Only the privilege differs from the kernel's.
        assert_eq!(code & !(3 << 45), KERNEL_CODE_DESCRIPTOR);
        assert_eq!(data & !(3 << 45), KERNEL_DATA_DESCRIPTOR);
    }

    #[test]
    fn tss_descriptor_splits_base_and_limit() {
        let [low, high] = tss_descriptor(0xFFFF_8000_1234_5678, 103);
        assert_eq!(low & 0xFFFF, 103);
        assert_eq!((low >> 16) & 0xFF_FFFF, 0x34_5678);
        assert_eq!((low >> 40) & 0xFF, 0x89, "present, available 64-bit TSS");
        assert_eq!((low >> 48) & 0xF, 0);
        assert_eq!(low >> 56, 0x12);
        assert_eq!(high, 0xFFFF_8000);
    }

    #[test]
    fn set_tss_fills_both_slots_and_layout_is_104_bytes() {
        let tss = Tss::new();
        let mut gdt = Gdt::new();
        gdt.set_tss(&tss);
        let [low, high] = tss_descriptor(&tss as *const Tss as u64, 103);
        assert_eq!(gdt.entries()[(TSS_SELECTOR >> 3) as usize], low);
        assert_eq!(gdt.entries()[(TSS_SELECTOR >> 3) as usize + 1], high);
        assert_eq!(core::mem::size_of::<Tss>(), 104);
        let iomap = tss.iomap_base;
        assert_eq!(iomap, 104);
        assert_eq!(core::mem::offset_of!(Tss, ist), 36);
        assert_eq!(core::mem::offset_of!(Tss, rsp), 4);
    }
}
