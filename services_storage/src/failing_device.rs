//! # Failing Block Device
//!
//! A BlockDevice wrapper that can simulate failures for testing crash-safe storage.
//! Useful for testing recovery scenarios without requiring actual power loss.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use hal::{BlockDevice, BlockError};

/// Policy for when failures should occur
#[derive(Debug, Clone)]
pub enum FailurePolicy {
    /// Never fail (passthrough)
    Never,
    /// Fail after N writes
    AfterWrites(usize),
    /// Fail on specific block indices
    OnBlocks(Vec<u64>),
    /// Fail after N writes to specific blocks
    AfterWritesToBlocks { count: usize, blocks: Vec<u64> },
}

/// Wrapper around a BlockDevice that can simulate failures
pub struct FailingBlockDevice<D: BlockDevice> {
    inner: D,
    policy: FailurePolicy,
    write_count: usize,
    block_write_counts: BTreeMap<u64, usize>,
}

impl<D: BlockDevice> FailingBlockDevice<D> {
    /// Create a new failing block device with the given policy
    pub fn new(inner: D, policy: FailurePolicy) -> Self {
        Self {
            inner,
            policy,
            write_count: 0,
            block_write_counts: BTreeMap::new(),
        }
    }

    /// Check if write should fail based on policy
    fn should_fail(&mut self, block_idx: u64) -> bool {
        match &self.policy {
            FailurePolicy::Never => false,
            FailurePolicy::AfterWrites(n) => self.write_count >= *n,
            FailurePolicy::OnBlocks(blocks) => blocks.contains(&block_idx),
            FailurePolicy::AfterWritesToBlocks { count, blocks } => {
                if blocks.contains(&block_idx) {
                    let block_count = self.block_write_counts.entry(block_idx).or_insert(0);
                    *block_count >= *count
                } else {
                    false
                }
            }
        }
    }

    /// Get the underlying device (for inspection)
    pub fn inner(&self) -> &D {
        &self.inner
    }

    /// Get mutable access to the underlying device
    pub fn inner_mut(&mut self) -> &mut D {
        &mut self.inner
    }

    /// Get the number of writes that have occurred
    pub fn write_count(&self) -> usize {
        self.write_count
    }

    /// Reset the failure policy
    pub fn set_policy(&mut self, policy: FailurePolicy) {
        self.policy = policy;
        self.write_count = 0;
        self.block_write_counts.clear();
    }
}

impl<D: BlockDevice> BlockDevice for FailingBlockDevice<D> {
    fn block_count(&self) -> u64 {
        self.inner.block_count()
    }

    fn read_block(&mut self, block_idx: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        self.inner.read_block(block_idx, buffer)
    }

    fn write_block(&mut self, block_idx: u64, buffer: &[u8]) -> Result<(), BlockError> {
        if self.should_fail(block_idx) {
            return Err(BlockError::IoError);
        }

        self.write_count += 1;
        if let FailurePolicy::AfterWritesToBlocks { blocks, .. } = &self.policy {
            if blocks.contains(&block_idx) {
                *self.block_write_counts.entry(block_idx).or_insert(0) += 1;
            }
        }

        self.inner.write_block(block_idx, buffer)
    }

    fn flush(&mut self) -> Result<(), BlockError> {
        // Flush failures can also be simulated
        if matches!(self.policy, FailurePolicy::AfterWrites(n) if self.write_count >= n) {
            return Err(BlockError::IoError);
        }
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use hal::{RamDisk, BLOCK_SIZE};

    #[test]
    fn test_failing_device_never() {
        let disk = RamDisk::new(10);
        let mut failing = FailingBlockDevice::new(disk, FailurePolicy::Never);

        let data = [0x42u8; BLOCK_SIZE];
        assert!(failing.write_block(0, &data).is_ok());
        assert!(failing.write_block(1, &data).is_ok());
    }

    #[test]
    fn test_failing_device_after_writes() {
        let disk = RamDisk::new(10);
        let mut failing = FailingBlockDevice::new(disk, FailurePolicy::AfterWrites(2));

        let data = [0x42u8; BLOCK_SIZE];
        assert!(failing.write_block(0, &data).is_ok());
        assert!(failing.write_block(1, &data).is_ok());
        assert_eq!(failing.write_block(2, &data), Err(BlockError::IoError));
    }

    #[test]
    fn test_failing_device_on_blocks() {
        let disk = RamDisk::new(10);
        let mut failing = FailingBlockDevice::new(disk, FailurePolicy::OnBlocks(vec![2, 5]));

        let data = [0x42u8; BLOCK_SIZE];
        assert!(failing.write_block(0, &data).is_ok());
        assert!(failing.write_block(1, &data).is_ok());
        assert_eq!(failing.write_block(2, &data), Err(BlockError::IoError));
        assert!(failing.write_block(3, &data).is_ok());
        assert_eq!(failing.write_block(5, &data), Err(BlockError::IoError));
    }

    #[test]
    fn test_failing_device_after_writes_to_blocks() {
        let disk = RamDisk::new(10);
        let mut failing = FailingBlockDevice::new(
            disk,
            FailurePolicy::AfterWritesToBlocks {
                count: 2,
                blocks: vec![3],
            },
        );

        let data = [0x42u8; BLOCK_SIZE];
        assert!(failing.write_block(3, &data).is_ok()); // First write to block 3
        assert!(failing.write_block(3, &data).is_ok()); // Second write to block 3
        assert_eq!(failing.write_block(3, &data), Err(BlockError::IoError)); // Third write should fail
    }

    #[test]
    fn test_failing_device_read_never_fails() {
        let disk = RamDisk::new(10);
        let mut failing = FailingBlockDevice::new(disk, FailurePolicy::AfterWrites(0));

        let mut buffer = [0u8; BLOCK_SIZE];
        assert!(failing.read_block(0, &mut buffer).is_ok());
    }

    #[test]
    fn test_failing_device_set_policy() {
        let disk = RamDisk::new(10);
        let mut failing = FailingBlockDevice::new(disk, FailurePolicy::Never);

        let data = [0x42u8; BLOCK_SIZE];
        assert!(failing.write_block(0, &data).is_ok());

        failing.set_policy(FailurePolicy::AfterWrites(0));
        assert_eq!(failing.write_block(1, &data), Err(BlockError::IoError));
    }

    #[test]
    fn test_failing_device_write_count() {
        let disk = RamDisk::new(10);
        let mut failing = FailingBlockDevice::new(disk, FailurePolicy::Never);

        assert_eq!(failing.write_count(), 0);

        let data = [0x42u8; BLOCK_SIZE];
        failing.write_block(0, &data).unwrap();
        assert_eq!(failing.write_count(), 1);

        failing.write_block(1, &data).unwrap();
        assert_eq!(failing.write_count(), 2);
    }
}
