//! x86_64 interrupt plumbing helpers.
//!
//! This module provides deterministic, testable IRQ routing scaffolding
//! without touching real hardware. It wires IDT registration, safe
//! handler registration, and PIC/APIC acknowledge paths.

use crate::idt::{Idt, IdtError};
use hal::interrupts::{InterruptError, InterruptRegistry};

/// Base vector for legacy PIC IRQs after remap.
pub const IRQ_BASE_VECTOR: u8 = 32;

/// Common IRQ lines we care about early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqLine {
    Timer,
    Keyboard,
    /// Explicit vector (already mapped).
    Vector(u8),
}

impl IrqLine {
    /// Returns the interrupt vector for this IRQ line.
    pub const fn vector(self) -> u8 {
        match self {
            IrqLine::Timer => IRQ_BASE_VECTOR,
            IrqLine::Keyboard => IRQ_BASE_VECTOR + 1,
            IrqLine::Vector(vector) => vector,
        }
    }
}

/// Interrupt acknowledgment strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckStrategy {
    Pic,
    Apic,
}

/// Tracks PIC acknowledgments (stubbed for tests).
#[derive(Debug, Default, Clone)]
pub struct PicController {
    ack_count: u64,
    last_irq: Option<IrqLine>,
}

impl PicController {
    pub fn new() -> Self {
        Self {
            ack_count: 0,
            last_irq: None,
        }
    }

    pub fn acknowledge(&mut self, irq: IrqLine) {
        self.ack_count = self.ack_count.saturating_add(1);
        self.last_irq = Some(irq);
    }

    pub fn ack_count(&self) -> u64 {
        self.ack_count
    }

    pub fn last_irq(&self) -> Option<IrqLine> {
        self.last_irq
    }
}

/// Tracks APIC acknowledgments (stubbed for tests).
#[derive(Debug, Default, Clone)]
pub struct ApicController {
    ack_count: u64,
    last_irq: Option<IrqLine>,
}

impl ApicController {
    pub fn new() -> Self {
        Self {
            ack_count: 0,
            last_irq: None,
        }
    }

    pub fn acknowledge(&mut self, irq: IrqLine) {
        self.ack_count = self.ack_count.saturating_add(1);
        self.last_irq = Some(irq);
    }

    pub fn ack_count(&self) -> u64 {
        self.ack_count
    }

    pub fn last_irq(&self) -> Option<IrqLine> {
        self.last_irq
    }
}

/// IRQ dispatch errors.
#[derive(Debug, PartialEq, Eq)]
pub enum IrqError {
    IdtNotInstalled,
    HandlerMissing(u8),
    RegistrationFailed(u8),
}

/// Interrupt dispatcher with safe registration and ack plumbing.
#[derive(Debug, Clone)]
pub struct InterruptDispatcher {
    idt: Idt,
    registry: InterruptRegistry,
    ack_strategy: AckStrategy,
    pic: PicController,
    apic: ApicController,
}

impl InterruptDispatcher {
    /// Creates a new dispatcher with a fresh IDT.
    pub fn new() -> Self {
        Self {
            idt: Idt::new(),
            registry: InterruptRegistry::new(),
            ack_strategy: AckStrategy::Pic,
            pic: PicController::new(),
            apic: ApicController::new(),
        }
    }

    /// Installs the IDT (skeleton).
    pub fn install_idt(&mut self) {
        self.idt.install();
    }

    /// Returns whether the IDT is installed.
    pub fn idt_installed(&self) -> bool {
        self.idt.is_installed()
    }

    /// Sets the acknowledge strategy.
    pub fn set_ack_strategy(&mut self, strategy: AckStrategy) {
        self.ack_strategy = strategy;
    }

    /// Registers a handler for a specific IRQ line.
    pub fn register_irq_handler(
        &mut self,
        irq: IrqLine,
        handler: fn(),
    ) -> Result<(), InterruptError> {
        let vector = irq.vector();
        self.registry.register(vector, handler)?;
        if let Err(err) = self.idt.register_handler(vector, handler) {
            // Roll back registry insertion to keep state consistent.
            self.registry.unregister(vector);
            return Err(match err {
                IdtError::AlreadyRegistered(_) => InterruptError::AlreadyRegistered(vector),
                IdtError::NotRegistered(_) => InterruptError::AlreadyRegistered(vector),
            });
        }
        Ok(())
    }

