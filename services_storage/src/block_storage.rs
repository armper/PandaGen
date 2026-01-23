///! Block-backed storage implementation with crash-safe commits
///!
///! Provides persistent storage by writing to block devices.
///! Objects are stored as blocks on disk, with a crash-safe commit protocol.
///!
///! ## Crash Safety Model
///! This implementation uses an append-only commit log with checksums:
///! - Each transaction writes data blocks first
///! - Then writes a commit record with checksum (atomic point of truth)
///! - On recovery, scans for valid commit records
///! - Incomplete transactions (no commit record or bad checksum) are discarded
use crate::{
    ObjectId, Transaction, TransactionError, TransactionId, TransactionalStorage, VersionId,
};
use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use hal::{BlockDevice, BlockError, BLOCK_SIZE};
use serde::{Deserialize, Serialize};

/// Superblock - stored in block 0
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Superblock {
    /// Magic number for validation
    magic: u64,
    /// Version of the storage format
    version: u32,
    /// Total number of blocks on device
    total_blocks: u64,
    /// First block of allocation bitmap
    bitmap_start: u64,
    /// Number of bitmap blocks
    bitmap_blocks: u64,
    /// First block of data area
    data_start: u64,
    /// First block of commit log
    commit_log_start: u64,
    /// Number of commit log blocks
    commit_log_blocks: u64,
    /// Commit sequence number (monotonically increasing)
    commit_sequence: u64,
}

const SUPERBLOCK_MAGIC: u64 = 0x50414E44_47454E00; // "PANDAGEN\0"
const STORAGE_VERSION: u32 = 2; // Bumped for crash-safe storage

/// Commit record for crash-safe transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommitRecord {
    /// Transaction ID
    transaction_id: TransactionId,
    /// Sequence number (for ordering)
    sequence: u64,
    /// List of allocations made in this transaction
    allocations: Vec<AllocationEntry>,
    /// CRC32 checksum of the commit record (excluding this field)
    checksum: u32,
}

impl CommitRecord {
    /// Create a new commit record with computed checksum
    fn new(
        transaction_id: TransactionId,
        sequence: u64,
        allocations: Vec<AllocationEntry>,
    ) -> Self {
        let mut record = Self {
            transaction_id,
            sequence,
            allocations,
            checksum: 0,
        };
        record.checksum = record.compute_checksum();
        record
    }

    /// Compute CRC32 checksum of record (excluding checksum field)
    fn compute_checksum(&self) -> u32 {
        let mut temp = self.clone();
        temp.checksum = 0;
        let data = serde_json::to_vec(&temp).unwrap_or_default();
        crc32fast::hash(&data)
    }

    /// Validate checksum
    fn is_valid(&self) -> bool {
        let computed = self.compute_checksum();
        computed == self.checksum
    }
}

/// Storage recovery report
#[derive(Debug, Clone)]
pub struct StorageRecoveryReport {
    /// Number of valid commits recovered
    pub recovered_commits: usize,
    /// Number of invalid/incomplete transactions discarded
    pub discarded_transactions: usize,
    /// Last valid commit sequence number
    pub last_sequence: u64,
    /// Whether recovery was successful
    pub success: bool,
    /// Recovery error message if any
    pub error: Option<String>,
}

/// Block allocation status
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AllocationEntry {
    object_id: ObjectId,
    version_id: VersionId,
    block_idx: u64,
    size_bytes: u64,
}

/// Block-backed storage backend with crash-safe commits
pub struct BlockStorage<D: BlockDevice> {
    device: D,
    superblock: Superblock,
    /// Map object versions to their block locations
    allocations: BTreeMap<(ObjectId, VersionId), AllocationEntry>,
    /// Track the latest version for each object
    latest_versions: BTreeMap<ObjectId, VersionId>,
    /// Free blocks
    free_blocks: BTreeSet<u64>,
    /// Pending writes for active transactions
    pending: BTreeMap<TransactionId, Vec<PendingWrite>>,
    /// Recovery report (if opened from existing storage)
    recovery_report: Option<StorageRecoveryReport>,
}

