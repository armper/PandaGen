//! CPU abstraction

/// CPU-specific operations
///
/// This trait abstracts CPU-specific operations like halting,
/// reading/writing control registers, etc.
pub trait CpuHal {
    /// Halts the CPU until the next interrupt
    fn halt(&self);

    /// Reads the current stack pointer
    fn stack_pointer(&self) -> usize;

    /// Reads the current instruction pointer
    fn instruction_pointer(&self) -> usize;

    /// Returns the CPU ID (for multi-core systems)
    fn cpu_id(&self) -> u32;
}
