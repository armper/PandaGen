//! Memory management abstraction

use core::fmt;

/// Errors that can occur during memory operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryError {
    /// Invalid address
    InvalidAddress(usize),

    /// Out of memory
    OutOfMemory,

    /// Permission denied
    PermissionDenied,
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryError::InvalidAddress(address) => {
                write!(f, "Invalid address: {address:#x}")
            }
            MemoryError::OutOfMemory => write!(f, "Out of memory"),
            MemoryError::PermissionDenied => write!(f, "Permission denied"),
        }
    }
}

impl core::error::Error for MemoryError {}

/// Memory management operations
///
/// This trait abstracts memory management operations.
/// Unlike POSIX (mmap, malloc), this is explicit about permissions.
pub trait MemoryHal {
    /// Allocates a page of memory
    ///
    /// Returns the physical address of the allocated page.
    fn allocate_page(&mut self) -> Result<usize, MemoryError>;

    /// Frees a page of memory
    fn free_page(&mut self, address: usize) -> Result<(), MemoryError>;

    /// Maps a virtual address to a physical address
    fn map_page(
        &mut self,
        virtual_addr: usize,
        physical_addr: usize,
        writable: bool,
        executable: bool,
    ) -> Result<(), MemoryError>;

    /// Unmaps a virtual address
    fn unmap_page(&mut self, virtual_addr: usize) -> Result<(), MemoryError>;
}