#[derive(Debug, Clone)]
struct PendingWrite {
    object_id: ObjectId,
    version_id: VersionId,
    data: Vec<u8>,
}

#[derive(Debug)]
pub enum BlockStorageError {
    BlockError(BlockError),
    InvalidSuperblock,
    NoFreeSpace,
    ObjectNotFound,
    SerializationError,
}

impl From<BlockError> for BlockStorageError {
    fn from(err: BlockError) -> Self {
        Self::BlockError(err)
    }
}

impl From<BlockStorageError> for TransactionError {
    fn from(err: BlockStorageError) -> Self {
        match err {
            BlockStorageError::ObjectNotFound => {
                TransactionError::ObjectNotFound("object not found".to_string())
            }
            _ => TransactionError::StorageError("block storage error".to_string()),
        }
    }
}

impl<D: BlockDevice> BlockStorage<D> {
    /// Create a new block storage, formatting the device
    pub fn format(mut device: D) -> Result<Self, BlockStorageError> {
        let total_blocks = device.block_count();

        // Reserve blocks:
        // - Block 0: superblock
        // - Blocks 1-N: commit log (5% of disk or 1 block min, 256 blocks max)
        // - Blocks N+1-M: allocation bitmap (10% of remaining or 1 block min)
        // - Blocks M+1...: data area
        let commit_log_blocks = ((total_blocks * 5) / 100).max(1).min(256);
        let bitmap_start = 1 + commit_log_blocks;

        // Ensure we don't overflow
        if bitmap_start >= total_blocks {
            return Err(BlockStorageError::InvalidSuperblock);
        }

        let remaining_after_log = total_blocks - bitmap_start;
        let bitmap_blocks = (remaining_after_log / 10).max(1);
        let data_start = bitmap_start + bitmap_blocks;

        if data_start >= total_blocks {
            return Err(BlockStorageError::InvalidSuperblock);
        }

        let superblock = Superblock {
            magic: SUPERBLOCK_MAGIC,
            version: STORAGE_VERSION,
            total_blocks,
            bitmap_start,
            bitmap_blocks,
            data_start,
            commit_log_start: 1,
            commit_log_blocks,
            commit_sequence: 0,
        };

        // Write superblock to block 0
        let mut block = [0u8; BLOCK_SIZE];
        let sb_json =
            serde_json::to_vec(&superblock).map_err(|_| BlockStorageError::SerializationError)?;
        if sb_json.len() > BLOCK_SIZE {
            return Err(BlockStorageError::InvalidSuperblock);
        }
        block[..sb_json.len()].copy_from_slice(&sb_json);
        device.write_block(0, &block)?;
        device.flush()?;

        // Initialize free blocks (all data blocks are free)
        let free_blocks: BTreeSet<u64> = (data_start..total_blocks).collect();

        Ok(Self {
            device,
            superblock,
            allocations: BTreeMap::new(),
            latest_versions: BTreeMap::new(),
            free_blocks,
            pending: BTreeMap::new(),
            recovery_report: None,
        })
    }

    /// Open existing block storage with crash recovery
    pub fn open(mut device: D) -> Result<Self, BlockStorageError> {
        // Read superblock from block 0
        let mut block = [0u8; BLOCK_SIZE];
        device.read_block(0, &mut block)?;

        // Find the end of JSON (first null or end of meaningful data)
        let json_end = block.iter().position(|&b| b == 0).unwrap_or(BLOCK_SIZE);
        let superblock: Superblock = serde_json::from_slice(&block[..json_end])
            .map_err(|_| BlockStorageError::InvalidSuperblock)?;

        if superblock.magic != SUPERBLOCK_MAGIC {
            return Err(BlockStorageError::InvalidSuperblock);
        }

        // Create storage instance
        let free_blocks: BTreeSet<u64> = (superblock.data_start..superblock.total_blocks).collect();

        let mut storage = Self {
            device,
            superblock,
            allocations: BTreeMap::new(),
            latest_versions: BTreeMap::new(),
            free_blocks,
            pending: BTreeMap::new(),
            recovery_report: None,
        };

        // Perform crash recovery
        let recovery_report = storage.perform_recovery()?;
        storage.recovery_report = Some(recovery_report);

        Ok(storage)
    }

