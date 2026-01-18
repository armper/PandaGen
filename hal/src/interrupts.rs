//! Interrupt handling abstraction

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
