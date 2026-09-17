//! Local APIC (xAPIC, memory-mapped) driver.
//!
//! Only what SMP bring-up needs: enabling the APIC through the spurious
//! interrupt vector register, reading the APIC id, end-of-interrupt, and
//! fixed inter-processor interrupts. Register traffic goes through
//! `ApicMmio`, so the driver is host-tested against a fake.

use core::sync::atomic::{AtomicU32, Ordering};

/// Physical base of the xAPIC register window.
pub const LAPIC_DEFAULT_PHYS: u64 = 0xFEE0_0000;

pub const REG_ID: usize = 0x020;
pub const REG_EOI: usize = 0x0B0;
pub const REG_SPURIOUS: usize = 0x0F0;
pub const REG_ICR_LOW: usize = 0x300;
pub const REG_ICR_HIGH: usize = 0x310;
pub const REG_LVT_TIMER: usize = 0x320;
pub const REG_TIMER_INITIAL: usize = 0x380;
pub const REG_TIMER_CURRENT: usize = 0x390;
pub const REG_TIMER_DIVIDE: usize = 0x3E0;

const LVT_MASKED: u32 = 1 << 16;
const LVT_TIMER_PERIODIC: u32 = 1 << 17;

const SVR_ENABLE: u32 = 1 << 8;
const ICR_DELIVERY_PENDING: u32 = 1 << 12;
const ICR_LEVEL_ASSERT: u32 = 1 << 14;
const ICR_DEST_ALL_EXCLUDING_SELF: u32 = 0b11 << 18;

/// 32-bit register access to the APIC window.
pub trait ApicMmio {
    fn read32(&self, offset: usize) -> u32;
    fn write32(&mut self, offset: usize, value: u32);
}

/// Real MMIO access through a virtual mapping of the register window.
pub struct RealApicMmio {
    base: usize,
}

impl RealApicMmio {
    /// # Safety
    /// `base` must be the virtual address of the LAPIC register window.
    pub const unsafe fn new(base: usize) -> Self {
        Self { base }
    }
}

impl ApicMmio for RealApicMmio {
    fn read32(&self, offset: usize) -> u32 {
        // SAFETY: base + offset lies inside the 4 KiB register window.
        unsafe { core::ptr::read_volatile((self.base + offset) as *const u32) }
    }

    fn write32(&mut self, offset: usize, value: u32) {
        // SAFETY: as above.
        unsafe { core::ptr::write_volatile((self.base + offset) as *mut u32, value) }
    }
}

pub struct LocalApic<M: ApicMmio> {
    mmio: M,
}

impl<M: ApicMmio> LocalApic<M> {
    pub const fn new(mmio: M) -> Self {
        Self { mmio }
    }

    pub fn id(&self) -> u32 {
        self.mmio.read32(REG_ID) >> 24
    }

    /// Software-enable the APIC with `spurious_vector` (must be >= 0x20).
    pub fn enable(&mut self, spurious_vector: u8) {
        let svr = self.mmio.read32(REG_SPURIOUS);
        self.mmio.write32(
            REG_SPURIOUS,
            (svr & !0xFF) | SVR_ENABLE | spurious_vector as u32,
        );
    }

    pub fn is_enabled(&self) -> bool {
        self.mmio.read32(REG_SPURIOUS) & SVR_ENABLE != 0
    }

    pub fn end_of_interrupt(&mut self) {
        self.mmio.write32(REG_EOI, 0);
    }