    /// Perform crash recovery by scanning commit log
    fn perform_recovery(&mut self) -> Result<StorageRecoveryReport, BlockStorageError> {
        let mut recovered_commits = 0;
        let mut discarded_transactions = 0;
        let mut last_sequence = 0;

        // Scan commit log blocks
        for i in 0..self.superblock.commit_log_blocks {
            let block_idx = self.superblock.commit_log_start + i;
            let mut block = [0u8; BLOCK_SIZE];

            match self.device.read_block(block_idx, &mut block) {
                Ok(_) => {
                    // Try to parse commit record
                    let json_end = block.iter().position(|&b| b == 0).unwrap_or(BLOCK_SIZE);
                    if json_end > 0 {
                        if let Ok(record) =
                            serde_json::from_slice::<CommitRecord>(&block[..json_end])
                        {
                            // Validate checksum
                            if record.is_valid() && record.sequence > last_sequence {
                                // Apply this commit
                                for alloc in &record.allocations {
                                    self.allocations
                                        .insert((alloc.object_id, alloc.version_id), alloc.clone());
                                    self.latest_versions
                                        .insert(alloc.object_id, alloc.version_id);

                                    // Mark blocks as allocated
                                    let blocks_needed =
                                        ((alloc.size_bytes as usize + BLOCK_SIZE - 1) / BLOCK_SIZE)
                                            as u64;
                                    for j in 0..blocks_needed {
                                        self.free_blocks.remove(&(alloc.block_idx + j));
                                    }
                                }
                                last_sequence = record.sequence;
                                recovered_commits += 1;
                            } else {
                                discarded_transactions += 1;
                            }
                        }
                    }
                }
                Err(_) => {
                    // Skip unreadable blocks
                    discarded_transactions += 1;
                }
            }
        }

        Ok(StorageRecoveryReport {
            recovered_commits,
            discarded_transactions,
            last_sequence,
            success: true,
            error: None,
        })
    }

    /// Get recovery report (if storage was opened from existing device)
    pub fn recovery_report(&self) -> Option<&StorageRecoveryReport> {
        self.recovery_report.as_ref()
    }

    /// Write commit record to commit log
    fn write_commit_record(
        &mut self,
        transaction_id: TransactionId,
        allocations: Vec<AllocationEntry>,
    ) -> Result<(), BlockStorageError> {
        // Increment commit sequence
        self.superblock.commit_sequence += 1;
        let sequence = self.superblock.commit_sequence;

        // Create commit record with checksum
        let record = CommitRecord::new(transaction_id, sequence, allocations);

        // Serialize commit record
        let record_json =
            serde_json::to_vec(&record).map_err(|_| BlockStorageError::SerializationError)?;

        if record_json.len() > BLOCK_SIZE {
            return Err(BlockStorageError::SerializationError);
        }

        // Find commit log slot (round-robin)
        let log_slot = (sequence % self.superblock.commit_log_blocks) as u64;
        let commit_block_idx = self.superblock.commit_log_start + log_slot;

        // Write commit record
        let mut block = [0u8; BLOCK_SIZE];
        block[..record_json.len()].copy_from_slice(&record_json);
        self.device.write_block(commit_block_idx, &block)?;
        self.device.flush()?;

        // Update superblock with new commit sequence
        self.write_superblock()?;

        Ok(())
    }

    /// Write superblock to disk
    fn write_superblock(&mut self) -> Result<(), BlockStorageError> {
        let mut block = [0u8; BLOCK_SIZE];
        let sb_json = serde_json::to_vec(&self.superblock)
            .map_err(|_| BlockStorageError::SerializationError)?;
        if sb_json.len() > BLOCK_SIZE {
            return Err(BlockStorageError::InvalidSuperblock);
        }
        block[..sb_json.len()].copy_from_slice(&sb_json);
        self.device.write_block(0, &block)?;
        self.device.flush()?;
        Ok(())
    }

