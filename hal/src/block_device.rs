/// Block device abstraction for storage
///
/// Provides a minimal block device API for reading and writing fixed-size blocks.
/// This is the foundation for persistent storage in PandaGen.
use core::fmt;

#[cfg(feature = "alloc")]
extern crate alloc;

/// Standard block size (4 KiB)
pub const BLOCK_SIZE: usize = 4096;

/// Block device errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// Block index out of bounds
    OutOfBounds,
    /// I/O error (hardware failure, timeout, etc.)
    IoError,
    /// Device not ready
    NotReady,
    /// Invalid block size
    InvalidSize,
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds => write!(f, "block index out of bounds"),
            Self::IoError => write!(f, "I/O error"),
            Self::NotReady => write!(f, "device not ready"),
            Self::InvalidSize => write!(f, "invalid block size"),
        }
    }
}

/// Block device trait
///
/// Implementers provide block-level read/write operations.
/// All operations work with fixed-size blocks (BLOCK_SIZE bytes).
pub trait BlockDevice {
    /// Get the total number of blocks on this device
    fn block_count(&self) -> u64;

    /// Get the block size (should always be BLOCK_SIZE)
    fn block_size(&self) -> usize {
        BLOCK_SIZE
    }

    /// Read a block into the provided buffer
    ///
    /// # Arguments
    /// * `block_idx` - Block index to read
    /// * `buffer` - Buffer to read into (must be at least BLOCK_SIZE bytes)
    ///
    /// # Errors
    /// Returns `BlockError::OutOfBounds` if block_idx >= block_count()
    /// Returns `BlockError::IoError` on hardware failure
    /// Returns `BlockError::InvalidSize` if buffer is too small
    fn read_block(&mut self, block_idx: u64, buffer: &mut [u8]) -> Result<(), BlockError>;

    /// Write a block from the provided buffer
    ///
    /// # Arguments
    /// * `block_idx` - Block index to write
    /// * `buffer` - Buffer to write from (must be at least BLOCK_SIZE bytes)
    ///
    /// # Errors
    /// Returns `BlockError::OutOfBounds` if block_idx >= block_count()
    /// Returns `BlockError::IoError` on hardware failure
    /// Returns `BlockError::InvalidSize` if buffer is too small
    fn write_block(&mut self, block_idx: u64, buffer: &[u8]) -> Result<(), BlockError>;

    /// Flush any pending writes to persistent storage
    ///
    /// For devices with write caching, this ensures all writes are durable.
    fn flush(&mut self) -> Result<(), BlockError> {
        // Default implementation: no-op (assume writes are synchronous)
        Ok(())
    }
}

/// RAM disk - an in-memory block device
///
/// Useful for testing and for volatile storage during development.
/// Data is lost on reboot.
#[cfg(feature = "alloc")]
pub struct RamDisk {
    blocks: alloc::vec::Vec<[u8; BLOCK_SIZE]>,
}

#[cfg(feature = "alloc")]
impl RamDisk {
    /// Create a new RAM disk with the specified number of blocks
    pub fn new(block_count: usize) -> Self {
        Self {
            blocks: alloc::vec![0; block_count]
                .into_iter()
                .map(|_| [0u8; BLOCK_SIZE])
                .collect(),
        }
    }

    /// Create a RAM disk with a specific capacity in megabytes
    pub fn with_capacity_mb(mb: usize) -> Self {
        let block_count = (mb * 1024 * 1024) / BLOCK_SIZE;
        Self::new(block_count)
    }
}

#[cfg(feature = "alloc")]
impl BlockDevice for RamDisk {
    fn block_count(&self) -> u64 {
        self.blocks.len() as u64
    }

    fn read_block(&mut self, block_idx: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        if block_idx >= self.block_count() {
            return Err(BlockError::OutOfBounds);
        }
        if buffer.len() < BLOCK_SIZE {
            return Err(BlockError::InvalidSize);
        }

        let block = &self.blocks[block_idx as usize];
        buffer[..BLOCK_SIZE].copy_from_slice(block);
        Ok(())
    }

    fn write_block(&mut self, block_idx: u64, buffer: &[u8]) -> Result<(), BlockError> {
        if block_idx >= self.block_count() {
            return Err(BlockError::OutOfBounds);
        }
        if buffer.len() < BLOCK_SIZE {
            return Err(BlockError::InvalidSize);
        }

        let block = &mut self.blocks[block_idx as usize];
        block.copy_from_slice(&buffer[..BLOCK_SIZE]);
        Ok(())
    }
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;
    extern crate std;

    #[test]
    fn test_ramdisk_creation() {
        let disk = RamDisk::new(10);
        assert_eq!(disk.block_count(), 10);
        assert_eq!(disk.block_size(), BLOCK_SIZE);
    }

    #[test]
    fn test_ramdisk_read_write() {
        let mut disk = RamDisk::new(10);

        // Write some data
        let write_data = [0x42u8; BLOCK_SIZE];
        disk.write_block(0, &write_data).unwrap();

        // Read it back
        let mut read_data = [0u8; BLOCK_SIZE];
        disk.read_block(0, &mut read_data).unwrap();

        assert_eq!(write_data, read_data);
    }

    #[test]
    fn test_ramdisk_out_of_bounds() {
        let mut disk = RamDisk::new(10);
        let mut buffer = [0u8; BLOCK_SIZE];

        // Reading beyond bounds should fail
        assert_eq!(
            disk.read_block(10, &mut buffer),
            Err(BlockError::OutOfBounds)
        );
        assert_eq!(
            disk.read_block(100, &mut buffer),
            Err(BlockError::OutOfBounds)
        );

        // Writing beyond bounds should fail
        assert_eq!(disk.write_block(10, &buffer), Err(BlockError::OutOfBounds));
        assert_eq!(disk.write_block(100, &buffer), Err(BlockError::OutOfBounds));
    }

    #[test]
    fn test_ramdisk_invalid_size() {
        let mut disk = RamDisk::new(10);
        let mut small_buffer = [0u8; 100];

        // Reading with too-small buffer should fail
        assert_eq!(
            disk.read_block(0, &mut small_buffer),
            Err(BlockError::InvalidSize)
        );

        // Writing with too-small buffer should fail
        assert_eq!(
            disk.write_block(0, &small_buffer),
            Err(BlockError::InvalidSize)
        );
    }

    #[test]
    fn test_ramdisk_persistence_within_session() {
        let mut disk = RamDisk::new(10);

        // Write different patterns to different blocks
        for block_idx in 0..10u64 {
            let pattern = (block_idx as u8).wrapping_mul(17);
            let write_data = [pattern; BLOCK_SIZE];
            disk.write_block(block_idx, &write_data).unwrap();
        }

        // Verify all blocks retained their data
        for block_idx in 0..10u64 {
            let pattern = (block_idx as u8).wrapping_mul(17);
            let mut read_data = [0u8; BLOCK_SIZE];
            disk.read_block(block_idx, &mut read_data).unwrap();
            assert_eq!(read_data, [pattern; BLOCK_SIZE]);
        }
    }

    #[test]
    fn test_ramdisk_with_capacity_mb() {
        let disk = RamDisk::with_capacity_mb(1);
        // 1 MB = 1024 KB = 1048576 bytes = 256 blocks of 4096 bytes
        assert_eq!(disk.block_count(), 256);
    }
}