    /// Dispatches an IRQ and acknowledges it via the active strategy.
    pub fn dispatch_irq(&mut self, irq: IrqLine) -> Result<(), IrqError> {
        if !self.idt.is_installed() {
            return Err(IrqError::IdtNotInstalled);
        }

        let vector = irq.vector();
        self.idt
            .trigger(vector)
            .map_err(|_| IrqError::HandlerMissing(vector))?;

        match self.ack_strategy {
            AckStrategy::Pic => self.pic.acknowledge(irq),
            AckStrategy::Apic => self.apic.acknowledge(irq),
        }

        Ok(())
    }

    /// Returns the number of PIC acknowledgments.
    pub fn pic_ack_count(&self) -> u64 {
        self.pic.ack_count()
    }

    /// Returns the number of APIC acknowledgments.
    pub fn apic_ack_count(&self) -> u64 {
        self.apic.ack_count()
    }

    /// Returns the last PIC IRQ acknowledged.
    pub fn pic_last_irq(&self) -> Option<IrqLine> {
        self.pic.last_irq()
    }

    /// Returns the last APIC IRQ acknowledged.
    pub fn apic_last_irq(&self) -> Option<IrqLine> {
        self.apic.last_irq()
    }
}

impl Default for InterruptDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};

    static TIMER_CALLS: AtomicUsize = AtomicUsize::new(0);
    static KEYBOARD_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn timer_handler() {
        TIMER_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    fn keyboard_handler() {
        KEYBOARD_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn test_dispatch_requires_idt() {
        let mut dispatcher = InterruptDispatcher::new();
        let result = dispatcher.dispatch_irq(IrqLine::Timer);
        assert_eq!(result, Err(IrqError::IdtNotInstalled));
    }

    #[test]
    fn test_irq_registration_and_dispatch_pic() {
        TIMER_CALLS.store(0, Ordering::SeqCst);
        let mut dispatcher = InterruptDispatcher::new();
        dispatcher.install_idt();

        dispatcher
            .register_irq_handler(IrqLine::Timer, timer_handler)
            .unwrap();

        dispatcher.dispatch_irq(IrqLine::Timer).unwrap();
        assert_eq!(TIMER_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(dispatcher.pic_ack_count(), 1);
        assert_eq!(dispatcher.pic_last_irq(), Some(IrqLine::Timer));
        assert_eq!(dispatcher.apic_ack_count(), 0);
    }

    #[test]
    fn test_irq_registration_and_dispatch_apic() {
        KEYBOARD_CALLS.store(0, Ordering::SeqCst);
        let mut dispatcher = InterruptDispatcher::new();
        dispatcher.install_idt();
        dispatcher.set_ack_strategy(AckStrategy::Apic);

        dispatcher
            .register_irq_handler(IrqLine::Keyboard, keyboard_handler)
            .unwrap();

        dispatcher.dispatch_irq(IrqLine::Keyboard).unwrap();
        assert_eq!(KEYBOARD_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(dispatcher.apic_ack_count(), 1);
        assert_eq!(dispatcher.apic_last_irq(), Some(IrqLine::Keyboard));
        assert_eq!(dispatcher.pic_ack_count(), 0);
    }

    #[test]
    fn test_irq_duplicate_registration() {
        let mut dispatcher = InterruptDispatcher::new();
        dispatcher.install_idt();
        dispatcher
            .register_irq_handler(IrqLine::Timer, timer_handler)
            .unwrap();

        let result = dispatcher.register_irq_handler(IrqLine::Timer, timer_handler);
        assert_eq!(
            result,
            Err(InterruptError::AlreadyRegistered(IrqLine::Timer.vector()))
        );
    }

    #[test]
    fn test_dispatch_missing_handler() {
        let mut dispatcher = InterruptDispatcher::new();
        dispatcher.install_idt();

        let result = dispatcher.dispatch_irq(IrqLine::Keyboard);
        assert_eq!(
            result,
            Err(IrqError::HandlerMissing(IrqLine::Keyboard.vector()))
        );
    }
}