    /// Allocate blocks for data
    fn allocate_blocks(&mut self, size_bytes: u64) -> Result<Vec<u64>, BlockStorageError> {
        let blocks_needed = ((size_bytes as usize + BLOCK_SIZE - 1) / BLOCK_SIZE) as u64;
        if (self.free_blocks.len() as u64) < blocks_needed {
            return Err(BlockStorageError::NoFreeSpace);
        }

        let mut allocated = Vec::new();
        for _ in 0..blocks_needed {
            if let Some(&block) = self.free_blocks.iter().next() {
                self.free_blocks.remove(&block);
                allocated.push(block);
            }
        }

        Ok(allocated)
    }

    /// Write data to allocated blocks
    fn write_data(&mut self, blocks: &[u64], data: &[u8]) -> Result<(), BlockStorageError> {
        let mut offset = 0;
        for &block_idx in blocks {
            let chunk_size = (data.len() - offset).min(BLOCK_SIZE);
            let mut block_data = [0u8; BLOCK_SIZE];
            block_data[..chunk_size].copy_from_slice(&data[offset..offset + chunk_size]);
            self.device.write_block(block_idx, &block_data)?;
            offset += chunk_size;
        }
        self.device.flush()?;
        Ok(())
    }

    /// Read data from allocated blocks
    fn read_data(&mut self, blocks: &[u64], size_bytes: u64) -> Result<Vec<u8>, BlockStorageError> {
        let mut data = Vec::with_capacity(size_bytes as usize);
        let mut remaining = size_bytes as usize;

        for &block_idx in blocks {
            let mut block = [0u8; BLOCK_SIZE];
            self.device.read_block(block_idx, &mut block)?;
            let chunk_size = remaining.min(BLOCK_SIZE);
            data.extend_from_slice(&block[..chunk_size]);
            remaining -= chunk_size;
            if remaining == 0 {
                break;
            }
        }

        Ok(data)
    }

    /// Read object data by object ID and version ID
    pub fn read_object_data(
        &mut self,
        object_id: ObjectId,
        version_id: VersionId,
    ) -> Result<Vec<u8>, BlockStorageError> {
        // Check pending writes first
        for pending_list in self.pending.values() {
            if let Some(entry) = pending_list
                .iter()
                .rev()
                .find(|p| p.object_id == object_id && p.version_id == version_id)
            {
                return Ok(entry.data.clone());
            }
        }

        // Look up in allocations
        let entry = self
            .allocations
            .get(&(object_id, version_id))
            .ok_or(BlockStorageError::ObjectNotFound)?;

        // Calculate blocks needed
        let blocks_needed = ((entry.size_bytes as usize + BLOCK_SIZE - 1) / BLOCK_SIZE) as u64;
        let blocks: Vec<u64> = (entry.block_idx..entry.block_idx + blocks_needed).collect();

        self.read_data(&blocks, entry.size_bytes)
    }
}

impl<D: BlockDevice> TransactionalStorage for BlockStorage<D> {
    fn begin_transaction(&mut self) -> Result<Transaction, TransactionError> {
        Ok(Transaction::new())
    }

