#![no_std]

//! # x86_64 Hardware Abstraction Layer
//!
//! This crate implements the HAL traits for x86_64 architecture.
//!
//! ## Scope
//!
//! This is a skeleton implementation. Real hardware operations are stubbed.
//! In a complete system, this would use actual x86_64 instructions.

#[macro_use]
extern crate alloc;

use core::prelude::v1::*;

use hal::memory::MemoryError;
use hal::{CpuHal, InterruptHal, MemoryHal};

pub mod idt;
pub mod interrupts;
pub mod keyboard;
pub mod port_io;
pub mod tick;
pub mod timer;
pub mod virtio;
pub mod virtio_blk;

pub use idt::{Idt, IdtError};
pub use interrupts::{AckStrategy, InterruptDispatcher, IrqLine};
pub use keyboard::X86Ps2Keyboard;
pub use port_io::{FakePortIo, PortIo, RealPortIo};
pub use tick::{KernelTickCounter, TickSource};
pub use timer::{FakeTimerDevice, HpetTimer, PitTimer};
pub use virtio::{VirtioMmioDevice, VirtqAvail, VirtqDesc, VirtqUsed, Virtqueue};
pub use virtio_blk::VirtioBlkDevice;

/// x86_64 CPU implementation (skeleton)
pub struct X86_64Cpu;

impl CpuHal for X86_64Cpu {
    fn halt(&self) {
        // In real implementation: unsafe { asm!("hlt") }
        // For now, this is a no-op
    }

    fn stack_pointer(&self) -> usize {
        // In real implementation: read RSP register
        0
    }

    fn instruction_pointer(&self) -> usize {
        // In real implementation: read RIP register
        0
    }

    fn cpu_id(&self) -> u32 {
        // In real implementation: CPUID instruction
        0
    }
}

/// x86_64 memory management implementation (skeleton)
pub struct X86_64Memory;

impl MemoryHal for X86_64Memory {
    fn allocate_page(&mut self) -> Result<usize, MemoryError> {
        // In real implementation: allocate from physical memory manager
        Err(MemoryError::OutOfMemory)
    }

    fn free_page(&mut self, _address: usize) -> Result<(), MemoryError> {
        // In real implementation: return page to physical memory manager
        Ok(())
    }

    fn map_page(
        &mut self,
        _virtual_addr: usize,
        _physical_addr: usize,
        _writable: bool,
        _executable: bool,
    ) -> Result<(), MemoryError> {
        // In real implementation: update page tables
        Ok(())
    }

    fn unmap_page(&mut self, _virtual_addr: usize) -> Result<(), MemoryError> {
        // In real implementation: clear page table entry
        Ok(())
    }
}

/// x86_64 interrupt handling implementation (skeleton)
pub struct X86_64Interrupts {
    enabled: bool,
    dispatcher: interrupts::InterruptDispatcher,
}

impl X86_64Interrupts {
    /// Creates a new interrupt handler
    pub fn new() -> Self {
        Self {
            enabled: false,
            dispatcher: interrupts::InterruptDispatcher::new(),
        }
    }

    /// Installs the IDT (skeleton).
    pub fn install_idt(&mut self) {
        self.dispatcher.install_idt();
    }

    /// Registers a handler with error reporting.
    pub fn register_irq_handler(
        &mut self,
        irq: interrupts::IrqLine,
        handler: fn(),
    ) -> Result<(), hal::interrupts::InterruptError> {
        self.dispatcher.register_irq_handler(irq, handler)
    }

    /// Returns whether the IDT is installed.
    pub fn idt_installed(&self) -> bool {
        self.dispatcher.idt_installed()
    }

    /// Dispatches a specific IRQ line.
    pub fn dispatch_irq(&mut self, irq: interrupts::IrqLine) -> Result<(), interrupts::IrqError> {
        self.dispatcher.dispatch_irq(irq)
    }

    /// Updates which controller is used for acknowledgments.
    pub fn set_ack_strategy(&mut self, strategy: interrupts::AckStrategy) {
        self.dispatcher.set_ack_strategy(strategy);
    }
}

impl Default for X86_64Interrupts {
    fn default() -> Self {
        Self::new()
    }
}

impl InterruptHal for X86_64Interrupts {
    fn enable_interrupts(&mut self) {
        // In real implementation: STI instruction
        self.enabled = true;
    }

    fn disable_interrupts(&mut self) {
        // In real implementation: CLI instruction
        self.enabled = false;
    }

    fn interrupts_enabled(&self) -> bool {
        self.enabled
    }

    fn register_handler(&mut self, _vector: u8, _handler: fn()) {
        let _ = self
            .dispatcher
            .register_irq_handler(interrupts::IrqLine::Vector(_vector), _handler);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_operations() {
        let cpu = X86_64Cpu;
        cpu.halt();
        assert_eq!(cpu.cpu_id(), 0);
    }

    #[test]
    fn test_interrupt_control() {
        let mut interrupts = X86_64Interrupts::new();
        assert!(!interrupts.interrupts_enabled());

        interrupts.enable_interrupts();
        assert!(interrupts.interrupts_enabled());

        interrupts.disable_interrupts();
        assert!(!interrupts.interrupts_enabled());
    }

    #[test]
    fn test_idt_install_and_register() {
        let mut interrupts = X86_64Interrupts::new();
        assert!(!interrupts.idt_installed());
        interrupts.install_idt();
        assert!(interrupts.idt_installed());

        fn handler_stub() {}

        interrupts
            .register_irq_handler(interrupts::IrqLine::Vector(32), handler_stub)
            .unwrap();
        let result = interrupts.register_irq_handler(interrupts::IrqLine::Vector(32), handler_stub);
        assert_eq!(
            result,
            Err(hal::interrupts::InterruptError::AlreadyRegistered(32))
        );
    }
}