    fn wait_idle(&self, max_spins: u32) -> bool {
        for _ in 0..max_spins {
            if self.mmio.read32(REG_ICR_LOW) & ICR_DELIVERY_PENDING == 0 {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    /// Set the timer divide configuration (1, 2, 4, ..., 128).
    pub fn set_timer_divide(&mut self, divide: u32) {
        let bits = match divide {
            1 => 0b1011,
            2 => 0b0000,
            4 => 0b0001,
            8 => 0b0010,
            16 => 0b0011,
            32 => 0b1000,
            64 => 0b1001,
            _ => 0b1010,
        };
        self.mmio.write32(REG_TIMER_DIVIDE, bits);
    }

    /// Run the timer once from `initial` with interrupts masked, so the
    /// current-count register can be sampled for calibration.
    pub fn start_timer_masked(&mut self, initial: u32) {
        self.mmio.write32(REG_LVT_TIMER, LVT_MASKED);
        self.mmio.write32(REG_TIMER_INITIAL, initial);
    }

    /// Periodic timer delivering `vector` every `initial` divided ticks.
    pub fn start_timer_periodic(&mut self, vector: u8, initial: u32) {
        self.mmio
            .write32(REG_LVT_TIMER, LVT_TIMER_PERIODIC | vector as u32);
        self.mmio.write32(REG_TIMER_INITIAL, initial);
    }

    pub fn stop_timer(&mut self) {
        self.mmio.write32(REG_TIMER_INITIAL, 0);
        self.mmio.write32(REG_LVT_TIMER, LVT_MASKED);
    }

    pub fn timer_current(&self) -> u32 {
        self.mmio.read32(REG_TIMER_CURRENT)
    }

    /// Fixed IPI to one physical APIC id. Returns false if the previous
    /// IPI never finished sending.
    pub fn send_ipi(&mut self, dest_apic_id: u32, vector: u8) -> bool {
        if !self.wait_idle(1_000_000) {
            return false;
        }
        self.mmio.write32(REG_ICR_HIGH, dest_apic_id << 24);
        self.mmio
            .write32(REG_ICR_LOW, ICR_LEVEL_ASSERT | vector as u32);
        true
    }

    /// Fixed IPI to every other CPU.
    pub fn send_ipi_all_excluding_self(&mut self, vector: u8) -> bool {
        if !self.wait_idle(1_000_000) {
            return false;
        }
        self.mmio.write32(REG_ICR_HIGH, 0);
        self.mmio.write32(
            REG_ICR_LOW,
            ICR_DEST_ALL_EXCLUDING_SELF | ICR_LEVEL_ASSERT | vector as u32,
        );
        true
    }
}

/// Shared handle to the LAPIC window so interrupt handlers on any CPU can
/// acknowledge interrupts without owning a driver value.
pub struct SharedLapic {
    base: AtomicUsizeCell,
}

struct AtomicUsizeCell(core::sync::atomic::AtomicUsize);

impl SharedLapic {
    pub const fn new() -> Self {
        Self {
            base: AtomicUsizeCell(core::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// # Safety
    /// `base` must be the virtual address of the LAPIC register window.
    pub unsafe fn set_base(&self, base: usize) {
        self.base.0.store(base, Ordering::Release);
    }

    pub fn is_set(&self) -> bool {
        self.base.0.load(Ordering::Acquire) != 0
    }

    /// A driver over the shared window, if one was set.
    pub fn get(&self) -> Option<LocalApic<RealApicMmio>> {
        match self.base.0.load(Ordering::Acquire) {
            0 => None,
            // SAFETY: base was set by `set_base` under its contract.
            base => Some(LocalApic::new(unsafe { RealApicMmio::new(base) })),
        }
    }
}

impl Default for SharedLapic {
    fn default() -> Self {
        Self::new()
    }
}

/// Counts IPIs received, for diagnostics.
pub static IPI_COUNT: AtomicU32 = AtomicU32::new(0);

#[cfg(test)]
mod tests {
    use super::*;
    extern crate alloc;
    use alloc::vec::Vec;

    struct FakeMmio {
        regs: [u32; 0x400 / 4],
        writes: Vec<(usize, u32)>,
    }

    impl Default for FakeMmio {
        fn default() -> Self {
            Self {
                regs: [0; 0x400 / 4],
                writes: Vec::new(),
            }
        }
    }

    impl ApicMmio for FakeMmio {
        fn read32(&self, offset: usize) -> u32 {
            self.regs[offset / 4]
        }
        fn write32(&mut self, offset: usize, value: u32) {
            self.regs[offset / 4] = value;
            self.writes.push((offset, value));
        }
    }

    #[test]
    fn id_is_upper_byte() {
        let mut mmio = FakeMmio::default();
        mmio.regs[REG_ID / 4] = 3 << 24;
        assert_eq!(LocalApic::new(mmio).id(), 3);
    }

    #[test]
    fn enable_sets_bit8_and_vector_preserving_other_bits() {
        let mut mmio = FakeMmio::default();
        mmio.regs[REG_SPURIOUS / 4] = 0x0000_0200 | 0x0F;
        let mut apic = LocalApic::new(mmio);
        assert!(!apic.is_enabled());
        apic.enable(0xFF);
        assert!(apic.is_enabled());
        assert_eq!(apic.mmio.regs[REG_SPURIOUS / 4], 0x0000_0200 | 0x100 | 0xFF);
    }

    #[test]
    fn ipi_writes_high_then_low() {
        let mut apic = LocalApic::new(FakeMmio::default());
        assert!(apic.send_ipi(2, 0xF0));
        assert_eq!(
            apic.mmio.writes,
            alloc::vec![
                (REG_ICR_HIGH, 2 << 24),
                (REG_ICR_LOW, ICR_LEVEL_ASSERT | 0xF0)
            ]
        );
        apic.mmio.writes.clear();
        assert!(apic.send_ipi_all_excluding_self(0xF0));
        assert_eq!(apic.mmio.writes[0], (REG_ICR_HIGH, 0));
        assert_eq!(
            apic.mmio.writes[1],
            (
                REG_ICR_LOW,
                ICR_DEST_ALL_EXCLUDING_SELF | ICR_LEVEL_ASSERT | 0xF0
            )
        );
    }

    #[test]
    fn ipi_fails_when_previous_never_completes() {
        let mut mmio = FakeMmio::default();
        mmio.regs[REG_ICR_LOW / 4] = ICR_DELIVERY_PENDING;
        let mut apic = LocalApic::new(mmio);
        assert!(!apic.send_ipi(1, 0xF0));
        assert!(apic.mmio.writes.is_empty());
    }

    #[test]
    fn eoi_writes_zero() {
        let mut apic = LocalApic::new(FakeMmio::default());
        apic.end_of_interrupt();
        assert_eq!(apic.mmio.writes, alloc::vec![(REG_EOI, 0)]);
    }

    #[test]
    fn timer_programming() {
        let mut apic = LocalApic::new(FakeMmio::default());
        apic.set_timer_divide(16);
        assert_eq!(apic.mmio.regs[REG_TIMER_DIVIDE / 4], 0b0011);
        apic.set_timer_divide(1);
        assert_eq!(apic.mmio.regs[REG_TIMER_DIVIDE / 4], 0b1011);
        apic.start_timer_masked(u32::MAX);
        assert_eq!(apic.mmio.regs[REG_LVT_TIMER / 4], LVT_MASKED);
        assert_eq!(apic.mmio.regs[REG_TIMER_INITIAL / 4], u32::MAX);
        apic.mmio.regs[REG_TIMER_CURRENT / 4] = 1234;
        assert_eq!(apic.timer_current(), 1234);
        apic.start_timer_periodic(0xF1, 50_000);
        assert_eq!(apic.mmio.regs[REG_LVT_TIMER / 4], LVT_TIMER_PERIODIC | 0xF1);
        assert_eq!(apic.mmio.regs[REG_TIMER_INITIAL / 4], 50_000);
        apic.stop_timer();
        assert_eq!(apic.mmio.regs[REG_TIMER_INITIAL / 4], 0);
        assert_eq!(apic.mmio.regs[REG_LVT_TIMER / 4], LVT_MASKED);
    }

    #[test]
    fn shared_handle_starts_unset() {
        let shared = SharedLapic::new();
        assert!(!shared.is_set());
        assert!(shared.get().is_none());
    }
}