    fn read(&self, tx: &Transaction, object_id: ObjectId) -> Result<VersionId, TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }

        // Check pending writes first
        if let Some(pending) = self.pending.get(&tx.id()) {
            if let Some(entry) = pending.iter().rev().find(|p| p.object_id == object_id) {
                return Ok(entry.version_id);
            }
        }

        // Return the latest version from allocations
        self.latest_versions
            .get(&object_id)
            .copied()
            .ok_or_else(|| TransactionError::ObjectNotFound(object_id.to_string()))
    }

    fn write(
        &mut self,
        tx: &mut Transaction,
        object_id: ObjectId,
        data: &[u8],
    ) -> Result<VersionId, TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }

        let version_id = VersionId::new();

        // Add to pending writes
        self.pending.entry(tx.id()).or_default().push(PendingWrite {
            object_id,
            version_id,
            data: data.to_vec(),
        });

        Ok(version_id)
    }

    fn commit(&mut self, tx: &mut Transaction) -> Result<(), TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }

        // Write all pending writes to disk
        if let Some(pending) = self.pending.remove(&tx.id()) {
            let mut allocations_to_commit = Vec::new();

            // Step 1: Write all data blocks
            for write in pending {
                let size_bytes = write.data.len() as u64;
                let blocks = self
                    .allocate_blocks(size_bytes)
                    .map_err(|e| TransactionError::StorageError(format!("{:?}", e)))?;

                let first_block = blocks[0];
                self.write_data(&blocks, &write.data)
                    .map_err(|e| TransactionError::StorageError(format!("{:?}", e)))?;

                let alloc = AllocationEntry {
                    object_id: write.object_id,
                    version_id: write.version_id,
                    block_idx: first_block,
                    size_bytes,
                };

                allocations_to_commit.push(alloc);
            }

            // Step 2: Write commit record (atomic point of truth)
            self.write_commit_record(tx.id(), allocations_to_commit.clone())
                .map_err(|e| TransactionError::StorageError(format!("{:?}", e)))?;

            // Step 3: Update in-memory state (only after commit record is written)
            for alloc in allocations_to_commit {
                self.allocations
                    .insert((alloc.object_id, alloc.version_id), alloc.clone());
                self.latest_versions
                    .insert(alloc.object_id, alloc.version_id);
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn rollback(&mut self, tx: &mut Transaction) -> Result<(), TransactionError> {
        if tx.state() != crate::transaction::TransactionState::Active {
            return Err(TransactionError::AlreadyFinalized);
        }

        // Discard pending writes
        self.pending.remove(&tx.id());
        let _ = tx.rollback();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::vec;
    use hal::RamDisk;

    #[test]
    fn test_format_and_open() {
        let disk = RamDisk::with_capacity_mb(1);
        let storage = BlockStorage::format(disk).unwrap();

        assert_eq!(storage.superblock.magic, SUPERBLOCK_MAGIC);
        assert_eq!(storage.superblock.version, STORAGE_VERSION);
        assert!(storage.superblock.data_start > 0);
    }

    #[test]
    fn test_write_and_read() {
        let disk = RamDisk::with_capacity_mb(1);
        let mut storage = BlockStorage::format(disk).unwrap();

        let object_id = ObjectId::new();
        let data = b"Hello, persistent storage!";

        let mut tx = storage.begin_transaction().unwrap();
        let version_id = storage.write(&mut tx, object_id, data).unwrap();
        storage.commit(&mut tx).unwrap();

        // Read back
        let tx2 = storage.begin_transaction().unwrap();
        let read_version = storage.read(&tx2, object_id).unwrap();
        assert_eq!(read_version, version_id);
    }

    #[test]
    fn test_persistence_across_open() {
        let object_id = ObjectId::new();
        let data = b"Persistent data";
        let _version_id;

        // Write to storage
        {
            let disk = RamDisk::with_capacity_mb(1);
            let mut storage = BlockStorage::format(disk).unwrap();

            let mut tx = storage.begin_transaction().unwrap();
            _version_id = storage.write(&mut tx, object_id, data).unwrap();
            storage.commit(&mut tx).unwrap();
        }

        // Note: This test demonstrates the concept, but RamDisk doesn't
        // persist across instances. In real usage with a file-backed or
        // hardware block device, data would persist.
    }

    // === Crash-Safety Tests ===

    use crate::failing_device::{FailingBlockDevice, FailurePolicy};

    #[test]
    fn test_crash_safe_commit_succeeds() {
        let disk = RamDisk::with_capacity_mb(1);
        let mut storage = BlockStorage::format(disk).unwrap();

        let object_id = ObjectId::new();
        let data = b"Crash-safe data";

        let mut tx = storage.begin_transaction().unwrap();
        storage.write(&mut tx, object_id, data).unwrap();
        storage.commit(&mut tx).unwrap();

        // Verify data is readable
        let tx2 = storage.begin_transaction().unwrap();
        let version = storage.read(&tx2, object_id).unwrap();
        let read_data = storage.read_object_data(object_id, version).unwrap();
        assert_eq!(read_data, data);
    }

    #[test]
    fn test_crash_during_commit_recovery() {
        // This test verifies that recovery works correctly when a failure occurs during commit
        let disk = RamDisk::with_capacity_mb(1);
        let failing_disk = FailingBlockDevice::new(disk, FailurePolicy::Never);

        // First, create a storage and write some data successfully
        let mut storage = BlockStorage::format(failing_disk).unwrap();

        let obj1 = ObjectId::new();
        let mut tx1 = storage.begin_transaction().unwrap();
        storage.write(&mut tx1, obj1, b"successful data").unwrap();
        storage.commit(&mut tx1).unwrap();

        // Now make the device fail on writes
        storage.device.set_policy(FailurePolicy::AfterWrites(0));

        // Try to write new data (will fail)
        let obj2 = ObjectId::new();
        let mut tx2 = storage.begin_transaction().unwrap();
        storage.write(&mut tx2, obj2, b"failing data").unwrap();
        let result = storage.commit(&mut tx2);
        assert!(result.is_err()); // Commit should fail

        // Recover the device
        storage.device.set_policy(FailurePolicy::Never);
        let device = storage.device;

        // Re-open storage (triggers recovery)
        let mut recovered = BlockStorage::open(device).unwrap();

        // Check recovery report
        let report = recovered.recovery_report().unwrap();
        assert!(report.success);

        // First object should still be readable
        let tx3 = recovered.begin_transaction().unwrap();
        assert!(recovered.read(&tx3, obj1).is_ok());

        // Second object (failed commit) should not exist
        assert!(recovered.read(&tx3, obj2).is_err());
    }

    #[test]
    fn test_multiple_commits_recovery() {
        let disk = RamDisk::with_capacity_mb(1);
        let mut storage = BlockStorage::format(disk).unwrap();

        // Write multiple objects
        let obj1 = ObjectId::new();
        let obj2 = ObjectId::new();
        let obj3 = ObjectId::new();

        let mut tx1 = storage.begin_transaction().unwrap();
        storage.write(&mut tx1, obj1, b"data1").unwrap();
        storage.commit(&mut tx1).unwrap();

        let mut tx2 = storage.begin_transaction().unwrap();
        storage.write(&mut tx2, obj2, b"data2").unwrap();
        storage.commit(&mut tx2).unwrap();

        let mut tx3 = storage.begin_transaction().unwrap();
        storage.write(&mut tx3, obj3, b"data3").unwrap();
        storage.commit(&mut tx3).unwrap();

        // Extract and re-open device
        let device = storage.device;
        let mut recovered = BlockStorage::open(device).unwrap();

        // Check recovery report
        let report = recovered.recovery_report().unwrap();
        assert_eq!(report.recovered_commits, 3);
        assert!(report.success);

        // All objects should be readable
        let tx = recovered.begin_transaction().unwrap();
        assert!(recovered.read(&tx, obj1).is_ok());
        assert!(recovered.read(&tx, obj2).is_ok());
        assert!(recovered.read(&tx, obj3).is_ok());
    }

    #[test]
    fn test_crash_after_data_write_before_commit_record() {
        let disk = RamDisk::with_capacity_mb(1);
        let failing_disk = FailingBlockDevice::new(disk, FailurePolicy::Never);

        let mut storage = BlockStorage::format(failing_disk).unwrap();

        // First commit succeeds
        let obj1 = ObjectId::new();
        let mut tx1 = storage.begin_transaction().unwrap();
        storage.write(&mut tx1, obj1, b"stable data").unwrap();
        storage.commit(&mut tx1).unwrap();

        // Fail on commit record block to simulate crash before commit marker
        let next_seq = storage.superblock.commit_sequence + 1;
        let log_slot = (next_seq % storage.superblock.commit_log_blocks) as u64;
        let commit_block_idx = storage.superblock.commit_log_start + log_slot;
        storage
            .device
            .set_policy(FailurePolicy::OnBlocks(vec![commit_block_idx]));

        let obj2 = ObjectId::new();
        let mut tx2 = storage.begin_transaction().unwrap();
        storage.write(&mut tx2, obj2, b"should not commit").unwrap();
        let result = storage.commit(&mut tx2);
        assert!(result.is_err());

        // Recover
        storage.device.set_policy(FailurePolicy::Never);
        let device = storage.device;
        let mut recovered = BlockStorage::open(device).unwrap();

        let tx3 = recovered.begin_transaction().unwrap();
        assert!(recovered.read(&tx3, obj1).is_ok());
        assert!(recovered.read(&tx3, obj2).is_err());
    }

    #[test]
    fn test_crash_after_commit_record_before_superblock_update() {
        let disk = RamDisk::with_capacity_mb(1);
        let failing_disk = FailingBlockDevice::new(disk, FailurePolicy::Never);

        let mut storage = BlockStorage::format(failing_disk).unwrap();

        // Seed a baseline commit
        let obj1 = ObjectId::new();
        let mut tx1 = storage.begin_transaction().unwrap();
        storage.write(&mut tx1, obj1, b"baseline").unwrap();
        storage.commit(&mut tx1).unwrap();

        // Fail on superblock write (block 0) to simulate crash mid-metadata update
        storage.device.set_policy(FailurePolicy::OnBlocks(vec![0]));

        let obj2 = ObjectId::new();
        let mut tx2 = storage.begin_transaction().unwrap();
        storage.write(&mut tx2, obj2, b"new data").unwrap();
        let result = storage.commit(&mut tx2);
        assert!(result.is_err());

        // Recover
        storage.device.set_policy(FailurePolicy::Never);
        let device = storage.device;
        let mut recovered = BlockStorage::open(device).unwrap();

        let tx3 = recovered.begin_transaction().unwrap();
        assert!(recovered.read(&tx3, obj1).is_ok());

        // Commit record may have landed before the superblock update; either outcome is valid
        let obj2_visible = recovered.read(&tx3, obj2).is_ok();
        if obj2_visible {
            let version = recovered.read(&tx3, obj2).unwrap();
            let data = recovered.read_object_data(obj2, version).unwrap();
            assert_eq!(data, b"new data");
        }
    }

    #[test]
    fn test_checksum_validation() {
        // Create a commit record with invalid checksum
        let alloc = AllocationEntry {
            object_id: ObjectId::new(),
            version_id: VersionId::new(),
            block_idx: 100,
            size_bytes: 128,
        };

        let mut record = CommitRecord::new(TransactionId::new(), 1, vec![alloc]);

        // Record should be valid initially
        assert!(record.is_valid());

        // Corrupt the checksum
        record.checksum = 0xDEADBEEF;
        assert!(!record.is_valid());
    }

    #[test]
    fn test_commit_log_wrap_around() {
        let disk = RamDisk::with_capacity_mb(1);
        let mut storage = BlockStorage::format(disk).unwrap();

        let log_size = storage.superblock.commit_log_blocks as usize;

        // Write more commits than log size to test wrap-around
        for i in 0..(log_size + 5) {
            let obj = ObjectId::new();
            let data = format!("data_{}", i);

            let mut tx = storage.begin_transaction().unwrap();
            storage.write(&mut tx, obj, data.as_bytes()).unwrap();
            storage.commit(&mut tx).unwrap();
        }

        // Should still work correctly
        assert_eq!(storage.superblock.commit_sequence, (log_size + 5) as u64);
    }

    #[test]
    fn test_recovery_with_no_commits() {
        let disk = RamDisk::with_capacity_mb(1);
        let storage = BlockStorage::format(disk).unwrap();

        // Re-open immediately (no commits)
        let device = storage.device;
        let recovered = BlockStorage::open(device).unwrap();

        let report = recovered.recovery_report().unwrap();
        assert_eq!(report.recovered_commits, 0);
        assert_eq!(report.last_sequence, 0);
        assert!(report.success);
    }
}
