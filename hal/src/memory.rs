//! Memory management abstraction

use thiserror::Error;

/// Errors that can occur during memory operations
#[derive(Debug, Error)]
pub enum MemoryError {
    /// Invalid address
    #[error("Invalid address: {0:#x}")]
    InvalidAddress(usize),

    /// Out of memory
    #[error("Out of memory")]
    OutOfMemory,

    /// Permission denied
    #[error("Permission denied")]
    PermissionDenied,
}

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
