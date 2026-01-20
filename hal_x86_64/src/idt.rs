//! Minimal IDT skeleton for x86_64.
//!
//! This is a safe, testable placeholder for interrupt registration.
//! It does not install a real IDT in hardware.

/// IDT registration errors.
#[derive(Debug, PartialEq, Eq)]
pub enum IdtError {
    AlreadyRegistered(u8),
    NotRegistered(u8),
}

/// Minimal IDT structure with handler table.
#[derive(Debug, Clone)]
pub struct Idt {
    handlers: [Option<fn()>; 256],
    installed: bool,
}

impl Idt {
    /// Creates a new empty IDT.
    pub const fn new() -> Self {
        Self {
            handlers: [None; 256],
            installed: false,
        }
    }

    /// Installs the IDT (skeleton).
    pub fn install(&mut self) {
        self.installed = true;
    }

    /// Returns whether the IDT is installed.
    pub fn is_installed(&self) -> bool {
        self.installed
    }

    /// Registers a handler for an interrupt vector.
    pub fn register_handler(&mut self, vector: u8, handler: fn()) -> Result<(), IdtError> {
        let slot = &mut self.handlers[vector as usize];
        if slot.is_some() {
            return Err(IdtError::AlreadyRegistered(vector));
        }
        *slot = Some(handler);
        Ok(())
    }

    /// Returns a handler, if present.
    pub fn handler(&self, vector: u8) -> Option<fn()> {
        self.handlers[vector as usize]
    }

    /// Triggers a handler (test-only helper).
    pub fn trigger(&self, vector: u8) -> Result<(), IdtError> {
        let Some(handler) = self.handler(vector) else {
            return Err(IdtError::NotRegistered(vector));
        };
        handler();
        Ok(())
    }
}

impl Default for Idt {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler_stub() {}

    #[test]
    fn test_idt_install() {
        let mut idt = Idt::new();
        assert!(!idt.is_installed());
        idt.install();
        assert!(idt.is_installed());
    }

    #[test]
    fn test_idt_register_and_trigger() {
        let mut idt = Idt::new();
        idt.register_handler(32, handler_stub).unwrap();
        assert!(idt.handler(32).is_some());
        idt.trigger(32).unwrap();
    }

    #[test]
    fn test_idt_duplicate_register() {
        let mut idt = Idt::new();
        idt.register_handler(33, handler_stub).unwrap();
        let result = idt.register_handler(33, handler_stub);
        assert_eq!(result, Err(IdtError::AlreadyRegistered(33)));
    }
}
