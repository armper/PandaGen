//! Interrupt handling abstraction

use alloc::collections::BTreeMap;

/// Interrupt handler trait
///
/// This trait abstracts interrupt handling. Different architectures
/// have different interrupt mechanisms, but all can implement this trait.
pub trait InterruptHal {
    /// Enables interrupts
    fn enable_interrupts(&mut self);

    /// Disables interrupts
    fn disable_interrupts(&mut self);

    /// Returns whether interrupts are enabled
    fn interrupts_enabled(&self) -> bool;

    /// Registers an interrupt handler
    ///
    /// # Arguments
    ///
    /// * `vector` - The interrupt vector number
    /// * `handler` - Function to call when interrupt occurs
    fn register_handler(&mut self, vector: u8, handler: fn());
}

/// Interrupt registration errors for safe APIs.
#[derive(Debug, PartialEq, Eq)]
pub enum InterruptError {
    /// A handler is already registered for this vector.
    AlreadyRegistered(u8),
}

/// Safe interrupt registration registry.
///
/// This provides a minimal guardrail against double-registration and
/// allows systems to inspect installed handlers without touching the IDT.
#[derive(Debug, Clone)]
pub struct InterruptRegistry {
    handlers: BTreeMap<u8, fn()>,
}

impl InterruptRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            handlers: BTreeMap::new(),
        }
    }

    /// Registers a handler for a vector.
    pub fn register(&mut self, vector: u8, handler: fn()) -> Result<(), InterruptError> {
        if self.handlers.contains_key(&vector) {
            return Err(InterruptError::AlreadyRegistered(vector));
        }
        self.handlers.insert(vector, handler);
        Ok(())
    }

    /// Registers and installs a handler via the provided HAL.
    pub fn register_with_hal<H: InterruptHal>(
        &mut self,
        hal: &mut H,
        vector: u8,
        handler: fn(),
    ) -> Result<(), InterruptError> {
        self.register(vector, handler)?;
        hal.register_handler(vector, handler);
        Ok(())
    }

    /// Returns the handler for a vector, if any.
    pub fn handler(&self, vector: u8) -> Option<fn()> {
        self.handlers.get(&vector).copied()
    }

    /// Unregisters a handler for a vector, returning it if present.
    pub fn unregister(&mut self, vector: u8) -> Option<fn()> {
        self.handlers.remove(&vector)
    }
}

impl Default for InterruptRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler_stub() {}

    #[test]
    fn test_interrupt_registry_register() {
        let mut registry = InterruptRegistry::new();
        registry.register(32, handler_stub).unwrap();
        assert!(registry.handler(32).is_some());
    }

    #[test]
    fn test_interrupt_registry_duplicate() {
        let mut registry = InterruptRegistry::new();
        registry.register(33, handler_stub).unwrap();
        let result = registry.register(33, handler_stub);
        assert_eq!(result, Err(InterruptError::AlreadyRegistered(33)));
    }

    #[test]
    fn test_interrupt_registry_unregister() {
        let mut registry = InterruptRegistry::new();
        registry.register(40, handler_stub).unwrap();
        assert!(registry.handler(40).is_some());
        let removed = registry.unregister(40);
        assert!(removed.is_some());
        assert!(registry.handler(40).is_none());
    }
}
